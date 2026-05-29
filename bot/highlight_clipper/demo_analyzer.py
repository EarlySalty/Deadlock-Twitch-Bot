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
    combo_score: int = 0
    has_high_impact: bool = False
    combo_label: str = ""
    health_pct: float = 1.0    # Health-Prozent beim Kill (1.0 = 100%)
    is_clutch: bool = False    # True wenn health_pct < 0.35

    @property
    def excitement_score(self) -> int:
        """Gesamtpunktzahl — Health-basiert, nicht Ability-basiert."""
        score = 0
        if self.health_pct < 0.20:
            score += 5   # Extrem niedrige HP — ultra clutch
        elif self.health_pct < 0.35:
            score += 3   # Gefährlich niedrig — echter Clutch
        elif self.health_pct < 0.50:
            score += 1   # Etwas unter Druck
        # Kombos als Zusatz-Kontext, aber nicht primäre Metrik
        if self.has_high_impact and self.combo_score >= 2:
            score += 1
        return score


async def analyze_match(
    demo_path: Path,
    hero_id: int,
    kill_times_s: list[float],
    player_name: str = "",
) -> list[KillMoment]:
    """Extrahiert KillMoments mit Health- und Combo-Scoring aus der Demo."""
    loop = asyncio.get_event_loop()
    try:
        abilities = await loop.run_in_executor(None, _parse_abilities, demo_path)
    except Exception:
        log.exception("HighlightClipper: Demo-Analyse fehlgeschlagen")
        return []

    player_abilities = [(t, name) for t, hid, name in abilities if hid == hero_id]

    kill_ticks = [round(t * _TICK_RATE) for t in kill_times_s]

    # Entity-Index für unseren Spieler — anhand der bekannten Kill-Ticks
    entity_idx = await loop.run_in_executor(None, _find_player_entity, demo_path, kill_ticks)
    moments: list[KillMoment] = []

    for kill_tick in kill_ticks:
        window = _COMBO_WINDOW_S * _TICK_RATE
        combo = [
            name for t, name in player_abilities
            if kill_tick - window <= t <= kill_tick
        ]
        high_impact = any(a in _HIGH_IMPACT for a in combo)
        label = _build_combo_label(combo)

        # Minimum-Health im Fight-Fenster: Kill ± 15s (Fight kann danach weitergehen)
        health_pct = 1.0
        if entity_idx is not None:
            health_pct = await loop.run_in_executor(
                None, _get_min_health_in_window,
                demo_path, entity_idx,
                max(0, kill_tick - 320),     # 5s vor Kill
                kill_tick + 960,             # 15s nach Kill
                320,                         # 5s Schritte
            )

        is_clutch = health_pct < 0.35
        moments.append(KillMoment(
            game_time_s=kill_tick / _TICK_RATE,
            tick=kill_tick,
            combo_abilities=combo,
            combo_score=len(combo),
            has_high_impact=high_impact,
            combo_label=label,
            health_pct=health_pct,
            is_clutch=is_clutch,
        ))

    return moments


def _find_player_entity(demo_path: Path, kill_ticks: list[int]) -> int | None:
    """Findet den Entity-Index des Spielers über seine bekannten Kill-Ticks."""
    result = _run_boon(["events", str(demo_path)])
    current_tick: int | None = None
    in_death = False
    tolerance = 200  # ~3s bei 64 ticks/s

    for line in result.splitlines():
        m = re.match(r"\[tick (\d+)\] player_death", line)
        if m:
            current_tick = int(m.group(1))
            in_death = True
            continue
        if in_death:
            if line.strip() == "--":
                in_death = False
                continue
            if current_tick and any(abs(current_tick - kt) < tolerance for kt in kill_ticks):
                m2 = re.match(r"\s+attacker_pawn: (-?\d+)", line)
                if m2:
                    pawn = int(m2.group(1))
                    return (pawn & 0xFFFFFFFF) & 0x7FFF
    return None


def _get_health_at_tick(demo_path: Path, entity_idx: int, tick: int) -> tuple[int, int] | None:
    import json as _json
    result = _run_boon(["entities", str(demo_path), "--tick", str(tick),
                        "--filter", "CitadelPlayerPawn", "--json"])
    if not result:
        return None
    try:
        ents = _json.loads(result)
        for e in ents:
            if e.get("index") == entity_idx:
                f = e.get("fields", {})
                hp = f.get("m_iHealth")
                max_hp = f.get("m_iMaxHealth")
                if hp is not None and max_hp:
                    return int(hp), int(max_hp)
    except Exception:
        pass
    return None


def _get_min_health_in_window(
    demo_path: Path, entity_idx: int, start_tick: int, end_tick: int, step: int
) -> float:
    """Minimum Health-Prozent im Zeitfenster — erkennt low-HP-Momente während/nach dem Kill."""
    min_pct = 1.0
    for tick in range(start_tick, end_tick, step):
        hp_data = _get_health_at_tick(demo_path, entity_idx, tick)
        if hp_data:
            hp, max_hp = hp_data
            if max_hp > 0:
                pct = min(1.0, hp / max_hp)
                if pct < min_pct:
                    min_pct = pct
    return min_pct


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
