"""Streamer admin and verification helpers for the dashboard mixin."""

from __future__ import annotations

import asyncio

import discord

from ..core.constants import log
from ..discord_role_sync import normalize_discord_user_id, sync_streamer_role
from ..storage import pg as storage

VERIFICATION_SUCCESS_DM_MESSAGE = (
    "🎉 Glückwunsch! Du wurdest erfolgreich als **Streamer-Partner** verifiziert und bist jetzt offiziell Teil des "
    "Streamer-Teams. Wir melden uns, falls wir noch Fragen haben – ansonsten schauen wir uns deine Angaben kurz an. "
    "Bei Fragen kannst du dich gerne hier melden: https://discord.com/channels/1289721245281292288/1428062025145385111"
)


def _row_to_dict(row) -> dict:
    if row is None:
        return {}
    if hasattr(row, "keys"):
        return {k: row[k] for k in row.keys()}
    try:
        return dict(row)
    except Exception:
        return {}


def _dashboard_set_discord_flag_sync(normalized: str, is_on_discord: bool) -> None:
    with storage.transaction() as conn:
        row = storage.set_streamer_discord_member(
            conn,
            twitch_login=normalized,
            is_on_discord=is_on_discord,
        )
        if not row:
            raise ValueError(f"{normalized} ist nicht gespeichert")


async def _dashboard_set_discord_flag(self, login: str, is_on_discord: bool) -> str:
    normalized = self._normalize_login(login)
    if not normalized:
        raise ValueError("Ungültiger Login")

    await asyncio.to_thread(
        self._dashboard_set_discord_flag_sync,
        normalized,
        is_on_discord,
    )

    if is_on_discord:
        return f"{normalized} als Discord-Mitglied markiert"
    return f"Discord-Markierung für {normalized} entfernt"


def _dashboard_archive_sync(normalized: str, desired: str) -> str:
    with storage.transaction() as conn:
        active_row = storage.load_active_partner(conn, twitch_login=normalized)
        history_row = storage.load_latest_partner_history(conn, twitch_login=normalized)
        if not active_row and not history_row:
            raise ValueError(f"{normalized} ist nicht gespeichert")
        if not active_row and history_row:
            current_status = str(
                (
                    history_row.get("status")
                    if hasattr(history_row, "keys")
                    else history_row[20]
                )
                or ""
            ).strip().lower()
            if current_status and current_status != "active":
                raise ValueError(f"{normalized} ist departnered und nicht nur archiviert")
            raise ValueError(f"{normalized} ist kein aktiver Partner")

        current = None
        if active_row:
            current = (
                active_row.get("admin_archived_at")
                if hasattr(active_row, "keys")
                else None
            )

        if desired == "archive":
            if current:
                return f"{normalized} ist bereits archiviert (seit {current})"
            storage.set_streamer_archive_state(conn, twitch_login=normalized, archived=True)
            return f"{normalized} archiviert"

        if desired == "unarchive":
            if not current:
                return f"{normalized} ist nicht archiviert"
            storage.set_streamer_archive_state(conn, twitch_login=normalized, archived=False)
            return f"{normalized} ent-archiviert"

        if current:
            storage.set_streamer_archive_state(conn, twitch_login=normalized, archived=False)
            return f"{normalized} reaktiviert"
        storage.set_streamer_archive_state(conn, twitch_login=normalized, archived=True)
        return f"{normalized} archiviert"


async def _dashboard_archive(self, login: str, mode: str) -> str:
    normalized = self._normalize_login(login)
    if not normalized:
        raise ValueError("Ungültiger Login")

    mode_clean = (mode or "").strip().lower()
    if mode_clean in {"archive", "on", "set"}:
        desired = "archive"
    elif mode_clean in {"unarchive", "off", "unset", "restore"}:
        desired = "unarchive"
    else:
        desired = "toggle"

    return await asyncio.to_thread(self._dashboard_archive_sync, normalized, desired)


def _dashboard_load_twitch_user_id_from_raid_auth_sync(normalized: str) -> str | None:
    with storage.readonly_connection() as conn:
        raid_row = conn.execute(
            "SELECT twitch_user_id FROM twitch_raid_auth WHERE LOWER(twitch_login)=LOWER(%s)",
            (normalized,),
        ).fetchone()
        if not raid_row:
            return None
        return str(raid_row[0] or "").strip() or None


def _dashboard_save_discord_profile_sync(
    normalized: str,
    *,
    twitch_user_id: str | None,
    discord_user_id: str | None,
    discord_display_name: str | None,
    mark_member: bool,
) -> None:
    with storage.transaction() as conn:
        storage.save_streamer_discord_profile(
            conn,
            twitch_login=normalized,
            twitch_user_id=twitch_user_id,
            discord_user_id=discord_user_id,
            discord_display_name=discord_display_name,
            mark_member=mark_member,
        )


async def _dashboard_save_discord_profile(
    self,
    login: str,
    *,
    discord_user_id: str | None,
    discord_display_name: str | None,
    mark_member: bool,
) -> str:
    normalized = self._normalize_login(login)
    if not normalized:
        raise ValueError("Ungültiger Login")

    discord_id_clean = (discord_user_id or "").strip()
    if discord_id_clean and not discord_id_clean.isdigit():
        raise ValueError("Discord-ID muss eine Zahl sein")

    display_name_clean = (discord_display_name or "").strip()
    if len(display_name_clean) > 120:
        display_name_clean = display_name_clean[:120]

    twitch_user_id: str | None = None
    try:
        twitch_user_id = await asyncio.to_thread(
            self._dashboard_load_twitch_user_id_from_raid_auth_sync,
            normalized,
        )
    except Exception:
        log.debug(
            "Konnte user_id nicht aus raid_auth laden für %s",
            normalized,
            exc_info=True,
        )

    twitch_api = self._dashboard_bot_service().twitch_api()
    if not twitch_user_id and twitch_api:
        try:
            users = await twitch_api.get_users([normalized])
            user = users.get(normalized)
            if user:
                twitch_user_id = user.get("id")
                log.info("Fetched twitch_user_id %s for %s from API", twitch_user_id, normalized)
        except Exception:
            log.warning(
                "Konnte user_id nicht von API holen für %s",
                normalized,
                exc_info=True,
            )

    try:
        await asyncio.to_thread(
            self._dashboard_save_discord_profile_sync,
            normalized,
            twitch_user_id=twitch_user_id,
            discord_user_id=discord_id_clean or None,
            discord_display_name=display_name_clean or None,
            mark_member=mark_member,
        )
    except Exception:
        raise ValueError("Discord-ID wird bereits verwendet")

    return f"Discord-Daten für {normalized} aktualisiert"


def _dashboard_verify_storage_step(self, login: str, mode: str) -> dict[str, object]:
    if mode in {"permanent", "temp"}:
        row_data = None
        should_notify = False
        copied = 0
        with storage.transaction() as c:
            source_row = c.execute(
                """
                SELECT twitch_user_id, discord_user_id, discord_display_name, manual_verified_at
                FROM twitch_streamers
                WHERE twitch_login=%s
                """,
                (login,),
            ).fetchone()
            partner_row = storage.load_active_partner(c, twitch_login=login)
            twitch_user_id = ""
            if source_row:
                row_data = _row_to_dict(source_row)
                twitch_user_id = str(row_data.get("twitch_user_id") or "").strip()
                should_notify = row_data.get("manual_verified_at") is None
            elif partner_row:
                row_data = {
                    "twitch_user_id": partner_row["twitch_user_id"] if hasattr(partner_row, "keys") else partner_row[1],
                    "discord_user_id": partner_row["discord_user_id"] if hasattr(partner_row, "keys") else partner_row[21],
                    "discord_display_name": partner_row["discord_display_name"] if hasattr(partner_row, "keys") else partner_row[22],
                    "manual_verified_at": partner_row["manual_verified_at"] if hasattr(partner_row, "keys") else partner_row[11],
                }
                twitch_user_id = str(row_data.get("twitch_user_id") or "").strip()
                should_notify = row_data.get("manual_verified_at") is None

            if not twitch_user_id:
                return {"kind": "message", "message": f"{login} ist nicht gespeichert"}

            verification = storage.verification_payload(mode)
            storage.promote_streamer_to_partner(
                c,
                twitch_login=login,
                twitch_user_id=twitch_user_id,
                discord_user_id=row_data.get("discord_user_id") if row_data else None,
                discord_display_name=row_data.get("discord_display_name") if row_data else None,
                is_on_discord=1 if row_data and row_data.get("discord_user_id") else 0,
                **verification,
            )
            copied = storage.backfill_tracked_stats_from_category(c, login)

        base_msg = (
            f"{login} dauerhaft verifiziert"
            if mode == "permanent"
            else f"{login} für 30 Tage verifiziert"
        )
        return {
            "kind": "verified",
            "base_msg": base_msg,
            "copied": copied,
            "should_notify": should_notify,
            "row_data": row_data,
        }

    if mode == "clear":
        with storage.transaction() as c:
            result = storage.departner_active_partner(
                c,
                twitch_login=login,
                clear_verification=True,
            )
            if not result:
                return {"kind": "message", "message": f"{login} ist nicht gespeichert"}
        return {
            "kind": "cleared",
            "message": f"Verifizierung für {login} zurückgesetzt (keine DM versendet)",
            "row_data": result,
        }

    if mode == "failed":
        row_data = None
        with storage.transaction() as c:
            identity_row = storage.load_streamer_identity(c, twitch_login=login)
            if identity_row:
                row_data = _row_to_dict(identity_row)
            archived = storage.departner_active_partner(
                c,
                twitch_login=login,
                clear_verification=True,
            )
            if archived and row_data is None:
                row_data = archived
        if not row_data:
            return {"kind": "message", "message": f"{login} ist nicht gespeichert"}
        return {"kind": "failed", "row_data": row_data}

    return {"kind": "message", "message": "Unbekannter Modus"}


async def _ensure_streamer_role(self, row_data: dict | None) -> str:
    if not row_data:
        return ""
    user_id_raw = row_data.get("discord_user_id")
    if not user_id_raw:
        log.info(
            "Streamer verification: no Discord ID stored for %s",
            row_data.get("discord_display_name"),
        )
        return ""
    normalized_id = normalize_discord_user_id(str(user_id_raw))
    if not normalized_id:
        log.warning("Streamer verification: invalid Discord ID %r", user_id_raw)
        return "(Streamer-Rolle konnte nicht vergeben werden – ungültige Discord-ID)"
    changed = await sync_streamer_role(
        self.bot,
        normalized_id,
        should_have_role=True,
        reason="Streamer-Verifizierung über Dashboard bestätigt",
        logger=log,
    )
    return "(Streamer-Rolle vergeben)" if changed else ""


async def _remove_streamer_role(self, row_data: dict | None, *, reason: str) -> str:
    if not row_data:
        return ""
    user_id_raw = row_data.get("discord_user_id")
    if not user_id_raw:
        log.info(
            "Streamer role removal skipped for %s because no Discord ID is stored",
            row_data.get("discord_display_name"),
        )
        return ""
    normalized_id = normalize_discord_user_id(str(user_id_raw))
    if not normalized_id:
        log.warning("Streamer role removal skipped due to invalid Discord ID %r", user_id_raw)
        return "(Streamer-Rolle konnte nicht entfernt werden – ungültige Discord-ID)"
    changed = await sync_streamer_role(
        self.bot,
        normalized_id,
        should_have_role=False,
        reason=reason,
        logger=log,
    )
    return "(Streamer-Rolle entfernt)" if changed else ""


async def _notify_verification_success(self, login: str, row_data: dict | None) -> str:
    if not row_data:
        log.info("Keine Discord-Daten für %s zum Versenden der Erfolgsnachricht gefunden", login)
        return ""
    user_id_raw = row_data.get("discord_user_id")
    if not user_id_raw:
        log.info("Keine Discord-ID für %s hinterlegt – überspringe Erfolgsnachricht", login)
        return ""
    try:
        user_id_int = int(str(user_id_raw))
    except (TypeError, ValueError):
        log.warning("Ungültige Discord-ID %r für %s – keine Erfolgsnachricht", user_id_raw, login)
        return "(Discord-DM konnte nicht zugestellt werden)"

    user = self.bot.get_user(user_id_int)
    if user is None:
        try:
            user = await self.bot.fetch_user(user_id_int)
        except discord.NotFound:
            user = None
        except discord.HTTPException:
            log.exception("Konnte Discord-User %s nicht abrufen", user_id_int)
            user = None
    if user is None:
        log.warning("Discord-User %s (%s) konnte nicht gefunden werden", user_id_int, login)
        return "(Discord-DM konnte nicht zugestellt werden)"
    try:
        await user.send(VERIFICATION_SUCCESS_DM_MESSAGE)
    except discord.Forbidden:
        log.warning("DM an %s (%s) wegen erfolgreicher Verifizierung blockiert", user_id_int, login)
        return "(Discord-DM konnte nicht zugestellt werden)"
    except discord.HTTPException:
        log.exception("Konnte Erfolgsnachricht nach Verifizierung nicht an %s senden", user_id_int)
        return "(Discord-DM konnte nicht zugestellt werden)"
    log.info("Verifizierungs-Erfolgsnachricht an %s (%s) gesendet", user_id_int, login)
    return ""


async def _dashboard_verify(self, login: str, mode: str) -> str:
    login = self._normalize_login(login)
    if not login:
        return "Ungültiger Login"

    storage_result = await asyncio.to_thread(self._dashboard_verify_storage_step, login, mode)
    result_kind = str(storage_result.get("kind") or "")
    if result_kind == "message":
        return str(storage_result.get("message") or "Unbekannter Modus")
    if result_kind == "verified":
        row_data = storage_result.get("row_data")
        should_notify = bool(storage_result.get("should_notify"))
        copied = int(storage_result.get("copied") or 0)
        base_msg = str(storage_result.get("base_msg") or "").strip()
        notes: list[str] = []
        if copied:
            notes.append(f"({copied} historische Datenpunkte übernommen)")
        if should_notify:
            dm_note = await self._notify_verification_success(login, row_data)
            if dm_note:
                notes.append(dm_note)
        role_note = await self._ensure_streamer_role(row_data)
        if role_note:
            notes.append(role_note)
        merged = " ".join(notes).strip()
        return f"{base_msg} {merged}".strip()
    if result_kind == "cleared":
        message = str(storage_result.get("message") or f"Verifizierung für {login} zurückgesetzt")
        role_note = await self._remove_streamer_role(
            storage_result.get("row_data"),
            reason="Streamer-Verifizierung über Dashboard entfernt",
        )
        return f"{message} {role_note}".strip()
    if result_kind != "failed":
        return "Unbekannter Modus"

    row_data = storage_result.get("row_data")
    if not isinstance(row_data, dict):
        return f"{login} ist nicht gespeichert"
    role_note = await self._remove_streamer_role(
        row_data,
        reason="Streamer-Verifizierung über Dashboard fehlgeschlagen",
    )
    user_id_raw = row_data.get("discord_user_id")
    if not user_id_raw:
        return f"Keine Discord-ID für {login} hinterlegt {role_note}".strip()
    try:
        user_id_int = int(str(user_id_raw))
    except (TypeError, ValueError):
        return f"Ungültige Discord-ID für {login} {role_note}".strip()
    user = self.bot.get_user(user_id_int)
    if user is None:
        try:
            user = await self.bot.fetch_user(user_id_int)
        except discord.NotFound:
            user = None
        except discord.HTTPException:
            log.exception("Konnte Discord-User %s nicht abrufen", user_id_int)
            user = None
    if user is None:
        return f"Discord-User {user_id_int} konnte nicht gefunden werden {role_note}".strip()

    message = (
        "Hey! Deine Deadlock-Streamer-Verifizierung konnte leider nicht abgeschlossen werden. "
        "Du erfüllst aktuell nicht alle Voraussetzungen. Bitte prüfe die Anforderungen erneut "
        "und starte die Verifizierung anschließend mit /streamer noch einmal."
    )
    try:
        await user.send(message)
    except discord.Forbidden:
        log.warning(
            "DM an %s (%s) wegen fehlgeschlagener Verifizierung blockiert",
            user_id_int,
            login,
        )
        return (
            f"Konnte {row_data.get('discord_display_name') or user.name} nicht per DM erreichen. "
            f"{role_note}"
        ).strip()
    except discord.HTTPException:
        log.exception(
            "Konnte Verifizierungsfehler-Nachricht nicht senden an %s",
            user_id_int,
        )
        return f"Nachricht konnte nicht gesendet werden {role_note}".strip()
    log.info(
        "Verifizierungsfehler-Benachrichtigung an %s (%s) gesendet",
        user_id_int,
        login,
    )
    return (
        f"{login}: Discord-User wurde über die fehlgeschlagene Verifizierung informiert "
        f"{role_note}"
    ).strip()


__all__ = [
    "_dashboard_archive",
    "_dashboard_archive_sync",
    "_dashboard_load_twitch_user_id_from_raid_auth_sync",
    "_dashboard_save_discord_profile",
    "_dashboard_save_discord_profile_sync",
    "_dashboard_set_discord_flag",
    "_dashboard_set_discord_flag_sync",
    "_dashboard_verify",
    "_dashboard_verify_storage_step",
    "_ensure_streamer_role",
    "_notify_verification_success",
    "_remove_streamer_role",
]
