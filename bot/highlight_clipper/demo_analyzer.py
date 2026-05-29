from __future__ import annotations

import asyncio
import logging
import re
from dataclasses import dataclass, field
from pathlib import Path

log = logging.getLogger("TwitchStreams.HighlightClipper")

_BOON_PATH = Path(__file__).resolve().parents[2] / "tools" / "boon"
_TICK_RATE = 64
_COMBO_WINDOW_S = 10.0

# Abilities die als "High-Impact" gelten (Ultimates, starke CC-Abilities)
_HIGH_IMPACT = {
    "citadel_ability_uppercut",
    "citadel_ability_hook",
    "citadel_ability_flying_strike",
    "citadel_ability_infinity_slash",
    "citadel_ability_power_slash",
    "citadel_ability_lightning_ball",
    "citadel_ability_storm_cloud",
    "ability_ult_combo",
    "ability_power_surge",
    "drifter_darkness",
    "ability_vampirebat_batswarm",
    "ability_necro_zombiewall",
    "ability_incendiary_projectile",
}


@dataclass
class KillMoment:
    game_time_s: float
    tick: int
    combo_abilities: list[str] = field(default_factory=list)
    combo_score: int = 0       # Anzahl Abilities in Combo-Fenster
    has_high_impact: bool = False
    combo_label: str = ""


async def analyze_match(
    demo_path: Path,
    hero_id: int,
    kill_times_s: list[float],
    player_name: str = "",
) -> list[KillMoment]:
    """Extrahiert KillMoments mit Combo-Scoring aus der Demo."""
    loop = asyncio.get_event_loop()
    try:
        abilities = await loop.run_in_executor(None, _parse_abilities, demo_path)
    except Exception:
        log.exception("HighlightClipper: Demo-Analyse fehlgeschlagen")
        return []

    player_abilities = [(t, name) for t, hid, name in abilities if hid == hero_id]

    # Kill-Ticks aus API-Zeitstempeln ableiten (robuster als Namen-Matching)
    kill_ticks = [round(t * _TICK_RATE) for t in kill_times_s]

    moments: list[KillMoment] = []

    for kill_tick in kill_ticks:
        window = _COMBO_WINDOW_S * _TICK_RATE
        combo = [
            name for t, name in player_abilities
            if kill_tick - window <= t <= kill_tick
        ]
        high_impact = any(a in _HIGH_IMPACT for a in combo)
        label = _build_combo_label(combo)
        moments.append(KillMoment(
            game_time_s=kill_tick / _TICK_RATE,
            tick=kill_tick,
            combo_abilities=combo,
            combo_score=len(combo),
            has_high_impact=high_impact,
            combo_label=label,
        ))

    return moments


def _build_combo_label(abilities: list[str]) -> str:
    """Erzeugt ein lesbares Combo-Label wie 'Hook → Bomb → Uppercut'."""
    if not abilities:
        return "Kill"
    pretty = {
        "citadel_ability_hook": "Hook",
        "citadel_ability_sticky_bomb": "Bomb",
        "citadel_ability_uppercut": "Uppercut",
        "citadel_ability_power_slash": "Power Slash",
        "citadel_ability_flying_strike": "Flying Strike",
        "citadel_ability_infinity_slash": "Infinity Slash",
        "citadel_ability_lightning_ball": "Lightning Ball",
        "citadel_ability_storm_cloud": "Storm Cloud",
        "ability_ult_combo": "Ultimate",
        "ability_power_surge": "Power Surge",
        "ability_nano_dash": "Dash",
        "ability_flame_dash": "Flame Dash",
        "drifter_darkness": "Darkness",
        "ability_vampirebat_batswarm": "Bat Swarm",
        "ability_throw_sand": "Sand Throw",
        "ability_intimidate": "Intimidate",
    }
    seen: list[str] = []
    for a in abilities:
        label = pretty.get(a) or a.replace("citadel_ability_", "").replace("ability_", "").replace("_", " ").title()
        if label not in seen:
            seen.append(label)
    return " → ".join(seen)


def _parse_abilities(demo_path: Path) -> list[tuple[int, int, str]]:
    result = _run_boon(["abilities", str(demo_path)])
    abilities = []
    for line in result.splitlines():
        parts = line.split()
        if len(parts) >= 3 and parts[0].isdigit() and parts[1].isdigit():
            abilities.append((int(parts[0]), int(parts[1]), parts[2]))
    return abilities


def _parse_kills(demo_path: Path, player_name: str) -> list[int]:
    result = _run_boon(["events", str(demo_path)])
    kills = []
    current_tick: int | None = None
    in_death = False
    attacker_name = ""

    for line in result.splitlines():
        tick_match = re.match(r"\[tick (\d+)\] player_death", line)
        if tick_match:
            current_tick = int(tick_match.group(1))
            in_death = True
            attacker_name = ""
            continue
        if in_death:
            if line.strip() == "--" or (line.startswith("[tick") and "player_death" not in line):
                if attacker_name == player_name and current_tick is not None:
                    kills.append(current_tick)
                in_death = False
                continue
            m = re.match(r"\s+attackername: (.+)", line)
            if m:
                attacker_name = m.group(1).strip()

    return kills


def _run_boon(args: list[str]) -> str:
    import subprocess
    if not _BOON_PATH.exists():
        log.error("HighlightClipper: boon Binary nicht gefunden: %s", _BOON_PATH)
        return ""
    try:
        result = subprocess.run(
            [str(_BOON_PATH)] + args,
            capture_output=True, text=True, timeout=60
        )
        return result.stdout
    except Exception:
        log.exception("HighlightClipper: boon fehlgeschlagen")
        return ""
