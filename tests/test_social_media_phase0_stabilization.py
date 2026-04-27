from __future__ import annotations

import contextlib
import os
import shutil
import socket
import subprocess
import time
import unittest
import uuid
from datetime import UTC, datetime, timedelta
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch
from urllib.parse import parse_qs, urlsplit

import psycopg
from psycopg.rows import dict_row

from bot.social_media.oauth_manager import OAuthStateValidationError
from bot.social_media.oauth_manager import OAuthTokenRefreshError
from bot.social_media.oauth_manager import SocialMediaOAuthManager
from bot.social_media.storage import apply_phase0_stabilization
from bot.social_media.token_refresh_worker import SocialMediaTokenRefreshWorker


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


def _extract_state(auth_url: str) -> str:
    state_values = parse_qs(urlsplit(auth_url).query).get("state", [])
    if not state_values:
        raise AssertionError(f"missing state in auth url: {auth_url}")
    return str(state_values[0])


class _BytesCrypto:
    def decrypt_field(self, value, aad):
        del aad
        if isinstance(value, (bytes, bytearray, memoryview)):
            return bytes(value).decode("utf-8")
        return str(value)

    def encrypt_field(self, value, aad, kid="v1"):
        del aad, kid
        return str(value).encode("utf-8")


class _FakeDiscordUser:
    def __init__(self) -> None:
        self.sent_embeds: list[object] = []

    async def send(self, *, embed=None, **kwargs) -> None:
        del kwargs
        self.sent_embeds.append(embed)


class _FakeDiscordBot:
    def __init__(self, user: _FakeDiscordUser) -> None:
        self._user = user

    def get_user(self, user_id: int):
        del user_id
        return self._user

    async def fetch_user(self, user_id: int):
        del user_id
        return self._user


class SocialMediaPhase0PostgresIntegrationTests(unittest.IsolatedAsyncioTestCase):
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

        cls.container_name = f"deadlock-social-media-pg-{uuid.uuid4().hex[:12]}"
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

        cls.dsn = f"postgresql://postgres:{password}@127.0.0.1:{host_port}/{database}"
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
            super().tearDownClass()

    def setUp(self) -> None:
        self._reset_database()

    def _reset_database(self) -> None:
        assert self.dsn is not None
        with psycopg.connect(self.dsn, autocommit=True) as conn:
            conn.execute("DROP SCHEMA IF EXISTS public CASCADE")
            conn.execute("CREATE SCHEMA public")
            conn.execute("GRANT ALL ON SCHEMA public TO public")

    @contextlib.contextmanager
    def _transaction(self):
        assert self.dsn is not None
        with psycopg.connect(self.dsn, row_factory=dict_row) as conn:
            try:
                yield conn
            except Exception:
                conn.rollback()
                raise
            else:
                conn.commit()

    @contextlib.contextmanager
    def _readonly_connection(self):
        assert self.dsn is not None
        with psycopg.connect(self.dsn, autocommit=True, row_factory=dict_row) as conn:
            yield conn

    def _create_legacy_social_media_schema(self) -> None:
        assert self.dsn is not None
        with psycopg.connect(self.dsn, autocommit=True) as conn:
            conn.execute(
                """
                CREATE TABLE oauth_state_tokens (
                    state_token TEXT PRIMARY KEY,
                    platform TEXT NOT NULL,
                    streamer_login TEXT,
                    redirect_uri TEXT,
                    pkce_verifier TEXT,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    expires_at TEXT NOT NULL
                )
                """
            )
            conn.execute(
                """
                CREATE TABLE social_media_platform_auth (
                    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    platform TEXT NOT NULL,
                    streamer_login TEXT,
                    access_token_enc BYTEA NOT NULL,
                    refresh_token_enc BYTEA,
                    client_id TEXT,
                    client_secret_enc BYTEA,
                    token_expires_at TEXT,
                    scopes TEXT,
                    platform_user_id TEXT,
                    platform_username TEXT,
                    enc_version INTEGER DEFAULT 1,
                    enc_kid TEXT DEFAULT 'v1',
                    authorized_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    last_refreshed_at TEXT,
                    enabled INTEGER DEFAULT 1
                )
                """
            )
            conn.execute(
                """
                CREATE UNIQUE INDEX idx_social_platform_auth_streamer_unique
                    ON social_media_platform_auth(platform, streamer_login)
                 WHERE streamer_login IS NOT NULL
                """
            )
            conn.execute(
                """
                CREATE UNIQUE INDEX idx_social_platform_auth_global_unique
                    ON social_media_platform_auth(platform)
                 WHERE streamer_login IS NULL
                """
            )
            conn.execute(
                """
                CREATE TABLE twitch_clips_social_media (
                    id SERIAL PRIMARY KEY,
                    clip_id TEXT NOT NULL UNIQUE,
                    clip_url TEXT NOT NULL,
                    clip_title TEXT,
                    streamer_login TEXT NOT NULL,
                    created_at TEXT NOT NULL
                )
                """
            )

    def _bootstrap_schema(self) -> None:
        self._create_legacy_social_media_schema()
        with psycopg.connect(self.dsn) as conn:
            apply_phase0_stabilization(conn)
            conn.commit()

    def test_phase0_repairs_sequence_drift_for_social_media_tables(self) -> None:
        self._bootstrap_schema()

        with self._transaction() as conn:
            conn.execute(
                """
                INSERT INTO twitch_clips_social_media (
                    clip_id,
                    clip_url,
                    clip_title,
                    streamer_login,
                    created_at
                )
                VALUES
                    ('clip-1', 'https://clips.example/1', 'Clip One', 'streamer_a', '2026-04-26T10:00:00+00:00'),
                    ('clip-2', 'https://clips.example/2', 'Clip Two', 'streamer_a', '2026-04-26T10:01:00+00:00')
                """
            )
            conn.execute(
                """
                INSERT INTO social_media_platform_auth (
                    platform,
                    streamer_login,
                    access_token_enc,
                    refresh_token_enc,
                    client_id,
                    client_secret_enc,
                    token_expires_at,
                    enabled
                )
                VALUES
                    ('youtube', NULL, %s, %s, 'client-1', %s, '2026-04-27T10:00:00+00:00', 1),
                    ('tiktok', 'streamer_a', %s, %s, 'client-2', %s, '2026-04-27T11:00:00+00:00', 1)
                """,
                (
                    b"access-1",
                    b"refresh-1",
                    b"secret-1",
                    b"access-2",
                    b"refresh-2",
                    b"secret-2",
                ),
            )
            conn.execute(
                "SELECT setval(pg_get_serial_sequence('twitch_clips_social_media', 'id'), 1, false)"
            )
            conn.execute(
                "SELECT setval(pg_get_serial_sequence('social_media_platform_auth', 'id'), 1, false)"
            )
            apply_phase0_stabilization(conn)

            clip_row = conn.execute(
                """
                INSERT INTO twitch_clips_social_media (
                    clip_id,
                    clip_url,
                    clip_title,
                    streamer_login,
                    created_at
                )
                VALUES (
                    'clip-3',
                    'https://clips.example/3',
                    'Clip Three',
                    'streamer_a',
                    '2026-04-26T10:02:00+00:00'
                )
                RETURNING id
                """
            ).fetchone()
            auth_row = conn.execute(
                """
                INSERT INTO social_media_platform_auth (
                    platform,
                    streamer_login,
                    access_token_enc,
                    refresh_token_enc,
                    client_id,
                    client_secret_enc,
                    token_expires_at,
                    enabled
                )
                VALUES (
                    'instagram',
                    'streamer_b',
                    %s,
                    %s,
                    'client-3',
                    %s,
                    '2026-04-27T12:00:00+00:00',
                    1
                )
                RETURNING id
                """,
                (
                    b"access-3",
                    b"refresh-3",
                    b"secret-3",
                ),
            ).fetchone()

        self.assertIsNotNone(clip_row)
        self.assertIsNotNone(auth_row)
        self.assertEqual(int(clip_row["id"]), 3)
        self.assertEqual(int(auth_row["id"]), 3)

    async def test_oauth_state_round_trip_supports_cross_runtime_and_negative_cases(self) -> None:
        self._bootstrap_schema()
        redirect_uri = "https://dashboard.example/social-media/oauth/callback/youtube"

        with (
            patch.dict(
                os.environ,
                {
                    "YOUTUBE_CLIENT_ID": "youtube-client-id",
                    "YOUTUBE_CLIENT_SECRET": "youtube-client-secret",
                },
                clear=False,
            ),
            patch("bot.social_media.oauth_manager.get_crypto", return_value=_BytesCrypto()),
            patch("bot.social_media.oauth_manager.transaction", side_effect=self._transaction),
        ):
            manager_a = SocialMediaOAuthManager()
            auth_url = manager_a.generate_auth_url("youtube", "streamer_a", redirect_uri)
            state = _extract_state(auth_url)

            manager_b = SocialMediaOAuthManager()
            manager_b._youtube_exchange_code = AsyncMock(
                return_value={
                    "access_token": "access-1",
                    "refresh_token": "refresh-1",
                    "expires_at": datetime(2026, 4, 27, 10, 0, tzinfo=UTC),
                    "client_id": "youtube-client-id",
                    "client_secret": "youtube-client-secret",
                    "scopes": "scope-a scope-b",
                }
            )

            with self.assertRaises(OAuthStateValidationError):
                await manager_b.handle_callback(
                    "code-1",
                    state,
                    expected_platform="youtube",
                    expected_redirect_uri="https://dashboard.example/social-media/oauth/callback/instagram",
                )

            result = await manager_b.handle_callback(
                "code-1",
                state,
                expected_platform="youtube",
                expected_redirect_uri=redirect_uri,
            )
            self.assertEqual(
                result,
                {
                    "platform": "youtube",
                    "streamer_login": "streamer_a",
                },
            )

            with self.assertRaises(OAuthStateValidationError):
                await manager_b.handle_callback(
                    "code-1",
                    state,
                    expected_platform="youtube",
                    expected_redirect_uri=redirect_uri,
                )

            expired_state = _extract_state(
                manager_a.generate_auth_url("youtube", "streamer_b", redirect_uri)
            )
            with self._transaction() as conn:
                conn.execute(
                    """
                    UPDATE oauth_state_tokens
                    SET expires_at = %s
                    WHERE state_token = %s
                    """,
                    (
                        datetime.now(UTC) - timedelta(minutes=1),
                        expired_state,
                    ),
                )
            with self.assertRaises(OAuthStateValidationError):
                await manager_b.handle_callback(
                    "code-2",
                    expired_state,
                    expected_platform="youtube",
                    expected_redirect_uri=redirect_uri,
                )

            wrong_platform_state = _extract_state(
                manager_a.generate_auth_url("youtube", "streamer_c", redirect_uri)
            )
            with self.assertRaises(OAuthStateValidationError):
                await manager_b.handle_callback(
                    "code-3",
                    wrong_platform_state,
                    expected_platform="tiktok",
                    expected_redirect_uri=redirect_uri,
                )

        with self._readonly_connection() as conn:
            row = conn.execute(
                """
                SELECT streamer_login, client_id
                FROM social_media_platform_auth
                WHERE platform = 'youtube' AND streamer_login = 'streamer_a'
                """
            ).fetchone()

        self.assertIsNotNone(row)
        self.assertEqual(row["streamer_login"], "streamer_a")
        self.assertEqual(row["client_id"], "youtube-client-id")

    async def test_refresh_invalid_grant_sends_admin_dm_only_once_per_24h(self) -> None:
        self._bootstrap_schema()
        fake_user = _FakeDiscordUser()
        fake_bot = _FakeDiscordBot(fake_user)
        worker = object.__new__(SocialMediaTokenRefreshWorker)
        worker.bot = fake_bot
        worker.enabled = True
        worker.interval_seconds = 300
        worker.refresh_threshold_hours = 1
        worker.crypto = _BytesCrypto()
        worker.oauth_manager = SimpleNamespace(
            refresh_token=AsyncMock(
                side_effect=OAuthTokenRefreshError(
                    platform="youtube",
                    error_kind="invalid_grant",
                    message="youtube token refresh failed: {'error': 'invalid_grant'}",
                    status=400,
                    transient=False,
                    payload={"error": "invalid_grant"},
                )
            )
        )
        row = {
            "platform": "youtube",
            "streamer_login": "streamer_a",
            "refresh_token_enc": b"refresh-token",
            "client_id": "client-id",
            "client_secret_enc": b"client-secret",
            "enc_version": 1,
        }
        base_now = datetime(2026, 4, 26, 12, 0, tzinfo=UTC)

        with patch.dict(
            os.environ,
            {
                "SOCIAL_MEDIA_REAUTH_ADMIN_DISCORD_USER_ID": "424242",
            },
            clear=False,
        ), patch(
            "bot.social_media.token_refresh_worker.transaction",
            side_effect=self._transaction,
        ), patch(
            "bot.social_media.token_refresh_worker.readonly_connection",
            side_effect=self._readonly_connection,
        ):
            with patch("bot.social_media.token_refresh_worker._utcnow", return_value=base_now):
                await worker._refresh_platform_token(row)
            with patch(
                "bot.social_media.token_refresh_worker._utcnow",
                return_value=base_now + timedelta(hours=1),
            ):
                await worker._refresh_platform_token(row)
            with patch(
                "bot.social_media.token_refresh_worker._utcnow",
                return_value=base_now + timedelta(hours=25),
            ):
                await worker._refresh_platform_token(row)

        self.assertEqual(len(fake_user.sent_embeds), 2)
        first_embed = fake_user.sent_embeds[0]
        self.assertEqual(first_embed.title, "Social Media Re-Auth erforderlich")
        self.assertEqual(first_embed.fields[0].name, "Streamer")
        self.assertEqual(first_embed.fields[0].value, "streamer_a")
        self.assertEqual(first_embed.fields[1].value, "youtube")
        self.assertEqual(first_embed.fields[2].value, "invalid_grant")


if __name__ == "__main__":
    unittest.main()
