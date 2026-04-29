from __future__ import annotations

import json
from pathlib import Path

from .config import STATE_PATH

_DEFAULT_STATE = {
    "processed_matches": [],
    "last_checked": 0,
}


def load_state() -> dict:
    path = Path(STATE_PATH)
    if not path.exists():
        return dict(_DEFAULT_STATE)
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return dict(_DEFAULT_STATE)
    processed_matches = payload.get("processed_matches")
    if not isinstance(processed_matches, list):
        processed_matches = []
    return {
        "processed_matches": [int(match_id) for match_id in processed_matches if _is_int(match_id)],
        "last_checked": int(payload.get("last_checked") or 0),
    }


def save_state(state: dict) -> None:
    path = Path(STATE_PATH)
    path.parent.mkdir(parents=True, exist_ok=True)
    normalized = {
        "processed_matches": [int(match_id) for match_id in state.get("processed_matches", []) if _is_int(match_id)],
        "last_checked": int(state.get("last_checked") or 0),
    }
    path.write_text(
        json.dumps(normalized, ensure_ascii=True, indent=2, sort_keys=True),
        encoding="utf-8",
    )


def is_match_processed(state: dict, match_id: int) -> bool:
    return int(match_id) in {int(item) for item in state.get("processed_matches", []) if _is_int(item)}


def mark_match_processed(state: dict, match_id: int) -> None:
    processed = [int(item) for item in state.get("processed_matches", []) if _is_int(item)]
    match_id = int(match_id)
    if match_id not in processed:
        processed.append(match_id)
    state["processed_matches"] = processed
    save_state(state)


def _is_int(value: object) -> bool:
    try:
        int(value)
    except (TypeError, ValueError):
        return False
    return True
