import contextlib
import sqlite3
import unittest
from unittest.mock import patch

from bot.social_media.clip_manager import ClipManager
from bot.social_media.credential_manager import SocialMediaCredentialManager
from bot.social_media.oauth_manager import SocialMediaOAuthManager
from bot.social_media.upload_worker import UploadWorker
from bot.storage import pg as storage_pg


class _FetchOneCursor:
    def __init__(self, row):
        self._row = row

    def fetchone(self):
        return self._row


class _FetchAllCursor:
    def __init__(self, rows):
        self._rows = rows

    def fetchall(self):
        return self._rows


class _RecordingConn:
    def __init__(self, row=None) -> None:
        self.row = row
        self.executed: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql: str, params=(), *args, **kwargs):
        del args, kwargs
        self.executed.append((sql, tuple(params or ())))
        return _FetchOneCursor(self.row)


class _FakeCrypto:
    def decrypt_field(self, value, aad):
        del aad
        return value

    def encrypt_field(self, value, aad, kid="v1"):
        del aad, kid
        return f"enc:{value}"


class _SqliteCompatConn:
    def __init__(self, conn):
        self._conn = conn

    def execute(self, sql: str, params=(), *args, **kwargs):
        del args, kwargs
        return self._conn.execute(sql.replace("%s", "?"), params)


class SocialMediaCredentialManagerTests(unittest.TestCase):
    def test_get_credentials_queries_streamer_specific_then_global_fallback(self) -> None:
        conn = _RecordingConn(
            {
                "id": 7,
                "platform": "youtube",
                "streamer_login": None,
                "access_token_enc": "access-token",
                "refresh_token_enc": "refresh-token",
                "client_id": "client-id",
                "client_secret_enc": "client-secret",
                "token_expires_at": "2026-03-18T10:00:00+00:00",
                "scopes": "scope-a scope-b",
                "platform_user_id": "yt-user",
                "platform_username": "channel-name",
                "enc_version": 1,
                "enc_kid": "v1",
            }
        )

        with patch(
            "bot.social_media.credential_manager.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            with patch(
                "bot.social_media.credential_manager.get_crypto",
                return_value=_FakeCrypto(),
            ):
                manager = SocialMediaCredentialManager()
                creds = manager.get_credentials("youtube", "earlysalty")

        self.assertIsNotNone(creds)
        self.assertIsNone(creds["streamer_login"])
        self.assertEqual(creds["access_token"], "access-token")
        sql, params = conn.executed[0]
        self.assertIn("streamer_login = %s", sql)
        self.assertIn("streamer_login IS NULL", sql)
        self.assertIn("CASE WHEN streamer_login = %s THEN 1 ELSE 0 END DESC", sql)
        self.assertEqual(
            params,
            ("youtube", "earlysalty", "earlysalty", "earlysalty", "earlysalty"),
        )

    def test_get_all_platforms_status_marks_global_fallbacks(self) -> None:
        with patch("bot.social_media.credential_manager.get_crypto", return_value=_FakeCrypto()):
            with patch.object(
                SocialMediaCredentialManager,
                "get_credentials",
                side_effect=[
                    {
                        "streamer_login": None,
                        "platform_username": "shared-user",
                        "platform_user_id": "shared-id",
                        "expires_at": "2099-03-18T10:00:00+00:00",
                        "scopes": "scope-a",
                    },
                    None,
                    None,
                ],
            ):
                manager = SocialMediaCredentialManager()
                platforms = manager.get_all_platforms_status("earlysalty")

        self.assertTrue(platforms["tiktok"]["connected"])
        self.assertTrue(platforms["tiktok"]["uses_global_fallback"])
        self.assertFalse(platforms["youtube"]["connected"])
        self.assertFalse(platforms["youtube"]["uses_global_fallback"])


class ClipManagerQueueRecoveryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self.conn.execute(
            """
            CREATE TABLE twitch_clips_social_media (
                id INTEGER PRIMARY KEY,
                clip_id TEXT,
                clip_url TEXT,
                clip_title TEXT,
                streamer_login TEXT,
                local_file_path TEXT,
                converted_file_path TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_clips_upload_queue (
                id INTEGER PRIMARY KEY,
                clip_id INTEGER NOT NULL,
                platform TEXT NOT NULL,
                status TEXT DEFAULT 'pending',
                priority INTEGER DEFAULT 0,
                title TEXT,
                description TEXT,
                hashtags TEXT,
                scheduled_at TEXT,
                attempts INTEGER DEFAULT 0,
                last_error TEXT,
                last_attempt_at TEXT,
                created_at TEXT NOT NULL,
                completed_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            INSERT INTO twitch_clips_social_media (
                id, clip_id, clip_url, clip_title, streamer_login, local_file_path, converted_file_path
            ) VALUES (1, 'clip-1', 'https://clips.example/1', 'Clip One', 'streamer_a', NULL, NULL)
            """
        )

    def tearDown(self) -> None:
        self.conn.close()

    def test_queue_upload_reuses_stale_processing_rows(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_clips_upload_queue (
                id, clip_id, platform, status, priority, title, description, hashtags,
                scheduled_at, attempts, last_error, last_attempt_at, created_at, completed_at
            ) VALUES (
                11, 1, 'youtube', 'processing', 0, 'old', 'old desc', NULL,
                NULL, 1, 'worker crash', '2026-03-18T08:00:00+00:00', '2026-03-18T07:00:00+00:00', NULL
            )
            """
        )
        manager = ClipManager()

        with patch(
            "bot.social_media.clip_manager.transaction",
            side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
        ):
            queue_id = manager.queue_upload(
                clip_db_id=1,
                platform="youtube",
                title="new title",
                description="new description",
                hashtags=["a", "b"],
                priority=9,
            )

        row = self.conn.execute(
            """
            SELECT id, status, title, description, hashtags, priority, last_error, last_attempt_at
              FROM twitch_clips_upload_queue
             WHERE id = 11
            """
        ).fetchone()
        total_rows = self.conn.execute("SELECT COUNT(*) FROM twitch_clips_upload_queue").fetchone()[0]

        self.assertEqual(queue_id, 11)
        self.assertEqual(total_rows, 1)
        self.assertEqual(row["status"], "pending")
        self.assertEqual(row["title"], "new title")
        self.assertEqual(row["description"], "new description")
        self.assertEqual(row["hashtags"], '["a", "b"]')
        self.assertEqual(row["priority"], 9)
        self.assertIsNone(row["last_error"])
        self.assertIsNone(row["last_attempt_at"])

    def test_get_upload_queue_reclaims_stale_processing_rows_before_loading_pending(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_clips_upload_queue (
                id, clip_id, platform, status, priority, title, description, hashtags,
                scheduled_at, attempts, last_error, last_attempt_at, created_at, completed_at
            ) VALUES (
                12, 1, 'youtube', 'processing', 0, NULL, NULL, NULL,
                NULL, 1, 'worker crash', '2026-03-18T08:00:00+00:00', '2026-03-18T07:00:00+00:00', NULL
            )
            """
        )
        manager = ClipManager()

        with patch(
            "bot.social_media.clip_manager.transaction",
            side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
        ):
            queue = manager.get_upload_queue(
                status="pending",
                reclaim_stale_processing_before="2026-03-18T08:30:00+00:00",
                limit=5,
            )

        row = self.conn.execute(
            "SELECT status, last_error FROM twitch_clips_upload_queue WHERE id = 12"
        ).fetchone()

        self.assertEqual(len(queue), 1)
        self.assertEqual(queue[0]["id"], 12)
        self.assertEqual(row["status"], "pending")
        self.assertIsNone(row["last_error"])


class SocialMediaOAuthManagerTests(unittest.IsolatedAsyncioTestCase):
    async def test_save_encrypted_tokens_uses_global_partial_upsert_and_preserves_existing_fields(
        self,
    ) -> None:
        conn = _RecordingConn()

        with patch(
            "bot.social_media.oauth_manager.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            with patch(
                "bot.social_media.oauth_manager.get_crypto",
                return_value=_FakeCrypto(),
            ):
                manager = SocialMediaOAuthManager()
                await manager.save_encrypted_tokens(
                    "youtube",
                    None,
                    {
                        "access_token": "access-token",
                        "client_id": "client-id",
                        "expires_at": "2026-03-18T10:00:00+00:00",
                    },
                )

        sql, params = conn.executed[0]
        self.assertIn("ON CONFLICT (platform) WHERE streamer_login IS NULL", sql)
        self.assertIn("social_media_platform_auth.refresh_token_enc", sql)
        self.assertIn("social_media_platform_auth.client_secret_enc", sql)
        self.assertIn("social_media_platform_auth.platform_username", sql)
        self.assertEqual(params[0], "youtube")
        self.assertIsNone(params[1])
        self.assertIsNone(params[3])
        self.assertIsNone(params[5])

    async def test_save_encrypted_tokens_uses_streamer_partial_upsert_for_streamer_records(
        self,
    ) -> None:
        conn = _RecordingConn()

        with patch(
            "bot.social_media.oauth_manager.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            with patch(
                "bot.social_media.oauth_manager.get_crypto",
                return_value=_FakeCrypto(),
            ):
                manager = SocialMediaOAuthManager()
                await manager.save_encrypted_tokens(
                    "tiktok",
                    "earlysalty",
                    {
                        "access_token": "access-token",
                        "refresh_token": "refresh-token",
                        "client_id": "client-id",
                        "client_secret": "client-secret",
                        "expires_at": "2026-03-18T10:00:00+00:00",
                    },
                )

        sql, params = conn.executed[0]
        self.assertIn(
            "ON CONFLICT (platform, streamer_login) WHERE streamer_login IS NOT NULL",
            sql,
        )
        self.assertEqual(params[0], "tiktok")
        self.assertEqual(params[1], "earlysalty")
        self.assertEqual(params[3], "enc:refresh-token")
        self.assertEqual(params[5], "enc:client-secret")


class UploadWorkerTests(unittest.IsolatedAsyncioTestCase):
    async def test_process_queue_resolves_uploaders_per_queue_item_streamer(self) -> None:
        worker = object.__new__(UploadWorker)
        worker.max_parallel = 2
        worker.clip_manager = type(
            "ClipManagerStub",
            (),
            {
                "get_upload_queue": staticmethod(
                    lambda **kwargs: [
                        {"id": 1, "platform": "youtube", "streamer_login": "streamer_one"},
                        {"id": 2, "platform": "youtube", "streamer_login": "streamer_two"},
                    ]
                )
            },
        )()

        resolve_calls: list[tuple[str, str | None]] = []
        processed: list[tuple[int, str]] = []

        async def _resolve(platform, streamer_login, uploader_cache):
            del uploader_cache
            resolve_calls.append((platform, streamer_login))
            return f"uploader:{streamer_login}"

        async def _process(item, uploader):
            processed.append((item["id"], uploader))
            return True

        worker._resolve_uploader = _resolve
        worker._process_upload = _process

        await UploadWorker._process_queue(worker)

        self.assertEqual(
            resolve_calls,
            [("youtube", "streamer_one"), ("youtube", "streamer_two")],
        )
        self.assertEqual(
            processed,
            [(1, "uploader:streamer_one"), (2, "uploader:streamer_two")],
        )

    async def test_build_uploader_authenticates_youtube_before_return(self) -> None:
        worker = object.__new__(UploadWorker)
        created = []

        class _FakeYouTubeUploader:
            def __init__(self, client_id: str, client_secret: str) -> None:
                self.client_id = client_id
                self.client_secret = client_secret
                self.authenticate_calls: list[dict] = []
                created.append(self)

            async def authenticate(self, credentials: dict) -> bool:
                self.authenticate_calls.append(credentials)
                return True

        with patch("bot.social_media.uploaders.YouTubeUploader", _FakeYouTubeUploader):
            uploader = await UploadWorker._build_uploader(
                worker,
                "youtube",
                {
                    "client_id": "client-id",
                    "client_secret": "client-secret",
                    "access_token": "access-token",
                    "refresh_token": None,
                },
            )

        self.assertIs(uploader, created[0])
        self.assertEqual(
            created[0].authenticate_calls,
            [{"access_token": "access-token", "refresh_token": None}],
        )


class _PgMaintenanceConn:
    def __init__(
        self,
        *,
        indexdef: str | None = None,
        duplicate_platforms: list[str] | None = None,
    ) -> None:
        self.indexdef = indexdef
        self.duplicate_platforms = duplicate_platforms or []
        self.executed: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql: str, params=(), *args, **kwargs):
        del args, kwargs
        sql_text = str(sql)
        params_tuple = tuple(params or ())
        self.executed.append((sql_text, params_tuple))

        if "FROM information_schema.tables" in sql_text:
            table = params_tuple[0] if params_tuple else None
            if table in {"twitch_raid_auth", "social_media_platform_auth"}:
                return _FetchOneCursor((1,))
            return _FetchOneCursor(None)

        if "SELECT indexdef" in sql_text:
            row = None if self.indexdef is None else (self.indexdef,)
            return _FetchOneCursor(row)

        if "COUNT(*) AS row_count" in sql_text:
            return _FetchAllCursor([(platform, 2) for platform in self.duplicate_platforms])

        return _FetchOneCursor(None)


class PgSocialMediaMaintenanceTests(unittest.TestCase):
    def test_ensure_twitch_raid_auth_login_index_replaces_case_sensitive_index(self) -> None:
        conn = _PgMaintenanceConn(
            indexdef="CREATE UNIQUE INDEX idx_twitch_raid_auth_login ON public.twitch_raid_auth USING btree (twitch_login)"
        )

        storage_pg._ensure_twitch_raid_auth_login_index(conn)

        statements = [sql for sql, _ in conn.executed]
        self.assertTrue(any("DROP INDEX IF EXISTS idx_twitch_raid_auth_login" in sql for sql in statements))
        self.assertTrue(any("LOWER(twitch_login)" in sql for sql in statements))

    def test_ensure_social_media_auth_indexes_deduplicates_global_rows_and_adds_partial_indexes(
        self,
    ) -> None:
        conn = _PgMaintenanceConn(duplicate_platforms=["youtube"])

        storage_pg._ensure_social_media_auth_indexes(conn)

        statements = [sql for sql, _ in conn.executed]
        self.assertTrue(any("DELETE FROM social_media_platform_auth" in sql for sql in statements))
        self.assertTrue(
            any("idx_social_platform_auth_streamer_unique" in sql for sql in statements)
        )
        self.assertTrue(any("idx_social_platform_auth_global_unique" in sql for sql in statements))


if __name__ == "__main__":
    unittest.main()
