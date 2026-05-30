"""OAuth-Bootstrap + verschlüsselter Token-Store für den Engagement-Sende-Account.

Der Engagement-Layer spricht im Chat NICHT über die zentrale Bot-Identität,
sondern über einen separaten, unauffälligen Twitch-Account (den „Stammgast"/
Smoke-Account). Damit dessen Token genauso sauber gemanagt wird wie die
Streamer-Tokens — verschlüsselt at-rest, automatisch refreshed — läuft die
Beschaffung über einen eigenen, vollständig vom Raid-/Streamer-OAuth getrennten
Flow:

    1. build_authorize_url()  -> Admin klickt 1x „Authorize" (als Smoke-Account)
    2. /callback/engagement-sender -> handle_callback() tauscht Code -> Token
    3. get_valid_access_token() -> liefert frischen Access-Token (Refresh on demand)

Bewusst getrennt von ``bot/raid/auth.py``: eigener State-Marker
(``platform = 'engagement_sender'``), eigene Redirect-URI, eigene Tabelle
``twitch_engagement_sender_auth``. Der normale Streamer-OAuth bleibt unberührt.

Tokens werden mit demselben Field-Crypto (AES-256-GCM, AAD-gebunden) wie die
Streamer-Tokens verschlüsselt. Client-ID/Secret kommen aus der bestehenden
Bot-App (``TWITCH_CLIENT_ID`` / ``TWITCH_CLIENT_SECRET`` via Infisical).
"""

from __future__ import annotations

import logging
import os
import secrets
import time
import urllib.parse
from datetime import datetime, timedelta, timezone

import aiohttp

from bot.compat.field_crypto import get_crypto
from bot.storage.pg import query_one, transaction

log = logging.getLogger("TwitchStreams.Engagement.SenderAuth")

# === Identität des Sende-Accounts (per Code, keine Env) ===
SENDER_LOGIN = "iamspyingthroughtyourcam"
SCOPES = ("user:write:chat", "user:bot")
# In der Twitch-App registriert; Caddy proxyt /callback/engagement-sender -> 8765
# (eigener handle-Block, gespiegelt vom /callback/twitch-Block).
REDIRECT_URI = "https://deutsche-deadlock-community.de/callback/engagement-sender"
PLATFORM = "engagement_sender"

AUTHORIZE_URL = "https://id.twitch.tv/oauth2/authorize"
TOKEN_URL = "https://id.twitch.tv/oauth2/token"  # noqa: S105
USERS_URL = "https://api.twitch.tv/helix/users"

_STATE_TTL_SECONDS = 600          # Authorize-Link gültig 10 min
_REFRESH_SKEW_SECONDS = 300       # 5 min vor Ablauf proaktiv refreshen


class SenderAuthError(RuntimeError):
    """Onboarding/Token-Beschaffung für den Sende-Account fehlgeschlagen."""


def _client_credentials() -> tuple[str, str]:
    client_id = (os.getenv("TWITCH_CLIENT_ID") or "").strip()
    client_secret = (os.getenv("TWITCH_CLIENT_SECRET") or "").strip()
    if not client_id or not client_secret:
        raise SenderAuthError("TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET nicht gesetzt")
    return client_id, client_secret


def _access_aad(user_id: str) -> str:
    return f"{PLATFORM}|access_token|{user_id}"


def _refresh_aad(user_id: str) -> str:
    return f"{PLATFORM}|refresh_token|{user_id}"


# === Schema (idempotent, self-contained) ===

def ensure_table() -> None:
    """Legt die Token-Tabelle an, falls sie fehlt. Bewusst getrennt von twitch_raid_auth."""
    with transaction() as conn:
        conn.execute(
            """
            CREATE TABLE IF NOT EXISTS twitch_engagement_sender_auth (
                twitch_user_id     TEXT PRIMARY KEY,
                twitch_login       TEXT NOT NULL,
                access_token_enc   BYTEA NOT NULL,
                refresh_token_enc  BYTEA NOT NULL,
                scopes             TEXT,
                token_expires_at   BIGINT NOT NULL,
                updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
            )
            """
        )


# === Authorize-Link ===

def build_authorize_url() -> str:
    """Erzeugt einen Twitch-Authorize-Link mit getracktem State in oauth_state_tokens."""
    client_id, _ = _client_credentials()
    ensure_table()
    state = "engsender-" + secrets.token_urlsafe(24)
    expires_at = (datetime.now(timezone.utc) + timedelta(seconds=_STATE_TTL_SECONDS)).isoformat()

    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO oauth_state_tokens
                (state_token, platform, streamer_login, redirect_uri, expires_at)
            VALUES (%s, %s, %s, %s, %s)
            ON CONFLICT (state_token) DO UPDATE
                SET expires_at = EXCLUDED.expires_at
            """,
            [state, PLATFORM, SENDER_LOGIN, REDIRECT_URI, expires_at],
        )

    params = {
        "client_id": client_id,
        "redirect_uri": REDIRECT_URI,
        "response_type": "code",
        "scope": " ".join(SCOPES),
        "state": state,
        "force_verify": "true",
    }
    return AUTHORIZE_URL + "?" + urllib.parse.urlencode(params, quote_via=urllib.parse.quote)


def _consume_state(state: str) -> bool:
    """Verbraucht einen noch gültigen engagement_sender-State atomar. True wenn gültig."""
    if not state:
        return False
    with transaction() as conn:
        row = conn.execute(
            """
            DELETE FROM oauth_state_tokens
            WHERE state_token = %s AND platform = %s
            RETURNING expires_at
            """,
            [state, PLATFORM],
        ).fetchone()
    if not row:
        return False
    raw_exp = row[0]
    try:
        exp = datetime.fromisoformat(str(raw_exp))
        if exp.tzinfo is None:
            exp = exp.replace(tzinfo=timezone.utc)
    except (ValueError, TypeError):
        return True  # existierte, Format unklar -> nicht künstlich abweisen
    return datetime.now(timezone.utc) <= exp


# === Token-Store ===

def _store_tokens(
    *,
    user_id: str,
    login: str,
    access_token: str,
    refresh_token: str,
    expires_in: int,
    scopes: str,
) -> None:
    ensure_table()
    crypto = get_crypto()
    access_enc = crypto.encrypt_field(access_token, _access_aad(user_id))
    refresh_enc = crypto.encrypt_field(refresh_token, _refresh_aad(user_id))
    expires_at = int(time.time()) + int(expires_in or 0)
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO twitch_engagement_sender_auth
                (twitch_user_id, twitch_login, access_token_enc, refresh_token_enc,
                 scopes, token_expires_at, updated_at)
            VALUES (%s, %s, %s, %s, %s, %s, now())
            ON CONFLICT (twitch_user_id) DO UPDATE SET
                twitch_login      = EXCLUDED.twitch_login,
                access_token_enc  = EXCLUDED.access_token_enc,
                refresh_token_enc = EXCLUDED.refresh_token_enc,
                scopes            = EXCLUDED.scopes,
                token_expires_at  = EXCLUDED.token_expires_at,
                updated_at        = now()
            """,
            [user_id, login, access_enc, refresh_enc, scopes, expires_at],
        )


def _load_row() -> dict | None:
    ensure_table()
    row = query_one(
        """
        SELECT twitch_user_id, twitch_login, access_token_enc, refresh_token_enc,
               scopes, token_expires_at
        FROM twitch_engagement_sender_auth
        ORDER BY updated_at DESC
        LIMIT 1
        """
    )
    if row is None:
        return None
    return {
        "user_id": row[0],
        "login": row[1],
        "access_enc": bytes(row[2]) if row[2] is not None else None,
        "refresh_enc": bytes(row[3]) if row[3] is not None else None,
        "scopes": row[4],
        "expires_at": int(row[5]) if row[5] is not None else 0,
    }


# === HTTP: Exchange + Refresh (eigene Calls, Raid-Manager unberührt) ===

async def _post_token(data: dict) -> dict:
    async with aiohttp.ClientSession() as session:
        async with session.post(TOKEN_URL, data=data) as r:
            txt = await r.text()
            if r.status != 200:
                # Body kann eine Twitch-Fehlermeldung enthalten, aber NIE das Secret.
                raise SenderAuthError(f"Twitch token endpoint HTTP {r.status}: {txt[:200]}")
            import json
            return json.loads(txt)


async def _fetch_user(access_token: str, client_id: str) -> tuple[str, str]:
    headers = {"Client-ID": client_id, "Authorization": f"Bearer {access_token}"}
    async with aiohttp.ClientSession() as session:
        async with session.get(USERS_URL, headers=headers) as r:
            if r.status != 200:
                raise SenderAuthError(f"helix/users HTTP {r.status}")
            payload = await r.json()
    data = (payload.get("data") or [])
    if not data:
        raise SenderAuthError("helix/users lieferte keine Daten")
    return str(data[0].get("id") or ""), str(data[0].get("login") or "")


async def handle_callback(code: str, state: str) -> dict:
    """Tauscht den Authorize-Code gegen Token und legt ihn verschlüsselt ab.

    Returns: {"login": str, "user_id": str}. Raises SenderAuthError bei Problemen.
    """
    import asyncio

    code = (code or "").strip()
    if not code:
        raise SenderAuthError("Kein Code im Callback")

    state_ok = await asyncio.to_thread(_consume_state, state)
    if not state_ok:
        raise SenderAuthError("State ungültig oder abgelaufen")

    client_id, client_secret = _client_credentials()
    token = await _post_token(
        {
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
            "grant_type": "authorization_code",
            "redirect_uri": REDIRECT_URI,
        }
    )
    access_token = str(token.get("access_token") or "")
    refresh_token = str(token.get("refresh_token") or "")
    expires_in = int(token.get("expires_in") or 0)
    scope_field = token.get("scope")
    if isinstance(scope_field, list):
        scope_str = " ".join(scope_field)
    else:
        scope_str = str(scope_field or " ".join(SCOPES))
    if not access_token or not refresh_token:
        raise SenderAuthError("Token-Response unvollständig")

    user_id, login = await _fetch_user(access_token, client_id)
    if not user_id:
        raise SenderAuthError("Konnte User-ID nicht bestimmen")

    await asyncio.to_thread(
        _store_tokens,
        user_id=user_id,
        login=login or SENDER_LOGIN,
        access_token=access_token,
        refresh_token=refresh_token,
        expires_in=expires_in,
        scopes=scope_str,
    )
    log.info("Engagement-Sender autorisiert: login=%s user_id=%s", login, user_id)
    return {"login": login or SENDER_LOGIN, "user_id": user_id}


async def get_valid_access_token() -> tuple[str, str] | None:
    """Liefert (access_token, user_id) für den Sende-Account; refresht bei Ablauf.

    Returns None, wenn kein Account onboarded ist (dann fällt der Aufrufer auf
    sein Default-Verhalten zurück).
    """
    import asyncio

    row = await asyncio.to_thread(_load_row)
    if row is None or not row.get("access_enc") or not row.get("refresh_enc"):
        return None

    user_id = row["user_id"]
    crypto = get_crypto()

    now = int(time.time())
    if now < row["expires_at"] - _REFRESH_SKEW_SECONDS:
        try:
            access_token = crypto.decrypt_field(row["access_enc"], _access_aad(user_id))
            return access_token, user_id
        except Exception:
            log.warning("Engagement-Sender: Access-Token-Decrypt fehlgeschlagen, versuche Refresh")

    # Refresh
    try:
        refresh_token = crypto.decrypt_field(row["refresh_enc"], _refresh_aad(user_id))
    except Exception:
        log.exception("Engagement-Sender: Refresh-Token-Decrypt fehlgeschlagen")
        return None

    client_id, client_secret = _client_credentials()
    try:
        token = await _post_token(
            {
                "client_id": client_id,
                "client_secret": client_secret,
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
            }
        )
    except SenderAuthError:
        log.exception("Engagement-Sender: Token-Refresh fehlgeschlagen")
        return None

    new_access = str(token.get("access_token") or "")
    new_refresh = str(token.get("refresh_token") or refresh_token)
    expires_in = int(token.get("expires_in") or 0)
    scope_field = token.get("scope")
    scope_str = " ".join(scope_field) if isinstance(scope_field, list) else str(scope_field or row.get("scopes") or "")
    if not new_access:
        return None

    await asyncio.to_thread(
        _store_tokens,
        user_id=user_id,
        login=row.get("login") or SENDER_LOGIN,
        access_token=new_access,
        refresh_token=new_refresh,
        expires_in=expires_in,
        scopes=scope_str,
    )
    return new_access, user_id
