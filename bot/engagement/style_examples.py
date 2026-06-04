"""Konkrete Stil-Beispiele aus echtem Channel-Chat (Few-Shot).

Ergänzt `persona.py`: während persona den Vibe *statistisch beschreibt*
("kurze Sätze, vereinzelte Emojis, Slang"), zeigt dieses Modul dem Modell
*konkrete echte Nachrichten* als Stilvorlage. LLMs imitieren Beispiele
treffsicherer als Stil-Adjektive — das ist die "show, don't tell"-Schicht
gegen AI-isches Schreiben.

Quelle: die letzten User-Turns aus `twitch_engagement_conversation` (dieselbe
Tabelle wie persona). Gefiltert auf kurze, saubere, repräsentative Zeilen.

WICHTIG: Der erzeugte Prompt-Block trennt hart zwischen Stil und Inhalt — das
Modell soll NUR Schreibweise/Ton/Länge nachahmen, niemals Behauptungen oder
Spielfakten aus den Beispielen übernehmen. Sonst käme die Halluzinations-
Kontamination über die Hintertür zurück (vgl. das erfundene "Cornucopius"-Item).
Faktenquelle bleibt ausschliesslich das Wiki-Grounding (`deadlock_wiki`).
"""

from __future__ import annotations

import asyncio
import time

from bot.storage.pg import query_all

_POOL_LIMIT = 120        # so viele jüngste Turns durchsuchen
_MAX_EXAMPLES = 8        # so viele Beispiele in den Prompt
_GOLD_KEEP = 4           # so viele Gold-Standard-Zeilen IMMER zuerst
_MIN_LEN = 8
_MAX_LEN = 100

_cache: dict[str, tuple[float, str]] = {}
_CACHE_TTL_SEC = 600.0   # 10min

# Kuratierter Stil-Fallback: echtes Deadlock-Twitch-Chat-Register (DE/EN gemischt,
# Kleinschreibung, Slang, Emotes, kurz). Wird genutzt, wenn ein Channel noch zu
# wenig eigene Turns hat (kalter Start) — sonst stünde das Modell ohne Stilvorlage
# da und schreibt generisch-AI-haft. NUR Schreibweise/Ton; bewusst reine
# Vibe-/Reaktionszeilen ohne erfundene Item-/Zahlen-Fakten (Inhalt wird eh ignoriert).
_SEED_EXAMPLES: list[str] = [
    "lol was war das für ein dive",
    "brudi warum gehst du da solo rein 😭",
    "der flick war einfach nasty ngl",
    "ok der gap close ist kriminell",
    "no shot dass der das überlebt hat",
    "warum peelt da eigentlich keiner",
    "der teamfight grad war komplett wild",
    "der heal kam mega clutch",
    "sheesh die combo war eklig",
    "läuft bei dir heut richtig gut",
    "yo that dive was actually nasty",
    "bro why go solo in there lol",
    "that last fight was so clean",
    "no way he survived that one",
    "the gap close is straight up crime",
    "man this lane is rough rn",
]

# Gold-Standard: echter Schreibstil von EarlySalty (vom User als Ziel-Vibe bestätigt) —
# kurz, trocken, viel Banter/Roast, oft nur ein paar Wörter, Deadlock wenn's passt.
# Wird IMMER zuerst eingespeist, damit der Bot in diesem Register schreibt.
_GOLD_EXAMPLES: list[str] = [
    "wilder take",
    "haha legit",
    "uno reverse karte",
    "wieder geistig am start ne",
    "das es scheiße ist weiß ich haha",
    "der findet das loch eh nicht",
    "der hätte dich da eig wegbügeln müssen",
    "außer du parrierst halt",
    "ja so was wie burger boxing oder wie nennt man das",
    "aber meta ist deutlich angenehmer grade",
    "und die haben noch 2 heal creeps lol",
    "bisschen rough aber passt schon",
]


def _sync_load_user_turns(channel_login: str, limit: int) -> list[str]:
    rows = query_all(
        """
        SELECT content FROM twitch_engagement_conversation
        WHERE channel_login = %s AND role = 'user'
        ORDER BY ts DESC LIMIT %s
        """,
        [channel_login, limit],
    )
    return [r[0] for r in rows if r and r[0]]


def _is_good_example(text: str) -> bool:
    if not (_MIN_LEN <= len(text) <= _MAX_LEN):
        return False
    if " " not in text:  # Einzel-Token / reines Emote
        return False
    if text[:1] in ("!", "/", "."):  # Commands
        return False
    low = text.lower()
    if "http" in low or "www." in low:  # Links
        return False
    letters = [c for c in text if c.isalpha()]
    if letters and sum(c.isupper() for c in letters) / len(letters) > 0.6:
        return False  # CAPS-Spam
    return True


_MAX_SAME_STARTER = 2  # max so viele Beispiele mit gleichem Starter-Wort

def _select_examples(texts: list[str], max_n: int = _MAX_EXAMPLES) -> list[str]:
    out: list[str] = []
    seen: set[str] = set()
    starter_count: dict[str, int] = {}
    for raw in texts:
        text = " ".join(str(raw).split())
        if not _is_good_example(text):
            continue
        key = text.lower()
        if key in seen:
            continue
        starter = text.split()[0].lower().rstrip(".,!?") if text.split() else ""
        if starter_count.get(starter, 0) >= _MAX_SAME_STARTER:
            continue
        seen.add(key)
        starter_count[starter] = starter_count.get(starter, 0) + 1
        out.append(text)
        if len(out) >= max_n:
            break
    return out


def _assemble_examples(channel_examples: list[str]) -> list[str]:
    """Gold-Standard (EarlySalty-Register) zuerst, dann channel-eigene Zeilen, dann Seeds.

    So schreibt der Bot immer im kurzen, trockenen Gold-Register, behält aber die
    Stimme des Channels bei, sobald genug eigene Zeilen da sind.
    """
    out: list[str] = []
    seen: set[str] = set()
    for source in (_GOLD_EXAMPLES[:_GOLD_KEEP], channel_examples, _SEED_EXAMPLES):
        for raw in source:
            if len(out) >= _MAX_EXAMPLES:
                break
            cand = " ".join(str(raw).split())
            if not _is_good_example(cand):
                continue
            key = cand.lower()
            if key in seen:
                continue
            seen.add(key)
            out.append(cand)
    return out


def _build_fragment(examples: list[str]) -> str:
    if not examples:
        return ""
    lines = "\n".join(f"- {e}" for e in examples)
    return (
        "So schreiben echte Leute hier — kurz, trocken, mit Banter, oft nur ein paar Wörter. "
        "Ahme NUR Schreibweise, Ton und Länge nach (Kleinschreibung/Slang wie üblich, knapp, "
        "keine perfekte Grammatik). "
        "Den INHALT dieser Beispiele und alle darin enthaltenen Behauptungen oder Spielfakten "
        "IGNORIERST du komplett — sie sind reine Stilvorlage, keine Quelle:\n"
        f"{lines}"
    )


async def build_style_fragment(channel_login: str) -> str:
    """Few-Shot-Stilblock pro Channel; cached 10min.

    Channel-eigene Zeilen bevorzugt, mit kuratierten Seeds aufgefüllt — der Block
    ist daher auch auf kaltem Channel nie leer (gegen generisch-AI-haftes Schreiben).
    """
    now = time.time()
    cached = _cache.get(channel_login)
    if cached and (now - cached[0]) < _CACHE_TTL_SEC:
        return cached[1]
    texts = await asyncio.to_thread(_sync_load_user_turns, channel_login, _POOL_LIMIT)
    examples = _assemble_examples(_select_examples(texts))
    fragment = _build_fragment(examples)
    _cache[channel_login] = (now, fragment)
    return fragment
