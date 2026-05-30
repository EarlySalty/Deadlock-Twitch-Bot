"""Zielgerichtete Discord-Promos mit MiniMax-Preset-Auswahl.

Flow:
    1. Aktive Chatter aus dem Promo-Aktivitäts-Bucket holen
    2. Kandidaten filtern: keine Stammgäste, noch nicht heute gepitcht
    3. Optional einen User-Kandidaten wählen; globale Promos abwechseln
    4. MiniMax wählt das beste Preset anhand des User-Kontexts (kein Freitext)
    5. Template rendern + senden

Max 1 Pitch pro User pro Tag (in-memory, reset bei Bot-Restart).
Global- und User-Promos wechseln sich ab.
"""
from __future__ import annotations

import asyncio
import logging
import random
import secrets
import time

from bot.storage.pg import query_all, query_one

from .promo_presets import (
    GLOBAL_PRESETS,
    PRESET_MAP,
    USER_PRESETS,
    PromoPreset,
)

log = logging.getLogger("TwitchStreams.ChatBot")

# ── Schwellen ──────────────────────────────────────────────────────────────
_STAMMGAST_MIN_MESSAGES = 10   # ≥ N Messages/30 Tage → Stammgast → ausschließen
_STAMMGAST_DAYS = 30
_USER_PITCH_COOLDOWN_SEC = 24 * 3600   # max 1x pro User pro Tag
_CHANNEL_TARGETED_COOLDOWN_SEC = 15 * 60  # min 15 Min zwischen zielgerichteten Pitches
_MINIMAX_TIMEOUT_SEC = 5.0             # MiniMax-Auswahl-Call darf nicht blockieren

# ── State (in-memory) ──────────────────────────────────────────────────────
_user_last_pitched: dict[tuple[str, str], float] = {}   # (channel, user_id) → mono ts
_channel_last_targeted: dict[str, float] = {}           # channel → mono ts
_channel_last_type: dict[str, str] = {}                 # channel → "global" | "user"


# ── User-Klassifizierung ───────────────────────────────────────────────────

def _sync_is_stammgast(twitch_user_id: str, channel_login: str) -> bool:
    row = query_one(
        f"""
        SELECT COUNT(*) AS cnt
          FROM twitch_engagement_conversation
         WHERE channel_login = %s
           AND twitch_user_id = %s
           AND role = 'user'
           AND ts > NOW() - INTERVAL '{int(_STAMMGAST_DAYS)} days'
        """,
        [channel_login, twitch_user_id],
    )
    if row is None:
        return False
    cnt = int(row[0] if not hasattr(row, "keys") else row["cnt"] or 0)
    return cnt >= _STAMMGAST_MIN_MESSAGES


def _sync_user_context_snippets(
    twitch_user_id: str, channel_login: str, limit: int = 5
) -> list[str]:
    """Letzte paar User-Messages für MiniMax-Kontext."""
    rows = query_all(
        """
        SELECT content
          FROM twitch_engagement_conversation
         WHERE channel_login = %s
           AND twitch_user_id = %s
           AND role = 'user'
         ORDER BY ts DESC
         LIMIT %s
        """,
        [channel_login, twitch_user_id, limit],
    )
    snippets = []
    for row in rows:
        text = str(row[0] if not hasattr(row, "keys") else row["content"] or "").strip()
        if text:
            snippets.append(text)
    return snippets


# ── MiniMax Preset-Auswahl ─────────────────────────────────────────────────

async def _pick_preset_with_minimax(
    presets: list[PromoPreset],
    user_snippets: list[str],
    user_login: str,
) -> PromoPreset:
    """MiniMax wählt das passendste Preset – antwortet nur mit der ID.

    Bei Fehler oder unbekannter ID: zufälliges Preset aus dem Pool.
    """
    if len(presets) == 1 or not user_snippets:
        return secrets.choice(presets)

    preset_list = "\n".join(
        f'- {p.id}: {", ".join(p.tags)}' for p in presets
    )
    context = " | ".join(user_snippets[:3])
    system = (
        "Du wählst das passendste Discord-Einladungs-Preset für einen Twitch-Nutzer. "
        "Antworte ausschließlich mit der Preset-ID, kein anderer Text."
    )
    user_msg = (
        f"User @{user_login} hat zuletzt geschrieben: {context}\n\n"
        f"Verfügbare Presets (ID: Tags):\n{preset_list}\n\n"
        "Welche ID passt am besten?"
    )

    try:
        from bot.engagement.minimax_chat import (
            ChatMessage,
            EngagementMinimaxClient,
            LLMProviderUnavailable,
        )
        client = EngagementMinimaxClient()
        response = await asyncio.wait_for(
            client.generate(
                system_prompt=system,
                history=[ChatMessage(role="user", content=user_msg)],
                max_output_tokens=20,
            ),
            timeout=_MINIMAX_TIMEOUT_SEC,
        )
        chosen_id = (response.text or "").strip().lower()
        matched = PRESET_MAP.get(chosen_id)
        if matched and matched in presets:
            return matched
        # Partial match
        for p in presets:
            if p.id in chosen_id:
                return p
    except (LLMProviderUnavailable, asyncio.TimeoutError):
        pass
    except Exception:
        log.debug("MiniMax Preset-Auswahl fehlgeschlagen", exc_info=True)

    return secrets.choice(presets)


# ── Kandidaten-Auswahl ─────────────────────────────────────────────────────

def _pick_user_target(
    active_chatters: list[str],
    channel_login: str,
    now: float,
) -> tuple[str, str] | None:
    """Wählt einen Chatter der noch nicht heute gepitcht wurde und kein Stammgast ist.

    Returns (twitch_login, twitch_user_id) oder None.
    Läuft synchron – Stammgast-Check per DB.
    """
    day_ago = now - _USER_PITCH_COOLDOWN_SEC
    candidates = [
        login for login in active_chatters
        if _user_last_pitched.get((channel_login, login), 0.0) < day_ago
    ]
    if not candidates:
        return None

    random.shuffle(candidates)
    for login in candidates[:6]:  # max 6 prüfen damit DB nicht überlastet wird
        try:
            # user_id via Login nachschlagen
            row = query_one(
                """
                SELECT chatter_id
                  FROM twitch_session_chatters
                 WHERE LOWER(chatter_login) = LOWER(%s)
                   AND LOWER(streamer_login) = LOWER(%s)
                 ORDER BY last_seen_at DESC
                 LIMIT 1
                """,
                [login, channel_login],
            )
            user_id = str(
                (row[0] if not hasattr(row, "keys") else row["chatter_id"]) or ""
            ).strip() if row else ""
            if not user_id:
                continue
            if _sync_is_stammgast(user_id, channel_login):
                continue
            return login, user_id
        except Exception:
            log.debug("Kandidaten-Check fehlgeschlagen für %s", login, exc_info=True)
    return None


# ── Haupt-Entry-Point ──────────────────────────────────────────────────────

async def maybe_send_targeted_promo(
    *,
    bot,
    channel_login: str,
    channel_id: str,
    active_chatters: list[str],
    invite_url: str,
    now: float,
) -> bool:
    """Versucht einen zielgerichteten oder globalen Promo-Pitch zu senden.

    Gibt True zurück wenn eine Nachricht gesendet wurde.
    """
    # Kanal-Cooldown
    last_targeted = _channel_last_targeted.get(channel_login, 0.0)
    if now - last_targeted < _CHANNEL_TARGETED_COOLDOWN_SEC:
        return False

    last_type = _channel_last_type.get(channel_login, "global")
    want_user = last_type == "global"  # abwechseln

    send_fn = getattr(bot, "_send_chat_message", None)
    send_ann = getattr(bot, "_send_announcement", None)
    make_ch = getattr(bot, "_make_promo_channel", None)
    mark = getattr(bot, "_mark_promo_sent", None)

    if not callable(send_fn) or not callable(make_ch):
        return False

    chosen_preset: PromoPreset | None = None
    target_login: str | None = None
    target_user_id: str | None = None

    if want_user and active_chatters:
        # User-spezifischer Pitch versuchen
        result = await asyncio.to_thread(
            _pick_user_target, active_chatters, channel_login, now
        )
        if result:
            target_login, target_user_id = result
            snippets = await asyncio.to_thread(
                _sync_user_context_snippets, target_user_id, channel_login
            )
            chosen_preset = await _pick_preset_with_minimax(
                list(USER_PRESETS), snippets, target_login
            )

    if chosen_preset is None:
        # Globaler Pitch (Fallback oder planmäßig)
        chosen_preset = await _pick_preset_with_minimax(
            list(GLOBAL_PRESETS),
            [],
            "",
        )

    text = chosen_preset.text.format(
        invite=invite_url,
        login=target_login or "",
    )

    channel_obj = make_ch(channel_login, channel_id)
    if chosen_preset.type == "global" and callable(send_ann):
        ok = await send_ann(channel_obj, text, color="purple", source="promo")
    else:
        ok = await send_fn(channel_obj, text, source="promo")

    if not ok:
        return False

    _channel_last_targeted[channel_login] = now
    _channel_last_type[channel_login] = chosen_preset.type

    if target_login and target_user_id:
        _user_last_pitched[(channel_login, target_login)] = now

    if callable(mark):
        mark(channel_login, now, reason="targeted_promo")

    log.info(
        "Targeted-Promo gesendet (channel=%s, preset=%s, target=%s)",
        channel_login,
        chosen_preset.id,
        target_login or "global",
    )
    return True
