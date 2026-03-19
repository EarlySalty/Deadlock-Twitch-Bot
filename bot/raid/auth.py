# cogs/twitch/raid_manager.py
"""
Raid Bot Manager - RaidAuthManager

Verwaltet:
- OAuth User Access Tokens für Streamer
"""

import asyncio
import json
import logging
import os
import secrets
import time
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from urllib.parse import urlencode

import aiohttp
import discord

from ..api.twitch_auth import (
    TwitchClientConfigError,
    is_invalid_client_response,
    normalize_twitch_credential,
)
from ..api.token_error_handler import TokenErrorHandler
from ..storage import (
    backfill_tracked_stats_from_category,
    get_conn,
    load_active_partner,
    load_streamer_identity,
    set_partner_raid_bot_enabled,
)
from .scope_profiles import (
    AUTO_SCOPE_PROFILE,
    BASE_SCOPE_PROFILE,
    BASE_STREAMER_SCOPES,
    DASHBOARD_REAUTH_SCOPE_PROFILE,
    normalize_scope_profile,
    parse_scope_profile_meta,
    scopes_for_profile,
)

TWITCH_TOKEN_URL = "https://id.twitch.tv/oauth2/token"  # noqa: S105
TWITCH_AUTHORIZE_URL = "https://id.twitch.tv/oauth2/authorize"
TWITCH_API_BASE = "https://api.twitch.tv/helix"

# Erforderliche Scopes für Raid-Funktionalität + Zusatz-Metriken (Follower/Chat)
# Hinweis: Re-Auth notwendig, falls bisher nur channel:manage:raids erteilt war.
RAID_SCOPES = list(BASE_STREAMER_SCOPES)

RAID_TARGET_COOLDOWN_DAYS = 7  # Avoid repeating the same raid target if alternatives exist
RECRUIT_DISCORD_INVITE = (
    os.getenv("RECRUIT_DISCORD_INVITE") or ""
).strip() or "Discord: Server hinzufügen & Code eingeben: z5TfVHuQq2"
RECRUIT_DISCORD_INVITE_DIRECT = (
    os.getenv("RECRUIT_DISCORD_INVITE_DIRECT") or ""
).strip() or "https://discord.gg/z5TfVHuQq2"

_recruit_direct_invite_threshold_raw = (
    os.getenv("RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS") or "120"
).strip()
try:
    RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS = max(0, int(_recruit_direct_invite_threshold_raw))
except ValueError:
    RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS = 120


def _parse_env_int(name: str, default: int = 0) -> int:
    raw = (os.getenv(name) or "").strip()
    if not raw:
        return default
    try:
        return int(raw)
    except ValueError:
        return default


STREAMER_ROLE_ID = _parse_env_int("STREAMER_ROLE_ID", 1313624729466441769)
STREAMER_GUILD_ID = _parse_env_int("STREAMER_GUILD_ID", 0)
FALLBACK_MAIN_GUILD_ID = _parse_env_int("MAIN_GUILD_ID", 0)

log = logging.getLogger("TwitchStreams.RaidManager")
_OAUTH_STATE_PLATFORM_RAID = "twitch_raid"
_OAUTH_STATE_TTL_SECONDS = 600


@dataclass(frozen=True, slots=True)
class RaidOAuthState:
    requested_login: str
    scope_profile: str
    expected_twitch_login: str | None = None
    discord_user_id: str | None = None


def _safe_int(val, default: int = 0) -> int:
    try:
        return int(val)
    except Exception:
        return default


def _normalize_twitch_login(value: str | None) -> str | None:
    normalized = str(value or "").strip().lower()
    return normalized or None


def _normalize_state_discord_user_id(value: str | int | None) -> str | None:
    normalized = str(value or "").strip()
    if normalized.isdigit():
        return normalized
    return None


def _serialize_state_meta(
    scope_profile: str | None,
    *,
    expected_twitch_login: str | None = None,
    discord_user_id: str | None = None,
) -> str:
    payload: dict[str, str] = {
        "scope_profile": normalize_scope_profile(scope_profile),
    }
    normalized_login = _normalize_twitch_login(expected_twitch_login)
    if normalized_login:
        payload["expected_twitch_login"] = normalized_login
    normalized_discord_user_id = _normalize_state_discord_user_id(discord_user_id)
    if normalized_discord_user_id:
        payload["discord_user_id"] = normalized_discord_user_id
    return json.dumps(payload, separators=(",", ":"), sort_keys=True)


def _parse_state_meta(raw_value: str | None) -> tuple[str, str | None, str | None]:
    raw_text = str(raw_value or "").strip()
    if not raw_text:
        return BASE_SCOPE_PROFILE, None, None
    try:
        decoded = json.loads(raw_text)
    except Exception:
        return parse_scope_profile_meta(raw_text), None, None
    if not isinstance(decoded, dict):
        return parse_scope_profile_meta(raw_text), None, None
    scope_profile = normalize_scope_profile(decoded.get("scope_profile"))
    expected_login = _normalize_twitch_login(decoded.get("expected_twitch_login"))
    discord_user_id = _normalize_state_discord_user_id(decoded.get("discord_user_id"))
    return scope_profile, expected_login, discord_user_id


def _parse_expiry_ts(val) -> float:
    """
    Parse various expiry formats (ISO8601, epoch seconds as int/str).
    Returns 0.0 on failure so callers can force a refresh.
    """
    if val is None:
        return 0.0
    # Numeric epoch
    if isinstance(val, (int, float)):
        try:
            return float(val)
        except Exception:
            return 0.0
    text = str(val).strip()
    if not text:
        return 0.0
    if text.isdigit():
        try:
            return float(text)
        except Exception:
            return 0.0
    try:
        return datetime.fromisoformat(text.replace("Z", "+00:00")).timestamp()
    except Exception:
        return 0.0


def _mask_log_identifier(value: object, *, visible_prefix: int = 3, visible_suffix: int = 2) -> str:
    text = str(value or "").strip()
    if not text:
        return "<empty>"
    if len(text) <= visible_prefix + visible_suffix:
        return "***"
    return f"{text[:visible_prefix]}...{text[-visible_suffix:]}"


class RaidAuthManager:
    """Verwaltet OAuth User Access Tokens für Raid-Autorisierung."""

    def __init__(self, client_id: str, client_secret: str, redirect_uri: str):
        self.client_id = normalize_twitch_credential(client_id)
        self.client_secret = normalize_twitch_credential(client_secret)
        self.redirect_uri = redirect_uri
        self._state_tokens: dict[str, tuple[RaidOAuthState, float]] = {}  # state -> (meta, ts)
        self._pending_auth_urls: dict[str, str] = {}  # state -> full_twitch_auth_url
        self._lock = asyncio.Lock()
        self.token_error_handler = TokenErrorHandler()
        self._client_auth_blocked_until: float = 0.0
        self._client_auth_block_reason: str | None = None

        # Basis-URL für Short-Redirect ableiten (z.B. https://raid.earlysalty.com)
        _parts = redirect_uri.split("/twitch/", 1)
        self._base_url: str = _parts[0] if len(_parts) > 1 else ""

    def is_client_auth_blocked(self) -> bool:
        return time.time() < self._client_auth_blocked_until

    def _block_client_auth(
        self,
        reason: str,
        *,
        cooldown_seconds: float = 900.0,
    ) -> TwitchClientConfigError:
        self._client_auth_blocked_until = time.time() + float(cooldown_seconds)
        self._client_auth_block_reason = reason
        return TwitchClientConfigError(reason)

    def _raise_if_client_auth_blocked(self) -> None:
        if self.is_client_auth_blocked():
            raise TwitchClientConfigError(
                self._client_auth_block_reason
                or "Twitch OAuth requests are temporarily blocked due to invalid client configuration."
            )

    def _ensure_client_credentials(self) -> None:
        if self.client_id and self.client_secret:
            return

        reason = (
            "Twitch client credentials are missing or blank after trimming whitespace. "
            "Verify TWITCH_CLIENT_ID and TWITCH_CLIENT_SECRET, then restart the bot."
        )
        if not self.is_client_auth_blocked() or self._client_auth_block_reason != reason:
            log.error(reason)
        raise self._block_client_auth(reason)

    def _persist_state_token(
        self,
        state: str,
        state_info: RaidOAuthState,
        created_ts: float,
    ) -> None:
        """Persist OAuth state to shared DB so callback verification works across instances/restarts."""
        expires_at = datetime.fromtimestamp(created_ts, tz=UTC) + timedelta(
            seconds=_OAUTH_STATE_TTL_SECONDS
        )
        try:
            with get_conn() as conn:
                conn.execute(
                    """
                    INSERT INTO oauth_state_tokens
                        (state_token, platform, streamer_login, redirect_uri, pkce_verifier, expires_at)
                    VALUES (?, ?, ?, ?, ?, ?)
                    ON CONFLICT (state_token) DO UPDATE SET
                        platform = excluded.platform,
                        streamer_login = excluded.streamer_login,
                        redirect_uri = excluded.redirect_uri,
                        pkce_verifier = excluded.pkce_verifier,
                        expires_at = excluded.expires_at
                    """,
                    (
                        state,
                        _OAUTH_STATE_PLATFORM_RAID,
                        state_info.requested_login,
                        self.redirect_uri,
                        _serialize_state_meta(
                            state_info.scope_profile,
                            expected_twitch_login=state_info.expected_twitch_login,
                            discord_user_id=state_info.discord_user_id,
                        ),
                        expires_at.isoformat(),
                    ),
                )
        except Exception:
            log.exception(
                "Failed to persist raid oauth state for %s",
                _mask_log_identifier(state_info.requested_login),
            )

    def _lookup_state_token(self, state: str) -> RaidOAuthState | None:
        """Look up a still-valid raid OAuth state without consuming it."""
        try:
            with get_conn() as conn:
                row = conn.execute(
                    """
                    SELECT streamer_login, pkce_verifier
                    FROM oauth_state_tokens
                    WHERE state_token = ?
                      AND platform = ?
                      AND expires_at > ?
                    LIMIT 1
                    """,
                    (
                        state,
                        _OAUTH_STATE_PLATFORM_RAID,
                        datetime.now(UTC).isoformat(),
                    ),
                ).fetchone()
        except Exception:
            log.exception("Failed to lookup raid oauth state")
            return None

        if not row:
            return None
        login = str(row["streamer_login"] if hasattr(row, "keys") else row[0] or "").strip()
        if not login:
            return None
        meta_raw = row["pkce_verifier"] if hasattr(row, "keys") else row[1]
        scope_profile, expected_login, discord_user_id = _parse_state_meta(meta_raw)
        return RaidOAuthState(
            requested_login=login,
            scope_profile=scope_profile,
            expected_twitch_login=expected_login,
            discord_user_id=discord_user_id,
        )

    def _consume_state_token(self, state: str) -> RaidOAuthState | None:
        """Atomically consume a still-valid raid OAuth state from shared DB."""
        try:
            with get_conn() as conn:
                row = conn.execute(
                    """
                    DELETE FROM oauth_state_tokens
                    WHERE state_token = ?
                      AND platform = ?
                      AND expires_at > ?
                    RETURNING streamer_login, pkce_verifier
                    """,
                    (
                        state,
                        _OAUTH_STATE_PLATFORM_RAID,
                        datetime.now(UTC).isoformat(),
                    ),
                ).fetchone()
        except Exception:
            log.exception("Failed to consume raid oauth state")
            return None

        if not row:
            return None
        login = str(row["streamer_login"] if hasattr(row, "keys") else row[0] or "").strip()
        if not login:
            return None
        meta_raw = row["pkce_verifier"] if hasattr(row, "keys") else row[1]
        scope_profile, expected_login, discord_user_id = _parse_state_meta(meta_raw)
        return RaidOAuthState(
            requested_login=login,
            scope_profile=scope_profile,
            expected_twitch_login=expected_login,
            discord_user_id=discord_user_id,
        )

    @staticmethod
    def _has_existing_auth_row(
        conn,
        *,
        twitch_user_id: str | None = None,
        twitch_login: str | None = None,
    ) -> bool:
        normalized_user_id = str(twitch_user_id or "").strip()
        normalized_login = _normalize_twitch_login(twitch_login)
        if not normalized_user_id and not normalized_login:
            return False
        row = conn.execute(
            """
            SELECT 1
            FROM twitch_raid_auth
            WHERE (? <> '' AND twitch_user_id = ?)
               OR (? <> '' AND LOWER(COALESCE(twitch_login, '')) = ?)
            LIMIT 1
            """,
            (
                normalized_user_id,
                normalized_user_id,
                normalized_login or "",
                normalized_login or "",
            ),
        ).fetchone()
        return bool(row)

    def _has_existing_streamer_context(self, twitch_login: str) -> bool:
        normalized_login = str(twitch_login or "").strip().lower()
        if not normalized_login:
            return False

        discord_user_id = ""
        if normalized_login.startswith("discord:"):
            discord_user_id = normalized_login.split(":", 1)[1].strip()

        try:
            with get_conn() as conn:
                if discord_user_id:
                    identity = load_streamer_identity(conn, discord_user_id=discord_user_id)
                    if not identity:
                        return False
                    identity_login = (
                        identity["twitch_login"] if hasattr(identity, "keys") else identity[1]
                    )
                    identity_user_id = (
                        identity["twitch_user_id"] if hasattr(identity, "keys") else identity[0]
                    )
                    return bool(
                        load_active_partner(
                            conn,
                            twitch_login=identity_login,
                            twitch_user_id=identity_user_id,
                        )
                        or self._has_existing_auth_row(
                            conn,
                            twitch_user_id=identity_user_id,
                            twitch_login=identity_login,
                        )
                    )
                return bool(
                    load_active_partner(conn, twitch_login=normalized_login)
                    or self._has_existing_auth_row(conn, twitch_login=normalized_login)
                )
        except Exception:
            log.debug(
                "Could not resolve existing streamer context for %s",
                _mask_log_identifier(twitch_login),
                exc_info=True,
            )
            return False

    def _resolve_scope_profile(self, twitch_login: str, requested_profile: str | None) -> str:
        normalized = str(requested_profile or "").strip().lower()
        if normalized == DASHBOARD_REAUTH_SCOPE_PROFILE:
            return DASHBOARD_REAUTH_SCOPE_PROFILE
        if normalized == BASE_SCOPE_PROFILE:
            return BASE_SCOPE_PROFILE
        if self._has_existing_streamer_context(twitch_login):
            return DASHBOARD_REAUTH_SCOPE_PROFILE
        return BASE_SCOPE_PROFILE

    def _build_state_info(
        self,
        twitch_login: str,
        *,
        scope_profile: str,
        expected_twitch_login: str | None = None,
        discord_user_id: str | int | None = None,
    ) -> RaidOAuthState:
        requested_login = str(twitch_login or "").strip().lower()
        resolved_profile = self._resolve_scope_profile(requested_login, scope_profile)
        normalized_expected_login = _normalize_twitch_login(expected_twitch_login)
        if not normalized_expected_login and not requested_login.startswith("discord:"):
            normalized_expected_login = requested_login
        return RaidOAuthState(
            requested_login=requested_login,
            scope_profile=resolved_profile,
            expected_twitch_login=normalized_expected_login,
            discord_user_id=_normalize_state_discord_user_id(discord_user_id),
        )

    def _build_authorize_url(self, *, state: str, scope_profile: str) -> str:
        params = {
            "client_id": self.client_id,
            "redirect_uri": self.redirect_uri,
            "response_type": "code",
            "scope": " ".join(scopes_for_profile(scope_profile)),
            "state": state,
            "force_verify": "true",
        }
        return f"{TWITCH_AUTHORIZE_URL}?{urlencode(params)}"

    # ------------------------------------------------------------------
    # Encryption helpers (field-level AES-256-GCM)
    # ------------------------------------------------------------------

    @staticmethod
    def _get_crypto_optional():
        """Gibt FieldCrypto zurück oder None wenn nicht verfügbar."""
        try:
            from service.field_crypto import get_crypto  # noqa: PLC0415

            return get_crypto()
        except ModuleNotFoundError:
            try:
                from ..compat.field_crypto import get_crypto  # noqa: PLC0415

                return get_crypto()
            except Exception as exc:
                log.warning("field_crypto fallback nicht verfügbar: %s", exc)
                return None
        except Exception as exc:
            log.warning("field_crypto nicht verfügbar: %s", exc)
            return None

    def _try_decrypt(self, blob, aad: str, context: str):
        """Entschlüsselt ein enc-Blob; gibt None zurück wenn nicht möglich."""
        if not blob:
            return None
        crypto = self._get_crypto_optional()
        if not crypto:
            return None
        try:
            return crypto.decrypt_field(bytes(blob), aad)
        except Exception:
            log.warning(
                "Secure field decode failed [%s]; encrypted value will be ignored.",
                context,
            )
            log.debug("Secure field decode traceback [%s]", context, exc_info=True)
            return None

    def _try_encrypt(self, plaintext, aad: str, context: str):
        """Verschlüsselt Plaintext zu enc-Blob; gibt None zurück wenn nicht möglich."""
        if not plaintext:
            return None
        crypto = self._get_crypto_optional()
        if not crypto:
            return None
        try:
            return crypto.encrypt_field(str(plaintext), aad)
        except Exception:
            log.error(
                "Secure field encode failed [%s]; fallback remains active.",
                context,
            )
            log.debug("Secure field encode traceback [%s]", context, exc_info=True)
            return None

    def _resolve_token(self, enc_blob, aad: str, field_name: str, user_id: str):
        """
        Liest Token ausschließlich aus der verschlüsselten Spalte.

        Gibt None zurück, wenn enc fehlt oder nicht entschlüsselbar ist.
        """
        ctx = f"{field_name}|user={_mask_log_identifier(user_id)}"
        if enc_blob is None:
            log.error(
                "Encrypted field missing (enc=NULL) [%s] - plaintext fallback is disabled.",
                ctx,
            )
            return None

        decrypted = self._try_decrypt(enc_blob, aad, ctx)
        if decrypted is not None:
            log.debug("Encrypted field loaded [%s]", ctx)
            return decrypted

        log.error(
            "Encrypted field unreadable [%s] - plaintext fallback is disabled.",
            ctx,
        )
        return None

    def _write_token_refresh(
        self,
        conn,
        user_id: str,
        access_token: str,
        refresh_token: str,
        expires_at_iso: str,
    ) -> None:
        """
        Schreibt refreshte Tokens verschlüsselt in die DB.
        Klartext-Spalten werden zur Sicherheit nur noch mit 'ENC' überschrieben.
        """
        access_enc = self._try_encrypt(
            access_token,
            f"twitch_raid_auth|access_token|{user_id}|1",
            f"access_token|user={_mask_log_identifier(user_id)}",
        )
        refresh_enc = self._try_encrypt(
            refresh_token,
            f"twitch_raid_auth|refresh_token|{user_id}|1",
            f"refresh_token|user={_mask_log_identifier(user_id)}",
        )

        if access_enc is None or refresh_enc is None:
            log.error(
                "Refresh for user_id=%s: encrypted write failed; tokens NOT updated to avoid lockout.",
                _mask_log_identifier(user_id),
            )
            return

        conn.execute(
            """
            UPDATE twitch_raid_auth
               SET access_token = 'ENC', refresh_token = 'ENC',
                   access_token_enc = ?, refresh_token_enc = ?,
                   enc_version = 1, enc_kid = 'v1',
                   token_expires_at = ?, last_refreshed_at = CURRENT_TIMESTAMP
             WHERE twitch_user_id = ?
            """,
            (
                access_enc,
                refresh_enc,
                expires_at_iso,
                user_id,
            ),
        )

    # ------------------------------------------------------------------

    def generate_auth_url(
        self,
        twitch_login: str,
        *,
        scope_profile: str = AUTO_SCOPE_PROFILE,
        expected_twitch_login: str | None = None,
        discord_user_id: str | int | None = None,
    ) -> str:
        """Generiert eine OAuth-URL für Streamer-Autorisierung."""
        state = secrets.token_urlsafe(16)
        ts = time.time()
        state_info = self._build_state_info(
            twitch_login,
            scope_profile=scope_profile,
            expected_twitch_login=expected_twitch_login,
            discord_user_id=discord_user_id,
        )
        self._state_tokens[state] = (state_info, ts)
        self._persist_state_token(state, state_info, ts)
        return self._build_authorize_url(state=state, scope_profile=state_info.scope_profile)

    def generate_discord_button_url(
        self,
        twitch_login: str,
        *,
        scope_profile: str = AUTO_SCOPE_PROFILE,
        expected_twitch_login: str | None = None,
        discord_user_id: str | int | None = None,
    ) -> str:
        """Generiert einen kurzen Redirect-URL für Discord-Buttons (max 512 Zeichen).

        Discord-Button-URLs dürfen max. 512 Zeichen lang sein.  Der volle Twitch-
        OAuth-URL überschreitet dieses Limit.  Wir speichern den vollen URL im
        _pending_auth_urls-Dict und geben einen kurzen /twitch/raid/go?state=…
        Redirect-Link zurück, den der Webserver dann weiterleitet.
        """
        state = secrets.token_urlsafe(16)
        ts = time.time()
        state_info = self._build_state_info(
            twitch_login,
            scope_profile=scope_profile,
            expected_twitch_login=expected_twitch_login,
            discord_user_id=discord_user_id,
        )
        self._state_tokens[state] = (state_info, ts)
        self._persist_state_token(state, state_info, ts)

        full_url = self._build_authorize_url(state=state, scope_profile=state_info.scope_profile)
        self._pending_auth_urls[state] = full_url

        if self._base_url:
            return f"{self._base_url}/twitch/raid/go?state={state}"
        # Fallback: vollen URL trotzdem zurückgeben (kann bei sehr langen URLs fehlschlagen)
        return full_url

    def get_pending_auth_url(self, state: str) -> str | None:
        """Gibt den gespeicherten vollen OAuth-URL für einen State zurück (einmalig)."""
        # Wir löschen den Eintrag NICHT hier – verify_state macht das beim echten Callback.
        entry = self._pending_auth_urls.get(state)
        token_data = self._state_tokens.get(state)
        if entry and token_data and time.time() - token_data[1] <= _OAUTH_STATE_TTL_SECONDS:
            return entry

        # Fallback für split/multi-instance/restart:
        # wenn der state in DB noch gültig ist, den vollen OAuth-URL on-the-fly rekonstruieren.
        persisted_state = self._lookup_state_token(state)
        if not persisted_state:
            self._pending_auth_urls.pop(state, None)
            return None
        full_url = self._build_authorize_url(state=state, scope_profile=persisted_state.scope_profile)
        self._pending_auth_urls[state] = full_url
        return full_url

    def consume_state_details(self, state: str) -> RaidOAuthState | None:
        """Verifiziert State-Token und gibt die vollständigen Metadaten zurück."""
        self._pending_auth_urls.pop(state, None)
        state_from_db = self._consume_state_token(state)
        if state_from_db:
            self._state_tokens.pop(state, None)
            return state_from_db

        data = self._state_tokens.pop(state, None)
        if not data:
            return None

        state_info, timestamp = data
        if time.time() - timestamp > _OAUTH_STATE_TTL_SECONDS:
            log.warning("OAuth state for %s expired", state_info.requested_login)
            return None

        return state_info

    def verify_state(self, state: str) -> str | None:
        """Verifiziert State-Token und gibt den zugehörigen Login zurück (max 10 Min alt)."""
        state_info = self.consume_state_details(state)
        return state_info.requested_login if state_info else None

    def cleanup_states(self) -> None:
        """Entfernt abgelaufene State-Tokens aus dem Speicher."""
        now = time.time()
        expired = [
            s
            for s, (_state_info, ts) in self._state_tokens.items()
            if now - ts > _OAUTH_STATE_TTL_SECONDS
        ]
        for s in expired:
            del self._state_tokens[s]
            self._pending_auth_urls.pop(s, None)
        try:
            with get_conn() as conn:
                conn.execute(
                    """
                    DELETE FROM oauth_state_tokens
                    WHERE platform = ?
                      AND expires_at <= ?
                    """,
                    (_OAUTH_STATE_PLATFORM_RAID, datetime.now(UTC).isoformat()),
                )
        except Exception:
            log.exception("Failed to cleanup persisted raid oauth states")
        if expired:
            log.debug("Cleaned up %d expired auth states", len(expired))

    async def exchange_code_for_token(self, code: str, session: aiohttp.ClientSession) -> dict:
        """Tauscht Authorization Code gegen User Access Token."""
        self._raise_if_client_auth_blocked()
        self._ensure_client_credentials()
        data = {
            "client_id": self.client_id,
            "client_secret": self.client_secret,
            "code": code,
            "grant_type": "authorization_code",
            "redirect_uri": self.redirect_uri,
        }

        async with session.post(TWITCH_TOKEN_URL, data=data) as r:
            if r.status != 200:
                try:
                    err_body = await r.text()
                except Exception:
                    err_body = "<unreadable>"
                if is_invalid_client_response(r.status, err_body):
                    reason = (
                        "Twitch client credentials were rejected by Twitch OAuth "
                        "(invalid client). Verify TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET, "
                        "rotate the secret if needed, then restart the bot. "
                        "Suppressing additional Twitch OAuth requests for 15 minutes."
                    )
                    if not self.is_client_auth_blocked() or self._client_auth_block_reason != reason:
                        log.error(reason)
                    raise self._block_client_auth(reason)
                log.error(
                    "OAuth exchange failed with HTTP %s: %s",
                    r.status,
                    err_body[:500],
                )
                r.raise_for_status()
            data = await r.json()
            self._client_auth_blocked_until = 0.0
            self._client_auth_block_reason = None
            return data

    async def refresh_token(
        self,
        refresh_token: str,
        session: aiohttp.ClientSession,
        twitch_user_id: str = None,
        twitch_login: str = None,
    ) -> dict:
        """Erneuert einen abgelaufenen User Access Token."""
        self._raise_if_client_auth_blocked()
        self._ensure_client_credentials()
        data = {
            "client_id": self.client_id,
            "client_secret": self.client_secret,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }

        async with session.post(TWITCH_TOKEN_URL, data=data) as r:
            if r.status != 200:
                txt = await r.text()
                if is_invalid_client_response(r.status, txt):
                    reason = (
                        "Twitch client credentials were rejected by Twitch OAuth "
                        "(invalid client). Verify TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET, "
                        "rotate the secret if needed, then restart the bot. "
                        "Suppressing additional Twitch OAuth refresh attempts for 15 minutes."
                    )
                    if not self.is_client_auth_blocked() or self._client_auth_block_reason != reason:
                        log.error(reason)
                    raise self._block_client_auth(reason)
                error_msg = f"HTTP {r.status}: {txt[:300]}"
                log.error(
                    "OAuth refresh request failed for %s with HTTP status %s",
                    twitch_login or "unknown",
                    r.status,
                )

                # Nur echte "invalid refresh token"/"invalid_grant" Antworten dürfen
                # den Blacklist-Counter erhöhen. Andere 400er sind oft Request-/Config-
                # Probleme und sollten keinen Streamer sperren.
                response_lc = txt.lower()
                parsed_message = ""
                parsed_error = ""
                try:
                    payload = json.loads(txt)
                    if isinstance(payload, dict):
                        parsed_message = str(payload.get("message", "")).lower()
                        parsed_error = str(payload.get("error", "")).lower()
                except Exception:
                    log.debug("OAuth refresh error payload was not valid JSON", exc_info=True)

                is_invalid_refresh_grant = r.status == 400 and (
                    "invalid refresh token" in response_lc
                    or "invalid refresh token" in parsed_message
                    or "invalid_grant" in response_lc
                    or "invalid_grant" in parsed_message
                    or "invalid_grant" in parsed_error
                )

                if is_invalid_refresh_grant and twitch_user_id and twitch_login:
                    log.warning(
                        "Invalid OAuth refresh grant for %s (ID: %s) - adding to blacklist",
                        twitch_login,
                        twitch_user_id,
                    )

                    # Zur Blacklist hinzufügen
                    self.token_error_handler.add_to_blacklist(
                        twitch_user_id=twitch_user_id,
                        twitch_login=twitch_login,
                        error_message=error_msg,
                    )

                    # Discord-Benachrichtigung senden (async, fire-and-forget)
                    if hasattr(self, "_discord_bot") and self._discord_bot:
                        asyncio.create_task(
                            self.token_error_handler.notify_token_error(
                                twitch_user_id=twitch_user_id,
                                twitch_login=twitch_login,
                                error_message=error_msg,
                            )
                        )
                elif r.status == 400 and twitch_login:
                    log.warning(
                        "OAuth refresh for %s returned HTTP 400 but not an invalid refresh grant; skipping blacklist increment",
                        twitch_login,
                    )

                r.raise_for_status()
            data = await r.json()
            self._client_auth_blocked_until = 0.0
            self._client_auth_block_reason = None
            return data

    async def refresh_all_tokens(self, session: aiohttp.ClientSession) -> int:
        """
        Refreshes tokens for all authorized users if they are close to expiry (< 2 hours).
        Returns the number of refreshed tokens.
        """
        if self.is_client_auth_blocked():
            return 0
        refreshed_count = 0
        with get_conn() as conn:
            # Hole alle User mit raid_enabled=TRUE
            rows = conn.execute(
                """
                SELECT twitch_user_id, twitch_login,
                       refresh_token_enc,
                       enc_version, token_expires_at
                FROM twitch_raid_auth
                WHERE raid_enabled IS TRUE
                  AND needs_reauth IS NOT TRUE
                """
            ).fetchall()

        if not rows:
            return 0

        now_ts = time.time()

        # Parallelisierung möglich, aber hier sequenziell zur Sicherheit (Rate Limits)
        for row in rows:
            user_id = row["twitch_user_id"]
            login = row["twitch_login"]
            _enc_v = row["enc_version"] or 1
            refresh_tok = self._resolve_token(
                row["refresh_token_enc"],
                f"twitch_raid_auth|refresh_token|{user_id}|{_enc_v}",
                "refresh_token",
                str(user_id),
            )
            expires_iso = row["token_expires_at"]

            # Sicherheits-Check: Falls doch auf Blacklist, überspringen
            if self.token_error_handler.is_token_blacklisted(user_id):
                continue

            # Cooldown: Nicht zu schnell nach einem Fehler erneut versuchen (min. 2h Abstand)
            if self.token_error_handler.has_recent_failure(user_id):
                log.debug(
                    "Skipping OAuth refresh for broadcaster=%s (recent failure, cooldown active)",
                    _mask_log_identifier(login),
                )
                continue

            # Bug Fix: NULL refresh_token → kein Refresh möglich, überspringen
            if not refresh_tok:
                log.warning(
                    "No refresh grant stored for broadcaster=%s (user_id=%s); skipping",
                    _mask_log_identifier(login),
                    _mask_log_identifier(user_id),
                )
                continue

            expires_ts = _parse_expiry_ts(expires_iso)
            if not expires_ts:
                log.warning("Invalid expiry date for %s, forcing refresh", login)

            # Refresh wenn weniger als 2 Stunden (7200s) gültig
            if now_ts < expires_ts - 7200:
                continue

            async with self._lock:
                try:
                    # Double-Check im Lock: Expiry UND Refresh-Token neu lesen.
                    # Wenn ein anderer Prozess den Token bereits refresht hat, ist
                    # refresh_token_enc in der DB anders als unser pre-lock refresh_tok.
                    with get_conn() as conn:
                        current = conn.execute(
                            """SELECT token_expires_at, refresh_token_enc, enc_version
                               FROM twitch_raid_auth WHERE twitch_user_id = ?""",
                            (user_id,),
                        ).fetchone()

                    if current:
                        curr_iso = current[0]
                        if not curr_iso:
                            log.warning(
                                "Missing expiry timestamp in double-check for broadcaster=%s; skipping",
                                _mask_log_identifier(login),
                            )
                            continue
                        curr_ts = _parse_expiry_ts(curr_iso)
                        if time.time() < curr_ts - 7200:
                            continue  # Wurde bereits refresht (Expiry weit genug in der Zukunft)

                        # Refresh-Token geändert? → anderer Prozess hat bereits refresht.
                        # Frischesten Token aus DB verwenden, nicht den pre-lock-read.
                        _curr_enc_v = current["enc_version"] or 1
                        curr_refresh_tok = self._resolve_token(
                            current["refresh_token_enc"],
                            f"twitch_raid_auth|refresh_token|{user_id}|{_curr_enc_v}",
                            "refresh_token.dc2",
                            str(user_id),
                        )
                        if curr_refresh_tok and curr_refresh_tok != refresh_tok:
                            log.info(
                                "Refresh token changed for broadcaster=%s since pre-lock read "
                                "(another process refreshed). Using updated token.",
                                _mask_log_identifier(login),
                            )
                            refresh_tok = curr_refresh_tok
                except Exception as exc:
                    # Bug Fix: Bei Exception im Double-Check SKIP statt Fallthrough zum Refresh
                    log.warning(
                        "Double-check failed for %s, skipping refresh to be safe: %s",
                        login,
                        exc,
                    )
                    continue

                log.debug("Auto-refreshing OAuth grant for %s (background maintenance)", login)
                try:
                    token_data = await self.refresh_token(
                        refresh_tok, session, twitch_user_id=user_id, twitch_login=login
                    )
                    new_access = token_data["access_token"]
                    new_refresh = token_data.get("refresh_token") or refresh_tok
                    expires_in = _safe_int(token_data.get("expires_in", 3600), 3600)

                    new_expires_at = datetime.now(UTC).timestamp() + expires_in
                    new_expires_iso = datetime.fromtimestamp(new_expires_at, UTC).isoformat()

                    with get_conn() as conn:
                        self._write_token_refresh(
                            conn, user_id, new_access, new_refresh, new_expires_iso
                        )
                        # autocommit – no explicit commit needed
                    # Erfolgreicher Refresh → eventuelle Fehler-Counter zurücksetzen
                    self.token_error_handler.clear_failure_count(user_id)
                    refreshed_count += 1
                    # Kleines Delay um API Spikes zu vermeiden
                    await asyncio.sleep(0.5)

                except Exception as exc:
                    if isinstance(exc, TwitchClientConfigError):
                        break
                    if isinstance(exc, RuntimeError) and "Session is closed" in str(exc):
                        log.warning(
                            "Background refresh aborted for %s: shared HTTP session is closed",
                            login,
                        )
                        raise
                    log.error("Background refresh failed for %s: %s", login, exc)

        if refreshed_count > 0:
            log.debug("Maintenance: Refreshed %d user tokens", refreshed_count)

        return refreshed_count

    async def snapshot_and_flag_reauth(self) -> int:
        """Setzt needs_reauth=1 für alle. Gibt Anzahl betroffener Zeilen zurück."""
        with get_conn() as conn:
            conn.execute("""
                UPDATE twitch_raid_auth
                SET needs_reauth = TRUE,
                    reauth_notified_at = NULL
                WHERE needs_reauth IS NOT TRUE
                  AND (
                        access_token_enc IS NOT NULL
                     OR refresh_token_enc IS NOT NULL
                     OR NULLIF(access_token, '') IS NOT NULL
                     OR NULLIF(refresh_token, '') IS NOT NULL
                     OR authorized_at IS NOT NULL
                  )
            """)
            count = conn.execute("SELECT changes()").fetchone()[0]
        log.info(
            "snapshot_and_flag_reauth: %d Tokens auf needs_reauth=1 gesetzt",
            count,
        )
        return count

    def has_saved_auth_record(
        self,
        *,
        twitch_user_id: str | None = None,
        twitch_login: str | None = None,
    ) -> bool:
        try:
            with get_conn() as conn:
                return self._has_existing_auth_row(
                    conn,
                    twitch_user_id=twitch_user_id,
                    twitch_login=twitch_login,
                )
        except Exception:
            log.debug(
                "Could not resolve existing auth record for %s",
                _mask_log_identifier(twitch_user_id or twitch_login),
                exc_info=True,
            )
            return False

    def save_auth(
        self,
        twitch_user_id: str,
        twitch_login: str,
        access_token: str,
        refresh_token: str,
        expires_in: int | str,
        scopes: list[str],
        *,
        scope_profile: str = AUTO_SCOPE_PROFILE,
        activate_raid_features: bool = True,
    ) -> None:
        """Speichert OAuth-Tokens verschlüsselt in der Datenbank."""
        now = datetime.now(UTC)
        expires_at = now.timestamp() + _safe_int(expires_in, 3600)
        expires_at_iso = datetime.fromtimestamp(expires_at, UTC).isoformat()
        authorized_at = now.isoformat()
        resolved_profile = self._resolve_scope_profile(twitch_login, scope_profile)
        expected_scopes = list(scopes_for_profile(resolved_profile))
        normalized_scopes = [
            str(scope or "").strip()
            for scope in scopes
            if str(scope or "").strip()
        ]
        if set(normalized_scopes) != set(expected_scopes):
            raise ValueError(f"unexpected_scopes_for_profile:{resolved_profile}")

        access_enc = self._try_encrypt(
            access_token,
            f"twitch_raid_auth|access_token|{twitch_user_id}|1",
            f"access_token|user={_mask_log_identifier(twitch_user_id)}",
        )
        refresh_enc = self._try_encrypt(
            refresh_token,
            f"twitch_raid_auth|refresh_token|{twitch_user_id}|1",
            f"refresh_token|user={_mask_log_identifier(twitch_user_id)}",
        )

        if access_enc is None or refresh_enc is None:
            log.error(
                "save_auth für user_id=%s: enc-Verschlüsselung fehlgeschlagen – Speicherung abgebrochen (Sicherheits-Policy).",
                _mask_log_identifier(twitch_user_id),
            )
            return

        with get_conn() as conn:
            existing_row = conn.execute(
                """
                SELECT raid_enabled
                FROM twitch_raid_auth
                WHERE twitch_user_id = ?
                   OR LOWER(COALESCE(twitch_login, '')) = LOWER(?)
                LIMIT 1
                """,
                (
                    twitch_user_id,
                    twitch_login,
                ),
            ).fetchone()
            raid_enabled_value = (
                bool(existing_row["raid_enabled"] if hasattr(existing_row, "keys") else existing_row[0])
                if existing_row
                else bool(activate_raid_features)
            )
            conn.execute(
                """
                INSERT INTO twitch_raid_auth
                (twitch_user_id, twitch_login, access_token, refresh_token,
                 access_token_enc, refresh_token_enc, enc_version, enc_kid,
                 token_expires_at, scopes, authorized_at, raid_enabled)
                VALUES (?, ?, 'ENC', 'ENC', ?, ?, 1, 'v1', ?, ?, ?, ?)
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    twitch_login      = EXCLUDED.twitch_login,
                    access_token_enc  = EXCLUDED.access_token_enc,
                    refresh_token_enc = EXCLUDED.refresh_token_enc,
                    enc_version       = EXCLUDED.enc_version,
                    enc_kid           = EXCLUDED.enc_kid,
                    token_expires_at  = EXCLUDED.token_expires_at,
                    scopes            = EXCLUDED.scopes,
                    authorized_at     = EXCLUDED.authorized_at,
                    raid_enabled      = EXCLUDED.raid_enabled
                """,
                (
                    twitch_user_id,
                    twitch_login,
                    access_enc,
                    refresh_enc,
                    expires_at_iso,
                    " ".join(expected_scopes),
                    authorized_at,
                    raid_enabled_value,
                ),
            )
            # Re-Auth abgeschlossen: needs_reauth zurücksetzen
            conn.execute(
                """
                UPDATE twitch_raid_auth
                SET needs_reauth = FALSE,
                    reauth_notified_at = NULL
                WHERE twitch_user_id = ?
                """,
                (twitch_user_id,),
            )
            copied = backfill_tracked_stats_from_category(conn, twitch_login)
            if copied:
                log.info(
                    "Backfilled %d category samples into tracked for %s during raid auth save",
                    copied,
                    twitch_login,
                )
            # autocommit – no explicit commit needed

        # Bei erfolgreicher Auth: Von Blacklist entfernen (falls vorhanden)
        self.token_error_handler.remove_from_blacklist(twitch_user_id)

        log.info("Saved raid auth for %s (user_id=%s)", twitch_login, twitch_user_id)

    async def get_tokens_for_user(
        self, twitch_user_id: str, session: aiohttp.ClientSession
    ) -> tuple[str, str] | None:
        """
        Holt Access- UND Refresh-Token für einen User.
        Erneuert den Token automatisch, falls abgelaufen.
        Wird bewusst auch genutzt, wenn raid_enabled=0 (Chat-Bot/Moderation).
        """
        if self.is_client_auth_blocked():
            return None
        # Blacklist check
        if self.token_error_handler.is_token_blacklisted(twitch_user_id):
            return None

        # Cooldown: Nicht zu schnell nach einem Fehler erneut versuchen
        if self.token_error_handler.has_recent_failure(twitch_user_id):
            log.debug(
                "Auth lookup cooldown active for user_id=%s",
                _mask_log_identifier(twitch_user_id),
            )
            return None

        with get_conn() as conn:
            row = conn.execute(
                """
                SELECT access_token_enc, refresh_token_enc,
                       enc_version, token_expires_at, twitch_login, needs_reauth
                FROM twitch_raid_auth
                WHERE twitch_user_id = ?
                """,
                (twitch_user_id,),
            ).fetchone()

        if not row:
            return None

        needs_reauth = bool(row["needs_reauth"] if hasattr(row, "keys") else row[5])
        if needs_reauth:
            log.info(
                "Auth lookup denied for user_id=%s: dashboard reauth required",
                _mask_log_identifier(twitch_user_id),
            )
            return None

        _enc_v = row["enc_version"] or 1
        _uid = str(twitch_user_id)
        access_token = self._resolve_token(
            row["access_token_enc"],
            f"twitch_raid_auth|access_token|{_uid}|{_enc_v}",
            "access_token",
            _uid,
        )
        refresh_token = self._resolve_token(
            row["refresh_token_enc"],
            f"twitch_raid_auth|refresh_token|{_uid}|{_enc_v}",
            "refresh_token",
            _uid,
        )
        if not access_token or not refresh_token:
            log.warning(
                "Stored credentials missing for user_id=%s during auth lookup",
                _mask_log_identifier(twitch_user_id),
            )
            return None
        expires_at_iso = row["token_expires_at"]
        twitch_login = row["twitch_login"]
        expires_at = _parse_expiry_ts(expires_at_iso)

        # Token noch gültig? (5 Minuten Puffer)
        if time.time() < expires_at - 300:
            return access_token, refresh_token

        # Token abgelaufen -> refresh
        async with self._lock:
            # Erneuter Check innerhalb des Locks (Double-Check Locking Pattern)
            with get_conn() as conn:
                row_check = conn.execute(
                    """SELECT token_expires_at,
                              access_token_enc,
                              refresh_token_enc,
                              enc_version,
                              needs_reauth
                       FROM twitch_raid_auth WHERE twitch_user_id = ?""",
                    (twitch_user_id,),
                ).fetchone()

            if row_check:
                if bool(row_check["needs_reauth"] if hasattr(row_check, "keys") else row_check[4]):
                    log.info(
                        "Auth lookup denied for user_id=%s inside refresh lock: dashboard reauth required",
                        _mask_log_identifier(twitch_user_id),
                    )
                    return None
                _dc_uid = str(twitch_user_id)
                _dc_v = row_check["enc_version"] or 1
                curr_expires_iso = row_check["token_expires_at"]
                curr_access = self._resolve_token(
                    row_check["access_token_enc"],
                    f"twitch_raid_auth|access_token|{_dc_uid}|{_dc_v}",
                    "access_token.dc",
                    _dc_uid,
                )
                curr_refresh = self._resolve_token(
                    row_check["refresh_token_enc"],
                    f"twitch_raid_auth|refresh_token|{_dc_uid}|{_dc_v}",
                    "refresh_token.dc",
                    _dc_uid,
                )
                curr_expires = _parse_expiry_ts(curr_expires_iso)
                if time.time() < curr_expires - 300 and curr_access and curr_refresh:
                    return curr_access, curr_refresh

            log.info(
                "Refreshing OAuth grant for broadcaster=%s (auth lookup)",
                _mask_log_identifier(twitch_login),
            )
            try:
                token_data = await self.refresh_token(
                    refresh_token,
                    session,
                    twitch_user_id=twitch_user_id,
                    twitch_login=twitch_login,
                )
                new_access_token = token_data["access_token"]
                new_refresh_token = token_data.get("refresh_token") or refresh_token
                expires_in = _safe_int(token_data.get("expires_in", 3600), 3600)

                # Token in DB aktualisieren
                new_expires_at = datetime.now(UTC).timestamp() + expires_in
                new_expires_at_iso = datetime.fromtimestamp(new_expires_at, UTC).isoformat()

                with get_conn() as conn:
                    self._write_token_refresh(
                        conn,
                        twitch_user_id,
                        new_access_token,
                        new_refresh_token,
                        new_expires_at_iso,
                    )
                    # autocommit – no explicit commit needed

                self.token_error_handler.clear_failure_count(twitch_user_id)
                return new_access_token, new_refresh_token
            except Exception:
                log.exception("Failed to refresh OAuth grant for %s", twitch_login)
                return None

    async def get_valid_token(
        self, twitch_user_id: str, session: aiohttp.ClientSession
    ) -> str | None:
        """
        Holt ein gültiges Access Token für den Streamer.
        Erneuert es automatisch, falls abgelaufen.

        WICHTIG: Wenn der Token auf der Blacklist steht (ungültiger Refresh-Token),
        wird None zurückgegeben ohne Refresh-Versuch.
        """
        # SCHRITT 1: Blacklist-Check BEVOR wir überhaupt zur DB gehen
        if self.token_error_handler.is_token_blacklisted(twitch_user_id):
            log.warning(
                "OAuth grant for user_id=%s is blacklisted - skipping refresh attempt",
                twitch_user_id,
            )
            return None

        # Cooldown: Wenn kürzlich ein Fehler aufgetreten ist, nicht sofort nochmal versuchen
        if self.token_error_handler.has_recent_failure(twitch_user_id):
            log.debug(
                "OAuth grant for user_id=%s has recent failure, cooldown active - returning None",
                twitch_user_id,
            )
            return None

        # SCHRITT 2: Token aus DB holen
        with get_conn() as conn:
            row = conn.execute(
                """
                SELECT access_token_enc, refresh_token_enc,
                       enc_version, token_expires_at, twitch_login, needs_reauth
                FROM twitch_raid_auth
                WHERE twitch_user_id = ? AND raid_enabled IS TRUE
                """,
                (twitch_user_id,),
            ).fetchone()

        if not row:
            return None

        needs_reauth = bool(row["needs_reauth"] if hasattr(row, "keys") else row[5])
        if needs_reauth:
            log.info(
                "OAuth grant for user_id=%s requires dashboard reauth - returning None",
                _mask_log_identifier(twitch_user_id),
            )
            return None

        _enc_v = row["enc_version"] or 1
        _uid = str(twitch_user_id)
        access_token = self._resolve_token(
            row["access_token_enc"],
            f"twitch_raid_auth|access_token|{_uid}|{_enc_v}",
            "access_token",
            _uid,
        )
        refresh_token = self._resolve_token(
            row["refresh_token_enc"],
            f"twitch_raid_auth|refresh_token|{_uid}|{_enc_v}",
            "refresh_token",
            _uid,
        )
        if not access_token:
            log.warning(
                "Stored access credential unavailable for user_id=%s",
                _mask_log_identifier(twitch_user_id),
            )
            return None
        expires_at_iso = row["token_expires_at"]
        twitch_login = row["twitch_login"]
        expires_at = _parse_expiry_ts(expires_at_iso)

        # SCHRITT 3: Token noch gültig?
        if time.time() < expires_at - 300:  # 5 Minuten Puffer
            return access_token

        # SCHRITT 4: Token abgelaufen -> refresh (mit Blacklist-Protection)
        async with self._lock:
            # Double-Check Locking
            with get_conn() as conn:
                row_check = conn.execute(
                    """SELECT token_expires_at,
                              access_token_enc, enc_version,
                              raid_enabled, needs_reauth
                       FROM twitch_raid_auth WHERE twitch_user_id = ?""",
                    (twitch_user_id,),
                ).fetchone()

            if row_check:
                raid_enabled = bool(row_check["raid_enabled"] if hasattr(row_check, "keys") else row_check[3])
                needs_reauth = bool(row_check["needs_reauth"] if hasattr(row_check, "keys") else row_check[4])
                if not raid_enabled or needs_reauth:
                    return None
                _dc_uid = str(twitch_user_id)
                _dc_v = row_check["enc_version"] or 1
                curr_expires_iso = row_check["token_expires_at"]
                curr_access = self._resolve_token(
                    row_check["access_token_enc"],
                    f"twitch_raid_auth|access_token|{_dc_uid}|{_dc_v}",
                    "access_token.dc",
                    _dc_uid,
                )
                curr_expires = _parse_expiry_ts(curr_expires_iso)
                if time.time() < curr_expires - 300 and curr_access:
                    return curr_access

            if not refresh_token:
                log.warning(
                    "Stored refresh credential unavailable for user_id=%s - cannot refresh",
                    _mask_log_identifier(twitch_user_id),
                )
                return None

            log.info("Refreshing OAuth grant for %s", twitch_login)
            try:
                # Refresh mit User-Info für Blacklist-Tracking
                token_data = await self.refresh_token(
                    refresh_token,
                    session,
                    twitch_user_id=twitch_user_id,
                    twitch_login=twitch_login,
                )
                new_access_token = token_data["access_token"]
                new_refresh_token = token_data.get("refresh_token") or refresh_token
                expires_in = _safe_int(token_data.get("expires_in", 3600), 3600)

                # Token in DB aktualisieren
                new_expires_at = datetime.now(UTC).timestamp() + expires_in
                new_expires_at_iso = datetime.fromtimestamp(new_expires_at, UTC).isoformat()

                with get_conn() as conn:
                    self._write_token_refresh(
                        conn,
                        twitch_user_id,
                        new_access_token,
                        new_refresh_token,
                        new_expires_at_iso,
                    )
                    # autocommit – no explicit commit needed

                self.token_error_handler.clear_failure_count(twitch_user_id)
                return new_access_token
            except Exception:
                log.exception("Failed to refresh OAuth grant for %s", twitch_login)
                return None

    async def get_valid_token_for_login(
        self, twitch_login: str, session: aiohttp.ClientSession
    ) -> tuple[str, str] | None:
        """
        Liefert (twitch_user_id, access_token) für einen Login, falls autorisiert.
        """
        login = (twitch_login or "").strip().lower()
        if not login:
            return None
        with get_conn() as conn:
            row = load_active_partner(conn, twitch_login=login)
        if not row:
            return None
        twitch_user_id = row[1] if not hasattr(row, "keys") else row["twitch_user_id"]
        token = await self.get_valid_token(str(twitch_user_id), session)
        if token:
            return str(twitch_user_id), token
        return None

    def revoke_auth(self, twitch_user_id: str) -> None:
        """Entfernt die Raid-Autorisierung für einen Streamer."""
        login_hint = ""
        discord_user_id = ""
        with get_conn() as conn:
            auth_row = conn.execute(
                "SELECT twitch_login FROM twitch_raid_auth WHERE twitch_user_id = ?",
                (twitch_user_id,),
            ).fetchone()
            if auth_row:
                login_hint = str(
                    auth_row[0] if not hasattr(auth_row, "keys") else auth_row["twitch_login"] or ""
                )

            streamer_row = load_streamer_identity(
                conn,
                twitch_user_id=twitch_user_id,
                twitch_login=login_hint,
            )
            if streamer_row:
                discord_user_id = str(
                    streamer_row["discord_user_id"]
                    if hasattr(streamer_row, "keys")
                    else streamer_row[2]
                    or ""
                ).strip()

            set_partner_raid_bot_enabled(conn, twitch_user_id=twitch_user_id, enabled=False)

            conn.execute(
                "DELETE FROM twitch_raid_auth WHERE twitch_user_id = ?",
                (twitch_user_id,),
            )
            # autocommit – no explicit commit needed

        if discord_user_id and hasattr(self.token_error_handler, "schedule_streamer_role_sync"):
            self.token_error_handler.schedule_streamer_role_sync(
                discord_user_id,
                should_have_role=False,
                reason="Twitch-Bot Autorisierung entzogen",
            )
        log.info("Revoked raid auth for user_id=%s", twitch_user_id)

    def set_raid_enabled(self, twitch_user_id: str, enabled: bool) -> None:
        """Aktiviert/Deaktiviert Auto-Raid für einen Streamer."""
        with get_conn() as conn:
            conn.execute(
                "UPDATE twitch_raid_auth SET raid_enabled = ? WHERE twitch_user_id = ?",
                (bool(enabled), twitch_user_id),
            )
            set_partner_raid_bot_enabled(conn, twitch_user_id=twitch_user_id, enabled=enabled)
            # autocommit – no explicit commit needed
        log.info("Set raid_enabled=%s for user_id=%s", enabled, twitch_user_id)

    def has_enabled_auth(self, twitch_user_id: str) -> bool:
        """
        True, wenn ein OAuth-Grant mit raid_enabled=1 für den Streamer existiert.
        Nutzt DB-Check, damit wir vor Auto-Raids kurzschließen können.
        """
        with get_conn() as conn:
            row = conn.execute(
                "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = ?",
                (twitch_user_id,),
            ).fetchone()
        return bool(row and row[0])

    def get_scopes(self, twitch_user_id: str) -> list[str]:
        """Liefert die gespeicherten OAuth-Scopes für einen Streamer (lowercased, unabhängig von raid_enabled)."""
        try:
            with get_conn() as conn:
                row = conn.execute(
                    "SELECT scopes FROM twitch_raid_auth WHERE twitch_user_id = ?",
                    (twitch_user_id,),
                ).fetchone()
            scopes_raw = (row[0] if row else "") or ""
            scopes = [s.strip().lower() for s in scopes_raw.split() if s.strip()]
            return scopes
        except Exception:
            log.debug("get_scopes failed for %s", twitch_user_id, exc_info=True)
            return []
