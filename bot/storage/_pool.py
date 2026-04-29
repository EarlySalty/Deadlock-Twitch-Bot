"""Process-local psycopg connection pooling for the analytics database."""

from __future__ import annotations

import contextlib
import queue
import threading
import time
from collections.abc import Callable, Iterator
from urllib.parse import parse_qsl, urlsplit

import psycopg
from psycopg.conninfo import conninfo_to_dict


ConnectFn = Callable[[str, bool], psycopg.Connection]
PrepareFn = Callable[[psycopg.Connection, str], None]


def _normalized_conninfo_value(value: object) -> str:
    return str(value or "").strip()


def _dsn_registry_key(dsn: str) -> tuple[tuple[str, str], ...]:
    """Return a canonical, non-raw registry key for a DSN string."""
    raw_dsn = str(dsn or "").strip()
    if not raw_dsn:
        return (("dsn_hash", ""),)

    try:
        parsed = conninfo_to_dict(raw_dsn)
    except Exception:
        parsed = {}

    items: list[tuple[str, str]] = []
    for key, value in parsed.items():
        normalized = _normalized_conninfo_value(value)
        if not normalized:
            continue
        items.append((str(key).strip().lower(), normalized))
    if items:
        return tuple(sorted(items))

    try:
        parsed_url = urlsplit(raw_dsn if "://" in raw_dsn else f"postgresql://{raw_dsn}")
    except Exception:
        parsed_url = None

    if parsed_url is not None:
        url_items: list[tuple[str, str]] = []
        host = (parsed_url.hostname or "").strip().lower()
        if host:
            url_items.append(("host", host))
        if parsed_url.port is not None:
            url_items.append(("port", str(parsed_url.port).strip()))
        user = (parsed_url.username or "").strip()
        if user:
            url_items.append(("user", user))
        dbname = (parsed_url.path or "").strip().lstrip("/")
        if dbname:
            url_items.append(("dbname", dbname))
        query_items = []
        for key, value in sorted(parse_qsl(parsed_url.query, keep_blank_values=True)):
            normalized = _normalized_conninfo_value(value)
            if normalized:
                query_items.append((f"query:{key.strip().lower()}", normalized))
        if url_items or query_items:
            return tuple(sorted(url_items + query_items))

    sanitized = raw_dsn.replace("\r", "").replace("\n", "")
    sanitized = sanitized.replace("\\", "")
    if "@" in sanitized:
        sanitized = sanitized.split("@", 1)[-1]
    return (("dsn_opaque", sanitized[:256]),)


class PostgresConnectionPool:
    """Small synchronous connection pool for psycopg connections.

    The wider application is currently synchronous at the storage boundary, so a
    lightweight in-process pool gives most of the operational benefit without
    forcing a wider async rewrite.
    """

    def __init__(
        self,
        *,
        dsn: str,
        max_size: int,
        checkout_timeout: float,
        connect_fn: ConnectFn,
        prepare_fn: PrepareFn | None = None,
    ) -> None:
        self._dsn = dsn
        self._max_size = max(1, int(max_size))
        self._checkout_timeout = max(0.1, float(checkout_timeout))
        self._connect_fn = connect_fn
        self._prepare_fn = prepare_fn
        self._idle: queue.LifoQueue[psycopg.Connection] = queue.LifoQueue(
            maxsize=self._max_size
        )
        self._created = 0
        self._closed = False
        self._lock = threading.Lock()

    @property
    def dsn(self) -> str:
        return self._dsn

    def _create_connection(self, *, autocommit: bool) -> psycopg.Connection:
        conn = self._connect_fn(self._dsn, autocommit)
        if self._prepare_fn is not None:
            self._prepare_fn(conn, self._dsn)
        return conn

    @staticmethod
    def _ping_connection(conn: psycopg.Connection, *, autocommit: bool) -> None:
        if autocommit:
            conn.execute("SELECT 1").fetchone()
            return
        original_autocommit = bool(getattr(conn, "autocommit", False))
        if not original_autocommit:
            conn.autocommit = True
        try:
            conn.execute("SELECT 1").fetchone()
        finally:
            if not original_autocommit:
                conn.autocommit = False

    def _acquire_connection(self, *, autocommit: bool) -> psycopg.Connection:
        try:
            conn = self._idle.get_nowait()
        except queue.Empty:
            conn = None

        if conn is None:
            should_create = False
            with self._lock:
                if self._closed:
                    raise RuntimeError("connection pool is closed")
                if self._created < self._max_size:
                    self._created += 1
                    should_create = True
            if should_create:
                try:
                    return self._create_connection(autocommit=autocommit)
                except Exception:
                    with self._lock:
                        self._created = max(0, self._created - 1)
                    raise
            try:
                conn = self._idle.get(timeout=self._checkout_timeout)
            except queue.Empty as exc:
                raise TimeoutError(
                    "timed out waiting for a PostgreSQL connection"
                ) from exc

        if getattr(conn, "closed", False):
            self._discard(conn)
            return self._acquire_connection(autocommit=autocommit)

        try:
            if bool(getattr(conn, "autocommit", False)) != bool(autocommit):
                conn.autocommit = autocommit
        except Exception:
            self._discard(conn)
            return self._acquire_connection(autocommit=autocommit)

        try:
            self._ping_connection(conn, autocommit=autocommit)
        except Exception:
            self._discard(conn)
            return self._acquire_connection(autocommit=autocommit)

        return conn

    def _release_connection(self, conn: psycopg.Connection) -> None:
        if getattr(conn, "closed", False):
            self._discard(conn)
            return

        try:
            if not bool(getattr(conn, "autocommit", False)):
                with contextlib.suppress(Exception):
                    conn.rollback()
        except Exception:
            self._discard(conn)
            return

        with self._lock:
            if self._closed:
                try:
                    conn.close()
                finally:
                    self._created = max(0, self._created - 1)
                return

        try:
            self._idle.put_nowait(conn)
        except queue.Full:
            self._discard(conn)

    def _discard(self, conn: psycopg.Connection | None) -> None:
        if conn is not None:
            with contextlib.suppress(Exception):
                conn.close()
        with self._lock:
            self._created = max(0, self._created - 1)

    @contextlib.contextmanager
    def connection(self, *, autocommit: bool) -> Iterator[psycopg.Connection]:
        conn = self._acquire_connection(autocommit=autocommit)
        try:
            yield conn
        finally:
            self._release_connection(conn)

    def open_dedicated(self, *, autocommit: bool) -> psycopg.Connection:
        with self._lock:
            if self._closed:
                raise RuntimeError("connection pool is closed")
        return self._create_connection(autocommit=autocommit)

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
        while True:
            try:
                conn = self._idle.get_nowait()
            except queue.Empty:
                break
            with contextlib.suppress(Exception):
                conn.close()
        with self._lock:
            self._created = 0


class ConnectionPoolRegistry:
    """Maintain one pool per canonical DSN identity."""

    def __init__(
        self,
        *,
        max_size: int,
        checkout_timeout: float,
        connect_fn: ConnectFn,
        prepare_fn: PrepareFn | None = None,
    ) -> None:
        self._max_size = max_size
        self._checkout_timeout = checkout_timeout
        self._connect_fn = connect_fn
        self._prepare_fn = prepare_fn
        self._lock = threading.Lock()
        self._pools: dict[tuple[tuple[str, str], ...], PostgresConnectionPool] = {}

    def get_pool(self, dsn: str) -> PostgresConnectionPool:
        key = _dsn_registry_key(dsn)
        with self._lock:
            pool = self._pools.get(key)
            if pool is None:
                pool = PostgresConnectionPool(
                    dsn=dsn,
                    max_size=self._max_size,
                    checkout_timeout=self._checkout_timeout,
                    connect_fn=self._connect_fn,
                    prepare_fn=self._prepare_fn,
                )
                self._pools[key] = pool
            return pool

    def close_all(self) -> None:
        with self._lock:
            pools = list(self._pools.values())
            self._pools.clear()
        for pool in pools:
            pool.close()


class ConnectionStats:
    """Small helper for introspecting checkout timing in tests/debugging."""

    __slots__ = ("started_at",)

    def __init__(self) -> None:
        self.started_at = time.monotonic()
