from __future__ import annotations

import threading


class InMemoryEventSubStateRepository:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._rows: dict[tuple[str, str], tuple[float, float]] = {}
        self._initialized = False

    def ensure_initialized(self) -> None:
        self._initialized = True

    def _prune(self, *, now: float) -> None:
        expired = [
            row_key
            for row_key, row_value in self._rows.items()
            if float(row_value[0]) <= float(now)
        ]
        for row_key in expired:
            self._rows.pop(row_key, None)

    def is_active(self, kind: str, key: str, *, now: float) -> bool:
        with self._lock:
            self._prune(now=now)
            row = self._rows.get((kind, key))
            return bool(row and float(row[0]) > float(now))

    def claim(self, kind: str, key: str, *, ttl_seconds: float, now: float) -> bool:
        expires_at = float(now) + max(1.0, float(ttl_seconds))
        with self._lock:
            self._prune(now=now)
            row_key = (kind, key)
            current = self._rows.get(row_key)
            if current and float(current[0]) > float(now):
                return False
            self._rows[row_key] = (expires_at, float(now))
            return True

    def release(self, kind: str, key: str) -> None:
        with self._lock:
            self._rows.pop((kind, key), None)
