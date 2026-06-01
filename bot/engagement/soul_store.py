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

from bot.storage.pg import query_all, transaction

_MAX_ANCHORS = 5          # so viele jüngste Anker in den Prompt
_KEEP_ANCHORS = 30        # so viele Anker insgesamt behalten


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
