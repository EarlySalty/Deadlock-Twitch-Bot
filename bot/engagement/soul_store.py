"""Geschichtete Soul: dynamische Erweiterungen unter dem statischen Kern-Soul.

Der Kern-Soul ist eine Konstante in ``minimax_chat.py`` (Charakter/Stimme). Hier
kommen die wachsenden Teile dazu, persistiert in ``twitch_engagement_soul``:

- ``hero_takes`` — einmalig von MiniMax kuratierte Hero-Vorlieben (aus allen Helden
  + echten Abilities). Liefern die MEINUNG des Bots zu Helden, nicht seinen Ton.
- ``anchor`` — kurze Notizen, die der Bot sich mit der Zeit selbst anhängt, wenn ein
  geiles Gespräch lief oder er was Cooles entdeckt hat. Macht die Soul lebendig.

``get_soul_extension_fragment`` baut daraus EIN Fragment, das die Pipeline direkt
unter den Kern-Soul hängt. Wichtig (User-Vorgabe): die Hero-Takes sind innerer
Geschmack — der over-excited Ton darf NICHT in den Chat schwappen, der Bot bleibt
trocken/knapp. Das steht explizit im Fragment-Vorspann.
"""

from __future__ import annotations

import asyncio
import logging
import re

from bot.storage.pg import query_all, transaction

log = logging.getLogger("TwitchStreams.Engagement.Soul")

_THINK_RE = re.compile(r"<think>.*?</think>", re.DOTALL | re.IGNORECASE)

_MAX_ANCHORS = 5          # so viele jüngste Anker in den Prompt
_KEEP_ANCHORS = 30        # so viele Anker insgesamt behalten

# Reflexions-Job (dynamische Anker)
_REFLECT_TURNS = 40       # so viele jüngste Turns ansehen
_REFLECT_MIN_TURNS = 8    # darunter lohnt sich Reflexion nicht
_ANCHOR_MAX_LEN = 220


def _sync_store_entry(kind: str, content: str) -> None:
    with transaction() as conn:
        conn.execute(
            "INSERT INTO twitch_engagement_soul (kind, content) VALUES (%s, %s)",
            [kind, content],
        )
        if kind == "anchor":
            conn.execute(
                """
                DELETE FROM twitch_engagement_soul
                WHERE kind = 'anchor' AND id NOT IN (
                    SELECT id FROM twitch_engagement_soul
                    WHERE kind = 'anchor' ORDER BY created_at DESC LIMIT %s
                )
                """,
                [int(_KEEP_ANCHORS)],
            )


def _sync_latest_hero_takes() -> str | None:
    rows = query_all(
        "SELECT content FROM twitch_engagement_soul WHERE kind='hero_takes' "
        "ORDER BY created_at DESC LIMIT 1"
    )
    return rows[0][0] if rows and rows[0] and rows[0][0] else None


def _sync_recent_anchors(limit: int) -> list[str]:
    rows = query_all(
        "SELECT content FROM twitch_engagement_soul WHERE kind='anchor' "
        "ORDER BY created_at DESC LIMIT %s",
        [int(limit)],
    )
    return [r[0] for r in rows if r and r[0]]


async def store_soul_entry(kind: str, content: str) -> None:
    await asyncio.to_thread(_sync_store_entry, kind, content)


async def get_soul_extension_fragment() -> str:
    """Hero-Takes + jüngste Anker als EIN Fragment unter den Kern-Soul; "" wenn nichts da."""
    takes = await asyncio.to_thread(_sync_latest_hero_takes)
    anchors = await asyncio.to_thread(_sync_recent_anchors, _MAX_ANCHORS)
    if not takes and not anchors:
        return ""

    parts: list[str] = [
        "Noch was zu dir — aber WICHTIG: das hier ist dein INNERER Geschmack und dein "
        "Gedächtnis, nicht dein Chat-Ton. Zieh daraus deine Meinung, aber bleib im Chat "
        "trocken und knapp wie immer. Kipp diese Begeisterung NICHT 1:1 raus, kein Gehype, "
        "kein Schwall — eine ruhige, beiläufige Zeile reicht. Beziehe dich nur auf Helden/"
        "Abilities, die hier vorkommen."
    ]
    if takes:
        parts.append(f"Deine Hero-Vorlieben:\n{takes}")
    if anchors:
        lines = "\n".join(f"- {a}" for a in anchors)
        parts.append(
            "Dinge, die dir zuletzt aufgefallen sind oder die du cool fandest "
            f"(nur beiläufig aufgreifen, wenn's grad passt):\n{lines}"
        )
    return "\n\n".join(parts)


# ---------------------------------------------------------------------------
# Dynamische Anker: der Bot schaut sich seine letzten Chats an und merkt sich
# selbst was, wenn ein geiles Gespräch lief oder er was Cooles entdeckt hat.
# ---------------------------------------------------------------------------

_ANCHOR_SYS = (
    "Du bist eine feste Twitch-Chat-Persönlichkeit (ein Deadlock-Stammgast). "
    "Antworte knapp und nur mit dem Verlangten."
)


def _anchor_user_prompt(transcript: str) -> str:
    return (
        "Hier ein Ausschnitt aus dem Chat, in dem du grad unterwegs warst ('ich' = du). "
        "Ist dir was hängengeblieben — ein geiles Gespräch, ein Running Gag, ein cooler Move "
        "den jemand beschrieben hat, oder was Cooles das du entdeckt hast — was DU dir als "
        "dieser Typ wirklich merken würdest? Wenn ja, schreib EINE kurze Ich-Notiz an dich "
        "selbst (max 1 Satz, locker, wie ein mentaler Merker, kein Namedropping von Usern als "
        "Fakt). Wenn nichts wirklich hängengeblieben ist, antworte EXAKT mit: NICHTS\n\n"
        f"Chat:\n{transcript}"
    )


def _sync_recent_convo(limit: int) -> list[tuple]:
    rows = query_all(
        "SELECT role, twitch_login, content FROM twitch_engagement_conversation "
        "ORDER BY ts DESC LIMIT %s",
        [int(limit)],
    )
    return list(reversed(rows))


def _sync_last_anchor() -> str | None:
    rows = query_all(
        "SELECT content FROM twitch_engagement_soul WHERE kind='anchor' "
        "ORDER BY created_at DESC LIMIT 1"
    )
    return rows[0][0] if rows and rows[0] and rows[0][0] else None


async def reflect_and_store_anchor(*, minimax=None) -> str | None:
    """Reflektiert die letzten Chats; speichert einen Anker, wenn was hängenblieb."""
    rows = await asyncio.to_thread(_sync_recent_convo, _REFLECT_TURNS)
    if len(rows) < _REFLECT_MIN_TURNS:
        return None
    if not any(r[0] == "assistant" for r in rows):
        return None  # der Bot war gar nicht aktiv → nichts zu erinnern

    lines = []
    for role, login, content in rows:
        who = "ich" if role == "assistant" else (login or "jemand")
        lines.append(f"{who}: {content}")
    transcript = "\n".join(lines)[-4000:]

    if minimax is None:
        from .minimax_chat import EngagementMinimaxClient

        minimax = EngagementMinimaxClient(timeout=180.0)
    try:
        oc = minimax._ensure_client()
        resp = await oc.chat.completions.create(
            model=minimax._model,
            messages=[
                {"role": "system", "content": _ANCHOR_SYS},
                {"role": "user", "content": _anchor_user_prompt(transcript)},
            ],
            max_tokens=2000,
            temperature=0.7,
        )
    except Exception:
        log.warning("SoulAnchor: MiniMax-Call fehlgeschlagen", exc_info=False)
        return None

    raw = resp.choices[0].message.content if resp.choices else ""
    text = _THINK_RE.sub("", raw or "").strip().strip('"')
    if not text or text.upper().startswith("NICHTS") or len(text) > _ANCHOR_MAX_LEN:
        return None

    last = await asyncio.to_thread(_sync_last_anchor)
    if last and text.strip().lower() == last.strip().lower():
        return None

    await store_soul_entry("anchor", text)
    log.info("SoulAnchor: neuer Anker gespeichert: %r", text[:90])
    return text
