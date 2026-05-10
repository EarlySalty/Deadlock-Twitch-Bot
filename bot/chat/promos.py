import asyncio
import logging
import secrets
import time
from collections import deque
from datetime import UTC, datetime, timedelta

from ..core.chat_bots import build_known_chat_bot_not_in_clause
from ..dashboard.billing.billing_plans import billing_plan_has_entitlement
from ..entitlements.resolver import resolve_plan_snapshot_for_refs
from ..promo_mode import (
    evaluate_global_promo_mode,
    load_global_promo_mode,
    validate_streamer_promo_message,
)
from ..storage import (
    query_one as _pg_query_one,
    query_all as _pg_query_all,
    readonly_connection,
    transaction,
)
from .constants import (
    _PROMO_ACTIVITY_ENABLED,
    _PROMO_COOLDOWN_MAX,
    _PROMO_COOLDOWN_MIN,
    _PROMO_INTERVAL_MIN,
    PROMO_ACTIVITY_CHATTER_DEDUP_SEC,
    PROMO_ACTIVITY_MIN_CHATTERS,
    PROMO_ACTIVITY_MIN_MSGS,
    PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO,
    PROMO_ACTIVITY_TARGET_MPM,
    PROMO_ACTIVITY_WINDOW_MIN,
    PROMO_ATTEMPT_COOLDOWN_MIN,
    PROMO_CHANNEL_ALLOWLIST,
    PROMO_DISCORD_INVITE,
    PROMO_IGNORE_COMMANDS,
    PROMO_LOOP_INTERVAL_SEC,
    PROMO_MESSAGES,
    PROMO_MESSAGES_CATEGORIZED,
    PROMO_NEW_CHATTERS_MIN,
    PROMO_OVERALL_COOLDOWN_MIN,
    PROMO_SEEN_CHATTER_MAX_AGE_SEC,
    PROMO_VIEWER_SPIKE_COOLDOWN_MIN,
    PROMO_VIEWER_SPIKE_ENABLED,
    PROMO_VIEWER_SPIKE_MIN_CHAT_SILENCE_SEC,
    PROMO_VIEWER_SPIKE_MIN_DELTA,
    PROMO_VIEWER_SPIKE_MIN_RATIO,
    PROMO_VIEWER_SPIKE_MIN_SESSIONS,
    PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES,
    PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT,
    PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT,
    SUBSCRIPTION_PLANS_ENABLED,
)

log = logging.getLogger("TwitchStreams.ChatBot")

_LURKER_TAX_SCOPE = "moderator:read:chatters"
_LURKER_TAX_FRESHNESS_MINUTES = 5
_LURKER_TAX_MIN_PRIOR_SESSIONS = 3
_LURKER_TAX_MIN_WATCHTIME_MINUTES = 240
_LURKER_TAX_MAX_MENTIONS = 2
_PROMO_ACTIVITY_BUCKET_MAXLEN = 2048
_PROMO_RUNTIME_STATE_MAX_AGE_SEC = 24 * 60 * 60
_PROMO_RUNTIME_PRUNE_INTERVAL_SEC = 60


def _sanitize_log_value(value: object | None) -> str:
    if value is None:
        return "<none>"
    return str(value).replace("\r", "\\r").replace("\n", "\\n")


class PromoMixin:
    def _promo_channel_allowed(self, login: str) -> bool:
        if not PROMO_MESSAGES:
            return False
        if PROMO_CHANNEL_ALLOWLIST and login not in PROMO_CHANNEL_ALLOWLIST:
            return False
        return True

    async def _get_promo_invite(self, login: str) -> tuple[str | None, bool]:
        resolver = getattr(self, "_resolve_streamer_invite", None)
        if callable(resolver):
            try:
                result = await resolver(login)
                if isinstance(result, tuple):
                    invite, is_specific = result
                else:
                    invite, is_specific = result, True
                if invite:
                    return str(invite), bool(is_specific)
            except Exception:
                log.debug("_resolve_streamer_invite failed for %s", login, exc_info=True)

        if PROMO_DISCORD_INVITE:
            return PROMO_DISCORD_INVITE, False
        return None, False

    @staticmethod
    def _parse_lurker_tax_datetime(raw_value) -> datetime | None:
        if raw_value is None:
            return None
        if isinstance(raw_value, datetime):
            parsed = raw_value
        else:
            text = str(raw_value or "").strip()
            if not text:
                return None
            if len(text) == 10 and text[4] == "-" and text[7] == "-":
                text = f"{text}T23:59:59+00:00"
            normalized = text.replace("Z", "+00:00")
            try:
                parsed = datetime.fromisoformat(normalized)
            except ValueError:
                return None
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=UTC)
        return parsed.astimezone(UTC)

    def _ensure_lurker_tax_streamer_plan_columns(self, conn) -> None:
        state = getattr(self, "_lurker_tax_storage_ready", False)
        if state:
            return
        try:
            conn.execute(
                "ALTER TABLE streamer_plans "
                "ADD COLUMN IF NOT EXISTS lurker_tax_enabled INTEGER NOT NULL DEFAULT 0"
            )
        except Exception:
            try:
                conn.execute(
                    "ALTER TABLE streamer_plans "
                    "ADD COLUMN lurker_tax_enabled INTEGER NOT NULL DEFAULT 0"
                )
            except Exception:
                pass
        self._lurker_tax_storage_ready = True

    def _load_lurker_tax_settings(self, login: str) -> dict[str, object]:
        normalized_login = str(login or "").strip().lower()
        default_payload = {
            "login": normalized_login,
            "twitch_user_id": "",
            "plan_id": "raid_free",
            "is_paid_plan": False,
            "enabled": False,
            "has_moderator_read_chatters": False,
            "active_session_id": None,
            "is_live": False,
        }
        if not normalized_login or not SUBSCRIPTION_PLANS_ENABLED:
            return default_payload

        try:
            with readonly_connection() as conn:
                self._ensure_lurker_tax_streamer_plan_columns(conn)

                streamer_row = conn.execute(
                    """
                    SELECT twitch_user_id, twitch_login
                      FROM twitch_streamer_identities
                     WHERE LOWER(twitch_login) = LOWER(%s)
                     LIMIT 1
                    """,
                    (normalized_login,),
                ).fetchone()
                twitch_user_id = str(
                    ((streamer_row["twitch_user_id"] if hasattr(streamer_row, "keys") else streamer_row[0])
                    if streamer_row else "")
                    or ""
                ).strip()
                canonical_login = str(
                    ((streamer_row["twitch_login"] if hasattr(streamer_row, "keys") else streamer_row[1])
                    if streamer_row else normalized_login)
                    or normalized_login
                ).strip() or normalized_login

                if twitch_user_id:
                    settings_row = conn.execute(
                        """
                        SELECT twitch_user_id, twitch_login, lurker_tax_enabled, manual_plan_id, manual_plan_expires_at
                          FROM streamer_plans
                         WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                            OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                         ORDER BY
                            CASE WHEN TRIM(COALESCE(twitch_user_id, '')) = %s THEN 0 ELSE 1 END
                         LIMIT 1
                        """,
                        (twitch_user_id, canonical_login, twitch_user_id),
                    ).fetchone()
                else:
                    settings_row = conn.execute(
                        """
                        SELECT twitch_user_id, twitch_login, lurker_tax_enabled
                          FROM streamer_plans
                         WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                         LIMIT 1
                        """,
                        (canonical_login,),
                    ).fetchone()

                enabled = False
                if settings_row:
                    enabled = bool(
                        (
                            settings_row["lurker_tax_enabled"]
                            if hasattr(settings_row, "keys")
                            else settings_row[2]
                        )
                        or 0
                    )
                snapshot = resolve_plan_snapshot_for_refs(
                    [value for value in (twitch_user_id, canonical_login, normalized_login) if value],
                    conn=conn,
                    fallback_ref=canonical_login,
                )
                plan_id = str(snapshot.get("plan_id") or "raid_free").strip() or "raid_free"

                if twitch_user_id:
                    auth_row = conn.execute(
                        """
                        SELECT scopes
                          FROM twitch_raid_auth
                         WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                            OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                         ORDER BY
                            CASE WHEN TRIM(COALESCE(twitch_user_id, '')) = %s THEN 0 ELSE 1 END
                         LIMIT 1
                        """,
                        (twitch_user_id, canonical_login, twitch_user_id),
                    ).fetchone()
                else:
                    auth_row = conn.execute(
                        """
                        SELECT scopes
                          FROM twitch_raid_auth
                         WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                         LIMIT 1
                        """,
                        (canonical_login,),
                    ).fetchone()
                scopes_raw = str(
                    ((auth_row["scopes"] if hasattr(auth_row, "keys") else auth_row[0]) if auth_row else "")
                    or ""
                ).strip()
                scopes = {scope.strip().lower() for scope in scopes_raw.split() if scope.strip()}

                live_row = conn.execute(
                    """
                    SELECT active_session_id, is_live
                      FROM twitch_live_state
                     WHERE LOWER(streamer_login) = LOWER(%s)
                     LIMIT 1
                    """,
                    (canonical_login,),
                ).fetchone()
                active_session_id = (
                    (live_row["active_session_id"] if hasattr(live_row, "keys") else live_row[0])
                    if live_row
                    else None
                )
                is_live = bool(
                    (
                        live_row["is_live"] if hasattr(live_row, "keys") else live_row[1]
                    )
                    if live_row
                    else False
                )
        except Exception:
            log.debug("Lurker-Tax settings lookup failed for %s", normalized_login, exc_info=True)
            return default_payload

        token_mgr = getattr(self, "_token_manager", None)
        bot_scopes: set[str] = set()
        bot_scopes_loaded = False
        if token_mgr is not None:
            try:
                bot_scopes = {
                    str(scope).strip().lower()
                    for scope in (getattr(token_mgr, "scopes", None) or set())
                    if str(scope).strip()
                }
                # If the token manager hasn't validated yet, scopes may still be unknown.
                bot_scopes_loaded = bool(
                    getattr(token_mgr, "bot_id", None) or getattr(token_mgr, "expires_at", None)
                )
            except Exception:
                bot_scopes = set()
                bot_scopes_loaded = False

        has_chatters_scope = (_LURKER_TAX_SCOPE in scopes) or (
            token_mgr is not None
            and bot_scopes_loaded
            and _LURKER_TAX_SCOPE in bot_scopes
        )

        return {
            "login": canonical_login.lower(),
            "twitch_user_id": twitch_user_id,
            "plan_id": plan_id,
            "is_paid_plan": billing_plan_has_entitlement(plan_id, "chat.lurker_tax"),
            "enabled": enabled,
            # Migration: Lurker-Tax wird bot-zentriert ueber den zentralen Bot-Token ermoeglicht.
            "has_moderator_read_chatters": has_chatters_scope,
            "active_session_id": int(active_session_id) if active_session_id else None,
            "is_live": is_live,
        }

    def _set_lurker_tax_enabled(
        self,
        *,
        twitch_login: str,
        twitch_user_id: str = "",
        plan_id: str = "",
        enabled: bool,
    ) -> bool:
        login_value = str(twitch_login or "").strip().lower()
        user_id_value = str(twitch_user_id or "").strip()
        plan_name = {
            "raid_boost": "raid_boost",
            "analysis_dashboard": "analysis",
            "bundle_analysis_raid_boost": "bundle",
        }.get(str(plan_id or "").strip(), "free")
        if not login_value:
            return False

        try:
            with transaction() as conn:
                self._ensure_lurker_tax_streamer_plan_columns(conn)
                if user_id_value:
                    conn.execute(
                        """
                        INSERT INTO streamer_plans (
                            twitch_user_id,
                            twitch_login,
                            plan_name,
                            lurker_tax_enabled
                        )
                        VALUES (%s, %s, %s, %s)
                        ON CONFLICT (twitch_user_id) DO UPDATE SET
                            twitch_login = COALESCE(NULLIF(EXCLUDED.twitch_login, ''), streamer_plans.twitch_login),
                            plan_name = COALESCE(NULLIF(EXCLUDED.plan_name, ''), streamer_plans.plan_name),
                            lurker_tax_enabled = EXCLUDED.lurker_tax_enabled
                        """,
                        (user_id_value, login_value, plan_name, 1 if enabled else 0),
                    )
                else:
                    existing_row = conn.execute(
                        """
                        SELECT 1
                          FROM streamer_plans
                         WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                         LIMIT 1
                        """,
                        (login_value,),
                    ).fetchone()
                    if not existing_row:
                        return False
                    conn.execute(
                        """
                        UPDATE streamer_plans
                           SET lurker_tax_enabled = %s
                         WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                        """,
                        (1 if enabled else 0, login_value),
                    )
        except Exception:
            log.debug("Lurker-Tax setting update failed for %s", login_value, exc_info=True)
            return False
        return True

    def _lurker_tax_mentions_for_session(self, session_id: int) -> set[str]:
        state = getattr(self, "_lurker_tax_mentions", None)
        if not isinstance(state, dict):
            state = {}
            self._lurker_tax_mentions = state
        mentioned = state.get(session_id)
        if not isinstance(mentioned, set):
            mentioned = set()
            state[session_id] = mentioned
        return mentioned

    def _build_lurker_tax_text(self, chatter_logins: list[str]) -> str:
        mentions = " ".join(f"@{login}" for login in chatter_logins if login)
        return (
            f"Lurker Steuer: {mentions} "
            "falls ihr gerade entspannt mitlest, denkt gern an eure Channel-Points."
        ).strip()

    def _get_lurker_tax_candidates(
        self,
        *,
        login: str,
        session_id: int,
        now_utc: datetime | None = None,
    ) -> list[dict[str, object]]:
        current_bot_clause, current_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="live_candidates.chatter_login",
            placeholder="%s",
        )
        historical_bot_clause, historical_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="sc.chatter_login",
            placeholder="%s",
        )
        freshness_cutoff = (
            (now_utc or datetime.now(UTC)) - timedelta(minutes=_LURKER_TAX_FRESHNESS_MINUTES)
        ).isoformat(timespec="seconds")
        rows = []
        try:
            with readonly_connection() as conn:
                rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                    f"""
                    WITH historical_lurks AS (
                        SELECT
                            CASE
                                WHEN TRIM(COALESCE(sc.chatter_id, '')) <> '' THEN 'id:' || TRIM(sc.chatter_id)
                                ELSE 'login:' || LOWER(sc.chatter_login)
                            END AS chatter_identity_key,
                            COUNT(DISTINCT sc.session_id) AS prior_lurk_sessions,
                            COALESCE(
                                SUM(
                                    CASE
                                        WHEN sc.first_message_at IS NULL OR sc.last_seen_at IS NULL THEN 0
                                        WHEN sc.last_seen_at <= sc.first_message_at THEN 0
                                        ELSE EXTRACT(EPOCH FROM (sc.last_seen_at - sc.first_message_at)) / 60.0
                                    END
                                ),
                                0
                            ) AS estimated_lurk_minutes
                        FROM twitch_session_chatters sc
                        JOIN twitch_stream_sessions s ON s.id = sc.session_id
                        WHERE LOWER(sc.streamer_login) = LOWER(%s)
                          AND s.ended_at IS NOT NULL
                          AND COALESCE(sc.messages, 0) = 0
                          AND LOWER(COALESCE(CAST(sc.seen_via_chatters_api AS TEXT), '0')) IN ('1', 't', 'true')
                          AND {historical_bot_clause}
                        GROUP BY
                            CASE
                                WHEN TRIM(COALESCE(sc.chatter_id, '')) <> '' THEN 'id:' || TRIM(sc.chatter_id)
                                ELSE 'login:' || LOWER(sc.chatter_login)
                            END
                    ),
                    live_candidates AS (
                        SELECT
                            live_sc.chatter_login,
                            live_sc.chatter_id,
                            CASE
                                WHEN TRIM(COALESCE(live_sc.chatter_id, '')) <> '' THEN 'id:' || TRIM(live_sc.chatter_id)
                                ELSE 'login:' || LOWER(live_sc.chatter_login)
                            END AS chatter_identity_key
                        FROM twitch_session_chatters live_sc
                        WHERE live_sc.session_id = %s
                          AND LOWER(live_sc.streamer_login) = LOWER(%s)
                          AND COALESCE(live_sc.messages, 0) = 0
                          AND LOWER(COALESCE(CAST(live_sc.seen_via_chatters_api AS TEXT), '0')) IN ('1', 't', 'true')
                          AND live_sc.last_seen_at IS NOT NULL
                          AND live_sc.last_seen_at >= %s
                    )
                    SELECT
                        live_candidates.chatter_login,
                        live_candidates.chatter_id,
                        historical_lurks.prior_lurk_sessions,
                        historical_lurks.estimated_lurk_minutes
                    FROM live_candidates
                    JOIN historical_lurks
                      ON historical_lurks.chatter_identity_key = live_candidates.chatter_identity_key
                    WHERE historical_lurks.prior_lurk_sessions >= %s
                      AND historical_lurks.estimated_lurk_minutes >= %s
                      AND {current_bot_clause}
                    ORDER BY historical_lurks.estimated_lurk_minutes DESC, LOWER(live_candidates.chatter_login) ASC
                    """,
                    (
                        login,
                        *historical_bot_params,
                        session_id,
                        login,
                        freshness_cutoff,
                        _LURKER_TAX_MIN_PRIOR_SESSIONS,
                        _LURKER_TAX_MIN_WATCHTIME_MINUTES,
                        *current_bot_params,
                    ),
                ).fetchall()
        except Exception:
            log.debug("Lurker-Tax candidate lookup failed for %s", login, exc_info=True)
            return []

        payload: list[dict[str, object]] = []
        for row in rows:
            chatter_login = str(
                (row["chatter_login"] if hasattr(row, "keys") else row[0]) or ""
            ).strip().lower()
            if not chatter_login:
                continue
            payload.append(
                {
                    "chatter_login": chatter_login,
                    "chatter_id": str(
                        (row["chatter_id"] if hasattr(row, "keys") else row[1]) or ""
                    ).strip(),
                    "prior_lurk_sessions": int(
                        (row["prior_lurk_sessions"] if hasattr(row, "keys") else row[2]) or 0
                    ),
                    "estimated_lurk_minutes": float(
                        (row["estimated_lurk_minutes"] if hasattr(row, "keys") else row[3]) or 0.0
                    ),
                }
            )
        return payload

    def _select_lurker_tax_mentions(
        self,
        *,
        login: str,
        session_id: int,
        now_utc: datetime | None = None,
    ) -> list[str]:
        mentioned = self._lurker_tax_mentions_for_session(session_id)
        selected: list[str] = []
        for candidate in self._get_lurker_tax_candidates(
            login=login,
            session_id=session_id,
            now_utc=now_utc,
        ):
            chatter_login = str(candidate.get("chatter_login") or "").strip().lower()
            if not chatter_login or chatter_login in mentioned:
                continue
            selected.append(chatter_login)
            if len(selected) >= _LURKER_TAX_MAX_MENTIONS:
                break
        return selected

    def _record_raw_chat_message(self, login: str) -> None:
        if not login:
            return

        now = time.monotonic()
        raw_map = getattr(self, "_last_raw_chat_message_ts", None)
        if not isinstance(raw_map, dict):
            raw_map = {}
            self._last_raw_chat_message_ts = raw_map
        raw_map[login] = now

        raw_count_map = getattr(self, "_raw_msg_count_since_promo", None)
        if not isinstance(raw_count_map, dict):
            raw_count_map = {}
            self._raw_msg_count_since_promo = raw_count_map
        raw_count_map[login] = int(raw_count_map.get(login, 0)) + 1
        self._prune_promo_runtime_state(now)

    def _raw_msg_count_since_last_promo(self, login: str) -> int:
        raw_count_map = getattr(self, "_raw_msg_count_since_promo", None)
        if not isinstance(raw_count_map, dict):
            return 0
        return int(raw_count_map.get(login, 0))

    def _has_new_raw_chat_since_last_promo(self, login: str) -> bool:
        last_sent = self._last_promo_sent.get(login)
        if last_sent is None:
            return True

        raw_map = getattr(self, "_last_raw_chat_message_ts", None)
        if not isinstance(raw_map, dict):
            return False

        last_raw = raw_map.get(login)
        if last_raw is None:
            return False

        return float(last_raw) > float(last_sent)

    def _prune_promo_activity(self, bucket: deque[tuple[float, str]], now: float) -> None:
        window_sec = PROMO_ACTIVITY_WINDOW_MIN * 60
        while bucket and now - bucket[0][0] > window_sec:
            bucket.popleft()

    def _get_promo_activity_bucket(self, login: str) -> deque[tuple[float, str]]:
        bucket = self._promo_activity.get(login)
        if isinstance(bucket, deque) and bucket.maxlen == _PROMO_ACTIVITY_BUCKET_MAXLEN:
            return bucket
        normalized = deque(bucket or (), maxlen=_PROMO_ACTIVITY_BUCKET_MAXLEN)
        self._promo_activity[login] = normalized
        return normalized

    def _prune_promo_runtime_state(self, now: float, *, force: bool = False) -> None:
        last_pruned = float(getattr(self, "_promo_runtime_state_last_pruned_monotonic", 0.0) or 0.0)
        if not force and (now - last_pruned) < float(_PROMO_RUNTIME_PRUNE_INTERVAL_SEC):
            return
        self._promo_runtime_state_last_pruned_monotonic = now

        activity = getattr(self, "_promo_activity", None)
        if isinstance(activity, dict):
            for login, bucket in list(activity.items()):
                if not isinstance(bucket, deque):
                    activity.pop(login, None)
                    continue
                self._prune_promo_activity(bucket, now)
                if not bucket:
                    activity.pop(login, None)

        dedupe_state = getattr(self, "_promo_chatter_dedupe", None)
        if isinstance(dedupe_state, dict):
            for login in list(dedupe_state):
                self._prune_promo_chatter_dedupe(login, now)

        stale_before = now - float(_PROMO_RUNTIME_STATE_MAX_AGE_SEC)
        timestamp_maps = (
            getattr(self, "_last_raw_chat_message_ts", None),
            getattr(self, "_last_promo_sent", None),
            getattr(self, "_last_promo_attempt", None),
            getattr(self, "_last_promo_viewer_spike", None),
        )
        active_logins: set[str] = set()
        for mapping in timestamp_maps:
            if not isinstance(mapping, dict):
                continue
            for login, ts in list(mapping.items()):
                try:
                    ts_value = float(ts)
                except (TypeError, ValueError):
                    mapping.pop(login, None)
                    continue
                if ts_value < stale_before:
                    mapping.pop(login, None)
                else:
                    active_logins.add(str(login))

        raw_count_map = getattr(self, "_raw_msg_count_since_promo", None)
        if isinstance(raw_count_map, dict):
            for login in list(raw_count_map):
                if str(login) not in active_logins and str(login) not in getattr(
                    self, "_promo_activity", {}
                ):
                    raw_count_map.pop(login, None)

    def _prune_promo_chatter_dedupe(self, login: str, now: float) -> None:
        dedupe_state = getattr(self, "_promo_chatter_dedupe", None)
        if not isinstance(dedupe_state, dict):
            return
        chatter_last = dedupe_state.get(login)
        if not isinstance(chatter_last, dict) or not chatter_last:
            return

        max_age_sec = max(
            float(PROMO_ACTIVITY_WINDOW_MIN * 60),
            float(PROMO_ACTIVITY_CHATTER_DEDUP_SEC) * 4.0,
        )
        stale = [chatter for chatter, ts in chatter_last.items() if now - float(ts) > max_age_sec]
        for chatter in stale:
            chatter_last.pop(chatter, None)
        if not chatter_last:
            dedupe_state.pop(login, None)

    def _get_current_session_viewers(self, login: str) -> set[str]:
        """Gibt alle Viewer/Chatter aus der aktiven Session via Twitch-API zurück."""
        try:
            rows = _pg_query_all(
                """
                SELECT sc.chatter_login
                  FROM twitch_session_chatters sc
                  JOIN twitch_live_state ls ON ls.active_session_id = sc.session_id
                 WHERE LOWER(ls.streamer_login) = LOWER(%s)
                   AND ls.is_live = 1
                   AND COALESCE(TRIM(sc.chatter_login), '') <> ''
                """,
                (login,),
            )
            return {str(row[0] if not hasattr(row, "keys") else row["chatter_login"]).strip().lower() for row in rows if row}
        except Exception:
            log.debug("_get_current_session_viewers fehlgeschlagen für %s", login, exc_info=True)
            return set()

    def _get_current_viewers_combined(self, login: str, now: float) -> set[str]:
        """Kombiniert aktive Chat-Chatter (8-min-Fenster) mit API-getrackten Viewern."""
        chatters: set[str] = set()
        bucket = self._promo_activity.get(login)
        if bucket:
            self._prune_promo_activity(bucket, now)
            chatters = {c for _, c in bucket}
        api_viewers = self._get_current_session_viewers(login)
        return chatters | api_viewers

    def _get_new_chatters_in_window(self, login: str, now: float) -> int:
        """Zählt Viewer/Chatter, die seit der letzten Promo neu sind (Chat + API)."""
        current = self._get_current_viewers_combined(login, now)
        if not current:
            return 0
        seen_map = getattr(self, "_promo_seen_chatters", None)
        seen: set[str] = seen_map.get(login, set()) if isinstance(seen_map, dict) else set()
        return len(current - seen)

    def _update_seen_chatters(self, login: str, now: float) -> None:
        """Markiert alle aktuellen Viewer (Chat + API) als 'Promo gesehen'; lässt alte Einträge verfallen."""
        seen_map = getattr(self, "_promo_seen_chatters", None)
        if not isinstance(seen_map, dict):
            seen_map = {}
            self._promo_seen_chatters = seen_map

        seen_ts = getattr(self, "_promo_seen_chatters_ts", None)
        if not isinstance(seen_ts, dict):
            seen_ts = {}
            self._promo_seen_chatters_ts = seen_ts

        max_age = float(PROMO_SEEN_CHATTER_MAX_AGE_SEC)
        last_reset = float(seen_ts.get(login) or 0)
        if now - last_reset > max_age:
            seen_map.pop(login, None)

        current = self._get_current_viewers_combined(login, now)
        seen_map.setdefault(login, set()).update(current)
        seen_ts[login] = now

    def _record_promo_activity(self, login: str, chatter_login: str, now: float) -> None:
        dedupe_state = getattr(self, "_promo_chatter_dedupe", None)
        if not isinstance(dedupe_state, dict):
            dedupe_state = {}
            self._promo_chatter_dedupe = dedupe_state

        chatter_last = dedupe_state.setdefault(login, {})
        last_seen = chatter_last.get(chatter_login)
        if last_seen is not None and now - float(last_seen) < float(
            PROMO_ACTIVITY_CHATTER_DEDUP_SEC
        ):
            return

        chatter_last[chatter_login] = now
        self._prune_promo_chatter_dedupe(login, now)

        bucket = self._get_promo_activity_bucket(login)
        bucket.append((now, chatter_login))
        self._prune_promo_activity(bucket, now)
        self._prune_promo_runtime_state(now)

    def _get_promo_activity_stats(self, login: str, now: float) -> tuple[int, int, float]:
        bucket = self._promo_activity.get(login)
        if not bucket:
            return 0, 0, 0.0
        self._prune_promo_activity(bucket, now)
        msg_count = len(bucket)
        if msg_count <= 0:
            return 0, 0, 0.0
        unique_chatters = len({c for _, c in bucket})
        msgs_per_min = msg_count / max(1.0, float(PROMO_ACTIVITY_WINDOW_MIN))
        return msg_count, unique_chatters, msgs_per_min

    def _promo_cooldown_sec(self, msgs_per_min: float) -> float:
        min_cd = float(_PROMO_COOLDOWN_MIN)
        max_cd = float(_PROMO_COOLDOWN_MAX)
        if max_cd < min_cd:
            max_cd = min_cd
        target = float(PROMO_ACTIVITY_TARGET_MPM)
        ratio = 1.0 if target <= 0 else min(1.0, msgs_per_min / target)
        return (min_cd + (1.0 - ratio) * (max_cd - min_cd)) * 60.0

    def _overall_promo_cooldown_sec(self) -> float:
        return max(0.0, float(PROMO_OVERALL_COOLDOWN_MIN) * 60.0)

    def _overall_promo_ready(self, login: str, now: float) -> bool:
        overall_sec = self._overall_promo_cooldown_sec()
        if overall_sec <= 0:
            return True
        last_sent = self._last_promo_sent.get(login)
        if last_sent is None:
            return True
        return (now - float(last_sent)) >= overall_sec

    def _promo_attempt_allowed(self, login: str, now: float) -> bool:
        last_attempt = self._last_promo_attempt.get(login)
        if last_attempt is not None and now - last_attempt < (PROMO_ATTEMPT_COOLDOWN_MIN * 60):
            return False
        self._last_promo_attempt[login] = now
        try:
            from ..storage import save_promo_cooldown
            wall_now = time.time()
            mono_now = time.monotonic()
            save_promo_cooldown(login, "attempt", wall_now - (mono_now - now))
        except Exception:
            pass
        return True

    @staticmethod
    def _make_promo_channel(login: str, channel_id: str):
        class _Channel:
            __slots__ = ("name", "id")

            def __init__(self, name: str, cid: str):
                self.name = name
                self.id = cid

        return _Channel(login, channel_id)

    def _load_streamer_promo_message(self, login: str) -> str | None:
        try:
            with readonly_connection() as conn:
                row = conn.execute(
                    "SELECT promo_message FROM streamer_plans WHERE LOWER(twitch_login) = LOWER(%s)",
                    (login,),
                ).fetchone()
                if row and row["promo_message"]:
                    message = str(row["promo_message"]).strip()
                    issues = validate_streamer_promo_message(message)
                    if issues:
                        log.debug(
                            "Ignoring invalid promo_message for %s: %s",
                            login,
                            issues[0]["message"],
                        )
                        return None
                    return message
        except Exception:
            log.debug("Custom promo_message lookup failed for %s", login, exc_info=True)
        return None

    def _load_global_promo_message(self) -> str | None:
        try:
            with readonly_connection() as conn:
                config = load_global_promo_mode(conn)
        except Exception:
            log.debug("Global promo mode lookup failed", exc_info=True)
            return None

        evaluation = evaluate_global_promo_mode(config)
        message = str(evaluation.get("active_message") or "").strip()
        return message or None

    @staticmethod
    def _format_promo_template(template: str, invite: str) -> str | None:
        try:
            return str(template).format(invite=invite)
        except Exception:
            log.warning("Promo template could not be rendered", exc_info=True)
            return None

    def _mark_promo_sent(self, login: str, now: float, *, reason: str) -> None:
        self._last_promo_sent[login] = now
        raw_count_map = getattr(self, "_raw_msg_count_since_promo", None)
        if isinstance(raw_count_map, dict):
            raw_count_map[login] = 0
        self._update_seen_chatters(login, now)
        if reason == "viewer_spike":
            viewer_spike_map = getattr(self, "_last_promo_viewer_spike", None)
            if not isinstance(viewer_spike_map, dict):
                viewer_spike_map = {}
                self._last_promo_viewer_spike = viewer_spike_map
            viewer_spike_map[login] = now
        # Persist to DB
        try:
            from ..storage import save_promo_cooldown
            wall_now = time.time()
            mono_now = time.monotonic()
            wall_ts = wall_now - (mono_now - now)
            save_promo_cooldown(login, "sent", wall_ts)
            if reason == "viewer_spike":
                save_promo_cooldown(login, "viewer_spike", wall_ts)
        except Exception:
            log.debug("Promo cooldown persist failed for %s", login, exc_info=True)

    def _restore_promo_cooldowns(self) -> None:
        """Load persisted promo cooldowns from DB and populate in-memory dicts."""
        from ..storage import load_promo_cooldowns, cleanup_stale_promo_cooldowns

        try:
            cleanup_stale_promo_cooldowns(24)
        except Exception:
            pass

        rows = load_promo_cooldowns()
        if not rows:
            return

        wall_now = time.time()
        mono_now = time.monotonic()
        restored = 0

        for login, cooldown_type, wall_ts in rows:
            age_sec = wall_now - wall_ts
            if age_sec > 24 * 3600 or age_sec < 0:
                continue
            mono_ts = mono_now - age_sec

            if cooldown_type == "sent":
                self._last_promo_sent.setdefault(login, mono_ts)
            elif cooldown_type == "attempt":
                self._last_promo_attempt.setdefault(login, mono_ts)
            elif cooldown_type == "viewer_spike":
                self._last_promo_viewer_spike.setdefault(login, mono_ts)
            else:
                continue
            restored += 1

        if restored:
            log.info("Restored %d promo cooldown(s) from DB", restored)

    def _build_promo_text(self, login: str, invite: str, reason: str = "promo") -> str | None:
        global_message = self._load_global_promo_message()
        if global_message:
            return self._format_promo_template(global_message, invite)

        custom_message = self._load_streamer_promo_message(login)
        if custom_message:
            return self._format_promo_template(custom_message, invite)

        # Kategorisierte Nachrichten auswählen
        messages = PROMO_MESSAGES
        if PROMO_MESSAGES_CATEGORIZED:
            if reason == "viewer_spike":
                messages = PROMO_MESSAGES_CATEGORIZED.get("hype") or PROMO_MESSAGES
            elif reason == "chat_activity":
                # Mix aus Competitive, Community und Growth für aktive Chat-Phasen
                pool = []
                for cat in ("competitive", "community", "growth"):
                    pool.extend(PROMO_MESSAGES_CATEGORIZED.get(cat) or [])
                messages = pool if pool else PROMO_MESSAGES
            elif reason == "promo":
                # Bei periodischen Promos alles erlauben für maximale Varianz
                messages = PROMO_MESSAGES

        if not messages:
            return None
        return self._format_promo_template(secrets.choice(messages), invite)

    async def _send_promo_message(
        self, login: str, channel_id: str, now: float, *, reason: str
    ) -> bool:
        invite, is_specific = await self._get_promo_invite(login)
        if not invite:
            return False

        msg = self._build_promo_text(login, invite, reason=reason)
        if not msg:
            return False
        ok = await self._send_announcement(
            self._make_promo_channel(login, channel_id),
            msg,
            color="purple",
            source="promo",
        )
        if not ok:
            return False

        self._mark_promo_sent(login, now, reason=reason)

        if is_specific:
            marker = getattr(self, "_mark_streamer_invite_sent", None)
            if callable(marker):
                marker(login)
        return True

    def _has_recent_chat_activity(self, login: str, now: float) -> bool:
        msg_count, unique_chatters, _ = self._get_promo_activity_stats(login, now)
        return msg_count > 0 and unique_chatters > 0

    def _latest_chat_activity_age_sec(self, login: str, now: float) -> float | None:
        bucket = self._promo_activity.get(login)
        if bucket is None:
            return None
        self._prune_promo_activity(bucket, now)
        if len(bucket) == 0:
            return None
        last_ts = float(bucket[-1][0])
        return max(0.0, now - last_ts)

    def _get_viewer_spike_context(self, login: str) -> tuple[int, float, str, int, float] | None:
        row_sessions = None
        row_stats = None

        try:
            with readonly_connection():
                row_sessions = _pg_query_one(
                    """
                    SELECT AVG(avg_viewers) AS avg_viewers, COUNT(*) AS sample_count
                      FROM (
                            SELECT avg_viewers
                              FROM twitch_stream_sessions
                             WHERE streamer_login = %s
                               AND ended_at IS NOT NULL
                               AND avg_viewers > 0
                             ORDER BY started_at DESC
                             LIMIT %s
                      ) recent_sessions
                    """,
                    (login, int(max(1, PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT))),
                )
                row_stats = _pg_query_one(
                    """
                    SELECT AVG(viewer_count) AS avg_viewers, COUNT(*) AS sample_count
                      FROM (
                            SELECT viewer_count
                              FROM twitch_stats_tracked
                             WHERE LOWER(streamer) = %s
                               AND viewer_count > 0
                             ORDER BY ts_utc DESC
                             LIMIT %s
                      ) recent_stats
                    """,
                    (login, int(max(1, PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT))),
                )
                row_live = _pg_query_one(
                    """
                    SELECT last_viewer_count
                      FROM twitch_live_state
                     WHERE streamer_login = %s
                       AND is_live = 1
                    """,
                    (login,),
                )
        except Exception:
            log.debug(
                "Viewer-Spike-Kontext konnte für %s nicht geladen werden",
                login,
                exc_info=True,
            )
            return None

        if not row_live:
            return None

        current_viewers = int(
            (row_live["last_viewer_count"] if hasattr(row_live, "keys") else row_live[0]) or 0
        )
        if current_viewers <= 0:
            return None

        baseline = 0.0
        sample_count = 0
        source = ""
        if row_sessions is not None:
            sessions_avg = float(
                (row_sessions["avg_viewers"] if hasattr(row_sessions, "keys") else row_sessions[0])
                or 0.0
            )
            sessions_cnt = int(
                (row_sessions["sample_count"] if hasattr(row_sessions, "keys") else row_sessions[1])
                or 0
            )
            if sessions_cnt >= int(PROMO_VIEWER_SPIKE_MIN_SESSIONS) and sessions_avg > 0:
                baseline = sessions_avg
                sample_count = sessions_cnt
                source = "sessions"

        if baseline <= 0 and row_stats is not None:
            stats_avg = float(
                (row_stats["avg_viewers"] if hasattr(row_stats, "keys") else row_stats[0]) or 0.0
            )
            stats_cnt = int(
                (row_stats["sample_count"] if hasattr(row_stats, "keys") else row_stats[1]) or 0
            )
            if stats_cnt >= int(PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES) and stats_avg > 0:
                baseline = stats_avg
                sample_count = stats_cnt
                source = "tracked"

        if baseline <= 0:
            return None

        threshold = max(
            baseline * float(PROMO_VIEWER_SPIKE_MIN_RATIO),
            baseline + float(PROMO_VIEWER_SPIKE_MIN_DELTA),
        )
        return current_viewers, baseline, source, sample_count, threshold

    async def _maybe_send_promo_with_stats(self, login: str, channel_id: str, now: float) -> bool:
        if not self._promo_channel_allowed(login):
            return False
        if not self._overall_promo_ready(login, now):
            return False

        min_raw_msgs = max(0, int(PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO))
        if min_raw_msgs > 0 and self._raw_msg_count_since_last_promo(login) < min_raw_msgs:
            return False

        msg_count, unique_chatters, msgs_per_min = self._get_promo_activity_stats(login, now)
        if PROMO_ACTIVITY_MIN_MSGS > 0 and msg_count < PROMO_ACTIVITY_MIN_MSGS:
            return False
        if PROMO_ACTIVITY_MIN_CHATTERS > 0 and unique_chatters < PROMO_ACTIVITY_MIN_CHATTERS:
            return False

        last_sent = self._last_promo_sent.get(login)
        cooldown_sec = self._promo_cooldown_sec(msgs_per_min)
        if last_sent is not None and now - last_sent < cooldown_sec:
            return False

        if PROMO_NEW_CHATTERS_MIN > 0 and last_sent is not None:
            if self._get_new_chatters_in_window(login, now) < PROMO_NEW_CHATTERS_MIN:
                return False

        if not self._promo_attempt_allowed(login, now):
            return False

        ok = await self._send_promo_message(login, channel_id, now, reason="chat_activity")
        if ok:
            safe_channel_id = _sanitize_log_value(channel_id)
            log.info(
                "Chat-Promo gesendet (channel_id=%s, reason=chat_activity, activity=%d msgs/%d chatters, cooldown=%.1f min)",
                safe_channel_id,
                msg_count,
                unique_chatters,
                cooldown_sec / 60.0,
            )
        return ok

    async def _maybe_send_viewer_spike_promo(self, login: str, channel_id: str, now: float) -> bool:
        if not PROMO_VIEWER_SPIKE_ENABLED:
            return False
        if not self._promo_channel_allowed(login):
            return False
        if not self._overall_promo_ready(login, now):
            return False
        if not self._has_new_raw_chat_since_last_promo(login):
            return False
        activity_age_sec = self._latest_chat_activity_age_sec(login, now)
        if activity_age_sec is not None and activity_age_sec < float(
            PROMO_VIEWER_SPIKE_MIN_CHAT_SILENCE_SEC
        ):
            return False

        ctx = self._get_viewer_spike_context(login)
        if ctx is None:
            return False

        current_viewers, baseline, source, sample_count, threshold = ctx
        if float(current_viewers) < float(threshold):
            return False

        viewer_spike_map = getattr(self, "_last_promo_viewer_spike", None)
        if not isinstance(viewer_spike_map, dict):
            viewer_spike_map = {}
            self._last_promo_viewer_spike = viewer_spike_map

        last_viewer_promo = viewer_spike_map.get(login)
        viewer_cd_sec = float(PROMO_VIEWER_SPIKE_COOLDOWN_MIN) * 60.0
        if last_viewer_promo is not None and now - last_viewer_promo < viewer_cd_sec:
            return False

        if not self._promo_attempt_allowed(login, now):
            return False

        ok = await self._send_promo_message(login, channel_id, now, reason="viewer_spike")
        if ok:
            safe_channel_id = _sanitize_log_value(channel_id)
            log.info(
                "Chat-Promo gesendet (channel_id=%s, reason=viewer_spike, viewers=%d, baseline=%.1f, threshold=%.1f, source=%s:%d, cooldown=%.1f min)",
                safe_channel_id,
                current_viewers,
                baseline,
                threshold,
                source,
                sample_count,
                viewer_cd_sec / 60.0,
            )
        return ok

    async def _maybe_send_lurker_tax_reminder(self, login: str, channel_id: str, now: float) -> bool:
        if not SUBSCRIPTION_PLANS_ENABLED:
            return False
        if not self._overall_promo_ready(login, now):
            return False

        settings = self._load_lurker_tax_settings(login)
        if not bool(settings.get("is_live")):
            return False
        if not bool(settings.get("is_paid_plan")):
            return False
        if not bool(settings.get("enabled")):
            return False
        if not bool(settings.get("has_moderator_read_chatters")):
            return False

        session_id = settings.get("active_session_id")
        if not session_id:
            return False

        selected_logins = self._select_lurker_tax_mentions(
            login=login,
            session_id=int(session_id),
            now_utc=datetime.now(UTC),
        )
        if not selected_logins:
            return False

        message = self._build_lurker_tax_text(selected_logins)
        ok = await self._send_announcement(
            self._make_promo_channel(login, channel_id),
            message,
            color="orange",
            source="lurker_tax",
        )
        if not ok:
            return False

        self._lurker_tax_mentions_for_session(int(session_id)).update(selected_logins)
        self._mark_promo_sent(login, now, reason="lurker_tax")
        safe_channel_id = _sanitize_log_value(channel_id)
        log.info(
            "Lurker-Steuer Reminder gesendet (channel_id=%s, session=%s, mention_count=%d)",
            safe_channel_id,
            _sanitize_log_value(session_id),
            len(selected_logins),
        )
        return True

    async def _maybe_send_activity_promo(self, message) -> None:
        if not _PROMO_ACTIVITY_ENABLED:
            return

        channel = getattr(message, "channel", None)
        if channel is None:
            channel = getattr(message, "source_broadcaster", None) or getattr(
                message, "broadcaster", None
            )

        channel_name = getattr(channel, "name", "") or getattr(channel, "login", "") or ""
        login = channel_name.lstrip("#").lower()
        if not login or not self._promo_channel_allowed(login):
            return

        # WICHTIG: Promo-Messages nur für PARTNER (nicht Monitored-Only)!
        from ..core.partner_utils import is_partner_channel_for_chat_tracking

        if not is_partner_channel_for_chat_tracking(login):
            return

        if PROMO_IGNORE_COMMANDS:
            content = message.content or ""
            if content.strip().startswith(self.prefix or "!"):
                return

        author = getattr(message, "author", None)
        chatter_login = (getattr(author, "name", "") or "").lower()
        if not chatter_login:
            return

        now = time.monotonic()
        self._record_promo_activity(login, chatter_login, now)

        channel_id = getattr(channel, "id", None) or self._channel_ids.get(login)
        if not channel_id:
            return

        await self._maybe_send_promo_with_stats(login, str(channel_id), now)

    # ------------------------------------------------------------------
    # Periodische Chat-Promos
    # ------------------------------------------------------------------
    async def _periodic_promo_loop(self) -> None:
        """Hauptschleife: prüft alle X Sekunden, ob eine Promo gesendet werden soll."""
        loop_interval_sec = max(15, int(PROMO_LOOP_INTERVAL_SEC))
        self._restore_promo_cooldowns()
        try:
            while True:
                await asyncio.sleep(loop_interval_sec)
                try:
                    await self._send_promo_if_due()
                except Exception:
                    log.debug("_send_promo_if_due fehlgeschlagen", exc_info=True)
        except asyncio.CancelledError:
            log.info("Chat-Promo-Loop wurde abgebrochen")

    async def _send_promo_if_due(self) -> None:
        """Sendet eine Promo in jeden live-Kanal, für den das Intervall abgelaufen ist."""
        now = time.monotonic()
        self._prune_promo_runtime_state(now)
        live_channels = await self._get_live_channels_for_promo()
        lurker_tax_channels = await self._get_live_channels_for_lurker_tax()

        from ..core.partner_utils import is_partner_channel_for_chat_tracking

        for login, broadcaster_id in lurker_tax_channels:
            if not is_partner_channel_for_chat_tracking(login):
                continue
            await self._maybe_send_lurker_tax_reminder(login, str(broadcaster_id), now)

        if _PROMO_ACTIVITY_ENABLED or PROMO_VIEWER_SPIKE_ENABLED:
            for login, broadcaster_id in live_channels:
                if not self._promo_channel_allowed(login):
                    continue
                if not is_partner_channel_for_chat_tracking(login):
                    continue
                sent = False
                if _PROMO_ACTIVITY_ENABLED:
                    sent = await self._maybe_send_promo_with_stats(login, str(broadcaster_id), now)
                if not sent and PROMO_VIEWER_SPIKE_ENABLED:
                    await self._maybe_send_viewer_spike_promo(login, str(broadcaster_id), now)
            return

        interval_sec = max(_PROMO_INTERVAL_MIN * 60, self._overall_promo_cooldown_sec())
        for login, broadcaster_id in live_channels:
            if not is_partner_channel_for_chat_tracking(login):
                continue
            last = self._last_promo_sent.get(login)
            if last is None:
                self._last_promo_sent[login] = now
                try:
                    from ..storage import save_promo_cooldown
                    save_promo_cooldown(login, "sent", time.time())
                except Exception:
                    pass
                continue

            if now - last < interval_sec:
                continue

            invite, is_specific = await self._get_promo_invite(login)
            if not invite:
                continue

            msg = self._build_promo_text(login, invite)
            if not msg:
                continue

            class _Channel:
                __slots__ = ("name", "id")

                def __init__(self, name: str, channel_id: str):
                    self.name = name
                    self.id = channel_id

            ok = await self._send_chat_message(
                _Channel(login, broadcaster_id),
                msg,
                source="promo",
            )
            if ok:
                self._mark_promo_sent(login, now, reason="promo")
                if is_specific:
                    marker = getattr(self, "_mark_streamer_invite_sent", None)
                    if callable(marker):
                        marker(login)
                log.info(
                    "Chat-Promo gesendet (channel_id=%s)",
                    _sanitize_log_value(broadcaster_id),
                )
            else:
                log.debug(
                    "Chat-Promo fehlgeschlagen (channel_id=%s)",
                    _sanitize_log_value(broadcaster_id),
                )

    async def _get_live_channels_for_lurker_tax(self) -> list[tuple[str, str]]:
        """Gibt alle live-Kanäle mit aktiver Session zurück, in denen die Lurker Steuer laufen kann."""
        if not self._channel_ids:
            return []

        allowed_logins = {str(login).lower() for login in self._channel_ids if login}
        if not allowed_logins:
            return []

        try:
            rows = _pg_query_all(
                """
                SELECT streamer_login, twitch_user_id
                  FROM twitch_live_state
                 WHERE is_live = 1
                   AND active_session_id IS NOT NULL
                """
            )
        except Exception:
            log.debug("_get_live_channels_for_lurker_tax: DB-Query fehlgeschlagen", exc_info=True)
            return []

        channels: list[tuple[str, str]] = []
        for row in rows:
            login = str(
                (row["streamer_login"] if hasattr(row, "keys") else row[0]) or ""
            ).strip().lower()
            broadcaster_id = str(
                (row["twitch_user_id"] if hasattr(row, "keys") else row[1]) or ""
            ).strip()
            if not login or not broadcaster_id:
                continue
            if login in allowed_logins:
                channels.append((login, broadcaster_id))
        return channels

    async def _get_live_channels_for_promo(self) -> list[tuple[str, str]]:
        """Gibt alle live-Kanäle zurück, in denen der Bot aktiv ist (login, broadcaster_id)."""
        if not self._channel_ids:
            return []

        allowed_logins = {str(login).lower() for login in self._channel_ids if login}
        if not allowed_logins:
            return []

        target_game_lower = (getattr(self, "_target_game_lower", None) or "deadlock").strip().lower()

        try:
            with readonly_connection():
                if SUBSCRIPTION_PLANS_ENABLED:
                    rows = _pg_query_all(
                        """
                        SELECT s.twitch_login, s.twitch_user_id
                          FROM twitch_streamer_identities s
                          JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
                          LEFT JOIN streamer_plans p ON s.twitch_user_id = p.twitch_user_id
                         WHERE l.is_live = 1
                            AND LOWER(COALESCE(l.last_game, '')) = %s
                           AND COALESCE(p.promo_disabled, 0) = 0
                        """,
                        (target_game_lower,),
                    )
                else:
                    rows = _pg_query_all(
                        """
                        SELECT s.twitch_login, s.twitch_user_id
                          FROM twitch_streamer_identities s
                          JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
                         WHERE l.is_live = 1
                            AND LOWER(COALESCE(l.last_game, '')) = %s
                        """,
                        (target_game_lower,),
                    )
        except Exception:
            log.debug("_get_live_channels_for_promo: DB-Query fehlgeschlagen", exc_info=True)
            return []

        channels: list[tuple[str, str]] = []
        for row in rows:
            if not row[0] or not row[1]:
                continue
            login = str(row[0]).lower()
            if login in allowed_logins:
                channels.append((login, str(row[1])))
        return channels
