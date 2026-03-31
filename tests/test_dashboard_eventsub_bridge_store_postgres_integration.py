from __future__ import annotations

import contextlib
import socket
import subprocess
import time
import unittest
import uuid
from unittest.mock import patch

import psycopg

from bot.dashboard_service.eventsub_bridge import DashboardEventSubBridgeStore
from bot.storage import pg as storage_pg


def _free_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


class _DockerPostgresCluster:
    def __init__(self) -> None:
        self.container_name = f"deadlock-eventsub-pg-{uuid.uuid4().hex[:12]}"
        self.port = _free_tcp_port()
        self.dsn = f"postgresql://postgres:postgres@127.0.0.1:{self.port}/postgres"
        self.container_id: str | None = None

    @staticmethod
    def _docker_available() -> bool:
        try:
            subprocess.run(
                ["docker", "info"],
                check=True,
                capture_output=True,
                text=True,
                timeout=15,
            )
        except Exception:
            return False
        return True

    @classmethod
    def start_or_skip(cls) -> "_DockerPostgresCluster":
        if not cls._docker_available():
            raise unittest.SkipTest("docker daemon is not available")

        cluster = cls()
        try:
            cluster.start()
        except Exception as exc:
            with contextlib.suppress(Exception):
                cluster.stop()
            raise unittest.SkipTest(f"docker postgres test cluster unavailable: {exc}") from exc
        return cluster

    def start(self) -> None:
        result = subprocess.run(
            [
                "docker",
                "run",
                "-d",
                "--rm",
                "--name",
                self.container_name,
                "-e",
                "POSTGRES_PASSWORD=postgres",
                "-e",
                "POSTGRES_DB=postgres",
                "-p",
                f"127.0.0.1:{self.port}:5432",
                "postgres:16-alpine",
            ],
            check=True,
            capture_output=True,
            text=True,
            timeout=60,
        )
        self.container_id = result.stdout.strip()
        self._wait_until_ready()

    def _wait_until_ready(self) -> None:
        deadline = time.monotonic() + 90.0
        last_error: Exception | None = None
        while time.monotonic() < deadline:
            try:
                with psycopg.connect(self.dsn, connect_timeout=2) as conn:
                    conn.execute("SELECT 1")
                return
            except Exception as exc:
                last_error = exc
                time.sleep(0.5)
        raise RuntimeError(f"postgres container did not become ready: {last_error}")

    def stop(self) -> None:
        if not self.container_id:
            return
        with contextlib.suppress(Exception):
            subprocess.run(
                ["docker", "rm", "-f", self.container_id],
                check=False,
                capture_output=True,
                text=True,
                timeout=30,
            )
        self.container_id = None


class DashboardEventSubBridgeStorePostgresIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._cluster = _DockerPostgresCluster.start_or_skip()

    @classmethod
    def tearDownClass(cls) -> None:
        with contextlib.suppress(Exception):
            cls._cluster.stop()

    def setUp(self) -> None:
        storage_pg._reset_connection_pools()
        with contextlib.suppress(Exception):
            delattr(storage_pg._ensure_storage_bootstrap, "_schema_ok_for")
        with contextlib.suppress(Exception):
            delattr(storage_pg._require_runtime_storage_ready, "_ready_for")

    def tearDown(self) -> None:
        storage_pg._reset_connection_pools()

    def _patch_dsn(self):
        return patch("bot.storage.pg._load_dsn", return_value=self._cluster.dsn)

    def test_prepare_runtime_storage_bootstraps_and_persists_bridge_lifecycle(self) -> None:
        with self._patch_dsn():
            storage_pg.prepare_runtime_storage()
            store = DashboardEventSubBridgeStore()

            self.assertTrue(
                store.enqueue(
                    message_id="pg-msg-1",
                    sub_type="stream.offline",
                    payload={
                        "subscription": {"type": "stream.offline"},
                        "event": {"broadcaster_user_id": "42"},
                    },
                    now=1000.0,
                )
            )
            self.assertFalse(
                store.enqueue(
                    message_id="pg-msg-1",
                    sub_type="stream.offline",
                    payload={"event": {"broadcaster_user_id": "42"}},
                    now=1001.0,
                )
            )

            leased = store.lease_due(now=1000.0, lease_seconds=30.0, limit=10)
            self.assertEqual([row["message_id"] for row in leased], ["pg-msg-1"])
            self.assertEqual(leased[0]["attempt_count"], 0)

            store.mark_retry(
                message_id="pg-msg-1",
                attempt_count=2,
                error_message="temporary bridge failure",
                next_attempt_at=1010.0,
            )
            store.mark_deferred(
                message_id="pg-msg-1",
                error_message="eventsub notification dispatch inactive",
                next_attempt_at=1020.0,
            )
            store.mark_dead_letter(
                message_id="pg-msg-1",
                sub_type="stream.offline",
                payload_json='{"subscription":{"type":"stream.offline"}}',
                queued_at=1000.0,
                attempt_count=3,
                error_message="permanent failure",
                dead_lettered_at=1030.0,
            )
            store.mark_delivered(message_id="pg-msg-1")

        with psycopg.connect(self._cluster.dsn) as conn:
            outbox_count = conn.execute(
                "SELECT COUNT(*) FROM twitch_eventsub_bridge_outbox"
            ).fetchone()[0]
            dead_letter_count = conn.execute(
                "SELECT COUNT(*) FROM twitch_eventsub_bridge_dead_letter"
            ).fetchone()[0]
            self.assertEqual(outbox_count, 0)
            self.assertEqual(dead_letter_count, 1)

            row = conn.execute(
                """
                SELECT message_id, sub_type, payload_json, queued_at, dead_lettered_at, attempt_count, last_error
                FROM twitch_eventsub_bridge_dead_letter
                WHERE message_id = %s
                """,
                ("pg-msg-1",),
            ).fetchone()
            self.assertIsNotNone(row)
            assert row is not None
            self.assertEqual(row[0], "pg-msg-1")
            self.assertEqual(row[1], "stream.offline")
            self.assertEqual(row[4], 1030.0)
            self.assertEqual(row[5], 3)
            self.assertEqual(row[6], "permanent failure")

    def test_lease_due_skips_row_locked_by_separate_connection(self) -> None:
        with self._patch_dsn():
            storage_pg.prepare_runtime_storage()
            store = DashboardEventSubBridgeStore()
            store.enqueue(
                message_id="locked-msg",
                sub_type="stream.offline",
                payload={"event": {"broadcaster_user_id": "42"}},
                now=2000.0,
            )
            store.enqueue(
                message_id="free-msg",
                sub_type="stream.offline",
                payload={"event": {"broadcaster_user_id": "43"}},
                now=2000.0,
            )

            with psycopg.connect(self._cluster.dsn) as lock_conn:
                lock_conn.execute(
                    """
                    SELECT message_id
                      FROM twitch_eventsub_bridge_outbox
                     WHERE message_id = %s
                     FOR UPDATE
                    """,
                    ("locked-msg",),
                )

                first = store.lease_due(now=2000.0, lease_seconds=30.0, limit=2)
                self.assertEqual([row["message_id"] for row in first], ["free-msg"])

            second = store.lease_due(now=2000.0, lease_seconds=30.0, limit=2)
            self.assertEqual([row["message_id"] for row in second], ["locked-msg"])
