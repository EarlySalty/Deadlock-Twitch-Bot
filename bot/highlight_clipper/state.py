from __future__ import annotations

import json
from pathlib import Path

from .config import STATE_PATH


def load_state() -> dict:
    path = Path(STATE_PATH)
    if not path.exists():
        return {}
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    if not isinstance(payload, dict):
        return {}
    streamers = payload.get("streamers")
    if not isinstance(streamers, dict):
        return {}
    result: dict = {}
    for login, data in streamers.items():
        if not isinstance(data, dict):
            continue
        processed = data.get("processed_matches")
        if not isinstance(processed, list):
            processed = []
        result[str(login)] = {
            "processed_matches": [int(m) for m in processed if _is_int(m)],
            "last_checked": int(data.get("last_checked") or 0),
        }
    return result


def save_state(state: dict) -> None:
    path = Path(STATE_PATH)
    path.parent.mkdir(parents=True, exist_ok=True)
    normalized: dict = {}
    for login, data in state.items():
        if not isinstance(data, dict):
            continue
        normalized[str(login)] = {
            "processed_matches": [int(m) for m in data.get("processed_matches", []) if _is_int(m)],
            "last_checked": int(data.get("last_checked") or 0),
        }
    path.write_text(
        json.dumps({"streamers": normalized}, ensure_ascii=True, indent=2, sort_keys=True),
        encoding="utf-8",
    )


def is_match_processed(state: dict, login: str, match_id: int) -> bool:
    data = state.get(str(login)) or {}
    return int(match_id) in {int(m) for m in data.get("processed_matches", []) if _is_int(m)}


def mark_match_processed(state: dict, login: str, match_id: int) -> None:
    login = str(login)
    if login not in state:
        state[login] = {"processed_matches": [], "last_checked": 0}
    processed = [int(m) for m in state[login].get("processed_matches", []) if _is_int(m)]
    match_id = int(match_id)
    if match_id not in processed:
        processed.append(match_id)
    state[login]["processed_matches"] = processed
    save_state(state)


def _is_int(value: object) -> bool:
    try:
        int(value)
    except (TypeError, ValueError):
        return False
    return True
