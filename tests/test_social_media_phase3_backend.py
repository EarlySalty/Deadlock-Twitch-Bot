from __future__ import annotations

import contextlib
import json
import sqlite3
import unittest
from datetime import UTC, datetime
from decimal import Decimal
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from aiohttp import web

from bot.social_media.analytics import list_clip_analytics, list_reports
from bot.social_media.analytics.insights_worker import SocialMediaInsightsWorker
from bot.social_media.analytics.report_writer import SocialMediaReportWriter
from bot.social_media.dashboard import SocialMediaDashboard
from bot.social_media.storage import apply_phase3_analytics


class _SqliteCompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=(), *args, **kwargs):
        del args, kwargs
        normalized_sql = sql.replace("%s", "?")
        normalized_sql = normalized_sql.replace("'[]'::JSONB", "'[]'")
        normalized_sql = normalized_sql.replace("JSONB", "TEXT")
        normalized_sql = normalized_sql.replace("TIMESTAMPTZ", "TEXT")
        normalized_sql = normalized_sql.replace("BOOLEAN", "INTEGER")
        normalized_sql = normalized_sql.replace("SERIAL PRIMARY KEY", "INTEGER PRIMARY KEY AUTOINCREMENT")
        normalized_sql = normalized_sql.replace("NUMERIC(5,2)", "REAL")
        normalized_sql = normalized_sql.replace("NUMERIC(10, 6)", "REAL")
        normalized_sql = normalized_sql.replace("BTRIM(", "TRIM(")
        normalized_params = tuple(float(param) if isinstance(param, Decimal) else param for param in params)
        return self._conn.execute(normalized_sql, normalized_params)

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


BASE_SCHEMA_SQL = """
CREATE TABLE twitch_streamers (
    twitch_login TEXT PRIMARY KEY,
    twitch_user_id TEXT
);

CREATE TABLE social_media_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT,
    updated_by TEXT
);

CREATE TABLE twitch_clips_social_media (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    clip_id TEXT NOT NULL UNIQUE,
    clip_url TEXT,
    clip_title TEXT,
    clip_thumbnail_url TEXT,
    streamer_login TEXT NOT NULL,
    twitch_user_id TEXT,
    created_at TEXT,
    duration_seconds REAL,
    view_count INTEGER DEFAULT 0,
    game_name TEXT,
    status TEXT DEFAULT 'pending',
    source_kind TEXT DEFAULT 'twitch',
    upload_local_path TEXT,
    retention_until TEXT,
    discarded_at TEXT,
    layout_override_json TEXT,
    uploaded_tiktok INTEGER DEFAULT 0,
    uploaded_youtube INTEGER DEFAULT 0,
    uploaded_instagram INTEGER DEFAULT 0,
    tiktok_video_id TEXT,
    youtube_video_id TEXT,
    instagram_media_id TEXT
);

CREATE TABLE twitch_clips_social_analytics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    clip_id INTEGER NOT NULL,
    platform TEXT NOT NULL,
    views INTEGER DEFAULT 0,
    likes INTEGER DEFAULT 0,
    comments INTEGER DEFAULT 0,
    shares INTEGER DEFAULT 0,
    synced_at TEXT NOT NULL,
    engagement_rate REAL
);
"""


class _Phase3StorageTestBase(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self.conn.executescript(BASE_SCHEMA_SQL)
        self.conn.execute(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES (?, ?)",
            ("streamer_a", "user-1"),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_clips_social_media (
                clip_id, clip_url, clip_title, streamer_login, twitch_user_id,
                created_at, duration_seconds, status, uploaded_youtube, youtube_video_id,
                uploaded_tiktok, tiktok_video_id, uploaded_instagram, instagram_media_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "clip-1",
                "https://example/clip-1",
                "Pocket carry",
                "streamer_a",
                "user-1",
                "2026-04-20T10:00:00+00:00",
                18.0,
                "published_all",
                1,
                "yt-1",
                1,
                "tt-1",
                0,
                None,
            ),
        )
        self.patches = contextlib.ExitStack()
        for module in ("bot.social_media.analytics", "bot.social_media.dashboard"):
            self.patches.enter_context(
                patch(
                    f"{module}.transaction",
                    side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
                )
            )
            self.patches.enter_context(
                patch(
                    f"{module}.readonly_connection",
                    side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
                )
            )
        self.patches.enter_context(
            patch(
                "bot.social_media.analytics.report_writer.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.patches.enter_context(
            patch(
                "bot.social_media.analytics.insights_worker.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )

    def tearDown(self) -> None:
        self.patches.close()
        self.conn.close()
        super().tearDown()

    def _clip_db_id(self) -> int:
        row = self.conn.execute(
            "SELECT id FROM twitch_clips_social_media WHERE clip_id = ?",
            ("clip-1",),
        ).fetchone()
        return int(row["id"])


class MigrationTests(_Phase3StorageTestBase):
    def test_apply_phase3_adds_expected_columns_and_reports_table(self) -> None:
        apply_phase3_analytics(_SqliteCompatConn(self.conn))

        columns = {
            row["name"]
            for row in self.conn.execute("PRAGMA table_info(twitch_clips_social_analytics)").fetchall()
        }
        self.assertIn("bucket", columns)
        self.assertIn("watch_time_seconds", columns)
        self.assertIn("ctr_percent", columns)
        self.assertIn("provider", columns)
        self.assertIn("next_pull_at", columns)

        report_table = self.conn.execute(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'social_media_reports'"
        ).fetchone()
        self.assertIsNotNone(report_table)


class InsightsWorkerTests(_Phase3StorageTestBase, unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        super().setUp()
        apply_phase3_analytics(_SqliteCompatConn(self.conn))

    async def test_worker_persists_all_buckets_for_mocked_client(self) -> None:
        class _StubClient:
            async def fetch_video_analytics(self, video_id: str, bucket: str) -> dict:
                views = {"24h": 100, "7d": 420, "30d": 900}[bucket]
                return {
                    "provider": "stub-api",
                    "video_id": video_id,
                    "views": views,
                    "likes": views // 10,
                    "comments": views // 20,
                    "shares": views // 25,
                    "watch_time_seconds": views * 2,
                    "ctr_percent": 3.5,
                    "engagement_rate": 12.5,
                }

        worker = SocialMediaInsightsWorker(
            _DummyBot(),
            credential_manager=SimpleNamespace(),
            client_factory={
                "youtube": lambda _streamer: _StubClient(),
                "tiktok": lambda _streamer: _StubClient(),
            },
        )
        await worker._process_due_targets()

        records = list_clip_analytics(self._clip_db_id())
        self.assertEqual(len(records), 6)
        buckets = {(record.platform, record.bucket) for record in records}
        self.assertIn(("youtube", "24h"), buckets)
        self.assertIn(("youtube", "7d"), buckets)
        self.assertIn(("youtube", "30d"), buckets)
        self.assertIn(("tiktok", "24h"), buckets)


class ReportWriterTests(_Phase3StorageTestBase, unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        super().setUp()
        apply_phase3_analytics(_SqliteCompatConn(self.conn))
        clip_db_id = self._clip_db_id()
        for platform, views in (("youtube", 800), ("tiktok", 500)):
            self.conn.execute(
                """
                INSERT INTO twitch_clips_social_analytics (
                    clip_id, platform, bucket, views, likes, comments, shares,
                    watch_time_seconds, ctr_percent, engagement_rate, provider,
                    synced_at, next_pull_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    clip_db_id,
                    platform,
                    "7d",
                    views,
                    80,
                    20,
                    10,
                    1600,
                    4.5,
                    13.25,
                    "stub-api",
                    "2026-04-21T12:00:00+00:00",
                    "2026-04-22T12:00:00+00:00",
                ),
            )
        self.conn.commit()

    async def test_writer_persists_streamer_report_with_mocked_dispatcher(self) -> None:
        dispatcher = SimpleNamespace(
            generate_text=AsyncMock(
                return_value=SimpleNamespace(
                    content="# Wochenreport\n\nAlles laeuft.",
                    provider="ollama",
                    model="qwen2.5:7b-instruct-q4_K_M",
                )
            )
        )
        writer = SocialMediaReportWriter(dispatcher=dispatcher)
        report = await writer.write_streamer_report(
            "streamer_a",
            period_start=datetime(2026, 4, 21, tzinfo=UTC),
            period_end=datetime(2026, 4, 28, tzinfo=UTC),
            force=True,
        )
        self.assertEqual(report.kind, "streamer")
        self.assertEqual(report.streamer_login, "streamer_a")
        self.assertIn("Wochenreport", report.content_md)
        stored = list_reports(kind="streamer", streamer_login="streamer_a", limit=5)
        self.assertEqual(len(stored), 1)
        self.assertEqual(stored[0].model, "ollama:qwen2.5:7b-instruct-q4_K_M")


class AdminApiTests(_Phase3StorageTestBase, unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        super().setUp()
        apply_phase3_analytics(_SqliteCompatConn(self.conn))
        self.conn.execute(
            """
            INSERT INTO twitch_clips_social_analytics (
                clip_id, platform, bucket, views, likes, comments, shares,
                watch_time_seconds, ctr_percent, engagement_rate, provider,
                synced_at, next_pull_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                self._clip_db_id(),
                "youtube",
                "7d",
                700,
                90,
                22,
                13,
                1800,
                5.1,
                17.3,
                "stub-api",
                "2026-04-21T12:00:00+00:00",
                "2026-04-22T12:00:00+00:00",
            ),
        )
        self.conn.execute(
            """
            INSERT INTO social_media_reports (
                kind, streamer_login, period_start, period_end, content_md, model, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "admin",
                None,
                "2026-04-21T00:00:00+00:00",
                "2026-04-28T00:00:00+00:00",
                "# Admin\n\nReport",
                "ollama:qwen",
                "2026-04-28T00:10:00+00:00",
            ),
        )
        self.conn.commit()
        self.partner = SocialMediaDashboard(
            clip_manager=SimpleNamespace(),
            auth_checker=lambda _r: True,
            auth_level_getter=lambda _r: "partner",
        )
        self.admin = SocialMediaDashboard(
            clip_manager=SimpleNamespace(),
            auth_checker=lambda _r: True,
            auth_level_getter=lambda _r: "admin",
            auth_session_getter=lambda _r: {"discord_user_id": "discord-1"},
        )

    async def test_partner_blocked_on_phase3_admin_routes(self) -> None:
        forbidden_calls = [
            self.partner.api_admin_clip_analytics_get(
                SimpleNamespace(match_info={"clip_db_id": str(self._clip_db_id())})
            ),
            self.partner.api_admin_reports_list(SimpleNamespace(query={})),
            self.partner.api_admin_reports_run(
                SimpleNamespace(json=AsyncMock(return_value={"kind": "cross"}))
            ),
        ]
        for coro in forbidden_calls:
            with self.assertRaises(web.HTTPForbidden):
                await coro

    async def test_admin_can_list_clip_analytics_and_reports(self) -> None:
        analytics_response = await self.admin.api_admin_clip_analytics_get(
            SimpleNamespace(match_info={"clip_db_id": str(self._clip_db_id())})
        )
        analytics_payload = json.loads(analytics_response.text)
        self.assertEqual(analytics_payload["clip_db_id"], self._clip_db_id())
        self.assertEqual(len(analytics_payload["items"]), 1)

        reports_response = await self.admin.api_admin_reports_list(
            SimpleNamespace(query={"kind": "admin", "limit": "5"})
        )
        reports_payload = json.loads(reports_response.text)
        self.assertEqual(len(reports_payload["items"]), 1)
        self.assertEqual(reports_payload["items"][0]["kind"], "admin")

    async def test_admin_can_run_streamer_report(self) -> None:
        stub_report = SimpleNamespace(
            id=7,
            kind="streamer",
            streamer_login="streamer_a",
            period_start="2026-04-21T00:00:00+00:00",
            period_end="2026-04-28T00:00:00+00:00",
            content_md="# Streamer\n\nGenerated",
            model="ollama:qwen",
            created_at="2026-04-28T00:10:00+00:00",
        )
        with patch.object(
            SocialMediaReportWriter,
            "write_streamer_report",
            new=AsyncMock(return_value=stub_report),
        ):
            response = await self.admin.api_admin_reports_run(
                SimpleNamespace(
                    json=AsyncMock(return_value={"kind": "streamer", "streamer": "streamer_a"})
                )
            )
        payload = json.loads(response.text)
        self.assertEqual(payload["kind"], "streamer")
        self.assertEqual(payload["streamer_login"], "streamer_a")


if __name__ == "__main__":
    unittest.main()
