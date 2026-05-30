"""Discord-Promo-Presets.

Globale und user-spezifische Templates. MiniMax wählt anhand des
User-Kontexts das passendste aus — generiert keinen Freitext.

Jedes Preset hat:
    id       – eindeutige Kennung (stable)
    type     – "global" (Kanal-Announcement) oder "user" (mit @{login})
    text     – Twitch-Chat-Text; {invite} = Discord-URL, {login} = Chatter-Name
    tags     – Schlagwörter für die Präsentation an MiniMax
"""
from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class PromoPreset:
    id: str
    type: str  # "global" | "user"
    text: str
    tags: tuple[str, ...]


PRESETS: tuple[PromoPreset, ...] = (
    # ── Global (Kanal-Announcement) ────────────────────────────────────────
    PromoPreset(
        id="g_competitive",
        type="global",
        text="Kein Bock mehr auf Solo-Queue-Grief? Such dir feste Mates in unserer deutschen Deadlock-Community! {invite} 🔫",
        tags=("ranked", "mmr", "solo_queue", "competitive", "mates"),
    ),
    PromoPreset(
        id="g_community",
        type="global",
        text="Bock auf Inhouses oder kleine Turniere? Wir organisieren regelmäßig Events – schau gerne vorbei: {invite} 🏆",
        tags=("inhouse", "events", "tournament", "community", "fun"),
    ),
    PromoPreset(
        id="g_new_to_deadlock",
        type="global",
        text="Neu in Deadlock? Unsere Community hat Guides, Tipps und Leute die gerne helfen: {invite} 📚",
        tags=("new_player", "beginner", "guide", "help", "learning"),
    ),
    PromoPreset(
        id="g_meta",
        type="global",
        text="Patch-Diskussionen, Tier-Listen, Meta-Talks – alles bei uns auf Discord: {invite}",
        tags=("meta", "patch", "tierlist", "discussion", "build"),
    ),
    PromoPreset(
        id="g_chill",
        type="global",
        text="Wer nach dem Stream noch Deadlock zockt und ne Community sucht – wir sind auf Discord: {invite} 👀",
        tags=("chill", "after_stream", "looking_for_group", "casual"),
    ),
    # ── User-spezifisch (mit @mention) ────────────────────────────────────
    PromoPreset(
        id="u_welcome",
        type="user",
        text="@{login} Willkommen! Falls du noch eine deutsche Deadlock-Community suchst – hier entlang: {invite} 🎮",
        tags=("new", "first_time", "welcome", "lurker"),
    ),
    PromoPreset(
        id="u_mates",
        type="user",
        text="@{login} Falls du Mates zum Zocken suchst, in unserer Community wirst du fündig: {invite}",
        tags=("looking_for_group", "mates", "team", "duo", "party"),
    ),
    PromoPreset(
        id="u_lurker_viewer",
        type="user",
        text="@{login} Regelmäßig dabei? 👀 Falls du über Deadlock reden willst, komm gerne auf unseren Discord: {invite}",
        tags=("lurker", "regular_viewer", "silent", "watcher"),
    ),
    PromoPreset(
        id="u_ranked_grind",
        type="user",
        text="@{login} Ranked-Grind solo macht keinen Spaß – bei uns findest du Leute für den Duo-Queue: {invite} 🔫",
        tags=("ranked", "competitive", "grind", "duo", "elo"),
    ),
    PromoPreset(
        id="u_new_player",
        type="user",
        text="@{login} Neu in Deadlock? Unsere Community hilft gerne beim Einstieg – schau mal vorbei: {invite} 📚",
        tags=("new_player", "beginner", "help", "learning", "guide"),
    ),
)

PRESET_MAP: dict[str, PromoPreset] = {p.id: p for p in PRESETS}
GLOBAL_PRESETS: tuple[PromoPreset, ...] = tuple(p for p in PRESETS if p.type == "global")
USER_PRESETS: tuple[PromoPreset, ...] = tuple(p for p in PRESETS if p.type == "user")
