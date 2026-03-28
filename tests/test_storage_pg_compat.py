import contextlib
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import psycopg

from bot.storage._pool import ConnectionPoolRegistry
from bot.storage._rows import StorageRow
from bot.storage import pg as storage_pg
from bot.storage.pg import (
    _execute_with_savepoint,
    _ensure_observability_writer_started,
    _reset_connection_pools,
    _run_startup_maintenance,
    analytics_db_fingerprint,
    analytics_db_fingerprint_details,
    insert_observability_event,
    prepare_runtime_storage,
    readonly_connection,
    transaction,
)


class _RecordingConnection:
    def __init__(self) -> None:
        self.executed: list[tuple[str, tuple[object, ...]]] = []
        self.commits = 0

    def execute(self, sql: str, params=(), *args, **kwargs):
        self.executed.append((sql, tuple(params or ())))
        return SimpleNamespace(rowcount=0)

    def commit(self) -> None:
        self.commits += 1

    def cursor(self):
        return self

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False


class _SchemaCursor:
    def __init__(self, row=None, rows=None) -> None:
        self._row = row
        self._rows = rows or []
        self.rowcount = 0

    def fetchone(self):
        return self._row

    def fetchall(self):
        return list(self._rows)


class _SavepointAwareSchemaConnection:
    def __init__(self) -> None:
        self.executed: list[tuple[str, tuple[object, ...]]] = []
        self.aborted = False
        self.observability_flow_index_attempts = 0
        self.autocommit = False

    def execute(self, sql: str, params=(), *args, **kwargs):
        sql_text = str(sql).strip()
        params_tuple = tuple(params or ())
        self.executed.append((sql_text, params_tuple))
        upper = sql_text.upper()

        if upper.startswith("SAVEPOINT "):
            return _SchemaCursor()
        if upper.startswith("ROLLBACK TO SAVEPOINT "):
            self.aborted = False
            return _SchemaCursor()
        if upper.startswith("RELEASE SAVEPOINT "):
            return _SchemaCursor()
        if self.aborted:
            raise psycopg.errors.InFailedSqlTransaction(
                "current transaction is aborted"
            )

        if "FROM timescaledb_information.hypertables" in sql_text:
            return _SchemaCursor((True,))
        if "FROM timescaledb_information.dimensions" in sql_text:
            return _SchemaCursor(rows=[])
        if "SELECT COUNT(*) FROM clip_templates_global" in sql_text:
            return _SchemaCursor((0,))
        if (
            "CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_flow"
            in sql_text
        ):
            self.observability_flow_index_attempts += 1
            if self.observability_flow_index_attempts == 1:
                self.aborted = True
                raise psycopg.errors.FeatureNotSupported("compressed hypertable")
        if "SELECT" in upper:
            return _SchemaCursor(None, [])
        return _SchemaCursor()


class _PoolCursor:
    def __init__(self) -> None:
        self.rowcount = 0

    def executemany(self, sql, params_seq, *args, **kwargs):
        self.rowcount = len(list(params_seq))
        return self

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False


class _PoolConnection:
    def __init__(self, name: str) -> None:
        self.name = name
        self.closed = False
        self.autocommit = True
        self.commit_calls = 0
        self.rollback_calls = 0
        self.executed: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql: str, params=(), *args, **kwargs):
        self.executed.append((sql, tuple(params or ())))
        return _SchemaCursor()

    def commit(self) -> None:
        self.commit_calls += 1

    def rollback(self) -> None:
        self.rollback_calls += 1

    def close(self) -> None:
        self.closed = True

    def cursor(self):
        return _PoolCursor()


class _RuntimeSchemaVersionConnection(_PoolConnection):
    def __init__(
        self,
        name: str,
        *,
        schema_version_exists: bool,
        schema_version_columns: tuple[str, ...] = ("component", "version"),
        version_row=None,
        version_lookup_error: Exception | None = None,
    ) -> None:
        super().__init__(name)
        self.schema_version_exists = schema_version_exists
        self.schema_version_columns = schema_version_columns
        self.version_row = version_row
        self.version_lookup_error = version_lookup_error

    def execute(self, sql: str, params=(), *args, **kwargs):
        self.executed.append((sql, tuple(params or ())))
        sql_text = str(sql)
        if "FROM information_schema.tables" in sql_text and "schema_version" in sql_text:
            return _SchemaCursor((1,) if self.schema_version_exists else None)
        if "FROM information_schema.columns" in sql_text and "schema_version" in sql_text:
            return _SchemaCursor(rows=[(column,) for column in self.schema_version_columns])
        if "SELECT version" in sql_text and "FROM schema_version" in sql_text:
            if self.version_lookup_error is not None:
                raise self.version_lookup_error
            return _SchemaCursor(self.version_row)
        return _SchemaCursor()


def _clear_storage_bootstrap_state() -> None:
    with contextlib.suppress(Exception):
        _reset_connection_pools()
    with contextlib.suppress(Exception):
        delattr(storage_pg._ensure_storage_bootstrap, "_schema_ok_for")
    with contextlib.suppress(Exception):
        delattr(storage_pg._require_runtime_storage_ready, "_ready_for")


class StorageRowTests(unittest.TestCase):
    def test_storage_row_supports_index_and_name_access(self) -> None:
        row = StorageRow(("id", "login"), (7, "partner_one"))

        self.assertEqual(row[0], 7)
        self.assertEqual(row["login"], "partner_one")
        self.assertEqual(row.get("missing", "fallback"), "fallback")


class ObservabilityEventInsertTests(unittest.TestCase):
    def test_insert_observability_event_persists_json_payload(self) -> None:
        from unittest.mock import patch

        queued_records: list[
            tuple[str, str, str | None, str | None, str, str, str]
        ] = []

        with patch(
            "bot.storage.pg._enqueue_observability_event",
            side_effect=lambda record: queued_records.append(record),
        ):
            insert_observability_event(
                flow_type="chat_join",
                flow_id="flow-123",
                step="decision",
                decision="missing_scope",
                entity_login="partner_one",
                entity_id="1001",
                details={"missing": ["channel:bot"]},
            )

        self.assertEqual(len(queued_records), 1)
        params = queued_records[0]
        self.assertEqual(params[0], "chat_join")
        self.assertEqual(params[1], "flow-123")
        self.assertEqual(params[2], "partner_one")
        self.assertEqual(params[3], "1001")
        self.assertEqual(params[4], "decision")
        self.assertEqual(params[5], "missing_scope")
        self.assertIn('"missing": ["channel:bot"]', params[6])

    def test_observability_writer_thread_is_daemonized(self) -> None:
        from unittest.mock import patch
        from bot.storage import pg as storage_pg

        class _FakeThread:
            def __init__(self, *, target=None, name=None, daemon=None):
                self.target = target
                self.name = name
                self.daemon = daemon
                self.started = False

            def start(self) -> None:
                self.started = True

            def is_alive(self) -> bool:
                return self.started

        fake_threads: list[_FakeThread] = []

        def _thread_factory(*args, **kwargs):
            thread = _FakeThread(*args, **kwargs)
            fake_threads.append(thread)
            return thread

        with (
            patch.object(storage_pg, "_observability_writer_thread", None),
            patch.object(
                storage_pg,
                "_observability_writer_stop",
                SimpleNamespace(clear=lambda: None),
            ),
            patch("bot.storage.pg.threading.Thread", side_effect=_thread_factory),
            patch("bot.storage.pg.atexit.unregister"),
            patch("bot.storage.pg.atexit.register"),
        ):
            _ensure_observability_writer_started()

        self.assertEqual(len(fake_threads), 1)
        self.assertTrue(fake_threads[0].started)
        self.assertTrue(fake_threads[0].daemon)


class AnalyticsDbFingerprintTests(unittest.TestCase):
    def test_fingerprint_is_stable_and_obfuscated(self) -> None:
        dsn = "postgresql://demo:supersecret@example.internal:5432/analytics"

        fingerprint_first = analytics_db_fingerprint(dsn)
        fingerprint_second = analytics_db_fingerprint(dsn)
        details = analytics_db_fingerprint_details(dsn)

        self.assertEqual(fingerprint_first, fingerprint_second)
        self.assertTrue(fingerprint_first.startswith("pg:"))
        self.assertEqual(details["fingerprint"], fingerprint_first)
        self.assertNotIn("example.internal", fingerprint_first)
        self.assertNotIn("analytics", fingerprint_first)
        self.assertNotIn("example.internal", details["hostHash"])
        self.assertNotIn("analytics", details["databaseHash"])

    def test_fingerprint_ignores_credentials_and_tracks_db_identity_only(self) -> None:
        dsn_a = "postgresql://demo:supersecret@example.internal:5432/analytics"
        dsn_b = "postgresql://other:totallydifferent@example.internal:5432/analytics"
        dsn_c = "postgresql://demo:supersecret@example.internal:5432/analytics_replica"

        fingerprint_a = analytics_db_fingerprint(dsn_a)
        fingerprint_b = analytics_db_fingerprint(dsn_b)
        fingerprint_c = analytics_db_fingerprint(dsn_c)

        self.assertEqual(fingerprint_a, fingerprint_b)
        self.assertNotEqual(fingerprint_a, fingerprint_c)
        self.assertEqual(
            analytics_db_fingerprint_details(dsn_a),
            analytics_db_fingerprint_details(dsn_b),
        )


class ExecuteWithSavepointTests(unittest.TestCase):
    def test_autocommit_connection_skips_savepoint(self) -> None:
        conn = _RecordingConnection()
        conn.autocommit = True

        _execute_with_savepoint(conn, "SELECT 1")

        self.assertEqual(conn.executed, [("SELECT 1", ())])


class EnsureSchemaSavepointTests(unittest.TestCase):
    def test_ensure_schema_recovers_from_best_effort_observability_index_failure(
        self,
    ) -> None:
        from bot.storage import pg as storage_pg

        conn = _SavepointAwareSchemaConnection()

        storage_pg.ensure_schema(conn)

        statements = [sql for sql, _ in conn.executed]
        self.assertGreaterEqual(conn.observability_flow_index_attempts, 2)
        self.assertTrue(
            any(
                sql.startswith("ROLLBACK TO SAVEPOINT ddl_guard_") for sql in statements
            )
        )
        self.assertIn(
            "CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_entity "
            "ON twitch_observability_events(entity_login, created_at DESC)",
            statements,
        )


class PerDatabaseCacheTests(unittest.TestCase):
    def tearDown(self) -> None:
        with contextlib.suppress(Exception):
            delattr(_run_startup_maintenance, "_done_for")

    def test_startup_maintenance_is_cached_per_database(self) -> None:
        conn = object()

        with (
            patch("bot.storage.pg._align_serial_sequence") as align_mock,
            patch("bot.storage.pg._coerce_column_to_boolean"),
            patch("bot.storage.pg._cleanup_duplicate_live_state_rows"),
            patch("bot.storage.pg._ensure_unique_live_state_login_index"),
            patch("bot.storage.pg._ensure_twitch_raid_auth_login_index"),
            patch("bot.storage.pg._ensure_social_media_auth_indexes"),
        ):
            _run_startup_maintenance(conn, dsn="postgresql://demo@host-a:5432/db_a")
            _run_startup_maintenance(conn, dsn="postgresql://demo@host-a:5432/db_a")
            _run_startup_maintenance(conn, dsn="postgresql://demo@host-b:5432/db_b")

        self.assertEqual(align_mock.call_count, 8)


class ConnectionPoolArchitectureTests(unittest.TestCase):
    def tearDown(self) -> None:
        _clear_storage_bootstrap_state()

    def test_readonly_connection_reuses_pooled_native_connection(self) -> None:
        connections: list[_PoolConnection] = []

        def _connect(*args, **kwargs):
            conn = _PoolConnection(f"conn-{len(connections) + 1}")
            connections.append(conn)
            return conn

        with (
            patch("bot.storage.pg.psycopg.connect", side_effect=_connect),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/db",
            ),
            patch("bot.storage.pg._require_runtime_storage_ready"),
        ):
            with readonly_connection() as first:
                first_raw = first
            with readonly_connection() as second:
                second_raw = second

        self.assertEqual(len(connections), 1)
        self.assertIs(first_raw, second_raw)

    def test_postgres_first_transaction_commits_and_rolls_back_explicitly(self) -> None:
        connections: list[_PoolConnection] = []

        def _connect(*args, **kwargs):
            conn = _PoolConnection(f"conn-{len(connections) + 1}")
            connections.append(conn)
            return conn

        with (
            patch("bot.storage.pg.psycopg.connect", side_effect=_connect),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/db",
            ),
            patch("bot.storage.pg._require_runtime_storage_ready"),
        ):
            with transaction() as conn:
                conn.execute("SELECT 1")

            with self.assertRaisesRegex(RuntimeError, "boom"):
                with transaction() as conn:
                    conn.execute("SELECT 2")
                    raise RuntimeError("boom")

        self.assertEqual(len(connections), 1)
        self.assertGreaterEqual(connections[0].commit_calls, 1)
        self.assertGreaterEqual(connections[0].rollback_calls, 1)

    def test_readonly_connection_uses_postgres_first_prepare_without_legacy_wrapper(
        self,
    ) -> None:
        connections: list[_PoolConnection] = []

        def _connect(*args, **kwargs):
            conn = _PoolConnection(f"conn-{len(connections) + 1}")
            connections.append(conn)
            return conn

        with (
            patch("bot.storage.pg.psycopg.connect", side_effect=_connect),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/db",
            ),
            patch("bot.storage.pg._require_runtime_storage_ready"),
        ):
            with readonly_connection() as conn:
                self.assertIsInstance(conn, _PoolConnection)

        self.assertEqual(len(connections), 1)

    def test_connection_pool_registry_reuses_equivalent_dsn_variants(self) -> None:
        connections: list[_PoolConnection] = []

        def _connect(*args, **kwargs):
            conn = _PoolConnection(f"conn-{len(connections) + 1}")
            connections.append(conn)
            return conn

        registry = ConnectionPoolRegistry(
            max_size=1,
            checkout_timeout=1.0,
            connect_fn=_connect,
        )

        pool_a = registry.get_pool(
            "postgresql://demo:secret@example.internal:5432/analytics?sslmode=require&application_name=deadlock"
        )
        pool_b = registry.get_pool(
            "postgresql://demo:secret@example.internal:5432/analytics?application_name=deadlock&sslmode=require"
        )

        self.assertIs(pool_a, pool_b)
        self.assertEqual(len(connections), 0)

    def test_prepare_runtime_storage_runs_bootstrap_before_runtime_requests(self) -> None:
        connections: list[_PoolConnection] = []

        def _connect(*args, **kwargs):
            conn = _PoolConnection(f"conn-{len(connections) + 1}")
            connections.append(conn)
            return conn

        with (
            patch("bot.storage.pg.psycopg.connect", side_effect=_connect),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/db",
            ),
            patch("bot.storage.pg._prepare_postgres_connection") as prepare_mock,
        ):
            prepare_runtime_storage()

        self.assertEqual(len(connections), 1)
        prepare_mock.assert_called_once()

    def test_prepare_runtime_storage_records_runtime_schema_version(self) -> None:
        conn = _RuntimeSchemaVersionConnection(
            "conn-versioned",
            schema_version_exists=False,
        )

        with (
            patch.dict("os.environ", {"TWITCH_ALLOW_RUNTIME_SCHEMA_BOOTSTRAP": "1"}, clear=False),
            patch("bot.storage.pg.psycopg.connect", return_value=conn),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/db",
            ),
            patch("bot.storage.pg.ensure_schema") as ensure_mock,
        ):
            prepare_runtime_storage()

        self.assertGreaterEqual(ensure_mock.call_count, 1)
        ensure_mock.assert_called_with(conn)
        self.assertTrue(
            any("CREATE TABLE IF NOT EXISTS schema_version" in sql for sql, _ in conn.executed)
        )
        self.assertTrue(
            any(
                "INSERT INTO schema_version" in sql and params == ("storage_pg", 1)
                for sql, params in conn.executed
            )
        )
        self.assertTrue(
            any(
                "INSERT INTO schema_version" in sql and params == ("storage_pg", 3)
                for sql, params in conn.executed
            )
        )

    def test_prepare_runtime_storage_upgrades_existing_runtime_schema_v2_to_v3(self) -> None:
        conn = _RuntimeSchemaVersionConnection(
            "conn-version-upgrade",
            schema_version_exists=True,
            version_row=(2,),
        )

        with (
            patch.dict("os.environ", {"TWITCH_ALLOW_RUNTIME_SCHEMA_BOOTSTRAP": "1"}, clear=False),
            patch("bot.storage.pg.psycopg.connect", return_value=conn),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/db",
            ),
            patch("bot.storage.pg.ensure_schema") as ensure_mock,
        ):
            prepare_runtime_storage()

        ensure_mock.assert_called_once_with(conn)
        self.assertTrue(
            any(
                "INSERT INTO schema_version" in sql and params == ("storage_pg", 3)
                for sql, params in conn.executed
            )
        )

    def test_prepare_runtime_storage_respects_external_schema_version_table(self) -> None:
        conn = _RuntimeSchemaVersionConnection(
            "conn-external-schema",
            schema_version_exists=True,
            schema_version_columns=("name", "applied_at"),
        )

        with (
            patch("bot.storage.pg.psycopg.connect", return_value=conn),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/db",
            ),
            patch("bot.storage.pg.ensure_schema") as ensure_mock,
        ):
            prepare_runtime_storage()

        ensure_mock.assert_not_called()
        self.assertFalse(any("INSERT INTO schema_version" in sql for sql, _ in conn.executed))

    def test_prepare_runtime_storage_fails_closed_on_schema_version_query_errors(self) -> None:
        conn = _RuntimeSchemaVersionConnection(
            "conn-schema-error",
            schema_version_exists=True,
            version_lookup_error=RuntimeError("permission denied"),
        )

        with (
            patch("bot.storage.pg.psycopg.connect", return_value=conn),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/db",
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, "permission denied"):
                prepare_runtime_storage()

    def test_prepare_runtime_storage_requires_explicit_runtime_bootstrap_opt_in(self) -> None:
        conn = _RuntimeSchemaVersionConnection(
            "conn-bootstrap-required",
            schema_version_exists=False,
        )

        with (
            patch.dict("os.environ", {"TWITCH_ALLOW_RUNTIME_SCHEMA_BOOTSTRAP": "0"}, clear=False),
            patch("bot.storage.pg.psycopg.connect", return_value=conn),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/db",
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, "TWITCH_ALLOW_RUNTIME_SCHEMA_BOOTSTRAP=1"):
                prepare_runtime_storage()

    def test_prepare_runtime_storage_propagates_schema_bootstrap_failures(self) -> None:
        connections: list[_PoolConnection] = []

        def _connect(*args, **kwargs):
            conn = _PoolConnection(f"conn-{len(connections) + 1}")
            connections.append(conn)
            return conn

        with (
            patch.dict("os.environ", {"TWITCH_ALLOW_RUNTIME_SCHEMA_BOOTSTRAP": "1"}, clear=False),
            patch("bot.storage.pg.psycopg.connect", side_effect=_connect),
            patch(
                "bot.storage.pg._load_dsn",
                return_value="postgresql://demo@host:5432/failing_db",
            ),
            patch("bot.storage.pg.ensure_schema", side_effect=RuntimeError("schema boom")),
        ):
            with self.assertRaisesRegex(RuntimeError, "schema boom"):
                prepare_runtime_storage()

        with patch(
            "bot.storage.pg._load_dsn",
            return_value="postgresql://demo@host:5432/failing_db",
        ):
            with self.assertRaisesRegex(RuntimeError, "prepare_runtime_storage"):
                with readonly_connection():
                    self.fail("runtime storage should not be marked ready after bootstrap failure")

        cache_key = storage_pg._db_cache_key("postgresql://demo@host:5432/failing_db")
        ready_for = set(getattr(storage_pg._require_runtime_storage_ready, "_ready_for", set()))
        self.assertNotIn(cache_key, ready_for)
        self.assertEqual(len(connections), 1)

    def test_readonly_connection_requires_explicit_runtime_bootstrap(self) -> None:
        with patch(
            "bot.storage.pg._load_dsn",
            return_value="postgresql://demo@host:5432/db",
        ):
            with self.assertRaisesRegex(RuntimeError, "prepare_runtime_storage"):
                with readonly_connection():
                    self.fail("readonly_connection should require explicit runtime bootstrap")


if __name__ == "__main__":
    unittest.main()
