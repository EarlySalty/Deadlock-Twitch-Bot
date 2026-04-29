from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

from .config import MULTIKILL_MIN_KILLS
from .config import MULTIKILL_THRESHOLD_SECONDS
from .config import TEAMFIGHT_MIN_KILLS
from .config import TEAMFIGHT_THRESHOLD_SECONDS


@dataclass(slots=True)
class HighlightEvent:
    event_type: Literal["kill", "multikill", "teamfight"]
    game_time_s: int
    duration_s: int
    kill_count: int
    label: str


def detect_events(account_id: int, match_info: dict) -> list[HighlightEvent]:
    players = [player for player in (match_info.get("players") or []) if isinstance(player, dict)]
    player_slot = _find_player_slot(account_id, players)
    if player_slot is None:
        return []

    all_deaths = _collect_deaths(players)
    player_kills = [death for death in all_deaths if death["killer_player_slot"] == player_slot]
    player_kills.sort(key=lambda item: item["game_time_s"])
    all_deaths.sort(key=lambda item: item["game_time_s"])

    events: list[HighlightEvent] = []
    multikill_ranges = _find_multikill_ranges(player_kills)
    used_kill_indexes = {kill_index for start, end in multikill_ranges for kill_index in range(start, end)}

    for start, end in multikill_ranges:
        kills = player_kills[start:end]
        kill_count = len(kills)
        first_kill = kills[0]["game_time_s"]
        last_kill = kills[-1]["game_time_s"]
        events.append(
            HighlightEvent(
                event_type="multikill",
                game_time_s=first_kill,
                duration_s=last_kill - first_kill,
                kill_count=kill_count,
                label=f"{_multikill_name(kill_count)} ({kill_count} Kills)",
            )
        )

    for fight in _find_teamfights(all_deaths, player_slot):
        first_kill = fight[0]["game_time_s"]
        last_kill = fight[-1]["game_time_s"]
        events.append(
            HighlightEvent(
                event_type="teamfight",
                game_time_s=first_kill,
                duration_s=last_kill - first_kill,
                kill_count=len(fight),
                label=f"Team Fight ({len(fight)} Kills)",
            )
        )

    for kill_index, kill in enumerate(player_kills):
        if kill_index in used_kill_indexes:
            continue
        events.append(
            HighlightEvent(
                event_type="kill",
                game_time_s=kill["game_time_s"],
                duration_s=0,
                kill_count=1,
                label="Kill",
            )
        )

    return sorted(events, key=lambda event: (event.game_time_s, event.duration_s, event.event_type))


def _find_player_slot(account_id: int, players: list[dict]) -> int | None:
    for player in players:
        if _as_int(player.get("account_id")) == account_id:
            return _as_int(player.get("player_slot"))
    return None


def _collect_deaths(players: list[dict]) -> list[dict[str, int | None]]:
    deaths: list[dict[str, int | None]] = []
    for player in players:
        for death in player.get("death_details") or []:
            if not isinstance(death, dict):
                continue
            game_time_s = _as_int(death.get("game_time_s"))
            if game_time_s is None:
                continue
            deaths.append(
                {
                    "game_time_s": game_time_s,
                    "killer_player_slot": _as_int(death.get("killer_player_slot")),
                }
            )
    return deaths


def _find_multikill_ranges(player_kills: list[dict[str, int | None]]) -> list[tuple[int, int]]:
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


def _find_teamfights(
    all_deaths: list[dict[str, int | None]],
    player_slot: int,
) -> list[list[dict[str, int | None]]]:
    fights: list[list[dict[str, int | None]]] = []
    start = 0
    while start < len(all_deaths):
        end = start + 1
        while end < len(all_deaths):
            if all_deaths[end]["game_time_s"] - all_deaths[start]["game_time_s"] > TEAMFIGHT_THRESHOLD_SECONDS:
                break
            end += 1
        window = all_deaths[start:end]
        player_kills = sum(1 for death in window if death["killer_player_slot"] == player_slot)
        if len(window) >= TEAMFIGHT_MIN_KILLS and player_kills >= 2:
            fights.append(window)
            start = end
            continue
        start += 1
    return fights


def _multikill_name(kill_count: int) -> str:
    return {
        3: "Triple Kill",
        4: "Quadra Kill",
        5: "Penta Kill",
    }.get(kill_count, "Multi Kill")


def _as_int(value: object) -> int | None:
    try:
        return int(value)
    except (TypeError, ValueError):
        return None
