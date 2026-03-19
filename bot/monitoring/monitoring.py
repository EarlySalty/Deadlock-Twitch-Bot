"""Background polling and monitoring helpers for Twitch streams."""

from __future__ import annotations

import asyncio
import hashlib
import inspect
import json
import os
import time
from datetime import UTC, datetime, timedelta
from ipaddress import ip_address
from urllib.parse import urlsplit, urlunsplit

import aiohttp
import discord
from discord.ext import tasks

from .. import storage
from ..api.twitch_auth import TwitchClientConfigError
from ..core.constants import (
    INVITES_REFRESH_INTERVAL_HOURS,
    POLL_INTERVAL_SECONDS,
    TWITCH_TARGET_GAME_NAME,
    TWITCH_VOD_BUTTON_LABEL,
    log,
)
from .embeds_mixin import _EmbedsMixin
from .eventsub_mixin import _EventSubMixin
from .exp_sessions_mixin import _ExpSessionsMixin
from .sessions_mixin import _SessionsMixin


class TwitchMonitoringMixin(_EventSubMixin, _ExpSessionsMixin, _SessionsMixin, _EmbedsMixin):
    """Polling loops and helpers used by the Twitch cog."""

    @staticmethod
    def _env_truthy(value: str | None) -> bool:
        return str(value or "").strip().lower() in {"1", "true", "yes", "y", "on"}

    @staticmethod
    def _parse_env_int(name: str, default: int) -> int:
        raw = (os.getenv(name) or "").strip()
        if not raw:
            return int(default)
        try:
            parsed = int(raw)
        except ValueError:
            return int(default)
        return parsed if parsed > 0 else int(default)

    def _master_broker_token(self) -> str | None:
        for key in (
            "MASTER_BROKER_TOKEN",
            "MAIN_BOT_INTERNAL_TOKEN",
            "TWITCH_INTERNAL_API_TOKEN",
        ):
            value = (os.getenv(key) or "").strip()
            if value:
                return value
        return None

    @staticmethod
    def _is_loopback_host(host: str) -> bool:
        normalized = str(host or "").strip().lower().rstrip(".")
        if not normalized:
            return False
        if normalized == "localhost":
            return True
        try:
            return ip_address(normalized).is_loopback
        except ValueError:
            return False

    @classmethod
    def _normalize_master_broker_base_url(cls, value: str) -> str:
        raw = (value or "").strip()
        if not raw:
            raise ValueError("base_url is required")
        if "://" not in raw:
            raw = f"http://{raw}"

        try:
            parsed = urlsplit(raw)
        except Exception as exc:
            raise ValueError("base_url is invalid") from exc
        if not parsed.netloc:
            raise ValueError("base_url is invalid")
        if parsed.username or parsed.password:
            raise ValueError("base_url must not contain credentials")

        scheme = (parsed.scheme or "http").lower()
        if scheme not in {"http", "https"}:
            raise ValueError("base_url must use http or https")

        host = (parsed.hostname or "").strip()
        if not host or not cls._is_loopback_host(host):
            raise ValueError("base_url host must resolve to loopback")

        try:
            port = parsed.port
        except ValueError as exc:
            raise ValueError("base_url is invalid") from exc
        host_for_netloc = f"[{host}]" if ":" in host else host
        normalized_netloc = f"{host_for_netloc}:{port}" if port is not None else host_for_netloc
        normalized_path = (parsed.path or "").rstrip("/")
        return urlunsplit((scheme, normalized_netloc, normalized_path, "", ""))

    def _master_broker_base_url(self) -> str:
        explicit_base = (os.getenv("MASTER_BROKER_BASE_URL") or "").strip()
        if explicit_base:
            return self._normalize_master_broker_base_url(explicit_base)
        host = (os.getenv("MASTER_BROKER_HOST") or "127.0.0.1").strip() or "127.0.0.1"
        port = self._parse_env_int("MASTER_BROKER_PORT", 8770)
        return self._normalize_master_broker_base_url(f"http://{host}:{port}")

    def _announcement_transport_prefers_master_broker(self) -> bool:
        return self._master_broker_token() is not None

    def _announcement_transport_can_use_direct_discord(self) -> bool:
        if self._announcement_transport_prefers_master_broker():
            return False

        try:
            from ..runtime_mode import ROLE_TWITCH_WORKER, resolve_runtime_role

            if resolve_runtime_role() == ROLE_TWITCH_WORKER:
                return False
        except Exception:
            log.debug("Could not resolve Twitch runtime role for announcement transport", exc_info=True)

        return self.bot.__class__.__name__ != "HeadlessBot"

    def _announcement_transport_ready(
        self,
        *,
        channel_id: int | None,
        notify_channel: discord.TextChannel | None,
    ) -> bool:
        if not channel_id:
            return False
        if self._announcement_transport_prefers_master_broker():
            return True
        return self._announcement_transport_can_use_direct_discord() and notify_channel is not None

    @staticmethod
    def _build_announcement_idempotency_key(
        *,
        action: str,
        login: str,
        discriminator: str,
    ) -> str:
        raw = f"{action}|{str(login or '').strip().lower()}|{str(discriminator or '').strip()}"
        digest = hashlib.sha256(raw.encode("utf-8")).hexdigest()[:48]
        return f"twitch-{action}-{digest}"

    @staticmethod
    def _build_live_announcement_tracking_token(
        *,
        login: str,
        stream_id: str | None,
        started_at: str | None,
        fallback_title: str | None = None,
    ) -> str:
        raw = (
            f"{str(login or '').strip().lower()}|{str(stream_id or '').strip()}|"
            f"{str(started_at or '').strip()}|{str(fallback_title or '').strip()}"
        )
        return hashlib.sha256(raw.encode("utf-8")).hexdigest()[:16]

    @staticmethod
    def _is_same_live_announcement_stream(
        previous_state: dict[str, object],
        *,
        stream_id: str | None,
        started_at: str | None,
    ) -> bool:
        previous_stream_id = str(previous_state.get("last_stream_id") or "").strip()
        if stream_id and previous_stream_id:
            return previous_stream_id == stream_id
        previous_started_at = str(previous_state.get("last_started_at") or "").strip()
        if started_at and previous_started_at:
            return previous_started_at == started_at
        return False

    def _live_announcement_retry_cache(self) -> dict[str, dict[str, object]]:
        cache = getattr(self, "_live_announcement_retry_state", None)
        if isinstance(cache, dict):
            return cache
        cache = {}
        self._live_announcement_retry_state = cache
        return cache

    def _clear_live_announcement_retry_payload(self, login: str) -> None:
        login_key = str(login or "").strip().lower()
        if not login_key:
            return
        self._live_announcement_retry_cache().pop(login_key, None)

    @staticmethod
    def _parse_live_announcement_render_now(value: str | None) -> datetime | None:
        raw = str(value or "").strip()
        if not raw:
            return None
        try:
            parsed = datetime.fromisoformat(raw.replace("Z", "+00:00"))
        except ValueError:
            return None
        if parsed.tzinfo is None:
            return parsed.replace(tzinfo=UTC)
        return parsed.astimezone(UTC)

    def _resolve_live_announcement_retry_payload(
        self,
        *,
        login: str,
        stream: dict[str, object],
        previous_state: dict[str, object],
        stream_id: str | None,
        started_at: str | None,
        message_id: str | None,
        rendered_at: str,
    ) -> tuple[dict[str, object], str, datetime | None]:
        login_key = str(login or "").strip().lower()
        marker_raw = (
            f"{login_key}|{str(stream_id or '').strip()}|{str(started_at or '').strip()}"
        )
        marker = hashlib.sha256(marker_raw.encode("utf-8")).hexdigest()[:32]
        cached_state = self._live_announcement_retry_cache().get(login_key)
        cached_rendered_at_for_retry = None
        if not message_id and isinstance(cached_state, dict):
            cached_marker = str(cached_state.get("marker") or "").strip()
            cached_stream = cached_state.get("stream")
            cached_token = str(cached_state.get("tracking_token") or "").strip()
            cached_rendered_at = str(cached_state.get("rendered_at") or "").strip()
            if cached_marker == marker and isinstance(cached_stream, dict) and cached_token:
                # Cache hit on retry: use cached timestamp to maintain idempotency
                return (
                    dict(cached_stream),
                    cached_token,
                    self._parse_live_announcement_render_now(cached_rendered_at),
                )
            # Cache miss (marker mismatch): preserve timestamp for idempotency
            cached_rendered_at_for_retry = cached_rendered_at

        announcement_stream = dict(stream)
        render_now_value = str(rendered_at or "").strip()

        # On retry: use cached timestamp if available to ensure payload consistency
        if cached_rendered_at_for_retry and not render_now_value:
            render_now_value = cached_rendered_at_for_retry

        if (
            not message_id
            and self._is_same_live_announcement_stream(
                previous_state,
                stream_id=stream_id,
                started_at=started_at,
            )
        ):
            previous_title = str(previous_state.get("last_title") or "").strip()
            if previous_title:
                announcement_stream["title"] = previous_title
            previous_game = str(previous_state.get("last_game") or "").strip()
            if previous_game:
                announcement_stream["game_name"] = previous_game
            if previous_state.get("last_viewer_count") is not None:
                try:
                    announcement_stream["viewer_count"] = int(
                        previous_state.get("last_viewer_count") or 0
                    )
                except (TypeError, ValueError):
                    pass
            previous_started_at = str(previous_state.get("last_started_at") or "").strip()
            if previous_started_at:
                announcement_stream["started_at"] = previous_started_at
            previous_rendered_at = str(previous_state.get("last_seen_at") or "").strip()
            if previous_rendered_at:
                render_now_value = previous_rendered_at

        tracking_token = self._build_live_announcement_tracking_token(
            login=login_key,
            stream_id=stream_id,
            started_at=str(announcement_stream.get("started_at") or started_at or ""),
            fallback_title=str(announcement_stream.get("title") or ""),
        )
        # Cache new announcement: preserve timestamp across retries for idempotency.
        # When retrying, cache hit (above) returns early with cached timestamp.
        # Here we only write cache on first attempt (cache miss).
        self._live_announcement_retry_cache()[login_key] = {
            "marker": marker,
            "stream": dict(announcement_stream),
            "tracking_token": tracking_token,
            "rendered_at": render_now_value,
        }
        return (
            announcement_stream,
            tracking_token,
            self._parse_live_announcement_render_now(render_now_value),
        )

    async def _post_master_broker_json(
        self,
        *,
        path: str,
        payload: dict[str, object],
        idempotency_key: str,
    ) -> dict[str, object] | None:
        token = self._master_broker_token()
        if not token:
            return None

        base_url = self._master_broker_base_url()
        url = f"{base_url}{path}"
        headers = {
            "X-Internal-Token": token,
            "X-Idempotency-Key": idempotency_key,
        }
        timeout = aiohttp.ClientTimeout(total=10)

        try:
            async with aiohttp.ClientSession(timeout=timeout) as session:
                async with session.post(
                    url,
                    headers=headers,
                    json=payload,
                    allow_redirects=False,
                ) as response:
                    raw_text = await response.text()
                    try:
                        parsed = json.loads(raw_text) if raw_text.strip() else {}
                    except Exception:
                        parsed = {}
                    if response.status >= 400:
                        log.warning(
                            "Master broker request failed (%s %s): status=%s body=%s",
                            path,
                            idempotency_key,
                            response.status,
                            raw_text[:500],
                        )
                        return None
        except asyncio.TimeoutError:
            log.warning("Master broker request timed out for %s", path)
            return None
        except Exception:
            log.exception("Master broker request failed for %s", path)
            return None

        if not isinstance(parsed, dict) or not parsed.get("ok"):
            log.warning(
                "Master broker returned invalid response for %s: %r",
                path,
                parsed,
            )
            return None

        result = parsed.get("result")
        return result if isinstance(result, dict) else {}

    async def _send_live_announcement_via_broker(
        self,
        *,
        channel_id: int,
        login: str,
        stream_id: str | None,
        content: str,
        embed: discord.Embed,
        allowed_role_ids: list[int],
        view_spec: dict[str, str] | None,
    ) -> str | None:
        result = await self._post_master_broker_json(
            path="/internal/master/v1/discord/send-rich-message",
            payload={
                "channel_id": int(channel_id),
                "content": content or None,
                "embed": embed.to_dict(),
                "allowed_role_ids": allowed_role_ids,
                "view_spec": view_spec,
            },
            idempotency_key=self._build_announcement_idempotency_key(
                action="live-send",
                login=login,
                discriminator=str(stream_id or content or ""),
            ),
        )
        if not result:
            return None

        message_id = result.get("message_id")
        normalized_message_id = str(message_id or "").strip()
        return normalized_message_id if normalized_message_id else None

    async def _edit_live_announcement_via_broker(
        self,
        *,
        channel_id: int,
        login: str,
        message_id: str,
        content: str,
        embed: discord.Embed,
        view_spec: dict[str, str] | None,
    ) -> bool:
        result = await self._post_master_broker_json(
            path="/internal/master/v1/discord/edit-rich-message",
            payload={
                "channel_id": int(channel_id),
                "message_id": str(message_id),
                "content": content or None,
                "embed": embed.to_dict(),
                "view_spec": view_spec,
            },
            idempotency_key=self._build_announcement_idempotency_key(
                action="live-edit",
                login=login,
                discriminator=str(message_id),
            ),
        )
        return result is not None

    def _partner_raid_score_refresh_interval_seconds(self) -> float:
        return 300.0

    @staticmethod
    def _partner_raid_score_refresh_preferred_names(*, full_refresh: bool) -> tuple[str, ...]:
        if full_refresh:
            return (
                "refresh_all_partner_raid_scores_async",
                "refresh_all_partner_raid_score_caches_async",
                "refresh_partner_raid_score_cache_async",
                "refresh_partner_raid_scores_async",
                "refresh_all_partner_raid_scores",
                "refresh_all_partner_raid_score_caches",
                "refresh_partner_raid_score_cache",
                "refresh_partner_raid_scores",
            )
        return (
            "refresh_partner_raid_score_cache_async",
            "refresh_partner_raid_scores_async",
            "refresh_partner_raid_score_async",
            "refresh_partner_raid_score_cache",
            "refresh_partner_raid_scores",
            "refresh_partner_raid_score",
        )

    def _partner_raid_score_refresh_candidates(self) -> list[object]:
        candidates: list[object] = []
        for candidate in (
            getattr(self, "partner_raid_score_service", None),
            getattr(getattr(self, "_raid_bot", None), "partner_raid_score_service", None),
            getattr(self, "_raid_bot", None),
            self,
        ):
            if candidate is None or candidate in candidates:
                continue
            candidates.append(candidate)
        return candidates

    @staticmethod
    def _build_partner_raid_score_refresh_kwargs(
        func,
        *,
        twitch_user_id: str | None,
        login: str | None,
        trigger: str,
        full_refresh: bool,
    ) -> dict[str, object]:
        try:
            params = inspect.signature(func).parameters
        except (TypeError, ValueError):
            params = {}

        kwargs: dict[str, object] = {}
        for name in params:
            if name in {"self", "cls"}:
                continue
            if name in {"twitch_user_id", "broadcaster_user_id", "user_id", "partner_user_id"}:
                if twitch_user_id:
                    kwargs[name] = twitch_user_id
                continue
            if name in {"login", "twitch_login", "broadcaster_login", "partner_login"}:
                if login:
                    kwargs[name] = login
                continue
            if name in {"trigger", "reason", "source"}:
                kwargs[name] = trigger
                continue
            if name in {"full_refresh", "bulk", "refresh_all"}:
                kwargs[name] = full_refresh
                continue
            if name in {"immediate", "force"}:
                kwargs[name] = True
                continue
        return kwargs

    async def _request_partner_raid_score_refresh(
        self,
        *,
        twitch_user_id: str | None = None,
        login: str | None = None,
        trigger: str,
        full_refresh: bool = False,
    ) -> bool:
        preferred_names = self._partner_raid_score_refresh_preferred_names(
            full_refresh=full_refresh
        )

        for candidate in self._partner_raid_score_refresh_candidates():
            for name in preferred_names:
                func = getattr(candidate, name, None)
                if not callable(func):
                    continue
                try:
                    func_params = inspect.signature(func).parameters
                except (TypeError, ValueError):
                    func_params = {}
                kwargs = self._build_partner_raid_score_refresh_kwargs(
                    func,
                    twitch_user_id=twitch_user_id,
                    login=login,
                    trigger=trigger,
                    full_refresh=full_refresh,
                )
                if not full_refresh and not twitch_user_id:
                    required_id_names = {
                        "twitch_user_id",
                        "broadcaster_user_id",
                        "user_id",
                        "partner_user_id",
                    }
                    if any(required_name in func_params for required_name in required_id_names):
                        continue
                try:
                    if inspect.iscoroutinefunction(func):
                        result = await func(**kwargs)
                    else:
                        result = await asyncio.to_thread(func, **kwargs)
                except TypeError:
                    continue
                if inspect.isawaitable(result):
                    await result
                return True

        log.debug(
            "Partner raid score refresh skipped: no refresh service available (trigger=%s, user_id=%s, full=%s)",
            trigger,
            twitch_user_id or login or "",
            full_refresh,
        )
        return False

    async def _run_partner_raid_score_refresh_task(
        self,
        *,
        task_key: str,
        twitch_user_id: str | None,
        login: str | None,
        trigger: str,
        full_refresh: bool,
    ) -> None:
        try:
            await self._request_partner_raid_score_refresh(
                twitch_user_id=twitch_user_id,
                login=login,
                trigger=trigger,
                full_refresh=full_refresh,
            )
        except Exception:
            log.exception(
                "Partner raid score refresh failed (trigger=%s, user_id=%s, full=%s)",
                trigger,
                twitch_user_id or login or "",
                full_refresh,
            )
        finally:
            pending = getattr(self, "_partner_raid_score_refresh_pending", None)
            if isinstance(pending, set):
                pending.discard(task_key)

    def _schedule_partner_raid_score_refresh(
        self,
        *,
        twitch_user_id: str | None = None,
        login: str | None = None,
        trigger: str,
        full_refresh: bool = False,
    ) -> bool:
        key = f"all:{trigger}" if full_refresh else str(twitch_user_id or login or "").strip().lower()
        if not key:
            return False

        pending = getattr(self, "_partner_raid_score_refresh_pending", None)
        if not isinstance(pending, set):
            pending = set()
            self._partner_raid_score_refresh_pending = pending
        if key in pending:
            return False
        pending.add(key)

        task = self._run_partner_raid_score_refresh_task(
            task_key=key,
            twitch_user_id=twitch_user_id,
            login=login,
            trigger=trigger,
            full_refresh=full_refresh,
        )
        spawn = getattr(self, "_spawn_bg_task", None)
        if callable(spawn):
            spawn(task, f"partner_raid_score_refresh.{key}")
        else:
            asyncio.create_task(task, name=f"partner_raid_score_refresh.{key}")
        return True

    def _schedule_partner_raid_score_refreshes(
        self,
        refreshes: list[tuple[str, str | None, str]],
    ) -> int:
        scheduled = 0
        seen: set[str] = set()
        for twitch_user_id, login, trigger in refreshes:
            user_id = str(twitch_user_id or "").strip()
            dedupe_key = user_id or str(login or "").strip().lower()
            if not dedupe_key or dedupe_key in seen:
                continue
            seen.add(dedupe_key)
            if self._schedule_partner_raid_score_refresh(
                twitch_user_id=user_id or None,
                login=login,
                trigger=trigger,
            ):
                scheduled += 1
        return scheduled

    def _maybe_schedule_partner_raid_score_reconciliation(self, *, trigger: str) -> bool:
        now = time.monotonic()
        interval = self._partner_raid_score_refresh_interval_seconds()
        last_run = float(
            getattr(self, "_partner_raid_score_reconciliation_last_monotonic", 0.0) or 0.0
        )
        if last_run and (now - last_run) < max(60.0, interval):
            return False
        self._partner_raid_score_reconciliation_last_monotonic = now
        return self._schedule_partner_raid_score_refresh(
            trigger=trigger,
            full_refresh=True,
        )

    @staticmethod
    def _normalize_poll_interval_seconds(raw_value: object) -> int | None:
        raw_text = str(raw_value or "").strip()
        if not raw_text:
            return None
        try:
            value = int(raw_text)
        except (TypeError, ValueError):
            return None
        if value < 5 or value > 3600:
            return None
        return value

    def _default_poll_interval_seconds(self) -> int:
        default_value = self._normalize_poll_interval_seconds(POLL_INTERVAL_SECONDS)
        return default_value if default_value is not None else 15

    def _poll_interval_debug(
        self,
        message: str,
        *args: object,
        exc_info: bool = False,
    ) -> None:
        now = time.monotonic()
        last_logged = float(getattr(self, "_poll_interval_last_error_log_at", 0.0) or 0.0)
        if last_logged and (now - last_logged) < 300.0:
            return
        self._poll_interval_last_error_log_at = now
        log.debug(message, *args, exc_info=exc_info)

    def _ensure_poll_interval_settings_storage(self, conn) -> None:
        conn.execute(
            """
            CREATE TABLE IF NOT EXISTS twitch_global_settings (
                setting_key TEXT PRIMARY KEY,
                setting_value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
            """
        )
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_twitch_global_settings_updated_at "
            "ON twitch_global_settings(updated_at)"
        )

    def _read_persisted_poll_interval_seconds(self) -> int | None:
        table_name = str(
            getattr(self, "_poll_interval_settings_table", "twitch_global_settings")
            or "twitch_global_settings"
        ).strip()
        setting_key = str(
            getattr(self, "_poll_interval_settings_key", "poll_interval_seconds")
            or "poll_interval_seconds"
        ).strip()

        try:
            with storage.get_conn() as conn:
                self._ensure_poll_interval_settings_storage(conn)
                row = conn.execute(
                    f"SELECT setting_value FROM {table_name} WHERE setting_key = ? LIMIT 1",
                    (setting_key,),
                ).fetchone()
        except Exception:
            self._poll_interval_debug(
                "Polling-Intervall: Konnte persistente Einstellung nicht lesen",
                exc_info=True,
            )
            return None

        if row is None:
            self._poll_interval_last_invalid_value = None
            return None

        if hasattr(row, "get"):
            raw_value = row.get("setting_value")
        else:
            raw_value = row[0] if row else None

        normalized = self._normalize_poll_interval_seconds(raw_value)
        if normalized is not None:
            self._poll_interval_last_invalid_value = None
            return normalized

        invalid_marker = str(raw_value or "").strip() or "<empty>"
        if invalid_marker != getattr(self, "_poll_interval_last_invalid_value", None):
            self._poll_interval_last_invalid_value = invalid_marker
            log.debug(
                "Polling-Intervall: Ungueltiger DB-Wert %r, verwende Fallback %ss",
                invalid_marker,
                self._default_poll_interval_seconds(),
            )
        return None

    def _apply_poll_interval_seconds(self, seconds: int, *, reason: str) -> int:
        normalized = self._normalize_poll_interval_seconds(seconds)
        target_seconds = normalized if normalized is not None else self._default_poll_interval_seconds()
        current_seconds = self._normalize_poll_interval_seconds(
            getattr(self, "_poll_interval_seconds", self._default_poll_interval_seconds())
        )
        current_seconds = (
            current_seconds if current_seconds is not None else self._default_poll_interval_seconds()
        )

        if current_seconds == target_seconds:
            self._poll_interval_seconds = target_seconds
            self._admin_polling_interval_seconds = target_seconds
            return target_seconds

        self.poll_streams.change_interval(seconds=target_seconds)
        self._poll_interval_seconds = target_seconds
        self._admin_polling_interval_seconds = target_seconds

        if reason == "startup":
            log.info("Polling-Intervall initialisiert auf %ss", target_seconds)
        else:
            log.info("Polling-Intervall geaendert auf %ss (%s)", target_seconds, reason)
        return target_seconds

    def _sync_poll_interval_from_storage(
        self,
        *,
        force: bool = False,
        startup: bool = False,
    ) -> int:
        now = time.monotonic()
        resync_interval = float(
            getattr(self, "_poll_interval_resync_interval_seconds", 60.0) or 60.0
        )
        last_sync = float(getattr(self, "_poll_interval_last_sync_monotonic", 0.0) or 0.0)
        if not force and last_sync and (now - last_sync) < max(15.0, resync_interval):
            current_seconds = self._normalize_poll_interval_seconds(
                getattr(self, "_poll_interval_seconds", self._default_poll_interval_seconds())
            )
            return current_seconds if current_seconds is not None else self._default_poll_interval_seconds()

        self._poll_interval_last_sync_monotonic = now
        persisted_seconds = self._read_persisted_poll_interval_seconds()
        target_seconds = (
            persisted_seconds if persisted_seconds is not None else self._default_poll_interval_seconds()
        )
        reason = "startup" if startup else "storage_resync"
        if persisted_seconds is None and target_seconds == self._default_poll_interval_seconds():
            reason = "fallback_default"
        return self._apply_poll_interval_seconds(target_seconds, reason=reason)

    @staticmethod
    def _reauth_chat_reminder_text() -> str:
        return (
            "Kurze Erinnerung: Fuer den Raid-/Stats-Bot fehlt noch die neue Twitch-Autorisierung. "
            "Bitte im Dashboard einloggen und Twitch neu verbinden. "
            "Falls du die DM brauchst: Der Re-Auth-Link wurde dir bereits auf Discord geschickt."
        )

    async def _resolve_live_stream_id_for_login(self, login_lower: str) -> str | None:
        if not login_lower or not getattr(self, "api", None):
            return None
        try:
            streams = await self.api.get_streams_by_logins([login_lower])
            if not streams:
                return None
            stream_id = str((streams[0] or {}).get("id") or "").strip()
            return stream_id or None
        except Exception:
            log.debug(
                "ReAuth reminder: Konnte aktuelle stream_id nicht laden für %s",
                login_lower,
                exc_info=True,
            )
            return None

    async def _maybe_send_reauth_chat_reminder(
        self,
        *,
        chat_bot,
        broadcaster_id: str,
        login_lower: str,
    ) -> bool:
        """Sendet beim Streamstart einmalig eine freundliche Re-Auth-Erinnerung in den Twitch-Chat."""
        if not chat_bot or not broadcaster_id or not login_lower:
            return False

        broadcaster_key = str(broadcaster_id).strip()
        login_key = str(login_lower).strip().lower()
        if not broadcaster_key or not login_key:
            return False

        # Primärer Dedupe über stream_id (pro Streamstart genau eine Nachricht).
        stream_id = await self._resolve_live_stream_id_for_login(login_key)
        stream_guard = getattr(self, "_reauth_reminder_last_stream_id", None)
        if not isinstance(stream_guard, dict):
            stream_guard = {}
            self._reauth_reminder_last_stream_id = stream_guard
        if stream_id:
            if stream_guard.get(broadcaster_key) == stream_id:
                return False
            # Guard VOR dem Senden setzen – verhindert Doppel-Trigger durch
            # gleichzeitige EventSub- und Polling-Pfade (race condition fix).
            stream_guard[broadcaster_key] = stream_id
        else:
            # Fallback-Dedupe, falls stream_id temporär nicht geladen werden kann.
            fallback_guard = getattr(self, "_reauth_reminder_last_sent_ts", None)
            if not isinstance(fallback_guard, dict):
                fallback_guard = {}
                self._reauth_reminder_last_sent_ts = fallback_guard
            now_ts = time.time()
            last_ts = float(fallback_guard.get(broadcaster_key) or 0.0)
            if now_ts - last_ts < 300.0:
                return False
            fallback_guard[broadcaster_key] = now_ts

        send_chat = getattr(chat_bot, "_send_chat_message", None)
        if not callable(send_chat):
            return False

        make_channel = getattr(chat_bot, "_make_promo_channel", None)
        if callable(make_channel):
            channel = make_channel(login_key, broadcaster_key)
        else:

            class _Channel:
                __slots__ = ("name", "id")

                def __init__(self, name: str, cid: str):
                    self.name = name
                    self.id = cid

            channel = _Channel(login_key, broadcaster_key)

        ok = await send_chat(
            channel,
            self._reauth_chat_reminder_text(),
            source="migration_reminder",
        )
        if ok:
            log.info(
                "ReAuth reminder: Chat-Hinweis bei Streamstart gesendet für %s (%s)",
                login_key,
                broadcaster_key,
            )
        return bool(ok)

    def _get_target_game_lower(self) -> str:
        target = getattr(self, "_target_game_lower", None)
        if isinstance(target, str) and target:
            return target
        resolved = (TWITCH_TARGET_GAME_NAME or "").strip().lower()
        # Cache for subsequent lookups to avoid repeated normalization
        self._target_game_lower = resolved
        return resolved

    def _stream_is_in_target_category(self, stream: dict | None) -> bool:
        if not stream:
            return False
        target_game_lower = self._get_target_game_lower()
        if not target_game_lower:
            return False
        game_name = (stream.get("game_name") or "").strip().lower()
        return game_name == target_game_lower

    @staticmethod
    def _normalize_stream_meta(
        stream: dict,
    ) -> tuple[str | None, str | None, str | None]:
        game_name = (stream.get("game_name") or "").strip() or None
        stream_title = (stream.get("title") or "").strip() or None

        tags_raw = stream.get("tags")
        tags_serialized: str | None = None
        if isinstance(tags_raw, list):
            clean_tags = [str(tag).strip() for tag in tags_raw if str(tag).strip()]
            if clean_tags:
                tags_serialized = json.dumps(clean_tags, ensure_ascii=True, separators=(",", ":"))
        elif isinstance(tags_raw, str):
            tag_value = tags_raw.strip()
            if tag_value:
                tags_serialized = tag_value

        return game_name, stream_title, tags_serialized

    def _language_filter_values(self) -> list[str | None]:
        filters: list[str] | None = getattr(self, "_language_filters", None)
        if not filters:
            return [None]
        seen: list[str] = []
        for entry in filters:
            normalized = (entry or "").strip().lower()
            if not normalized or normalized in seen:
                continue
            seen.append(normalized)
        return [*seen] or [None]

    @tasks.loop(seconds=POLL_INTERVAL_SECONDS)
    async def poll_streams(self):
        try:
            self._sync_poll_interval_from_storage()
        except Exception:
            self._poll_interval_debug(
                "Polling-Intervall: Runtime-Resync fehlgeschlagen",
                exc_info=True,
            )
        if self.api is None:
            return
        try:
            await self._tick()
        except asyncio.CancelledError:
            raise
        except Exception:
            log.exception("Polling-Tick fehlgeschlagen")

    @poll_streams.before_loop
    async def _before_poll(self):
        await self.bot.wait_until_ready()
        try:
            self._sync_poll_interval_from_storage(force=True, startup=True)
        except Exception:
            self._poll_interval_debug(
                "Polling-Intervall: Start-Resync fehlgeschlagen",
                exc_info=True,
            )

    @tasks.loop(hours=INVITES_REFRESH_INTERVAL_HOURS)
    async def invites_refresh(self):
        try:
            await self._refresh_all_invites()
        except asyncio.CancelledError:
            raise
        except Exception:
            log.exception("Invite-Refresh fehlgeschlagen")

    @invites_refresh.before_loop
    async def _before_invites(self):
        await self.bot.wait_until_ready()

    async def _ensure_category_id(self):
        if self.api is None:
            return
        try:
            self._category_id = await self.api.get_category_id(TWITCH_TARGET_GAME_NAME)
            if self._category_id:
                log.debug("Deadlock category_id = %s", self._category_id)
        except TwitchClientConfigError:
            return
        except Exception:
            log.exception("Konnte Twitch-Kategorie-ID nicht ermitteln")

    def _load_tracked_streamers(self) -> tuple[list[dict[str, object]], set[str]]:
        """Load all channels the monitoring pipeline is responsible for tracking.

        Session writes stay centralized in monitoring/sessions_mixin. This loader
        therefore includes monitored-only channels here instead of opening
        sessions from any secondary path such as the chat bot.
        """
        partner_logins: set[str] = set()
        tracked: list[dict[str, object]] = []
        try:
            with storage.get_conn() as c:
                rows = c.execute(
                    """
                    SELECT twitch_login, twitch_user_id, require_discord_link,
                           archived_at::text AS archived_at, is_partner, discord_user_id,
                           live_ping_role_id, COALESCE(live_ping_enabled, 1) AS live_ping_enabled
                      FROM twitch_streamers_partner_state
                    UNION ALL
                    SELECT s.twitch_login, s.twitch_user_id, s.require_discord_link,
                           s.archived_at::text AS archived_at, 0 AS is_partner, s.discord_user_id,
                           s.live_ping_role_id, COALESCE(s.live_ping_enabled, 1) AS live_ping_enabled
                      FROM twitch_streamers s
                     WHERE COALESCE(s.is_monitored_only, 0) = 1
                       AND NOT EXISTS (
                           SELECT 1
                             FROM twitch_streamers_partner_state ps
                            WHERE LOWER(ps.twitch_login) = LOWER(s.twitch_login)
                       )
                    """
                ).fetchall()
            for row in rows:
                row_dict = dict(row)
                login = str(row_dict.get("twitch_login") or "").strip()
                if not login:
                    continue
                user_id = str(row_dict.get("twitch_user_id") or "").strip()
                require_link = bool(row_dict.get("require_discord_link"))
                archived_at_raw = row_dict.get("archived_at")
                archived_dt: datetime | None = None
                if archived_at_raw:
                    try:
                        archived_dt = datetime.fromisoformat(str(archived_at_raw))
                    except Exception:
                        archived_dt = None
                is_archived = archived_dt is not None
                is_verified = bool(row_dict.get("is_partner"))

                tracked.append(
                    {
                        "login": login,
                        "twitch_user_id": user_id,
                        "require_link": require_link,
                        "is_verified": is_verified,
                        "archived_at": archived_at_raw,
                        "is_archived": is_archived,
                        "discord_user_id": row_dict.get("discord_user_id"),
                        "live_ping_role_id": row_dict.get("live_ping_role_id"),
                        "live_ping_enabled": row_dict.get("live_ping_enabled", 1),
                    }
                )
                login_lower = login.lower()
                if login_lower and is_verified and not is_archived:
                    partner_logins.add(login_lower)
        except Exception:
            log.exception("Konnte tracked Streamer nicht aus DB lesen")
            tracked = []
            partner_logins = set()
        return tracked, partner_logins

    async def _tick(self):
        """Ein Tick: tracked Streamer + Kategorie-Streams prüfen, Postings/DB aktualisieren, Stats loggen."""
        if self.api is None:
            return
        if self.api.is_auth_blocked():
            return

        if not self._category_id:
            await self._ensure_category_id()
            if self.api.is_auth_blocked():
                return

        tracked, partner_logins = self._load_tracked_streamers()

        logins = [str(entry.get("login") or "") for entry in tracked if entry.get("login")]
        language_filters = self._language_filter_values()
        streams_by_login: dict[str, dict] = {}

        if logins:
            for language in language_filters:
                try:
                    streams = await self.api.get_streams_by_logins(logins, language=language)
                except TwitchClientConfigError:
                    return
                except Exception:
                    label = language or "any"
                    log.exception(
                        "Konnte Streams für tracked Logins nicht abrufen (language=%s)",
                        label,
                    )
                    continue
                for stream in streams:
                    login = (stream.get("user_login") or "").lower()
                    if login:
                        streams_by_login[login] = stream

        for login, stream in list(streams_by_login.items()):
            if login in partner_logins:
                stream["is_partner"] = True

        category_streams: list[dict] = []
        if self._category_id:
            collected: dict[str, dict] = {}
            for language in language_filters:
                remaining = self._category_sample_limit - len(collected)
                if remaining <= 0:
                    break
                try:
                    streams = await self.api.get_streams_by_category(
                        self._category_id,
                        language=language,
                        limit=max(1, remaining),
                    )
                except TwitchClientConfigError:
                    return
                except Exception:
                    label = language or "any"
                    log.exception("Konnte Kategorie-Streams nicht abrufen (language=%s)", label)
                    continue
                for stream in streams:
                    login = (stream.get("user_login") or "").lower()
                    if login and login not in collected:
                        collected[login] = stream
            category_streams = list(collected.values())

        for stream in category_streams:
            login = (stream.get("user_login") or "").lower()
            if login in partner_logins:
                stream["is_partner"] = True

        # Backfill: if a tracked streamer appears in category_streams but NOT in
        # streams_by_login (e.g. due to API inconsistency), add them from category
        # data so they still get tracked stats and session samples recorded.
        tracked_logins_set = {str(e.get("login") or "").lower() for e in tracked if e.get("login")}
        for stream in category_streams:
            cat_login = (stream.get("user_login") or "").lower()
            if cat_login and cat_login in tracked_logins_set and cat_login not in streams_by_login:
                streams_by_login[cat_login] = stream

        try:
            await self._process_postings(tracked, streams_by_login)
        except Exception:
            log.exception("Fehler in _process_postings")

        try:
            self._maybe_schedule_partner_raid_score_reconciliation(
                trigger="poll_tick_reconciliation"
            )
        except Exception:
            log.debug("Partner raid score reconciliation scheduling failed", exc_info=True)

        try:
            await self._record_eventsub_capacity_snapshot("poll_tick")
        except Exception:
            log.debug("EventSub: Snapshot im Poll-Tick fehlgeschlagen", exc_info=True)

        self._tick_count += 1
        if self._tick_count % self._log_every_n == 0:
            try:
                await self._log_stats(streams_by_login, category_streams)
            except Exception:
                log.exception("Fehler beim Stats-Logging")

        if self._tick_count % 100 == 0:
            try:
                closed = self._cleanup_orphaned_sessions()
                if closed:
                    log.info("Orphaned sessions bereinigt: %d geschlossen", closed)
            except Exception:
                log.debug("Orphaned session cleanup fehlgeschlagen", exc_info=True)

        # Partner-Rekrutierung (intern rate-limitiert auf 30 min)
        try:
            await self._run_partner_recruit(category_streams)
        except Exception:
            log.exception("Fehler bei Partner-Rekrutierung")

    async def _process_postings(
        self,
        tracked: list[dict[str, object]],
        streams_by_login: dict[str, dict],
    ):
        notify_channel_id = int(getattr(self, "_notify_channel_id", 0) or 0) or None
        notify_ch: discord.TextChannel | None = None
        if notify_channel_id:
            notify_ch = self.bot.get_channel(notify_channel_id) or None  # type: ignore[assignment]

        now_utc = datetime.now(tz=UTC)
        now_iso = now_utc.isoformat(timespec="seconds")
        pending_state_rows: list[
            tuple[
                str,
                str,
                int,
                str,
                str | None,
                str | None,
                int,
                str | None,
                str | None,
                str | None,
                str | None,
                int,
                int | None,
                str | None,
            ]
        ] = []
        partner_score_refreshes: list[tuple[str, str | None, str]] = []

        with storage.get_conn() as c:
            live_state_rows = c.execute("SELECT * FROM twitch_live_state").fetchall()

        live_state: dict[str, dict] = {}
        for row in live_state_rows:
            row_dict = dict(row)
            key = str(row_dict.get("streamer_login") or "").lower()
            if key:
                live_state[key] = row_dict

        target_game_lower = self._get_target_game_lower()

        for entry in tracked:
            login = str(entry.get("login") or "").strip()
            if not login:
                continue

            referral_url = self._build_referral_url(login)
            login_lower = login.lower()
            stream = streams_by_login.get(login_lower)
            previous_state = live_state.get(login_lower, {})
            is_archived = bool(entry.get("is_archived"))
            was_live = bool(previous_state.get("is_live", 0))
            is_live = bool(stream)
            twitch_user_id = str(entry.get("twitch_user_id") or "").strip() or None
            need_link = bool(entry.get("require_link"))
            is_verified = bool(entry.get("is_verified"))

            # Go-Live Detection: Subscribe stream.offline für raid-enabled Streamer
            if not was_live and is_live and twitch_user_id and is_verified:
                # Stream ist gerade live gegangen!
                handler = getattr(self, "_handle_stream_went_live", None)
                if handler:
                    # Checke ob der Streamer raid_bot_enabled hat
                    try:
                        with storage.get_conn() as c:
                            partner_row = storage.load_active_partner(
                                c,
                                twitch_user_id=twitch_user_id,
                            )
                        raid_enabled = bool(
                            (
                                partner_row["raid_bot_enabled"]
                                if partner_row and hasattr(partner_row, "keys")
                                else (partner_row[13] if partner_row else 0)
                            )
                            or 0
                        )
                        if raid_enabled:
                            # Asynchron aufrufen (fire-and-forget, blockiert nicht den Tick)
                            asyncio.create_task(
                                handler(twitch_user_id, login_lower),
                                name=f"golive.{login_lower}",
                            )
                    except Exception:
                        log.debug(
                            "Go-Live: Konnte raid_enabled Status nicht checken für %s",
                            login_lower,
                            exc_info=True,
                        )

            # Auto-Entarchivierung sobald jemand wieder streamt
            if is_live and is_archived:
                try:
                    await self._dashboard_archive(login, "unarchive")
                    is_archived = False
                    entry["is_archived"] = False
                except Exception:
                    log.debug("Auto-Unarchive fehlgeschlagen für %s", login, exc_info=True)
            previous_game = (previous_state.get("last_game") or "").strip()
            previous_game_lower = previous_game.lower()
            was_deadlock = previous_game_lower == target_game_lower
            stream_started_at_value = self._extract_stream_start(stream, previous_state)
            previous_stream_id = (previous_state.get("last_stream_id") or "").strip()
            current_stream_id_raw = stream.get("id") if stream else ""
            current_stream_id = str(current_stream_id_raw or "").strip()
            stream_id_value = current_stream_id or previous_stream_id or None
            had_deadlock_prev = bool(int(previous_state.get("had_deadlock_in_session", 0) or 0))
            active_session_id: int | None = None
            previous_last_deadlock_seen = (
                previous_state.get("last_deadlock_seen_at") or ""
            ).strip() or None

            if is_live and stream:
                try:
                    active_session_id = await self._ensure_stream_session(
                        login=login_lower,
                        stream=stream,
                        previous_state=previous_state,
                        twitch_user_id=twitch_user_id,
                    )
                except Exception:
                    log.exception("Konnte Streamsitzung nicht starten: %s", login)
            elif was_live and not is_live:
                try:
                    await self._finalize_stream_session(login=login_lower, reason="offline")
                except Exception:
                    log.exception("Konnte Streamsitzung nicht abschliessen: %s", login)
            elif not is_live and previous_state.get("active_session_id"):
                try:
                    await self._finalize_stream_session(login=login_lower, reason="stale")
                except Exception:
                    log.debug("Konnte alte Session nicht bereinigen: %s", login, exc_info=True)

            stream_restarted = (
                is_live
                and previous_stream_id
                and current_stream_id
                and previous_stream_id != current_stream_id
            )
            if not was_live:
                had_deadlock_prev = False
            elif stream_restarted:
                had_deadlock_prev = False
                if twitch_user_id and is_verified and not is_archived:
                    partner_score_refreshes.append(
                        (twitch_user_id, login_lower, "poll_stream_restarted")
                    )
            if not is_live or stream_restarted:
                self._clear_live_announcement_retry_payload(login_lower)

            message_id_previous = (
                str(previous_state.get("last_discord_message_id") or "").strip() or None
            )
            message_id_to_store = message_id_previous
            tracking_token_previous = (
                str(previous_state.get("last_tracking_token") or "").strip() or None
            )
            tracking_token_to_store = tracking_token_previous

            game_name = (stream.get("game_name") or "").strip() if stream else ""
            game_name_lower = game_name.lower()
            is_deadlock = (
                is_live and bool(target_game_lower) and game_name_lower == target_game_lower
            )
            had_deadlock_in_session = had_deadlock_prev or is_deadlock

            # --- Experimental hook: game transition ---
            if (
                is_live
                and was_live
                and game_name
                and previous_game
                and game_name_lower != previous_game_lower
            ):
                try:
                    exp_transition = getattr(self, "_exp_on_game_transition", None)
                    exp_get_id = getattr(self, "_get_exp_session_id", None)
                    if callable(exp_transition) and callable(exp_get_id):
                        exp_id = exp_get_id(login_lower)
                        if exp_id is not None:
                            viewer_count_now = int(stream.get("viewer_count") or 0)
                            exp_transition(
                                login=login_lower,
                                exp_session_id=exp_id,
                                from_game=previous_game,
                                to_game=game_name,
                                viewer_count=viewer_count_now,
                            )
                except Exception:
                    log.debug("exp: game_transition fehlgeschlagen für %s", login_lower, exc_info=True)
            had_deadlock_to_store = had_deadlock_in_session if is_live else False
            last_title_value = (
                stream.get("title") if stream else previous_state.get("last_title")
            ) or None
            last_game_value = (game_name or previous_state.get("last_game") or "").strip() or None
            last_viewer_count_value = (
                int(stream.get("viewer_count") or 0)
                if stream
                else int(previous_state.get("last_viewer_count") or 0)
            )
            last_deadlock_seen_at_value: str | None = None
            if is_deadlock:
                last_deadlock_seen_at_value = now_iso
            elif had_deadlock_to_store and previous_last_deadlock_seen:
                last_deadlock_seen_at_value = previous_last_deadlock_seen

            should_post = (
                self._announcement_transport_ready(
                    channel_id=notify_channel_id,
                    notify_channel=notify_ch,
                )
                and is_deadlock
                and (not was_live or not was_deadlock or not message_id_previous)
                and is_verified
                and not is_archived
            )

            if should_post:
                announcement_stream, announcement_tracking_token, announcement_render_now = (
                    self._resolve_live_announcement_retry_payload(
                        login=login_lower,
                        stream=stream,
                        previous_state=previous_state,
                        stream_id=stream_id_value,
                        started_at=stream_started_at_value,
                        message_id=message_id_previous,
                        rendered_at=now_iso,
                    )
                )
                last_title_value = str(announcement_stream.get("title") or "").strip() or None
                last_game_value = str(announcement_stream.get("game_name") or "").strip() or None
                try:
                    last_viewer_count_value = int(announcement_stream.get("viewer_count") or 0)
                except (TypeError, ValueError):
                    last_viewer_count_value = 0
                content, embed, view, allowed_mentions, new_tracking_token = (
                    await self._build_live_announcement_message(
                        login=login,
                        stream=announcement_stream,
                        streamer_entry=entry,
                        notify_channel=notify_ch,
                        tracking_token=announcement_tracking_token,
                        render_now=announcement_render_now,
                    )
                )
                if self._alert_mention:
                    prefix = self._sanitize_live_content(str(self._alert_mention).strip())
                    if prefix:
                        content = f"{prefix} {content}".strip()
                        alert_role_id = self._extract_role_id_from_mention(prefix)
                        if alert_role_id:
                            role_ids: list[int] = []
                            current_roles = allowed_mentions.roles
                            if isinstance(current_roles, (list, tuple, set)):
                                for role_obj in current_roles:
                                    role_id = getattr(role_obj, "id", None)
                                    if role_id:
                                        role_ids.append(int(role_id))
                            if alert_role_id not in role_ids:
                                role_ids.append(alert_role_id)
                            allowed_mentions = discord.AllowedMentions(
                                everyone=False,
                                users=False,
                                roles=[discord.Object(id=role_id) for role_id in role_ids]
                                if role_ids
                                else False,
                                replied_user=False,
                            )

                allowed_role_ids = self._allowed_role_ids_from_allowed_mentions(allowed_mentions)
                referral_url = self._build_referral_url(login)
                live_view_spec = (
                    self._build_twitch_live_tracking_view_spec(
                        login=login,
                        referral_url=referral_url,
                        tracking_token=new_tracking_token,
                        button_label=getattr(view, "button_label", ""),
                    )
                    if view is not None
                    else None
                )

                if self._announcement_transport_prefers_master_broker():
                    if notify_channel_id:
                        message_id_result = await self._send_live_announcement_via_broker(
                            channel_id=notify_channel_id,
                            login=login_lower,
                            stream_id=stream_id_value,
                            content=content,
                            embed=embed,
                            allowed_role_ids=allowed_role_ids,
                            view_spec=live_view_spec,
                        )
                    else:
                        message_id_result = None
                    if message_id_result is None:
                        log.warning("Konnte Go-Live-Posting nicht via Master-Broker senden: %s", login)
                    else:
                        self._clear_live_announcement_retry_payload(login_lower)
                        message_id_to_store = message_id_result
                        tracking_token_to_store = (
                            new_tracking_token if live_view_spec is not None else None
                        )
                elif notify_ch is not None and self._announcement_transport_can_use_direct_discord():
                    try:
                        message = await notify_ch.send(
                            content=content or None,
                            embed=embed,
                            view=view,
                            allowed_mentions=allowed_mentions,
                        )
                    except Exception:
                        log.exception("Konnte Go-Live-Posting nicht senden: %s", login)
                    else:
                        self._clear_live_announcement_retry_payload(login_lower)
                        message_id_to_store = str(message.id)
                        tracking_token_to_store = new_tracking_token if view is not None else None
                        if view is not None:
                            view.bind_to_message(
                                channel_id=getattr(notify_ch, "id", None),
                                message_id=message.id,
                            )
                            self._register_live_view(
                                tracking_token=new_tracking_token,
                                view=view,
                                message_id=message.id,
                            )
                else:
                    log.warning(
                        "Kein Announcement-Transport für Go-Live-Posting verfügbar: %s",
                        login,
                    )

                if message_id_to_store and active_session_id:
                    try:
                        with storage.get_conn() as c:
                            c.execute(
                                "UPDATE twitch_stream_sessions SET notification_text = ? WHERE id = ?",
                                (content or "", active_session_id),
                            )
                    except Exception:
                        log.debug(
                            "Could not save notification text for %s",
                            login,
                            exc_info=True,
                        )

            ended_deadlock_posting = (
                self._announcement_transport_ready(
                    channel_id=notify_channel_id,
                    notify_channel=notify_ch,
                )
                and message_id_previous
                and (not is_live or not is_deadlock)
            )
            # Auto-Raid per Polling für Partner deaktiviert – EventSub ist Primärpfad
            should_auto_raid = False

            if ended_deadlock_posting:
                display_name = (
                    stream.get("user_name") if stream else previous_state.get("streamer_login")
                ) or login
                try:
                    message_id_int = int(message_id_previous)
                except (TypeError, ValueError):
                    message_id_int = None

                if message_id_int is None:
                    log.warning(
                        "Ungültige Message-ID für Deadlock-Ende bei %s: %r",
                        login,
                        message_id_previous,
                    )
                else:
                    preview_image_url = await self._get_latest_vod_preview_url(
                        login=login,
                        twitch_user_id=twitch_user_id or previous_state.get("twitch_user_id"),
                    )

                    ended_content = f"**{display_name}** ist OFFLINE - VOD per Button."
                    offline_embed = self._build_offline_embed(
                        login=login,
                        display_name=display_name,
                        last_title=last_title_value,
                        last_game=last_game_value,
                        preview_image_url=preview_image_url,
                    )
                    offline_view_spec = self._build_link_button_view_spec(
                        referral_url,
                        label=TWITCH_VOD_BUTTON_LABEL,
                    )
                    if self._announcement_transport_prefers_master_broker():
                        updated = await self._edit_live_announcement_via_broker(
                            channel_id=int(notify_channel_id or 0),
                            login=login_lower,
                            message_id=message_id_previous,
                            content=ended_content,
                            embed=offline_embed,
                            view_spec=offline_view_spec,
                        )
                    elif notify_ch is not None:
                        try:
                            fetched_message = await notify_ch.fetch_message(message_id_int)
                        except discord.NotFound:
                            log.warning(
                                "Deadlock-Ende-Posting nicht mehr vorhanden für %s (ID %s)",
                                login,
                                message_id_previous,
                            )
                            message_id_to_store = None
                            tracking_token_to_store = None
                            self._drop_live_view(tracking_token_previous)
                            updated = False
                        except Exception:
                            log.exception("Konnte Deadlock-Ende-Posting nicht laden: %s", login)
                            updated = False
                        else:
                            offline_view = self._build_offline_link_view(
                                referral_url,
                                label=TWITCH_VOD_BUTTON_LABEL,
                            )
                            try:
                                await fetched_message.edit(
                                    content=ended_content,
                                    embed=offline_embed,
                                    view=offline_view,
                                )
                            except Exception:
                                log.exception(
                                    "Konnte Deadlock-Ende-Posting nicht aktualisieren: %s",
                                    login,
                                )
                                updated = False
                            else:
                                updated = True
                    else:
                        updated = False

                    if updated:
                        message_id_to_store = None
                        tracking_token_to_store = None
                        self._drop_live_view(tracking_token_previous)

            previous_user_id = str(previous_state.get("twitch_user_id") or "").strip()
            db_user_id = str(twitch_user_id or "").strip()
            if not db_user_id and previous_user_id and previous_user_id.lower() != login_lower:
                db_user_id = previous_user_id
            if not db_user_id:
                log.debug(
                    "Skipping live-state write without Twitch user id for %s",
                    login_lower,
                )
                continue
            db_message_id = str(message_id_to_store) if message_id_to_store else None
            db_streamer_login = login_lower

            pending_state_rows.append(
                (
                    db_user_id,
                    db_streamer_login,
                    int(is_live),
                    now_iso,
                    last_title_value,
                    last_game_value,
                    last_viewer_count_value,
                    db_message_id,
                    tracking_token_to_store,
                    stream_id_value,
                    stream_started_at_value,
                    int(had_deadlock_to_store),
                    active_session_id,
                    last_deadlock_seen_at_value,
                )
            )

            if twitch_user_id and is_verified and not is_archived:
                if not was_live and is_live:
                    partner_score_refreshes.append((twitch_user_id, login_lower, "poll_stream_online"))
                elif was_live and not is_live:
                    partner_score_refreshes.append((twitch_user_id, login_lower, "poll_stream_offline"))

            if need_link and self._alert_channel_id and (now_utc.minute % 10 == 0) and is_live:
                # Platzhalter für deinen Profil-/Panel-Check
                pass

        await self._persist_live_state_rows(pending_state_rows)
        try:
            self._schedule_partner_raid_score_refreshes(partner_score_refreshes)
        except Exception:
            log.debug("Partner raid score refresh scheduling failed", exc_info=True)
        await self._auto_archive_inactive_streamers()

    async def _persist_live_state_rows(
        self,
        rows: list[
            tuple[
                str,
                str,
                int,
                str,
                str | None,
                str | None,
                int,
                str | None,
                str | None,
                str | None,
                str | None,
                int,
                int | None,
            ]
        ],
    ) -> None:
        if not rows:
            return

        retry_delay = 0.5
        for attempt in range(3):
            try:
                with storage.get_conn() as c:
                    conflict_cleanup_rows = sorted(
                        {
                            (str(streamer_login or "").strip(), str(twitch_user_id or "").strip())
                            for twitch_user_id, streamer_login, *_ in rows
                            if str(twitch_user_id or "").strip()
                            and str(streamer_login or "").strip()
                        }
                    )
                    if conflict_cleanup_rows:
                        c.executemany(
                            "DELETE FROM twitch_live_state "
                            "WHERE LOWER(streamer_login) = LOWER(?) "
                            "AND LOWER(COALESCE(twitch_user_id, '')) <> LOWER(?)",
                            conflict_cleanup_rows,
                        )
                    c.executemany(
                        "INSERT INTO twitch_live_state ("
                        "twitch_user_id, streamer_login, is_live, last_seen_at, last_title, last_game, "
                        "last_viewer_count, last_discord_message_id, last_tracking_token, last_stream_id, "
                        "last_started_at, had_deadlock_in_session, active_session_id, last_deadlock_seen_at"
                        ") VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) "
                        "ON CONFLICT (twitch_user_id) DO UPDATE SET "
                        "streamer_login = EXCLUDED.streamer_login, "
                        "is_live = EXCLUDED.is_live, "
                        "last_seen_at = EXCLUDED.last_seen_at, "
                        "last_title = EXCLUDED.last_title, "
                        "last_game = EXCLUDED.last_game, "
                        "last_viewer_count = EXCLUDED.last_viewer_count, "
                        "last_discord_message_id = EXCLUDED.last_discord_message_id, "
                        "last_tracking_token = EXCLUDED.last_tracking_token, "
                        "last_stream_id = EXCLUDED.last_stream_id, "
                        "last_started_at = EXCLUDED.last_started_at, "
                        "had_deadlock_in_session = EXCLUDED.had_deadlock_in_session, "
                        "active_session_id = EXCLUDED.active_session_id, "
                        "last_deadlock_seen_at = EXCLUDED.last_deadlock_seen_at",
                        rows,
                    )
                return
            except Exception as exc:
                locked = "locked" in str(exc).lower()
                if not locked or attempt == 2:
                    log.exception(
                        "Konnte Live-State-Updates nicht speichern (%s Eintraege)",
                        len(rows),
                    )
                    return
                await asyncio.sleep(retry_delay)
                retry_delay *= 2

    async def _auto_archive_inactive_streamers(self, *, days: int = 10) -> None:
        """
        Archiviert Partner automatisch, wenn sie länger als `days` Tage nicht gestreamt haben.
        Läuft maximal alle 15 Minuten, um DB-Load gering zu halten.
        """
        now = datetime.now(UTC)
        last_run = getattr(self, "_last_archive_check", 0.0)
        if time.time() - last_run < 900:
            return
        self._last_archive_check = time.time()

        cutoff = now - timedelta(days=days)

        try:
            target_game = (
                os.getenv("TWITCH_TARGET_GAME_NAME") or TWITCH_TARGET_GAME_NAME or ""
            ).strip()
            with storage.get_conn() as c:
                rows = c.execute(
                    """
                    SELECT s.twitch_login,
                           s.archived_at,
                           MAX(
                               CASE
                                 WHEN LOWER(COALESCE(sess.game_name,'')) = LOWER(?)
                                 THEN COALESCE(sess.ended_at, sess.started_at)
                               END
                            ) AS last_deadlock_stream_at
                      FROM twitch_streamers_partner_state s
                      LEFT JOIN twitch_stream_sessions sess
                        ON LOWER(sess.streamer_login) = LOWER(s.twitch_login)
                     WHERE s.is_partner = 1
                     GROUP BY s.twitch_login, s.archived_at
                    """,
                    (target_game,),
                ).fetchall()
        except Exception:
            log.debug("Auto-Archivierung: konnte Streamer-Liste nicht laden", exc_info=True)
            return

        for row in rows:
            try:
                login = (row["twitch_login"] if hasattr(row, "keys") else row[0] or "").strip()
            except Exception:
                continue
            if not login:
                continue

            archived_at = row["archived_at"] if hasattr(row, "keys") else row[1]
            if archived_at:
                continue

            last_stream_raw = row["last_deadlock_stream_at"] if hasattr(row, "keys") else row[2]
            if not last_stream_raw:
                # Keine Historie -> keine automatische Archivierung
                continue

            try:
                last_dt = datetime.fromisoformat(str(last_stream_raw).replace("Z", "+00:00"))
                if last_dt.tzinfo is None:
                    last_dt = last_dt.replace(tzinfo=UTC)
            except Exception:
                log.debug(
                    "Auto-Archivierung: Datum unlesbar für %s (%r)",
                    login,
                    last_stream_raw,
                    exc_info=True,
                )
                continue

            if last_dt < cutoff:
                try:
                    result = await self._dashboard_archive(login, "archive")
                    if "bereits archiviert" not in result:
                        log.info(
                            "Auto-archiviert %s (letzter Stream %s, cutoff %s)",
                            login,
                            last_dt.date().isoformat(),
                            cutoff.date().isoformat(),
                        )
                except Exception:
                    log.debug("Auto-Archivierung fehlgeschlagen für %s", login, exc_info=True)

    @staticmethod
    def _parse_dt(value: str | None) -> datetime | None:
        if not value:
            return None
        try:
            dt = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
        except Exception:
            return None
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=UTC)
        return dt.astimezone(UTC)

    def _extract_stream_start(self, stream: dict | None, previous_state: dict) -> str | None:
        candidate = None
        if stream:
            candidate = stream.get("started_at") or stream.get("start_time")
        if not candidate:
            candidate = previous_state.get("last_started_at")
        dt = self._parse_dt(candidate)
        if dt:
            return dt.isoformat(timespec="seconds")
        return None

    async def _log_stats(self, streams_by_login: dict[str, dict], category_streams: list[dict]):
        now_utc = datetime.now(tz=UTC).isoformat(timespec="seconds")

        # 1) Tracked stats (only our target game)
        try:
            rows: list[tuple] = []
            for stream in streams_by_login.values():
                if not self._stream_is_in_target_category(stream):
                    continue
                login = (stream.get("user_login") or "").lower()
                viewers = int(stream.get("viewer_count") or 0)
                is_partner = bool(stream.get("is_partner"))
                game_name, stream_title, tags = self._normalize_stream_meta(stream)
                rows.append((now_utc, login, viewers, is_partner, game_name, stream_title, tags))

            if rows:
                with storage.get_conn() as c:
                    c.executemany(
                        """
                        INSERT INTO twitch_stats_tracked (
                            ts_utc, streamer, viewer_count, is_partner, game_name, stream_title, tags
                        ) VALUES (?, ?, ?, ?, ?, ?, ?)
                        """,
                        rows,
                    )
        except Exception:
            log.exception("Konnte tracked-Stats nicht loggen")

        # 2) Session samples – alle Spiele erfassen, nicht nur Deadlock
        try:
            seen_logins: set[str] = set()
            for stream in streams_by_login.values():
                login = (stream.get("user_login") or "").lower()
                self._record_session_sample(login=login, stream=stream)
                seen_logins.add(login)
            # Also record viewer timeline for category-discovered streams
            for stream in category_streams:
                login = (stream.get("user_login") or "").lower()
                if login and login not in seen_logins:
                    self._record_session_sample(login=login, stream=stream)
                    seen_logins.add(login)
        except Exception:
            log.debug("Konnte Session-Metrik nicht loggen", exc_info=True)

        # 3) Category-wide stats (all streams in category, regardless of Deadlock)
        try:
            rows: list[tuple] = []
            for stream in category_streams:
                login = (stream.get("user_login") or "").lower()
                viewers = int(stream.get("viewer_count") or 0)
                is_partner = bool(stream.get("is_partner"))
                game_name, stream_title, tags = self._normalize_stream_meta(stream)
                rows.append((now_utc, login, viewers, is_partner, game_name, stream_title, tags))

            if rows:
                with storage.get_conn() as c:
                    c.executemany(
                        """
                        INSERT INTO twitch_stats_category (
                            ts_utc, streamer, viewer_count, is_partner, game_name, stream_title, tags
                        ) VALUES (?, ?, ?, ?, ?, ?, ?)
                        """,
                        rows,
                    )
        except Exception:
            log.exception("Konnte category-Stats nicht loggen")

    async def _get_latest_vod_preview_url(
        self, *, login: str, twitch_user_id: str | None
    ) -> str | None:
        """Hole das juengste VOD-Thumbnail; faellt bei Fehler still auf None."""
        if self.api is None:
            return None
        try:
            return await self.api.get_latest_vod_thumbnail(user_id=twitch_user_id, login=login)
        except Exception:
            log.exception("Konnte VOD-Thumbnail nicht laden: %s", login)
            return None
