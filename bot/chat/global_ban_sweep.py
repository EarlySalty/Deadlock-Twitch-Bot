"""Proaktiver, offline-gegateter Global-Ban-Sweep.

Bannt Einträge der globalen Bannliste (``twitch_chatter_global_ban``) über alle
operativ aktiven Partner-Kanäle hinweg -- aber nur, wenn der jeweilige Streamer
gerade offline ist. Das vermeidet sichtbare Mid-Stream-Bans (am wenigsten
verwirrend, wenn niemand zuschaut). Idempotent über einen Applied-Ledger plus
Twitchs ``already banned``-Antwort.

Zwei Auslöser, beide offline-gegatet:
- 1h nach Stream-Ende je Kanal (über ``twitch_global_ban_sweep_due``)
- täglicher Sweep ~6 Uhr über alle Offline-Partner (Catch-up + Retry)

Bewusst still: keine Chat-Nachricht, kein Discord-Alert. Das reaktive
Sicherheitsnetz (``_enforce_global_chatter_ban``) bleibt davon unberührt und
fängt Gelistete ab, falls sie schreiben, bevor der Sweep den Kanal erreicht hat.
"""

from __future__ import annotations

import logging
from typing import Any

import aiohttp

log = logging.getLogger("TwitchStreams.GlobalBanSweep")

# Grund, der an Twitch und in den Logs landet.
GLOBAL_BAN_REASON = "Netzwerkweiter Ban: Verstoß gegen Community-Richtlinien"


async def _resolve_user_id(chat_bot: Any, login: str) -> str | None:
    """Löst einen Login über Helix ``users`` zur Twitch-user_id auf (Bot-Token)."""
    login = (login or "").strip().lower().lstrip("#")
    if not login:
        return None
    token_manager = getattr(chat_bot, "_token_manager", None)
    client_id = getattr(chat_bot, "_client_id", None)
    if not token_manager or not client_id:
        return None
    try:
        tokens = await token_manager.get_valid_token()
        if not tokens:
            return None
        access_token, _ = tokens
        headers = {"Client-ID": client_id, "Authorization": f"Bearer {access_token}"}
        async with aiohttp.ClientSession() as session:
            async with session.get(
                "https://api.twitch.tv/helix/users",
                headers=headers,
                params={"login": login},
            ) as resp:
                if resp.status != 200:
                    return None
                data = await resp.json()
                arr = data.get("data") or []
                if not arr:
                    return None
                return str(arr[0].get("id") or "") or None
    except Exception:
        log.debug("user_id-Auflösung fehlgeschlagen für %s", login, exc_info=True)
        return None


async def ban_user_direct(
    chat_bot: Any,
    *,
    broadcaster_id: str,
    target_user_id: str,
    channel_login: str,
    reason: str = GLOBAL_BAN_REASON,
    login_hint: str = "",
) -> bool:
    """Stiller Ban von ``target_user_id`` in ``broadcaster_id`` (Bot als Moderator).

    True bei Erfolg ODER wenn bereits gebannt. False bei echtem Fehler/403
    (z.B. Bot ist kein Mod im Kanal -- dann wird nichts im Ledger vermerkt und
    der nächste Sweep versucht es erneut). Keine Chat-Nachricht, kein Alert.
    """
    safe_bot_id = getattr(chat_bot, "bot_id_safe", None) or getattr(chat_bot, "bot_id", None)
    token_manager = getattr(chat_bot, "_token_manager", None)
    client_id = getattr(chat_bot, "_client_id", None)
    if not safe_bot_id or not token_manager or not client_id:
        return False
    if not broadcaster_id or not target_user_id:
        return False
    if str(target_user_id) == str(broadcaster_id):
        return False  # den Streamer niemals selbst bannen

    for attempt in range(2):
        try:
            tokens = await token_manager.get_valid_token()
            if not tokens:
                return False
            access_token, _ = tokens
            headers = {
                "Client-ID": client_id,
                "Authorization": f"Bearer {access_token}",
                "Content-Type": "application/json",
            }
            payload = {"data": {"user_id": str(target_user_id), "reason": reason[:500]}}
            async with aiohttp.ClientSession() as session:
                async with session.post(
                    "https://api.twitch.tv/helix/moderation/bans",
                    headers=headers,
                    params={
                        "broadcaster_id": str(broadcaster_id),
                        "moderator_id": str(safe_bot_id),
                    },
                    json=payload,
                ) as resp:
                    if resp.status in {200, 201, 202}:
                        log.info(
                            "Global-Ban gesetzt: %s in #%s",
                            login_hint or target_user_id,
                            channel_login,
                        )
                        return True
                    if resp.status == 401 and attempt == 0:
                        await token_manager.get_valid_token(force_refresh=True)
                        continue
                    txt = await resp.text()
                    if resp.status == 400 and "already banned" in txt.lower():
                        return True
                    if resp.status == 403:
                        log.warning(
                            "Global-Ban 403 in #%s: Bot ist dort wahrscheinlich kein Moderator",
                            channel_login,
                        )
                        return False
                    log.warning(
                        "Global-Ban fehlgeschlagen in #%s (user=%s): HTTP %s %s",
                        channel_login,
                        target_user_id,
                        resp.status,
                        txt[:160].replace("\n", " "),
                    )
                    return False
        except Exception:
            log.debug("Global-Ban Exception in #%s", channel_login, exc_info=True)
            if attempt == 1:
                return False
    return False


def _live_broadcaster_ids() -> set[str]:
    from ..core import partner_utils

    try:
        return {
            str(p.get("twitch_user_id"))
            for p in partner_utils.get_live_partners()
            if p.get("twitch_user_id")
        }
    except Exception:
        log.debug("Live-Partner-Abfrage fehlgeschlagen", exc_info=True)
        return set()


def _offline_partner_targets() -> list[tuple[str, str]]:
    """``(login, broadcaster_id)`` aller operativ aktiven Partner, die GERADE offline sind."""
    from ..core import partner_utils

    try:
        all_partners = partner_utils.get_all_partners(include_archived=False)
    except Exception:
        log.debug("Partner-Enumeration fehlgeschlagen", exc_info=True)
        return []
    live_ids = _live_broadcaster_ids()
    out: list[tuple[str, str]] = []
    for p in all_partners:
        login = str(p.get("twitch_login") or "").lower()
        bid = str(p.get("twitch_user_id") or "")
        if not login or not bid or bid in live_ids:
            continue
        out.append((login, bid))
    return out


async def apply_global_bans_to_channel(
    chat_bot: Any,
    broadcaster_login: str,
    broadcaster_id: str,
    *,
    applied_pairs: set[tuple[str, str]] | None = None,
) -> int:
    """Bannt alle noch nicht angewandten Listen-Einträge in EINEM offline Kanal.

    Gibt die Anzahl frisch gesetzter Bans zurück. Selbstschützend: bricht ab,
    wenn der Kanal kein operativer Partner oder gerade live ist; überspringt
    Ziele, die selbst operative Partner sind (nie einen Streamer bannen).
    """
    from ..core import partner_utils
    from ..storage import pg

    broadcaster_login = (broadcaster_login or "").lower()
    broadcaster_id = str(broadcaster_id or "")
    if not broadcaster_login or not broadcaster_id:
        return 0
    if not partner_utils.is_operational_partner_channel(broadcaster_login):
        return 0
    if broadcaster_id in _live_broadcaster_ids():
        return 0  # live -> nichts tun

    try:
        entries = pg.list_chatter_global_bans()
    except Exception:
        log.debug("Global-Ban-Liste konnte nicht geladen werden", exc_info=True)
        return 0
    if not entries:
        return 0
    if applied_pairs is None:
        applied_pairs = pg.load_applied_global_ban_pairs()

    banned = 0
    for entry in entries:
        login = str(entry.get("chatter_login") or "").lower()
        if not login:
            continue
        if (login, broadcaster_id) in applied_pairs:
            continue
        if partner_utils.is_operational_partner_channel(login):
            continue  # Ziel ist selbst ein Partner -> niemals bannen
        target_id = str(entry.get("chatter_id") or "")
        if not target_id:
            target_id = await _resolve_user_id(chat_bot, login) or ""
        if not target_id:
            continue
        ok = await ban_user_direct(
            chat_bot,
            broadcaster_id=broadcaster_id,
            target_user_id=target_id,
            channel_login=broadcaster_login,
            login_hint=login,
        )
        if ok:
            try:
                pg.record_global_ban_applied(login, broadcaster_id)
            except Exception:
                log.debug("Applied-Ledger-Eintrag fehlgeschlagen", exc_info=True)
            applied_pairs.add((login, broadcaster_id))
            banned += 1
    return banned


async def run_full_sweep(chat_bot: Any) -> int:
    """Täglicher Sweep: alle operativen Partner-Kanäle, die gerade offline sind."""
    from ..storage import pg

    targets = _offline_partner_targets()
    if not targets:
        return 0
    try:
        applied_pairs = pg.load_applied_global_ban_pairs()
    except Exception:
        applied_pairs = set()
    total = 0
    for login, bid in targets:
        total += await apply_global_bans_to_channel(
            chat_bot, login, bid, applied_pairs=applied_pairs
        )
    if total:
        log.info(
            "Global-Ban-Sweep: %d Ban(s) über %d Offline-Kanäle",
            total,
            len(targets),
        )
    return total


async def run_due_sweeps(chat_bot: Any) -> int:
    """Fällige Stream-Ende-Sweeps (1h nach Offline) abarbeiten.

    Ist der Kanal zum Fälligkeitszeitpunkt wieder live (Stream-Restart), bleibt
    die Fälligkeit bestehen und wird beim nächsten Offline erneut versucht.
    """
    from ..storage import pg

    try:
        due = pg.load_due_global_ban_sweeps()
    except Exception:
        return 0
    if not due:
        return 0
    live_ids = _live_broadcaster_ids()
    try:
        applied_pairs = pg.load_applied_global_ban_pairs()
    except Exception:
        applied_pairs = set()
    total = 0
    for row in due:
        login = str(row.get("broadcaster_login") or "").lower()
        bid = str(row.get("broadcaster_id") or "")
        if not login or not bid:
            try:
                pg.delete_global_ban_sweep(login)
            except Exception:
                pass
            continue
        if bid in live_ids:
            continue  # wieder live -> Fälligkeit für nächstes Offline behalten
        total += await apply_global_bans_to_channel(
            chat_bot, login, bid, applied_pairs=applied_pairs
        )
        try:
            pg.delete_global_ban_sweep(login)
        except Exception:
            log.debug("Fälligkeit konnte nicht gelöscht werden: %s", login, exc_info=True)
    return total
