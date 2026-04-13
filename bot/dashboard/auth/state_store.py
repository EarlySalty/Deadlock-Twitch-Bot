"""Durable auth/session state repository for dashboard OAuth and rate limits."""

from __future__ import annotations

import hashlib
import secrets
import time
from typing import Any

from ...storage import sessions_db

_TWITCH_OAUTH_STATE_TYPE = "oauth_state:twitch"
_DISCORD_ADMIN_OAUTH_STATE_TYPE = "oauth_state:discord_admin"
_AFFILIATE_OAUTH_STATE_TYPE = "oauth_state:affiliate"
_AFFILIATE_CONNECT_STATE_TYPE = "oauth_state:affiliate_connect"
_PARTNER_LOGIN_STATE_TYPE = "oauth_state:partner_login"
_TWITCH_SESSION_TYPE = "twitch"
_PARTNER_ACCESS_SESSION_TYPE = "partner_access"
_DISCORD_ADMIN_SESSION_TYPE = "discord_admin"
_RATE_LIMIT_SESSION_TYPE = "rate_limit:dashboard_auth"


def _now() -> float:
    return time.time()


class DashboardAuthRateLimitStoreUnavailable(RuntimeError):
    """Raised when the durable auth rate-limit store cannot be reached."""


class DashboardAuthStateCache:
    """Mutable in-process cache for one dashboard auth namespace.

    The cache stays intentionally small and is always paired with the durable
    repository. This keeps the call sites focused on auth flow logic instead of
    repeating getattr/setattr boilerplate.
    """

    def __init__(self, owner: Any, attr_name: str) -> None:
        self._owner = owner
        self._attr_name = str(attr_name or "").strip()

    def data(self) -> dict[str, dict[str, Any]]:
        cache = getattr(self._owner, self._attr_name, None)
        if isinstance(cache, dict):
            return cache
        cache = {}
        setattr(self._owner, self._attr_name, cache)
        return cache

    def get(self, key: str, default: Any = None) -> Any:
        return self.data().get(key, default)

    def pop(self, key: str, default: Any = None) -> Any:
        return self.data().pop(key, default)

    def put(self, key: str, payload: dict[str, Any]) -> dict[str, Any]:
        record = dict(payload)
        self.data()[key] = record
        return record

    def clear(self) -> None:
        self.data().clear()

    def prune_by_created_at(
        self,
        *,
        ttl_seconds: float,
        now: float | None = None,
        max_items: int | None = None,
    ) -> None:
        current = _now() if now is None else float(now)
        cache = self.data()
        expired = [
            key
            for key, row in cache.items()
            if current - float(row.get("created_at", 0.0) or 0.0) > float(ttl_seconds)
        ]
        for key in expired:
            cache.pop(key, None)

        if max_items is not None and max_items >= 0 and len(cache) > max_items:
            oldest = sorted(
                cache.items(),
                key=lambda item: float(item[1].get("created_at", 0.0) or 0.0),
            )
            for key, _ in oldest[: len(cache) - max_items]:
                cache.pop(key, None)

    def prune_by_expires_at(
        self,
        *,
        now: float | None = None,
        max_items: int | None = None,
    ) -> None:
        current = _now() if now is None else float(now)
        cache = self.data()
        expired = [
            key
            for key, row in cache.items()
            if float(row.get("expires_at", 0.0) or 0.0) <= current
        ]
        for key in expired:
            cache.pop(key, None)

        if max_items is not None and max_items >= 0 and len(cache) > max_items:
            oldest = sorted(
                cache.items(),
                key=lambda item: float(item[1].get("created_at", 0.0) or 0.0),
            )
            for key, _ in oldest[: len(cache) - max_items]:
                cache.pop(key, None)


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

    def load_twitch_oauth_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._load_session(_TWITCH_OAUTH_STATE_TYPE, state, now=now)

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

    def load_discord_admin_oauth_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._load_session(_DISCORD_ADMIN_OAUTH_STATE_TYPE, state, now=now)

    def save_affiliate_oauth_state(
        self,
        *,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None = None,
    ) -> None:
        self._save_state(
            state_type=_AFFILIATE_OAUTH_STATE_TYPE,
            state=state,
            payload=payload,
            ttl_seconds=ttl_seconds,
            now=now,
        )

    def consume_affiliate_oauth_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._consume_state(_AFFILIATE_OAUTH_STATE_TYPE, state, now=now)

    def save_affiliate_connect_state(
        self,
        *,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None = None,
    ) -> None:
        self._save_state(
            state_type=_AFFILIATE_CONNECT_STATE_TYPE,
            state=state,
            payload=payload,
            ttl_seconds=ttl_seconds,
            now=now,
        )

    def consume_affiliate_connect_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._consume_state(_AFFILIATE_CONNECT_STATE_TYPE, state, now=now)

    def save_partner_login_state(
        self,
        *,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None = None,
    ) -> None:
        self._save_state(
            state_type=_PARTNER_LOGIN_STATE_TYPE,
            state=state,
            payload=payload,
            ttl_seconds=ttl_seconds,
            now=now,
        )

    def consume_partner_login_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._consume_state(_PARTNER_LOGIN_STATE_TYPE, state, now=now)

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

    def load_partner_access_session(
        self,
        session_id: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._load_session(_PARTNER_ACCESS_SESSION_TYPE, session_id, now=now)

    def save_partner_access_session(
        self,
        *,
        session_id: str,
        payload: dict[str, Any],
        created_at: float,
        expires_at: float,
    ) -> None:
        sessions_db.upsert_session(
            session_id,
            _PARTNER_ACCESS_SESSION_TYPE,
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
    """Durable sliding-window limiter backed by shared PostgreSQL session storage."""

    def __init__(self) -> None:
        self._last_cleanup_at = 0.0

    def _maybe_cleanup_expired(self, *, now: float) -> None:
        # Rate-limit hits are short-lived; periodic purge keeps the shared session
        # table from accumulating expired login buckets between normal auth cleanups.
        if now - self._last_cleanup_at < 30.0:
            return
        sessions_db.delete_expired_sessions(now)
        self._last_cleanup_at = now

    def allow_request(
        self,
        *,
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

        self._maybe_cleanup_expired(now=current)

        bucket_prefix = self._bucket_prefix(key=key, window_seconds=window_seconds)
        try:
            return sessions_db.reserve_rate_limit_slot(
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
        except Exception as exc:
            raise DashboardAuthRateLimitStoreUnavailable(
                "dashboard auth rate limit store unavailable"
            ) from exc

    @staticmethod
    def _bucket_prefix(*, key: str, window_seconds: float) -> str:
        key_hash = hashlib.sha256(str(key or "").encode("utf-8", errors="ignore")).hexdigest()
        return f"rl:{int(window_seconds)}:{key_hash}"

    @staticmethod
    def _hit_record_id(bucket_prefix: str, now: float) -> str:
        return f"{bucket_prefix}:{int(now * 1000)}:{secrets.token_urlsafe(6)}"
