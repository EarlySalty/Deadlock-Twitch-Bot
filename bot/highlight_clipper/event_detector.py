from __future__ import annotations

from dataclasses import dataclass, field
from typing import Literal

from .config import MULTIKILL_MIN_KILLS
from .config import MULTIKILL_THRESHOLD_SECONDS
from .config import TEAMFIGHT_MIN_KILLS
from .config import TEAMFIGHT_THRESHOLD_SECONDS

_CLOSE_FIGHT_WINDOW_S = 40
_MAX_PRE_ROLL_S = 35  # Clip-Vorlauf nie länger als das


@dataclass(slots=True)
class HighlightEvent:
    event_type: Literal["multikill", "teamfight", "close_fight"]
    game_time_s: int
    duration_s: int
    kill_count: int
    label: str
    pre_roll_s: int = field(default=0)  # dynamischer Vorlauf basierend auf TTK


def detect_events(account_id: int, match_info: dict) -> list[HighlightEvent]:
    players = [p for p in (match_info.get("players") or []) if isinstance(p, dict)]
    player_slot = _find_player_slot(account_id, players)
    if player_slot is None:
        return []

    all_deaths = _collect_deaths(players)
    all_deaths.sort(key=lambda d: d["game_time_s"])

    player_kills = [d for d in all_deaths if d["killer_player_slot"] == player_slot]
    player_own_deaths = [d for d in all_deaths if d["killed_player_slot"] == player_slot]

    events: list[HighlightEvent] = []

    # 1) Multi-kills
    player_kills_sorted = sorted(player_kills, key=lambda d: d["game_time_s"])
    multikill_ranges = _find_multikill_ranges(player_kills_sorted)
    for start, end in multikill_ranges:
        kills = player_kills_sorted[start:end]
        kill_count = len(kills)
        first_t = kills[0]["game_time_s"]
        last_t = kills[-1]["game_time_s"]
        max_ttk = max((k.get("time_to_kill_s") or 0) for k in kills)
        pre_roll = min(int(max_ttk) + 5, _MAX_PRE_ROLL_S)
        events.append(HighlightEvent(
            event_type="multikill",
            game_time_s=first_t,
            duration_s=last_t - first_t,
            kill_count=kill_count,
            label=f"{_multikill_name(kill_count)} ({kill_count} Kills)",
            pre_roll_s=pre_roll,
        ))

    # 2) Teamfights (verkettete Todesfälle)
    for fight in _find_teamfights(all_deaths, player_slot):
        first_t = fight[0]["game_time_s"]
        last_t = fight[-1]["game_time_s"]
        player_fight_deaths = [
            d for d in fight
            if d["killer_player_slot"] == player_slot or d["killed_player_slot"] == player_slot
        ]
        max_ttk = max((d.get("time_to_kill_s") or 0) for d in player_fight_deaths) if player_fight_deaths else 0
        pre_roll = min(int(max_ttk) + 5, _MAX_PRE_ROLL_S)
        events.append(HighlightEvent(
            event_type="teamfight",
            game_time_s=first_t,
            duration_s=last_t - first_t,
            kill_count=len(fight),
            label=f"Team Fight ({len(fight)} Kills)",
            pre_roll_s=pre_roll,
        ))

    # 3) Close fights: Kill + eigener Tod innerhalb CLOSE_FIGHT_WINDOW_S
    for close in _find_close_fights(player_kills_sorted, player_own_deaths):
        events.append(close)

    events = _deduplicate_events(events)
    return sorted(events, key=lambda e: e.game_time_s)


def _find_close_fights(
    player_kills: list[dict],
    player_own_deaths: list[dict],
) -> list[HighlightEvent]:
    results: list[HighlightEvent] = []
    used_kill_ids: set[int] = set()
    used_death_ids: set[int] = set()

    for kill in player_kills:
        kill_t = kill["game_time_s"]
        kill_ttk = kill.get("time_to_kill_s") or 0
        for death in player_own_deaths:
            death_t = death["game_time_s"]
            death_ttk = death.get("time_to_kill_s") or 0
            if abs(kill_t - death_t) > _CLOSE_FIGHT_WINDOW_S:
                continue
            if id(kill) in used_kill_ids or id(death) in used_death_ids:
                continue

            first_t = min(kill_t, death_t)
            last_t = max(kill_t, death_t)
            max_ttk = max(kill_ttk, death_ttk)
            pre_roll = min(int(max_ttk) + 5, _MAX_PRE_ROLL_S)
            label = "Clutch Kill" if kill_t > death_t else "Close Fight"

            results.append(HighlightEvent(
                event_type="close_fight",
                game_time_s=first_t,
                duration_s=last_t - first_t,
                kill_count=1,
                label=label,
                pre_roll_s=pre_roll,
            ))
            used_kill_ids.add(id(kill))
            used_death_ids.add(id(death))
            break

    return results


def _deduplicate_events(events: list[HighlightEvent]) -> list[HighlightEvent]:
    result: list[HighlightEvent] = []
    for e in sorted(events, key=lambda x: (x.game_time_s, -x.duration_s)):
        e_start = e.game_time_s - e.pre_roll_s
        e_end = e.game_time_s + e.duration_s
        dominated = False
        for kept in result:
            k_start = kept.game_time_s - kept.pre_roll_s
            k_end = kept.game_time_s + kept.duration_s
            if k_start <= e_start and k_end >= e_end:
                dominated = True
                break
        if not dominated:
            result.append(e)
    return result


def _find_player_slot(account_id: int, players: list[dict]) -> int | None:
    for player in players:
        if _as_int(player.get("account_id")) == account_id:
            return _as_int(player.get("player_slot"))
    return None


def _collect_deaths(players: list[dict]) -> list[dict]:
    deaths: list[dict] = []
    for player in players:
        slot = _as_int(player.get("player_slot"))
        for death in player.get("death_details") or []:
            if not isinstance(death, dict):
                continue
            game_time_s = _as_int(death.get("game_time_s"))
            if game_time_s is None:
                continue
            deaths.append({
                "game_time_s": game_time_s,
                "killer_player_slot": _as_int(death.get("killer_player_slot")),
                "killed_player_slot": slot,
                "time_to_kill_s": death.get("time_to_kill_s"),
            })
    return deaths


def _find_multikill_ranges(player_kills: list[dict]) -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    start = 0
    while start < len(player_kills):
        end = start + 1
        while end < len(player_kills):
            if player_kills[end]["game_time_s"] - player_kills[start]["game_time_s"] > MULTIKILL_THRESHOLD_SECONDS:
                break
            end += 1
        if end - start >= MULTIKILL_MIN_KILLS:
            ranges.append((start, end))
            start = end
            continue
        start += 1
    return ranges


def _find_teamfights(all_deaths: list[dict], player_slot: int) -> list[list[dict]]:
    """Verkettete Teamfight-Erkennung: Tode die ≤ THRESHOLD_S auseinanderliegen gehören zusammen."""
    if not all_deaths:
        return []

    fights: list[list[dict]] = []
    current: list[dict] = [all_deaths[0]]

    for death in all_deaths[1:]:
        if death["game_time_s"] - current[-1]["game_time_s"] <= TEAMFIGHT_THRESHOLD_SECONDS:
            current.append(death)
        else:
            _maybe_add_fight(current, player_slot, fights)
            current = [death]

    _maybe_add_fight(current, player_slot, fights)
    return fights


def _maybe_add_fight(deaths: list[dict], player_slot: int, out: list) -> None:
    player_kills = sum(1 for d in deaths if d["killer_player_slot"] == player_slot)
    if len(deaths) >= TEAMFIGHT_MIN_KILLS and player_kills >= 1:
        out.append(deaths)


def _multikill_name(kill_count: int) -> str:
    return {2: "Double Kill", 3: "Triple Kill", 4: "Quadra Kill", 5: "Penta Kill"}.get(kill_count, "Multi Kill")


def _as_int(value: object) -> int | None:
    try:
        return int(value)
    except (TypeError, ValueError):
        return None
