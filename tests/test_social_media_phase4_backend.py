from __future__ import annotations

import contextlib
import json
import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from aiohttp import web

from bot.social_media.approval import (
    ApprovalService,
    get_approval_record,
    is_clip_approved_for,
)
from bot.social_media.clip_manager import ClipManager
from bot.social_media.dashboard import SocialMediaDashboard
from bot.social_media.layout import DEFAULT_STREAMER_LAYOUT
from bot.social_media.settings import get_auto_approve_settings, set_setting
from bot.social_media.storage import apply_phase4_approval


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
        return self._conn.execute(normalized_sql, params)

    def __getattr__(self, item):
        return getattr(self._conn, item)


SCHEMA_SQL = """
CREATE TABLE twitch_streamers (
    twitch_login TEXT PRIMARY KEY,
    twitch_user_id TEXT
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
    local_file_path TEXT,
    retention_until TEXT,
    discarded_at TEXT,
    layout_override_json TEXT,
    uploaded_tiktok INTEGER DEFAULT 0,
    uploaded_youtube INTEGER DEFAULT 0,
    uploaded_instagram INTEGER DEFAULT 0
);

CREATE TABLE social_media_clip_enrichment (
    clip_db_id INTEGER PRIMARY KEY,
    transcript_raw TEXT,
    transcript_corrected TEXT,
    transcript_segments TEXT,
    transcript_lang TEXT,
    detected_terms TEXT NOT NULL DEFAULT '[]',
    title_youtube TEXT,
    title_tiktok TEXT,
    title_instagram TEXT,
    description_youtube TEXT,
    description_tiktok TEXT,
    description_instagram TEXT,
    hashtags_youtube TEXT NOT NULL DEFAULT '[]',
    hashtags_tiktok TEXT NOT NULL DEFAULT '[]',
    hashtags_instagram TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'pending',
    llm_provider TEXT,
    llm_model TEXT,
    cost_usd_estimate REAL,
    error_message TEXT,
    started_at TEXT,
    completed_at TEXT,
    edited_by TEXT,
    updated_at TEXT
);

CREATE TABLE social_media_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT,
    updated_by TEXT
);

CREATE TABLE twitch_clips_upload_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
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
);
"""


class _StorageStubBase(unittest.TestCase):
    def setUp(self) -> None:  # noqa: D401
        super().setUp()
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self.conn.executescript(SCHEMA_SQL)
        def compat():
            return contextlib.nullcontext(_SqliteCompatConn(self.conn))

        self.patches = contextlib.ExitStack()
        for module in (
            "bot.social_media.approval.approval_service",
            "bot.social_media.dashboard",
            "bot.social_media.settings",
            "bot.social_media.enrichment",
        ):
            self.patches.enter_context(patch(f"{module}.transaction", side_effect=compat))
            self.patches.enter_context(patch(f"{module}.readonly_connection", side_effect=compat))
        self.patches.enter_context(
            patch("bot.social_media.clip_manager.transaction", side_effect=compat)
        )
        self.patches.enter_context(
            patch(
                "bot.social_media.dashboard.get_clip_effective_layout",
                return_value=DEFAULT_STREAMER_LAYOUT,
            )
        )
        self.conn.execute(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES (?, ?)",
            ("streamer_a", "user-1"),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_clips_social_media (
                clip_id, clip_url, clip_title, clip_thumbnail_url, streamer_login,
                twitch_user_id, created_at, duration_seconds, view_count, game_name,
                status, source_kind, upload_local_path, local_file_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "clip-approval-1",
                "https://clips.example/1",
                "Pocket clutch",
                "https://clips.example/thumb.jpg",
                "streamer_a",
                "user-1",
                "2026-04-27T12:00:00+00:00",
                18.0,
                321,
                "Deadlock",
                "awaiting_approval",
                "manual_upload",
                "data/clips/clip-approval-1.mp4",
                "data/clips/clip-approval-1.mp4",
            ),
        )
        self.conn.execute(
            """
            INSERT INTO social_media_clip_enrichment (
                clip_db_id, title_youtube, title_tiktok, title_instagram,
                description_youtube, description_tiktok, description_instagram,
                hashtags_youtube, hashtags_tiktok, hashtags_instagram, status, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                1,
                "YT Title",
                "TT Title",
                "IG Title",
                "YT Desc",
                "TT Desc",
                "IG Desc",
                json.dumps(["#Deadlock", "#Pocket"]),
                json.dumps(["#DeadlockTT"]),
                json.dumps(["#DeadlockIG"]),
                "done",
                "2026-04-27T12:05:00+00:00",
            ),
        )
        apply_phase4_approval(_SqliteCompatConn(self.conn))
        self.conn.commit()

    def tearDown(self) -> None:  # noqa: D401
        self.patches.close()
        self.conn.close()
        super().tearDown()


class Phase4MigrationTests(unittest.TestCase):
    def test_apply_phase4_approval_creates_table_and_defaults(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        conn.executescript(
            """
            CREATE TABLE twitch_clips_social_media (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                clip_id TEXT UNIQUE
            );
            CREATE TABLE social_media_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT,
                updated_by TEXT
            );
            """
        )

        apply_phase4_approval(_SqliteCompatConn(conn))

        columns = {
            row["name"]
            for row in conn.execute("PRAGMA table_info('social_media_clip_approval')").fetchall()
        }
        settings = {
            row["key"]: json.loads(row["value"])
            for row in conn.execute("SELECT key, value FROM social_media_settings").fetchall()
        }

        self.assertIn("clip_db_id", columns)
        self.assertIn("approved_platforms", columns)
        self.assertEqual(settings["auto_approve_youtube"], False)
        self.assertEqual(settings["auto_approve_tiktok"], False)
        self.assertEqual(settings["auto_approve_instagram"], False)
        conn.close()


class ApprovalServiceTests(_StorageStubBase, unittest.IsolatedAsyncioTestCase):
    async def test_send_dm_is_idempotent_and_persists_message_reference(self) -> None:
        bot = SimpleNamespace(
            get_user=lambda _user_id: None,
            fetch_user=AsyncMock(
                return_value=SimpleNamespace(
                    send=AsyncMock(
                        return_value=SimpleNamespace(
                            id=555,
                            channel=SimpleNamespace(id=777),
                        )
                    )
                )
            ),
        )
        service = ApprovalService(bot=bot, clip_manager=ClipManager())

        first = await service.send_dm(1, "42")
        second = await service.send_dm(1, "42")
        record = get_approval_record(1)

        self.assertEqual(first, {"message_id": "555", "channel_id": "777"})
        self.assertEqual(second, {"message_id": "555", "channel_id": "777"})
        self.assertEqual(record.dm_message_id, "555")
        self.assertEqual(bot.fetch_user.await_count, 1)

    async def test_handle_decision_queues_selected_and_auto_approved_platforms(self) -> None:
        set_setting("auto_approve_instagram", True, updated_by="admin")
        service = ApprovalService(clip_manager=ClipManager())

        record = await service.handle_decision(1, "approve", ["youtube"], "discord-1")

        queued = self.conn.execute(
            """
            SELECT platform, title, description, hashtags
              FROM twitch_clips_upload_queue
             ORDER BY platform ASC
            """
        ).fetchall()

        self.assertEqual(record.state, "approved")
        self.assertEqual(record.approved_platforms, ["youtube", "instagram"])
        self.assertTrue(is_clip_approved_for(1, "youtube"))
        self.assertTrue(is_clip_approved_for(1, "instagram"))
        self.assertFalse(is_clip_approved_for(1, "tiktok"))
        self.assertEqual([row["platform"] for row in queued], ["instagram", "youtube"])
        self.assertEqual(json.loads(queued[1]["hashtags"]), ["#Deadlock", "#Pocket"])

    async def test_handle_decision_edit_marks_clip_without_queueing(self) -> None:
        service = ApprovalService(clip_manager=ClipManager())

        record = await service.handle_decision(1, "edit", ["tiktok"], "discord-2")

        queued_count = self.conn.execute(
            "SELECT COUNT(*) AS total FROM twitch_clips_upload_queue"
        ).fetchone()["total"]
        clip_status = self.conn.execute(
            "SELECT status FROM twitch_clips_social_media WHERE id = 1"
        ).fetchone()["status"]

        self.assertEqual(record.state, "editing")
        self.assertEqual(queued_count, 0)
        self.assertEqual(clip_status, "editing")


class ApprovalAdminApiTests(_StorageStubBase, unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:  # noqa: D401
        super().setUp()
        self.partner = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _r: True,
            auth_level_getter=lambda _r: "partner",
        )
        self.admin = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _r: True,
            auth_level_getter=lambda _r: "admin",
            auth_session_getter=lambda _r: {"discord_user_id": "discord-admin"},
        )

    async def test_partner_blocked_on_phase4_admin_routes(self) -> None:
        forbidden_calls = [
            self.partner.api_admin_clip_approval_get(SimpleNamespace(match_info={"clip_db_id": "1"})),
            self.partner.api_admin_clip_approval_decision(
                SimpleNamespace(
                    match_info={"clip_db_id": "1"},
                    json=AsyncMock(return_value={"decision": "approve", "platforms": ["youtube"]}),
                )
            ),
            self.partner.api_admin_auto_approve_get(SimpleNamespace()),
            self.partner.api_admin_auto_approve_put(
                SimpleNamespace(json=AsyncMock(return_value={"youtube": True}))
            ),
        ]
        for coro in forbidden_calls:
            with self.assertRaises(web.HTTPForbidden):
                await coro

    async def test_admin_can_get_and_update_auto_approve_settings(self) -> None:
        get_response = await self.admin.api_admin_auto_approve_get(SimpleNamespace())
        put_response = await self.admin.api_admin_auto_approve_put(
            SimpleNamespace(
                json=AsyncMock(
                    return_value={"youtube": True, "tiktok": False, "instagram": True}
                )
            )
        )

        self.assertEqual(get_response.status, 200)
        self.assertEqual(put_response.status, 200)
        self.assertEqual(get_auto_approve_settings(), {"youtube": True, "tiktok": False, "instagram": True})

    async def test_admin_decision_endpoint_returns_updated_clip_and_approval(self) -> None:
        response = await self.admin.api_admin_clip_approval_decision(
            SimpleNamespace(
                match_info={"clip_db_id": "1"},
                json=AsyncMock(return_value={"decision": "approve", "platforms": ["tiktok"]}),
            )
        )

        self.assertEqual(response.status, 200)
        payload = json.loads(response.text)
        self.assertEqual(payload["approval"]["state"], "approved")
        self.assertEqual(payload["approval"]["approved_platforms"], ["tiktok"])
        self.assertEqual(payload["clip"]["status"], "approved")
        self.assertEqual(payload["clip"]["approval"]["state"], "approved")


if __name__ == "__main__":
    unittest.main()
