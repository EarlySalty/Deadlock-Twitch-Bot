"""Shared OAuth callback flow for raid authorization routes."""

from __future__ import annotations

import asyncio
import html
from collections.abc import Callable
from typing import Any

import aiohttp

from ...core.constants import log
from ...raid.scope_profiles import scopes_for_profile

TWITCH_HELIX_USERS_URL = "https://api.twitch.tv/helix/users"
DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL = "https://twitch.earlysalty.com/twitch/dashboard"
PUBLIC_STREAMER_ONBOARDING_LOGIN = "public:website_onboarding"


def _oauth_error_payload(*, status: int, title: str, body_html: str) -> dict[str, Any]:
    return {
        "status": status,
        "title": title,
        "body_html": body_html,
    }


def _normalize_scopes(scopes_raw: Any) -> list[str]:
    if isinstance(scopes_raw, str):
        return [scope for scope in scopes_raw.split() if scope]
    if isinstance(scopes_raw, list):
        return [str(scope).strip() for scope in scopes_raw if str(scope).strip()]
    return []


async def build_raid_oauth_callback_payload(
    *,
    code: str,
    state: str,
    error: str,
    raid_bot: Any | None,
    auth_manager: Any | None,
    success_redirect_url: str,
    failure_title: str,
    failure_body_html: str,
    schedule_background: Callable[[Any, str], Any] | None = None,
) -> dict[str, Any]:
    code_clean = str(code or "").strip()
    state_clean = str(state or "").strip()
    error_clean = str(error or "").strip()

    requested_login = ""
    session = getattr(raid_bot, "session", None) if raid_bot is not None else None
    owns_session = False

    try:
        if error_clean:
            expected_uri = (getattr(auth_manager, "redirect_uri", "") or "").strip()
            expected_html = (
                f"<p><code>{html.escape(expected_uri, quote=True)}</code></p>"
                if expected_uri
                else ""
            )
            if error_clean == "redirect_mismatch":
                message = (
                    "<p>Twitch hat die Redirect-URI abgelehnt (redirect_mismatch).</p>"
                    "<p>Bitte trage diese URL exakt in der Twitch Application unter "
                    "<strong>OAuth Redirect URLs</strong> ein und starte die Autorisierung neu:</p>"
                    f"{expected_html}"
                )
            else:
                message = (
                    "<p>OAuth-Fehler beim Autorisieren.</p>"
                    "<p>Bitte die Autorisierung erneut starten.</p>"
                )
            return _oauth_error_payload(status=400, title="Autorisierung fehlgeschlagen", body_html=message)

        if not code_clean or not state_clean:
            return _oauth_error_payload(
                status=400,
                title="Ungültige Anfrage",
                body_html="<p>Fehlender OAuth Code oder State.</p>",
            )

        if not raid_bot or not auth_manager:
            return _oauth_error_payload(
                status=503,
                title="Raid-Bot nicht verfügbar",
                body_html=(
                    "<p>Der Raid-Bot ist aktuell nicht initialisiert. "
                    "Bitte später erneut versuchen.</p>"
                ),
            )

        state_info = auth_manager.consume_state_details(state_clean)
        if not state_info:
            return _oauth_error_payload(
                status=400,
                title="Ungültiger State",
                body_html=(
                    "<p>Der OAuth-State ist ungültig oder abgelaufen. "
                    "Bitte den Link neu erzeugen.</p>"
                ),
            )

        requested_login = str(getattr(state_info, "requested_login", "") or "").strip()
        state_discord_user_id = str(getattr(state_info, "discord_user_id", "") or "").strip()

        if session is None:
            session = aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=20))
            owns_session = True

        token_data = await auth_manager.exchange_code_for_token(code_clean, session)

        access_token = str(token_data.get("access_token") or "").strip()
        refresh_token = str(token_data.get("refresh_token") or "").strip()
        if not access_token:
            raise RuntimeError("Missing access_token in Twitch OAuth response")
        if not refresh_token:
            raise RuntimeError("Missing refresh_token in Twitch OAuth response")

        headers = {
            "Client-ID": str(auth_manager.client_id),
            "Authorization": f"Bearer {access_token}",
        }
        async with session.get(TWITCH_HELIX_USERS_URL, headers=headers) as user_resp:
            if user_resp.status != 200:
                body = await user_resp.text()
                raise RuntimeError(
                    f"Failed to fetch Twitch user info ({user_resp.status}): {body[:300]}"
                )
            user_payload = await user_resp.json()

        users = user_payload.get("data") if isinstance(user_payload, dict) else None
        if not isinstance(users, list) or not users:
            raise RuntimeError("Missing Twitch user data in OAuth callback")
        user_info = users[0] or {}

        twitch_user_id = str(user_info.get("id") or "").strip()
        twitch_login = str(user_info.get("login") or "").strip().lower()
        if not twitch_user_id or not twitch_login:
            raise RuntimeError("Invalid Twitch user payload in OAuth callback")

        expected_twitch_user_id = str(getattr(state_info, "expected_twitch_user_id", "") or "").strip()
        if expected_twitch_user_id and twitch_user_id != expected_twitch_user_id:
            log.warning(
                "Raid OAuth callback user mismatch: expected=%s actual=%s state=%s",
                expected_twitch_user_id,
                twitch_user_id,
                requested_login or state_clean,
            )
            return _oauth_error_payload(
                status=403,
                title="Falscher Twitch-Account",
                body_html=(
                    "<p>Die Autorisierung wurde mit dem falschen Twitch-Account abgeschlossen.</p>"
                    "<p>Bitte den Link erneut öffnen und dich mit dem vorgesehenen Kanal anmelden.</p>"
                ),
            )

        expected_twitch_login = str(getattr(state_info, "expected_twitch_login", "") or "").strip().lower()
        if not expected_twitch_login:
            requested_login = str(getattr(state_info, "requested_login", "") or "").strip().lower()
            if requested_login and not (
                requested_login.startswith("discord:")
                or requested_login == PUBLIC_STREAMER_ONBOARDING_LOGIN
            ):
                expected_twitch_login = requested_login
        if not expected_twitch_user_id and expected_twitch_login and twitch_login != expected_twitch_login:
            log.warning(
                "Raid OAuth callback login mismatch: expected=%s actual=%s state=%s",
                expected_twitch_login,
                twitch_login,
                requested_login or state_clean,
            )
            return _oauth_error_payload(
                status=403,
                title="Falscher Twitch-Account",
                body_html=(
                    "<p>Die Autorisierung wurde mit dem falschen Twitch-Account abgeschlossen.</p>"
                    "<p>Bitte den Link erneut öffnen und dich mit dem vorgesehenen Kanal anmelden.</p>"
                ),
            )

        scopes = _normalize_scopes(token_data.get("scope", []))
        allowed_scopes = set(scopes_for_profile(getattr(state_info, "scope_profile", None)))
        unexpected_scopes = sorted({scope for scope in scopes if scope not in allowed_scopes})
        if unexpected_scopes:
            log.warning(
                "Raid OAuth callback returned scopes outside expected profile for %s: %s",
                twitch_login,
                ", ".join(unexpected_scopes),
            )
            return _oauth_error_payload(
                status=400,
                title="Ungültige Berechtigungen",
                body_html=(
                    "<p>Die Autorisierung wurde mit unerwarteten Berechtigungen abgeschlossen.</p>"
                    "<p>Bitte den Vorgang neu starten.</p>"
                ),
            )

        had_existing_auth = auth_manager.has_saved_auth_record(
            twitch_user_id=twitch_user_id,
            twitch_login=twitch_login,
        )

        auth_manager.save_auth(
            twitch_user_id=twitch_user_id,
            twitch_login=twitch_login,
            access_token=access_token,
            refresh_token=refresh_token,
            expires_in=int(token_data.get("expires_in", 3600) or 3600),
            scopes=scopes,
            scope_profile=getattr(state_info, "scope_profile", None),
            activate_raid_features=True,
        )

        post_setup = getattr(raid_bot, "complete_setup_for_streamer", None)
        sync_partner_state = getattr(raid_bot, "_sync_partner_state_after_auth", None)
        if callable(post_setup) and not had_existing_auth:
            followup = post_setup(
                twitch_user_id,
                twitch_login,
                state_discord_user_id=state_discord_user_id,
                activate_partner_features=True,
            )
            if callable(schedule_background):
                scheduled = schedule_background(followup, "twitch.raid.complete_setup")
                if scheduled is None:
                    raise RuntimeError("failed to schedule twitch.raid.complete_setup")
            else:
                asyncio.create_task(
                    followup,
                    name="twitch.raid.complete_setup",
                )
        elif callable(sync_partner_state) and state_discord_user_id:
            followup = sync_partner_state(
                twitch_user_id,
                twitch_login,
                state_discord_user_id=state_discord_user_id,
                activate_partner_features=True,
            )
            if callable(schedule_background):
                scheduled = schedule_background(
                    followup, "twitch.raid.sync_partner_state_after_auth"
                )
                if scheduled is None:
                    raise RuntimeError(
                        "failed to schedule twitch.raid.sync_partner_state_after_auth"
                    )
            else:
                asyncio.create_task(
                    followup,
                    name="twitch.raid.sync_partner_state_after_auth",
                )

        log.info("Raid auth successful for %s", twitch_login)
        return {
            "status": 200,
            "title": "Autorisierung erfolgreich",
            "body_html": (
                "<p>Der Raid-Bot wurde erfolgreich autorisiert.</p>"
                "<p>Du kannst dieses Fenster jetzt schließen.</p>"
            ),
            "redirect_url": success_redirect_url,
        }
    except Exception:
        log.exception(
            "Raid OAuth callback failed for state login=%s",
            requested_login or "<unknown>",
        )
        return _oauth_error_payload(
            status=500,
            title=failure_title,
            body_html=failure_body_html,
        )
    finally:
        if owns_session:
            try:
                await session.close()
            except Exception:
                log.debug("Could not close temporary OAuth callback session", exc_info=True)
