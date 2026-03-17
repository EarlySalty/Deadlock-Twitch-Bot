import unittest
from types import SimpleNamespace

from bot.storage.pg import (
    _CompatConnection,
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
        conn = _RecordingConnection()

        from unittest.mock import patch
        import contextlib

        with patch(
            "bot.storage.pg.get_conn",
            side_effect=lambda: contextlib.nullcontext(conn),
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

        self.assertEqual(conn.commits, 1)
        self.assertEqual(len(conn.executed), 1)
        sql, params = conn.executed[0]
        self.assertIn("INSERT INTO twitch_observability_events", sql)
        self.assertEqual(params[0], "chat_join")
        self.assertEqual(params[1], "flow-123")
        self.assertEqual(params[2], "partner_one")
        self.assertEqual(params[3], "1001")
        self.assertEqual(params[4], "decision")
        self.assertEqual(params[5], "missing_scope")
        self.assertIn('"missing": ["channel:bot"]', params[6])


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


if __name__ == "__main__":
    unittest.main()
