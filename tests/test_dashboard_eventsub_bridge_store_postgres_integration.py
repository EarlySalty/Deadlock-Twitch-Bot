from __future__ import annotations

import contextlib
import json
import os
import shutil
import socket
import subprocess
import time
import unittest
import uuid
from unittest.mock import patch

import psycopg

from bot.dashboard_service.eventsub_bridge import DashboardEventSubBridgeStore
from bot.storage import pg as storage_pg


def _clear_storage_bootstrap_state() -> None:
    with contextlib.suppress(Exception):
        storage_pg._reset_connection_pools()
    with contextlib.suppress(Exception):
        delattr(storage_pg._ensure_storage_bootstrap, "_schema_ok_for")
    with contextlib.suppress(Exception):
        delattr(storage_pg._require_runtime_storage_ready, "_ready_for")
    with contextlib.suppress(Exception):
        delattr(storage_pg._run_startup_maintenance, "_done_for")


def _docker_cli_available() -> bool:
    return shutil.which("docker") is not None


def _docker_daemon_available() -> bool:
    if not _docker_cli_available():
        return False
    result = subprocess.run(
        ["docker", "info"],
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    return result.returncode == 0


def _reserve_local_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        sock.listen(1)
        return int(sock.getsockname()[1])


class DashboardEventSubBridgeStorePostgresIntegrationTests(unittest.TestCase):
    container_name: str | None = None
    dsn: str | None = None

    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        if not _docker_cli_available():
            raise unittest.SkipTest("docker CLI is not available")
        if not _docker_daemon_available():
            raise unittest.SkipTest("docker daemon is not available")

        image = "postgres:16-alpine"
        pull = subprocess.run(
            ["docker", "pull", image],
            capture_output=True,
            text=True,
            timeout=240,
            check=False,
        )
        if pull.returncode != 0:
            raise unittest.SkipTest(f"could not pull {image}: {pull.stderr.strip()}")

        cls.container_name = f"deadlock-eventsub-pg-{uuid.uuid4().hex[:12]}"
        host_port = _reserve_local_port()
        password = "deadlock_test_pw"
        database = "deadlock_test_db"
        run = subprocess.run(
            [
                "docker",
                "run",
                "--rm",
                "-d",
                "--name",
                cls.container_name,
                "-e",
                f"POSTGRES_PASSWORD={password}",
                "-e",
                f"POSTGRES_DB={database}",
                "-p",
                f"127.0.0.1:{host_port}:5432",
                image,
            ],
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        if run.returncode != 0:
            raise unittest.SkipTest(f"could not start postgres container: {run.stderr.strip()}")

        cls.dsn = (
            f"postgresql://postgres:{password}@127.0.0.1:{host_port}/{database}"
        )

        deadline = time.time() + 60.0
        last_error = "postgres did not become ready"
        while time.time() < deadline:
            try:
                with psycopg.connect(cls.dsn) as conn:
                    conn.execute("SELECT 1")
                return
            except Exception as exc:  # pragma: no cover - readiness polling
                last_error = str(exc)
                time.sleep(1.0)

        cls.tearDownClass()
        raise unittest.SkipTest(f"postgres container did not become ready: {last_error}")

    @classmethod
    def tearDownClass(cls) -> None:
        try:
            if cls.container_name:
                subprocess.run(
                    ["docker", "rm", "-f", cls.container_name],
                    capture_output=True,
                    text=True,
                    timeout=30,
                    check=False,
                )
        finally:
            cls.container_name = None
            cls.dsn = None
            _clear_storage_bootstrap_state()
            super().tearDownClass()

    def setUp(self) -> None:
        _clear_storage_bootstrap_state()

    def tearDown(self) -> None:
        _clear_storage_bootstrap_state()

    def _runtime_env(self) -> dict[str, str]:
        assert self.dsn is not None
        return {
            storage_pg.ENV_DSN: self.dsn,
            "TWITCH_ALLOW_RUNTIME_SCHEMA_BOOTSTRAP": "1",
        }

    def test_prepare_runtime_storage_and_bridge_store_bootstrap_real_tables(self) -> None:
        with patch.dict(os.environ, self._runtime_env(), clear=False):
            storage_pg.prepare_runtime_storage()
            store = DashboardEventSubBridgeStore()
            store.ensure_initialized()

            with storage_pg.readonly_connection() as conn:
                tables = {
                    str(row[0])
                    for row in conn.execute(
                        """
                        SELECT table_name
                        FROM information_schema.tables
                        WHERE table_schema = current_schema()
                          AND table_name IN (
                              'twitch_eventsub_bridge_outbox',
                              'twitch_eventsub_bridge_dead_letter',
                              'schema_version'
                          )
                        ORDER BY table_name
                        """
                    ).fetchall()
                }
                indexes = {
                    str(row[0])
                    for row in conn.execute(
                        """
                        SELECT indexname
                        FROM pg_indexes
                        WHERE schemaname = current_schema()
                          AND indexname IN (
                              'idx_twitch_eventsub_bridge_outbox_due',
                              'idx_twitch_eventsub_bridge_dead_lettered_at'
                          )
                        ORDER BY indexname
                        """
                    ).fetchall()
                }
                version_row = conn.execute(
                    """
                    SELECT version
                    FROM schema_version
                    WHERE component = %s
                    """,
                    ("storage_pg",),
                ).fetchone()

        self.assertEqual(
            tables,
            {
                "schema_version",
                "twitch_eventsub_bridge_dead_letter",
                "twitch_eventsub_bridge_outbox",
            },
        )
        self.assertEqual(
            indexes,
            {
                "idx_twitch_eventsub_bridge_dead_lettered_at",
                "idx_twitch_eventsub_bridge_outbox_due",
            },
        )
        self.assertIsNotNone(version_row)
        self.assertEqual(int(version_row[0]), 3)

    def test_bridge_store_round_trip_works_against_real_postgres(self) -> None:
        with patch.dict(os.environ, self._runtime_env(), clear=False):
            storage_pg.prepare_runtime_storage()
            store = DashboardEventSubBridgeStore()

            inserted = store.enqueue(
                message_id="pg-msg-1",
                sub_type="stream.offline",
                payload={"event": {"id": 1}},
                now=1000.0,
            )
            duplicate = store.enqueue(
                message_id="pg-msg-1",
                sub_type="stream.offline",
                payload={"event": {"id": 1}},
                now=1001.0,
            )
            second_insert = store.enqueue(
                message_id="pg-msg-2",
                sub_type="channel.raid",
                payload={"event": {"id": 2}},
                now=1000.0,
            )

            leased = store.lease_due(now=1000.0, lease_seconds=30.0, limit=10)
            leased_again = DashboardEventSubBridgeStore().lease_due(
                now=1000.0,
                lease_seconds=30.0,
                limit=10,
            )

            store.mark_retry(
                message_id="pg-msg-1",
                attempt_count=2,
                error_message="retry failed",
                next_attempt_at=1010.0,
            )
            store.mark_deferred(
                message_id="pg-msg-2",
                error_message="eventsub notification dispatch inactive",
                next_attempt_at=1020.0,
            )
            store.mark_dead_letter(
                message_id="pg-msg-1",
                sub_type="stream.offline",
                payload_json=json.dumps({"event": {"id": 1}}, separators=(",", ":"), sort_keys=True),
                queued_at=1000.0,
                attempt_count=5,
                error_message="permanent failure",
                dead_lettered_at=1030.0,
            )
            store.mark_delivered(message_id="pg-msg-2")

            with storage_pg.readonly_connection() as conn:
                outbox_rows = conn.execute(
                    """
                    SELECT message_id, sub_type, attempt_count, next_attempt_at, last_error
                    FROM twitch_eventsub_bridge_outbox
                    ORDER BY message_id
                    """
                ).fetchall()
                dead_letter_rows = conn.execute(
                    """
                    SELECT message_id, sub_type, attempt_count, last_error
                    FROM twitch_eventsub_bridge_dead_letter
                    ORDER BY message_id
                    """
                ).fetchall()

        self.assertTrue(inserted)
        self.assertFalse(duplicate)
        self.assertTrue(second_insert)
        self.assertEqual([str(row["message_id"]) for row in leased], ["pg-msg-1", "pg-msg-2"])
        self.assertEqual(leased_again, [])
        self.assertEqual(outbox_rows, [])
        self.assertEqual(len(dead_letter_rows), 1)
        dead = dead_letter_rows[0]
        self.assertEqual(str(dead["message_id"]), "pg-msg-1")
        self.assertEqual(str(dead["sub_type"]), "stream.offline")
        self.assertEqual(int(dead["attempt_count"]), 5)
        self.assertEqual(str(dead["last_error"]), "permanent failure")
