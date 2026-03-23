"""Native PostgreSQL row helpers for the storage layer."""

from __future__ import annotations

from collections.abc import Sequence

import psycopg


class StorageRow:
    """Row object with tuple-style and name-based access for psycopg results."""

    __slots__ = ("_values", "_map")

    def __init__(self, names: Sequence[str], values: Sequence[object]) -> None:
        self._values = tuple(values)
        self._map = {name: value for name, value in zip(names, values, strict=False)}

    def __getitem__(self, key):
        if isinstance(key, int):
            return self._values[key]
        return self._map[key]

    def get(self, key, default=None):
        return self._map.get(key, default)

    def keys(self):
        return self._map.keys()

    def values(self):
        return self._map.values()

    def items(self):
        return self._map.items()

    def __iter__(self):
        return iter(self._values)

    def __len__(self) -> int:
        return len(self._values)

    def __repr__(self) -> str:  # pragma: no cover - debug helper
        return f"StorageRow({self._map})"


def storage_row_factory(cursor: psycopg.Cursor) -> psycopg.rows.RowMaker[StorageRow]:
    """Build StorageRow instances from psycopg cursor results."""
    names = [col.name for col in cursor.description] if cursor.description else []

    def _maker(values: Sequence[object]) -> StorageRow:
        return StorageRow(names, values)

    return _maker
