"""Durable auth/session state repository for dashboard OAuth and rate limits."""

from __future__ import annotations

import hashlib
import secrets
import time
from typing import Any

from ...core.constants import log
from ...storage import sessions_db

_TWITCH_OAUTH_STATE_TYPE = "oauth_state:twitch"
_DISCORD_ADMIN_OAUTH_STATE_TYPE = "oauth_state:discord_admin"
_TWITCH_SESSION_TYPE = "twitch"
_DISCORD_ADMIN_SESSION_TYPE = "discord_admin"
_RATE_LIMIT_SESSION_TYPE = "rate_limit:dashboard_auth"


def _now() -> float:
    return time.time()


class DashboardAuthStateRepository:
    """Persist OAuth states and authenticated sessions in the shared session store."""

    def save_twitch_oauth_state(
        self,
        *,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None = None,
    ) -> None:
        self._save_state(
            state_type=_TWITCH_OAUTH_STATE_TYPE,
            state=state,
            payload=payload,
            ttl_seconds=ttl_seconds,
            now=now,
        )

    def consume_twitch_oauth_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._consume_state(_TWITCH_OAUTH_STATE_TYPE, state, now=now)

    def save_discord_admin_oauth_state(
        self,
        *,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None = None,
    ) -> None:
        self._save_state(
            state_type=_DISCORD_ADMIN_OAUTH_STATE_TYPE,
            state=state,
            payload=payload,
            ttl_seconds=ttl_seconds,
            now=now,
        )

    def consume_discord_admin_oauth_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._consume_state(_DISCORD_ADMIN_OAUTH_STATE_TYPE, state, now=now)

    def load_dashboard_session(self, session_id: str, *, now: float | None = None) -> dict[str, Any] | None:
        return self._load_session(_TWITCH_SESSION_TYPE, session_id, now=now)

    def save_dashboard_session(
        self,
        *,
        session_id: str,
        payload: dict[str, Any],
        created_at: float,
        expires_at: float,
    ) -> None:
        sessions_db.upsert_session(session_id, _TWITCH_SESSION_TYPE, payload, created_at, expires_at)

    def load_discord_admin_session(
        self,
        session_id: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._load_session(_DISCORD_ADMIN_SESSION_TYPE, session_id, now=now)

    def save_discord_admin_session(
        self,
        *,
        session_id: str,
        payload: dict[str, Any],
        created_at: float,
        expires_at: float,
    ) -> None:
        sessions_db.upsert_session(
            session_id,
            _DISCORD_ADMIN_SESSION_TYPE,
            payload,
            created_at,
            expires_at,
        )

    @staticmethod
    def delete_session(session_id: str) -> None:
        sessions_db.delete_session(session_id)

    @staticmethod
    def delete_expired(now: float | None = None) -> None:
        sessions_db.delete_expired_sessions(_now() if now is None else float(now))

    def _save_state(
        self,
        *,
        state_type: str,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None,
    ) -> None:
        current = _now() if now is None else float(now)
        record = dict(payload)
        record.setdefault("created_at", current)
        sessions_db.upsert_session(
            state,
            state_type,
            record,
            float(record.get("created_at", current)),
            current + max(1.0, float(ttl_seconds)),
        )

    def _consume_state(
        self,
        state_type: str,
        state: str,
        *,
        now: float | None,
    ) -> dict[str, Any] | None:
        return sessions_db.pop_session(
            state,
            state_type,
            _now() if now is None else float(now),
        )

    def _load_session(
        self,
        session_type: str,
        session_id: str,
        *,
        now: float | None,
    ) -> dict[str, Any] | None:
        return sessions_db.load_session(
            session_id,
            session_type,
            _now() if now is None else float(now),
        )


class DashboardAuthRateLimitStore:
    """Durable sliding-window limiter with in-memory fallback for degraded mode."""

    def allow_request(
        self,
        *,
        owner: Any,
        key: str,
        max_requests: int,
        window_seconds: float,
        now: float | None = None,
    ) -> bool:
        current = _now() if now is None else float(now)
        if max_requests <= 0:
            return False
        if window_seconds <= 0:
            return True

        bucket_prefix = self._bucket_prefix(key=key, window_seconds=window_seconds)
        try:
            allowed = sessions_db.reserve_rate_limit_slot(
                bucket_key=bucket_prefix,
                session_type=_RATE_LIMIT_SESSION_TYPE,
                session_id=self._hit_record_id(bucket_prefix, current),
                payload={
                    "key_hash": bucket_prefix.rsplit(":", 1)[-1],
                    "seen_at": current,
                    "window_seconds": float(window_seconds),
                },
                created_at=current,
                expires_at=current + float(window_seconds),
                max_requests=max_requests,
            )
            if not allowed:
                self._mirror_rate_limit(owner, key, current, window_seconds, append_now=False)
                return False
            self._mirror_rate_limit(owner, key, current, window_seconds, append_now=True)
            return True
        except Exception as exc:
            log.debug("Dashboard auth rate limit store unavailable; using local fallback: %s", exc)
            return self._fallback_allow_request(
                owner=owner,
                key=key,
                max_requests=max_requests,
                window_seconds=window_seconds,
                now=current,
            )

    @staticmethod
    def _bucket_prefix(*, key: str, window_seconds: float) -> str:
        key_hash = hashlib.sha256(str(key or "").encode("utf-8", errors="ignore")).hexdigest()
        return f"rl:{int(window_seconds)}:{key_hash}"

    @staticmethod
    def _hit_record_id(bucket_prefix: str, now: float) -> str:
        return f"{bucket_prefix}:{int(now * 1000)}:{secrets.token_urlsafe(6)}"

    @staticmethod
    def _rate_limit_cache(owner: Any) -> dict[str, list[float]]:
        cache = getattr(owner, "_rate_limits", None)
        if isinstance(cache, dict):
            return cache
        cache = {}
        setattr(owner, "_rate_limits", cache)
        return cache

    def _fallback_allow_request(
        self,
        *,
        owner: Any,
        key: str,
        max_requests: int,
        window_seconds: float,
        now: float,
    ) -> bool:
        cache = self._rate_limit_cache(owner)
        hits = [ts for ts in cache.get(key, []) if now - float(ts) < window_seconds]
        if len(hits) >= max_requests:
            cache[key] = hits
            return False
        hits.append(now)
        cache[key] = hits
        if len(cache) > 1000:
            cache.clear()
        return True

    def _mirror_rate_limit(
        self,
        owner: Any,
        key: str,
        now: float,
        window_seconds: float,
        *,
        append_now: bool,
    ) -> None:
        cache = self._rate_limit_cache(owner)
        hits = [ts for ts in cache.get(key, []) if now - float(ts) < window_seconds]
        if append_now:
            hits.append(now)
        cache[key] = hits
