import contextlib
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import psycopg

from bot.storage.pg import (
    _CompatConnection,
    _execute_with_savepoint,
    _ensure_compat_functions,
    _ensure_observability_writer_started,
    _run_startup_maintenance,
    analytics_db_fingerprint,
    analytics_db_fingerprint_details,
    insert_observability_event,
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
            raise psycopg.errors.InFailedSqlTransaction("current transaction is aborted")

        if "FROM timescaledb_information.hypertables" in sql_text:
            return _SchemaCursor((True,))
        if "FROM timescaledb_information.dimensions" in sql_text:
            return _SchemaCursor(rows=[])
        if "SELECT COUNT(*) FROM clip_templates_global" in sql_text:
            return _SchemaCursor((0,))
        if "CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_flow" in sql_text:
            self.observability_flow_index_attempts += 1
            if self.observability_flow_index_attempts == 1:
                self.aborted = True
                raise psycopg.errors.FeatureNotSupported("compressed hypertable")
        if "SELECT" in upper:
            return _SchemaCursor(None, [])
        return _SchemaCursor()


class CompatConnectionExecuteScriptTests(unittest.TestCase):
    def test_executescript_splits_statements_without_breaking_quoted_sections(self) -> None:
        raw = _RecordingConnection()
        conn = _CompatConnection(raw)

        conn.executescript(
            """
            -- semicolon in comment;
            CREATE FUNCTION demo() RETURNS text
            LANGUAGE plpgsql
            AS $func$
            BEGIN
              RETURN 'hello;world';
            END;
            $func$;

            CREATE TABLE affiliate_demo (
              id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
              note TEXT DEFAULT 'semi;colon'
            );
            """
        )

        self.assertEqual(len(raw.executed), 2)
        self.assertIn("CREATE FUNCTION demo()", raw.executed[0][0])
        self.assertIn("RETURN 'hello;world'", raw.executed[0][0])
        self.assertIn("CREATE TABLE affiliate_demo", raw.executed[1][0])
        self.assertIn("DEFAULT 'semi;colon'", raw.executed[1][0])


class ObservabilityEventInsertTests(unittest.TestCase):
    def test_insert_observability_event_persists_json_payload(self) -> None:
        from unittest.mock import patch
        queued_records: list[tuple[str, str, str | None, str | None, str, str, str]] = []

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

        with patch.object(storage_pg, "_observability_writer_thread", None), patch.object(
            storage_pg, "_observability_writer_stop", SimpleNamespace(clear=lambda: None)
        ), patch("bot.storage.pg.threading.Thread", side_effect=_thread_factory), patch(
            "bot.storage.pg.atexit.unregister"
        ), patch("bot.storage.pg.atexit.register"):
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
    def test_ensure_schema_recovers_from_best_effort_observability_index_failure(self) -> None:
        from bot.storage import pg as storage_pg

        conn = _SavepointAwareSchemaConnection()

        storage_pg.ensure_schema(conn)

        statements = [sql for sql, _ in conn.executed]
        self.assertGreaterEqual(conn.observability_flow_index_attempts, 2)
        self.assertTrue(any(sql.startswith("ROLLBACK TO SAVEPOINT ddl_guard_") for sql in statements))
        self.assertIn(
            "CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_entity "
            "ON twitch_observability_events(entity_login, created_at DESC)",
            statements,
        )


class PerDatabaseCacheTests(unittest.TestCase):
    def tearDown(self) -> None:
        with contextlib.suppress(Exception):
            delattr(_ensure_compat_functions, "_installed_for")
        with contextlib.suppress(Exception):
            delattr(_run_startup_maintenance, "_done_for")

    def test_ensure_compat_functions_is_cached_per_database(self) -> None:
        conn = _RecordingConnection()

        _ensure_compat_functions(conn, dsn="postgresql://demo@host-a:5432/db_a")
        _ensure_compat_functions(conn, dsn="postgresql://demo@host-a:5432/db_a")
        _ensure_compat_functions(conn, dsn="postgresql://demo@host-b:5432/db_b")

        self.assertEqual(conn.commits, 2)

    def test_startup_maintenance_is_cached_per_database(self) -> None:
        conn = object()

        with patch("bot.storage.pg._align_serial_sequence") as align_mock, patch(
            "bot.storage.pg._coerce_column_to_boolean"
        ), patch("bot.storage.pg._cleanup_duplicate_live_state_rows"), patch(
            "bot.storage.pg._ensure_unique_live_state_login_index"
        ), patch("bot.storage.pg._ensure_twitch_raid_auth_login_index"), patch(
            "bot.storage.pg._ensure_social_media_auth_indexes"
        ):
            _run_startup_maintenance(conn, dsn="postgresql://demo@host-a:5432/db_a")
            _run_startup_maintenance(conn, dsn="postgresql://demo@host-a:5432/db_a")
            _run_startup_maintenance(conn, dsn="postgresql://demo@host-b:5432/db_b")

        self.assertEqual(align_mock.call_count, 8)


if __name__ == "__main__":
    unittest.main()
