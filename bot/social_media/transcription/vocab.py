"""Deadlock-Vokabular-Storage und CRUD.

Tabelle `deadlock_vocab`:
- term       (PRIMARY KEY) - lowercased aliasable lookup key
- canonical  - bevorzugte Schreibweise (z.B. "Pocket")
- category   - hero | item | ability | slang
- source     - deadlock_api | manual
- aliases    - JSONB array zusaetzlicher Schreibweisen
- weight     - Priorisierung beim Fuzzy-Match (hoeher = bevorzugt)
- updated_at - letztes Update
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field
from typing import Any, Iterable, Sequence

from ...storage import readonly_connection, transaction

log = logging.getLogger("TwitchStreams.SocialMedia.Vocab")

ALLOWED_CATEGORIES: frozenset[str] = frozenset({"hero", "item", "ability", "slang"})
ALLOWED_SOURCES: frozenset[str] = frozenset({"deadlock_api", "manual"})


@dataclass(frozen=True)
class VocabEntry:
    term: str
    canonical: str
    category: str
    source: str = "manual"
    aliases: tuple[str, ...] = field(default_factory=tuple)
    weight: int = 1
    updated_at: str | None = None

    def to_dict(self) -> dict[str, Any]:
        return {
            "term": self.term,
            "canonical": self.canonical,
            "category": self.category,
            "source": self.source,
            "aliases": list(self.aliases),
            "weight": self.weight,
            "updated_at": self.updated_at,
        }


def _normalize_term(term: str) -> str:
    value = str(term or "").strip().lower()
    if not value:
        raise ValueError("term is required")
    return value


def _normalize_aliases(aliases: Iterable[str] | None) -> list[str]:
    if not aliases:
        return []
    out: list[str] = []
    seen: set[str] = set()
    for raw in aliases:
        token = str(raw or "").strip()
        if not token:
            continue
        lower = token.lower()
        if lower in seen:
            continue
        seen.add(lower)
        out.append(token)
    return out


def _validate_category(category: str) -> str:
    value = str(category or "").strip().lower()
    if value not in ALLOWED_CATEGORIES:
        raise ValueError(
            f"category must be one of: {sorted(ALLOWED_CATEGORIES)} (got: {category!r})"
        )
    return value


def _validate_source(source: str) -> str:
    value = str(source or "").strip().lower() or "manual"
    if value not in ALLOWED_SOURCES:
        raise ValueError(
            f"source must be one of: {sorted(ALLOWED_SOURCES)} (got: {source!r})"
        )
    return value


def _decode_aliases(raw: Any) -> tuple[str, ...]:
    if raw is None:
        return tuple()
    if isinstance(raw, (list, tuple)):
        return tuple(str(x) for x in raw if x is not None)
    if isinstance(raw, (bytes, bytearray)):
        try:
            decoded = json.loads(raw.decode("utf-8"))
        except Exception:
            return tuple()
        return tuple(str(x) for x in decoded) if isinstance(decoded, list) else tuple()
    if isinstance(raw, str):
        if not raw:
            return tuple()
        try:
            decoded = json.loads(raw)
        except Exception:
            return tuple()
        return tuple(str(x) for x in decoded) if isinstance(decoded, list) else tuple()
    return tuple()


def _row_to_entry(row: Any) -> VocabEntry:
    if hasattr(row, "keys"):
        term = row["term"]
        canonical = row["canonical"]
        category = row["category"]
        source = row["source"]
        aliases_raw = row["aliases"]
        weight = row["weight"]
        updated_at = row["updated_at"]
    else:
        term, canonical, category, source, aliases_raw, weight, updated_at = (
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            row[5],
            row[6],
        )
    return VocabEntry(
        term=str(term),
        canonical=str(canonical),
        category=str(category),
        source=str(source),
        aliases=_decode_aliases(aliases_raw),
        weight=int(weight or 1),
        updated_at=str(updated_at) if updated_at is not None else None,
    )


def list_vocab(
    *,
    category: str | None = None,
    query: str | None = None,
    limit: int = 200,
    offset: int = 0,
) -> tuple[list[VocabEntry], int]:
    """List vocab entries with optional filtering and pagination."""
    sql_where = ["1=1"]
    params: list[Any] = []
    if category:
        sql_where.append("LOWER(category) = LOWER(%s)")
        params.append(category)
    if query:
        sql_where.append("(LOWER(term) LIKE %s OR LOWER(canonical) LIKE %s)")
        like = f"%{query.lower()}%"
        params.extend([like, like])
    where_sql = " AND ".join(sql_where)

    with readonly_connection() as conn:
        total_row = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
            f"SELECT COUNT(*) AS total FROM deadlock_vocab WHERE {where_sql}",
            tuple(params),
        ).fetchone()
        rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
            f"""
            SELECT term, canonical, category, source, aliases, weight, updated_at
              FROM deadlock_vocab
             WHERE {where_sql}
             ORDER BY weight DESC, canonical ASC
             LIMIT %s OFFSET %s
            """,
            tuple([*params, max(1, min(int(limit), 500)), max(0, int(offset))]),
        ).fetchall()

    if total_row is None:
        total = 0
    elif hasattr(total_row, "keys"):
        total = int(total_row["total"] or 0)
    else:
        total = int(total_row[0] or 0)

    return [_row_to_entry(r) for r in rows], total


def get_vocab_entry(term: str) -> VocabEntry | None:
    normalized = _normalize_term(term)
    with readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT term, canonical, category, source, aliases, weight, updated_at
              FROM deadlock_vocab
             WHERE term = %s
             LIMIT 1
            """,
            (normalized,),
        ).fetchone()
    return _row_to_entry(row) if row else None


def upsert_vocab_entry(
    *,
    term: str,
    canonical: str,
    category: str,
    source: str = "manual",
    aliases: Sequence[str] | None = None,
    weight: int = 1,
) -> VocabEntry:
    normalized_term = _normalize_term(term)
    canonical_value = str(canonical or "").strip()
    if not canonical_value:
        raise ValueError("canonical is required")
    cat = _validate_category(category)
    src = _validate_source(source)
    alias_list = _normalize_aliases(aliases)
    weight_value = max(1, int(weight or 1))

    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO deadlock_vocab (term, canonical, category, source, aliases, weight, updated_at)
            VALUES (%s, %s, %s, %s, %s, %s, CURRENT_TIMESTAMP)
            ON CONFLICT (term) DO UPDATE
                SET canonical = EXCLUDED.canonical,
                    category  = EXCLUDED.category,
                    source    = EXCLUDED.source,
                    aliases   = EXCLUDED.aliases,
                    weight    = EXCLUDED.weight,
                    updated_at = CURRENT_TIMESTAMP
            """,
            (
                normalized_term,
                canonical_value,
                cat,
                src,
                json.dumps(alias_list),
                weight_value,
            ),
        )

    return get_vocab_entry(normalized_term) or VocabEntry(
        term=normalized_term,
        canonical=canonical_value,
        category=cat,
        source=src,
        aliases=tuple(alias_list),
        weight=weight_value,
    )


def bulk_upsert_vocab_entries(entries: Iterable[VocabEntry]) -> tuple[int, int]:
    """Bulk-upsert vocab entries. Returns (inserted_or_updated, skipped)."""
    written = 0
    skipped = 0
    for entry in entries:
        try:
            upsert_vocab_entry(
                term=entry.term,
                canonical=entry.canonical,
                category=entry.category,
                source=entry.source,
                aliases=entry.aliases,
                weight=entry.weight,
            )
            written += 1
        except Exception:
            log.exception("Vocab-Upsert fehlgeschlagen für %s", entry.term)
            skipped += 1
    return written, skipped


def delete_vocab_entry(term: str) -> bool:
    normalized = _normalize_term(term)
    with transaction() as conn:
        cursor = conn.execute(
            "DELETE FROM deadlock_vocab WHERE term = %s",
            (normalized,),
        )
        rowcount = getattr(cursor, "rowcount", None)
    if rowcount is None:
        return get_vocab_entry(normalized) is None
    return int(rowcount or 0) > 0


def load_all_vocab() -> list[VocabEntry]:
    with readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT term, canonical, category, source, aliases, weight, updated_at
              FROM deadlock_vocab
             ORDER BY weight DESC, canonical ASC
            """,
        ).fetchall()
    return [_row_to_entry(r) for r in rows]


def load_all_vocab_safe() -> list[VocabEntry]:
    """Like `load_all_vocab` but returns [] on any DB failure."""
    try:
        return load_all_vocab()
    except Exception:
        log.exception("load_all_vocab_safe failed; returning empty list")
        return []
