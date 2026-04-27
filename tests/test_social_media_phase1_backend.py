from __future__ import annotations

import contextlib
import io
import json
import shutil
import sqlite3
import tempfile
import unittest
from datetime import UTC, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from aiohttp import web
from aiohttp.web_request import FileField

from bot.social_media.clip_manager import ClipManager
from bot.social_media.dashboard import SocialMediaDashboard
from bot.social_media.layout import DEFAULT_STREAMER_LAYOUT
from bot.social_media.layout import LayoutValidationError
from bot.social_media.layout import StreamerLayout
from bot.social_media.layout.storage import get_streamer_layout
from bot.social_media.layout.storage import upsert_streamer_layout
from bot.social_media.retention_worker import SocialMediaRetentionWorker


class _SqliteCompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=(), *args, **kwargs):
        del args, kwargs
        normalized_sql = sql.replace("%s", "?")
        if "CURRENT_TIMESTAMP" in normalized_sql and "RETURNING id, retention_until" in normalized_sql:
            normalized_sql = normalized_sql.replace(
                "RETURNING id, retention_until",
                "RETURNING id, COALESCE(retention_until, datetime(created_at, '+14 days')) AS retention_until",
            )
        return self._conn.execute(normalized_sql, params)

    def __getattr__(self, item):
        return getattr(self._conn, item)


class _DummyTask:
    def cancel(self) -> None:
        return None


class _DummyLoop:
    def create_task(self, coro):
        coro.close()
        return _DummyTask()


class _DummyBot:
    def __init__(self) -> None:
        self.loop = _DummyLoop()

    async def wait_until_ready(self) -> None:
        return None

    def is_closed(self) -> bool:
        return False


class _StreamingBytesIO:
    def __init__(self, total_size: int, fill: bytes = b"0") -> None:
        self.remaining = total_size
        self.fill = fill

    def read(self, size: int = -1) -> bytes:
        if self.remaining <= 0:
            return b""
        if size < 0 or size > self.remaining:
            size = self.remaining
        self.remaining -= size
        return self.fill * size


class SocialMediaPhase1BackendTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self._create_schema()
        self.conn.execute(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES (?, ?)",
            ("streamer_a", "user-1"),
        )
        self.patches = contextlib.ExitStack()
        self.patches.enter_context(
            patch(
                "bot.social_media.layout.storage.transaction",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.patches.enter_context(
            patch(
                "bot.social_media.layout.storage.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.patches.enter_context(
            patch(
                "bot.social_media.clip_manager.transaction",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.patches.enter_context(
            patch(
                "bot.social_media.retention.transaction",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.patches.enter_context(
            patch(
                "bot.social_media.retention.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.patches.enter_context(
            patch(
                "bot.social_media.dashboard.transaction",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.patches.enter_context(
            patch(
                "bot.social_media.dashboard.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.patches.enter_context(
            patch("bot.social_media.dashboard._magic", None)
        )

    def tearDown(self) -> None:
        self.patches.close()
        self.conn.close()
        shutil.rmtree("data/clips/uploads/streamer_a", ignore_errors=True)

    def _create_schema(self) -> None:
        self.conn.executescript(
            """
            CREATE TABLE twitch_streamers (
                twitch_login TEXT PRIMARY KEY,
                twitch_user_id TEXT
            );

            CREATE TABLE social_media_streamer_layout (
                streamer_login TEXT PRIMARY KEY,
                layout_json TEXT NOT NULL,
                cam_enabled INTEGER NOT NULL DEFAULT 1,
                mode TEXT NOT NULL DEFAULT 'pip',
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_by TEXT
            );

            CREATE TABLE social_media_platform_auth (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform TEXT NOT NULL,
                streamer_login TEXT,
                enabled INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE twitch_clips_social_media (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                clip_id TEXT NOT NULL UNIQUE,
                clip_url TEXT NOT NULL,
                clip_title TEXT,
                clip_thumbnail_url TEXT,
                streamer_login TEXT NOT NULL,
                twitch_user_id TEXT,
                created_at TEXT NOT NULL,
                duration_seconds REAL,
                view_count INTEGER DEFAULT 0,
                game_name TEXT,
                status TEXT DEFAULT 'pending',
                downloaded_at TEXT,
                local_file_path TEXT,
                converted_file_path TEXT,
                uploaded_tiktok INTEGER DEFAULT 0,
                uploaded_youtube INTEGER DEFAULT 0,
                uploaded_instagram INTEGER DEFAULT 0,
                tiktok_video_id TEXT,
                youtube_video_id TEXT,
                instagram_media_id TEXT,
                tiktok_uploaded_at TEXT,
                youtube_uploaded_at TEXT,
                instagram_uploaded_at TEXT,
                custom_title TEXT,
                custom_description TEXT,
                hashtags TEXT,
                music_track TEXT,
                last_analytics_sync TEXT,
                layout_override_json TEXT,
                source_kind TEXT NOT NULL DEFAULT 'twitch',
                upload_local_path TEXT,
                retention_until TEXT,
                discarded_at TEXT
            );
            """
        )

    def test_layout_validation_rejects_invalid_fields_and_mode(self) -> None:
        valid = StreamerLayout.from_mapping(DEFAULT_STREAMER_LAYOUT.to_layout_json(), mode="stacked")
        self.assertEqual(valid.mode, "stacked")

        with self.assertRaises(LayoutValidationError):
            StreamerLayout.from_mapping(
                {
                    **DEFAULT_STREAMER_LAYOUT.to_layout_json(),
                    "game_crop": {"x": -1, "y": 0, "w": 100, "h": 100},
                }
            )

        with self.assertRaises(LayoutValidationError):
            StreamerLayout.from_mapping(
                DEFAULT_STREAMER_LAYOUT.to_layout_json(),
                mode="diagonal",
            )

        with self.assertRaises(LayoutValidationError):
            StreamerLayout.from_mapping(
                {
                    **DEFAULT_STREAMER_LAYOUT.to_layout_json(),
                    "cam_crop": {"x": 1800, "y": 50, "w": 300, "h": 300},
                }
            )

    def test_upsert_and_get_streamer_layout_round_trip(self) -> None:
        layout = StreamerLayout.from_mapping(
            {
                **DEFAULT_STREAMER_LAYOUT.to_layout_json(),
                "cam_crop": {"x": 1200, "y": 20, "w": 300, "h": 300},
            },
            cam_enabled=False,
            mode="stacked",
        )

        upsert_streamer_layout("streamer_a", layout, updated_by="discord-42")
        loaded = get_streamer_layout("streamer_a")
        row = self.conn.execute(
            "SELECT updated_by FROM social_media_streamer_layout WHERE streamer_login = ?",
            ("streamer_a",),
        ).fetchone()

        self.assertIsNotNone(loaded)
        self.assertEqual(loaded.mode, "stacked")
        self.assertFalse(loaded.cam_enabled)
        self.assertEqual(loaded.cam_crop.x, 1200)
        self.assertEqual(row["updated_by"], "discord-42")

    async def test_register_clip_auto_applies_streamer_default_layout(self) -> None:
        layout = StreamerLayout.from_mapping(
            DEFAULT_STREAMER_LAYOUT.to_layout_json(),
            cam_enabled=True,
            mode="pip",
        )
        upsert_streamer_layout("streamer_a", layout, updated_by="discord-1")

        manager = ClipManager()
        clip_db_id = await manager.register_clip(
            clip_id="clip-auto-layout",
            clip_url="https://clips.example/clip-auto-layout",
            title="Clip Auto Layout",
            thumbnail_url="https://img.example/thumb.jpg",
            streamer_login="streamer_a",
            twitch_user_id="user-1",
            created_at="2026-04-26T12:00:00+00:00",
            duration=42.0,
        )

        row = self.conn.execute(
            "SELECT layout_override_json FROM twitch_clips_social_media WHERE id = ?",
            (clip_db_id,),
        ).fetchone()
        self.assertIsNotNone(row)
        payload = json.loads(row["layout_override_json"])
        self.assertEqual(payload["mode"], "pip")
        self.assertEqual(payload["game_crop"], layout.to_override_json()["game_crop"])

    async def test_upload_endpoint_success_unknown_streamer_duplicate_large_and_wrong_type(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _request: True,
            auth_level_getter=lambda _request: "admin",
        )

        with patch(
            "bot.social_media.dashboard.VideoProcessor.get_video_info",
            new=AsyncMock(return_value={"width": 1920, "height": 1080, "duration": 12.5}),
        ):
            success_request = SimpleNamespace(
                post=AsyncMock(
                    return_value={
                        "file": FileField(
                            "file",
                            "clip.mp4",
                            io.BytesIO(b"\x00\x00\x00\x18ftypisompayload"),
                            "video/mp4",
                            {},
                        ),
                        "streamer_login": "streamer_a",
                        "title": "Uploaded clip",
                        "clip_id": "manual-clip-1",
                    }
                )
            )
            response = await dashboard.api_upload_clip(success_request)

        payload = json.loads(response.text)
        self.assertEqual(response.status, 201)
        self.assertEqual(payload["clip_id"], "manual-clip-1")
        stored = self.conn.execute(
            "SELECT source_kind, upload_local_path FROM twitch_clips_social_media WHERE clip_id = ?",
            ("manual-clip-1",),
        ).fetchone()
        self.assertEqual(stored["source_kind"], "manual_upload")
        self.assertTrue(Path(stored["upload_local_path"]).exists())

        unknown_streamer_request = SimpleNamespace(
            post=AsyncMock(
                return_value={
                    "file": FileField(
                        "file",
                        "clip.mp4",
                        io.BytesIO(b"\x00\x00\x00\x18ftypisompayload"),
                        "video/mp4",
                        {},
                    ),
                    "streamer_login": "missing_streamer",
                }
            )
        )
        response = await dashboard.api_upload_clip(unknown_streamer_request)
        self.assertEqual(response.status, 404)

        duplicate_request = SimpleNamespace(
            post=AsyncMock(
                return_value={
                    "file": FileField(
                        "file",
                        "clip.mp4",
                        io.BytesIO(b"\x00\x00\x00\x18ftypisompayload"),
                        "video/mp4",
                        {},
                    ),
                    "streamer_login": "streamer_a",
                    "clip_id": "manual-clip-1",
                }
            )
        )
        response = await dashboard.api_upload_clip(duplicate_request)
        self.assertEqual(response.status, 409)

        wrong_type_request = SimpleNamespace(
            post=AsyncMock(
                return_value={
                    "file": FileField(
                        "file",
                        "clip.txt",
                        io.BytesIO(b"this is not an mp4"),
                        "text/plain",
                        {},
                    ),
                    "streamer_login": "streamer_a",
                    "clip_id": "manual-clip-2",
                }
            )
        )
        with self.assertRaises(web.HTTPUnsupportedMediaType):
            await dashboard.api_upload_clip(wrong_type_request)

        with patch("bot.social_media.dashboard._UPLOAD_MAX_BYTES", 64), patch(
            "bot.social_media.dashboard.VideoProcessor.get_video_info",
            new=AsyncMock(return_value={"width": 1920, "height": 1080, "duration": 12.5}),
        ):
            too_large_request = SimpleNamespace(
                post=AsyncMock(
                    return_value={
                        "file": FileField(
                            "file",
                            "clip.mp4",
                            _StreamingBytesIO(65, b"a"),
                            "video/mp4",
                            {},
                        ),
                        "streamer_login": "streamer_a",
                        "clip_id": "manual-clip-3",
                    }
                )
            )
            with self.assertRaises(web.HTTPRequestEntityTooLarge):
                await dashboard.api_upload_clip(too_large_request)

    async def test_retention_worker_deletes_only_expired_published_or_discarded_clips(self) -> None:
        now = datetime(2026, 4, 26, 12, 0, tzinfo=UTC)
        temp_dir = Path(tempfile.mkdtemp(prefix="social-media-retention-"))
        self.addCleanup(lambda: shutil.rmtree(temp_dir, ignore_errors=True))

        keep_file = temp_dir / "keep.mp4"
        published_file = temp_dir / "published.mp4"
        discarded_file = temp_dir / "discarded.mp4"
        keep_file.write_bytes(b"keep")
        published_file.write_bytes(b"published")
        discarded_file.write_bytes(b"discarded")

        self.conn.execute(
            """
            INSERT INTO social_media_platform_auth (platform, streamer_login, enabled)
            VALUES ('youtube', 'streamer_a', 1)
            """
        )
        self.conn.executemany(
            """
            INSERT INTO twitch_clips_social_media (
                id, clip_id, clip_url, clip_title, streamer_login, twitch_user_id, created_at,
                duration_seconds, status, source_kind, upload_local_path, local_file_path,
                retention_until, discarded_at, uploaded_youtube
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            [
                (
                    1,
                    "keep-clip",
                    "https://clips.example/keep",
                    "Keep",
                    "streamer_a",
                    "user-1",
                    (now - timedelta(days=13)).isoformat(),
                    10.0,
                    "pending",
                    "manual_upload",
                    str(keep_file),
                    str(keep_file),
                    (now + timedelta(days=1)).isoformat(),
                    None,
                    0,
                ),
                (
                    2,
                    "published-clip",
                    "https://clips.example/published",
                    "Published",
                    "streamer_a",
                    "user-1",
                    (now - timedelta(days=15)).isoformat(),
                    10.0,
                    "published_all",
                    "manual_upload",
                    str(published_file),
                    str(published_file),
                    (now - timedelta(minutes=1)).isoformat(),
                    None,
                    1,
                ),
                (
                    3,
                    "discarded-clip",
                    "https://clips.example/discarded",
                    "Discarded",
                    "streamer_a",
                    "user-1",
                    (now - timedelta(days=15)).isoformat(),
                    10.0,
                    "discarded",
                    "manual_upload",
                    str(discarded_file),
                    str(discarded_file),
                    (now - timedelta(minutes=1)).isoformat(),
                    now.isoformat(),
                    0,
                ),
            ],
        )
        self.conn.commit()

        worker = SocialMediaRetentionWorker(_DummyBot())
        with patch("bot.social_media.retention_worker._utcnow", return_value=now):
            await worker._cleanup_expired_clips()

        remaining = self.conn.execute(
            "SELECT id FROM twitch_clips_social_media ORDER BY id"
        ).fetchall()
        self.assertEqual([row["id"] for row in remaining], [1])
        self.assertTrue(keep_file.exists())
        self.assertFalse(published_file.exists())
        self.assertFalse(discarded_file.exists())

    async def test_admin_routes_enforce_guard_and_admin_success_paths(self) -> None:
        partner_dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _request: True,
            auth_level_getter=lambda _request: "partner",
        )
        admin_dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _request: True,
            auth_level_getter=lambda _request: "admin",
            auth_session_getter=lambda _request: {"discord_user_id": "discord-99"},
        )
        self.conn.execute(
            """
            INSERT INTO twitch_clips_social_media (
                clip_id, clip_url, clip_title, streamer_login, twitch_user_id, created_at, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "clip-admin-1",
                "https://clips.example/admin",
                "Admin Clip",
                "streamer_a",
                "user-1",
                "2026-04-26T12:00:00+00:00",
                "pending",
            ),
        )
        self.conn.commit()

        forbidden_calls = [
            partner_dashboard.api_admin_streamer_layout_get(
                SimpleNamespace(query={"streamer_login": "streamer_a"})
            ),
            partner_dashboard.api_admin_streamer_layout_put(
                SimpleNamespace(
                    json=AsyncMock(
                        return_value={
                            "streamer_login": "streamer_a",
                            "layout": DEFAULT_STREAMER_LAYOUT.to_layout_json(),
                            "cam_enabled": True,
                            "mode": "pip",
                        }
                    )
                )
            ),
            partner_dashboard.api_admin_clips(SimpleNamespace(query={})),
            partner_dashboard.api_admin_clip_detail(SimpleNamespace(match_info={"clip_db_id": "1"})),
            partner_dashboard.api_admin_clip_layout_put(
                SimpleNamespace(
                    match_info={"clip_db_id": "1"},
                    json=AsyncMock(return_value={"layout": None}),
                )
            ),
            partner_dashboard.api_admin_clip_discard(SimpleNamespace(match_info={"clip_db_id": "1"})),
            partner_dashboard.api_upload_clip(SimpleNamespace(post=AsyncMock(return_value={}))),
        ]
        for coro in forbidden_calls:
            with self.assertRaises(web.HTTPForbidden):
                await coro

        put_response = await admin_dashboard.api_admin_streamer_layout_put(
            SimpleNamespace(
                json=AsyncMock(
                    return_value={
                        "streamer_login": "streamer_a",
                        "layout": DEFAULT_STREAMER_LAYOUT.to_layout_json(),
                        "cam_enabled": False,
                        "mode": "stacked",
                    }
                )
            )
        )
        get_response = await admin_dashboard.api_admin_streamer_layout_get(
            SimpleNamespace(query={"streamer_login": "streamer_a"})
        )
        clips_response = await admin_dashboard.api_admin_clips(SimpleNamespace(query={}))

        self.assertEqual(put_response.status, 200)
        self.assertEqual(get_response.status, 200)
        self.assertEqual(clips_response.status, 200)
        put_payload = json.loads(put_response.text)
        get_payload = json.loads(get_response.text)
        self.assertEqual(put_payload["mode"], "stacked")
        self.assertFalse(put_payload["cam_enabled"])
        self.assertEqual(get_payload["updated_by"], "discord-99")


if __name__ == "__main__":
    unittest.main()
