import contextlib
import sqlite3
import time
import unittest
from datetime import UTC, datetime
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

import aiohttp

from bot.raid.bot import RaidBot
from bot.raid.mixin import TwitchRaidMixin

from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    @staticmethod
    def _translate(sql: str) -> str:
        translated = str(sql).replace("%s", "?")
        translated = translated.replace("NOW() - (%s::interval)", "datetime('now', ?)")
        translated = translated.replace("NOW() - INTERVAL '1 day'", "datetime('now', '-1 day')")
        translated = translated.replace("NOW() - INTERVAL '7 days'", "datetime('now', '-7 days')")
        translated = translated.replace("NOW() - INTERVAL '30 days'", "datetime('now', '-30 days')")
        return translated

    def execute(self, sql: str, params=()):
        return self._conn.execute(self._translate(sql), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


class _FrozenDateTime(datetime):
    @classmethod
    def now(cls, tz=None):
        current = cls(2026, 3, 10, 20, 0, 0, tzinfo=UTC)
        if tz is None:
            return current.replace(tzinfo=None)
        return current.astimezone(tz)


class _OfflineAutoRaidHarness(TwitchRaidMixin):
    def __init__(self, raid_bot: RaidBot) -> None:
        self._raid_bot = raid_bot
        self.api = None
        self._category_id = None

    async def _is_fully_authed(self, twitch_user_id: str) -> bool:
        return bool(twitch_user_id)


class RaidPartnerScoreSelectionTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(self.conn)
        self.session = aiohttp.ClientSession()
        self.raid_bot = RaidBot(
            client_id="client-id",
            client_secret="client-secret",
            redirect_uri="http://localhost/raid/callback",
            session=self.session,
        )

    async def asyncTearDown(self) -> None:
        await self.raid_bot.cleanup()
        await self.session.close()
        self.conn.close()

    def _conn_patch(self):
        stack = contextlib.ExitStack()
        stack.enter_context(
            patch(
                "bot.raid.bot.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
            )
        )
        stack.enter_context(
            patch(
                "bot.raid.bot.transaction",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
            )
        )
        return stack

    @staticmethod
    def _score_map(rows: dict[str, dict[str, object]]):
        def _loader(user_ids: list[str]):
            return {user_id: dict(rows[user_id]) for user_id in user_ids if user_id in rows}

        return _loader

    async def test_select_partner_candidate_prefers_highest_final_score(self) -> None:
        candidates = [
            {
                "user_id": "1001",
                "user_login": "alpha",
                "viewer_count": 50,
                "followers_total": 1000,
                "started_at": "2026-03-08T18:00:00+00:00",
            },
            {
                "user_id": "2002",
                "user_login": "bravo",
                "viewer_count": 10,
                "followers_total": 200,
                "started_at": "2026-03-08T18:10:00+00:00",
            },
        ]
        score_rows = {
            "1001": {"is_live": True, "final_score": 0.91, "today_received_raids": 5},
            "2002": {"is_live": True, "final_score": 0.66, "today_received_raids": 0},
        }

        with (
            self._conn_patch(),
            patch(
                "bot.raid.bot.load_partner_raid_score_map",
                side_effect=self._score_map(score_rows),
            ),
            patch.object(self.raid_bot, "_attach_followers_totals", new=AsyncMock()) as attach_mock,
        ):
            selected = await self.raid_bot._select_partner_candidate_by_score(
                candidates,
                "source-1",
            )

        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(selected["user_login"], "alpha")
        attach_mock.assert_not_awaited()

    async def test_select_partner_candidate_uses_today_received_raids_for_close_scores(self) -> None:
        candidates = [
            {
                "user_id": "1001",
                "user_login": "alpha",
                "viewer_count": 50,
                "followers_total": 1000,
                "started_at": "2026-03-08T18:00:00+00:00",
            },
            {
                "user_id": "2002",
                "user_login": "bravo",
                "viewer_count": 75,
                "followers_total": 800,
                "started_at": "2026-03-08T17:00:00+00:00",
            },
        ]
        score_rows = {
            "1001": {"is_live": True, "final_score": 0.90, "today_received_raids": 4},
            "2002": {"is_live": True, "final_score": 0.86, "today_received_raids": 1},
        }

        with (
            self._conn_patch(),
            patch(
                "bot.raid.bot.load_partner_raid_score_map",
                side_effect=self._score_map(score_rows),
            ),
            patch.object(self.raid_bot, "_attach_followers_totals", new=AsyncMock()) as attach_mock,
        ):
            selected = await self.raid_bot._select_partner_candidate_by_score(
                candidates,
                "source-2",
            )

        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(selected["user_login"], "bravo")
        attach_mock.assert_not_awaited()

    async def test_select_partner_candidate_falls_back_to_viewers_followers_started_at(self) -> None:
        candidates = [
            {
                "user_id": "1001",
                "user_login": "alpha",
                "viewer_count": 40,
                "followers_total": 500,
                "started_at": "2026-03-08T18:00:00+00:00",
            },
            {
                "user_id": "2002",
                "user_login": "bravo",
                "viewer_count": 10,
                "followers_total": 800,
                "started_at": "2026-03-08T17:00:00+00:00",
            },
            {
                "user_id": "3003",
                "user_login": "charlie",
                "viewer_count": 10,
                "followers_total": 300,
                "started_at": "2026-03-08T16:00:00+00:00",
            },
        ]
        score_rows = {
            "1001": {"is_live": True, "final_score": 0.90, "today_received_raids": 1},
            "2002": {"is_live": True, "final_score": 0.86, "today_received_raids": 1},
            "3003": {"is_live": True, "final_score": 0.87, "today_received_raids": 1},
        }

        with (
            self._conn_patch(),
            patch(
                "bot.raid.bot.load_partner_raid_score_map",
                side_effect=self._score_map(score_rows),
            ),
            patch.object(self.raid_bot, "_attach_followers_totals", new=AsyncMock()) as attach_mock,
        ):
            selected = await self.raid_bot._select_partner_candidate_by_score(
                candidates,
                "source-3",
            )

        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(selected["user_login"], "charlie")
        attach_mock.assert_awaited_once()

    async def test_select_partner_candidate_ignores_recent_target_cooldown_for_partners(self) -> None:
        candidates = [
            {
                "user_id": "1001",
                "user_login": "alpha",
                "viewer_count": 25,
                "followers_total": 400,
                "started_at": "2026-03-08T18:00:00+00:00",
            },
            {
                "user_id": "2002",
                "user_login": "bravo",
                "viewer_count": 20,
                "followers_total": 300,
                "started_at": "2026-03-08T17:00:00+00:00",
            },
            {
                "user_id": "3003",
                "user_login": "charlie",
                "viewer_count": 10,
                "followers_total": 100,
                "started_at": "2026-03-08T16:00:00+00:00",
            },
        ]
        score_rows = {
            "1001": {"is_live": True, "final_score": 0.95, "today_received_raids": 0},
            "3003": {"is_live": True, "final_score": 0.80, "today_received_raids": 0},
        }

        with (
            self._conn_patch(),
            patch(
                "bot.raid.bot.load_partner_raid_score_map",
                side_effect=self._score_map(score_rows),
            ),
            patch.object(self.raid_bot, "_get_recent_raid_targets", return_value={"1001"}),
            patch.object(self.raid_bot, "_attach_followers_totals", new=AsyncMock()),
        ):
            selected = await self.raid_bot._select_partner_candidate_by_score(
                candidates,
                "source-4",
            )

        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(selected["user_login"], "alpha")

    async def test_select_fairest_candidate_keeps_recent_target_cooldown_for_fallback(self) -> None:
        candidates = [
            {
                "user_id": "1001",
                "user_login": "alpha",
                "viewer_count": 10,
                "followers_total": 100,
                "started_at": "2026-03-08T18:00:00+00:00",
            },
            {
                "user_id": "2002",
                "user_login": "bravo",
                "viewer_count": 25,
                "followers_total": 100,
                "started_at": "2026-03-08T17:00:00+00:00",
            },
        ]

        with (
            self._conn_patch(),
            patch.object(self.raid_bot, "_get_recent_raid_targets", return_value={"1001"}),
            patch.object(self.raid_bot, "_attach_followers_totals", new=AsyncMock()) as attach_mock,
        ):
            selected = await self.raid_bot._select_fairest_candidate(
                candidates,
                "source-5",
            )

        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(selected["user_login"], "bravo")
        attach_mock.assert_awaited_once()

    async def test_on_raid_arrival_refreshes_cache_after_partner_confirmation(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_streamers (twitch_login, twitch_user_id, silent_raid)
            VALUES (?, ?, 0)
            """,
            ("targetlogin", "9009"),
        )
        self.raid_bot._pending_raids["9009"] = (
            "source_login",
            None,
            time.time(),
            True,
            42,
            None,
        )

        with (
            self._conn_patch(),
            patch(
                "bot.raid.bot.refresh_partner_raid_score_async",
                new=AsyncMock(return_value={"twitch_user_id": "9009"}),
            ) as refresh_mock,
            patch.object(
                self.raid_bot,
                "_send_partner_raid_message",
                new=AsyncMock(return_value=None),
            ) as send_mock,
        ):
            await self.raid_bot.on_raid_arrival(
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
                from_broadcaster_login="source_login",
                from_broadcaster_id="source-id",
                viewer_count=42,
            )

        refresh_mock.assert_awaited_once_with("9009")
        send_mock.assert_awaited_once()

    async def test_register_pending_raid_supersedes_older_pending_from_same_source(self) -> None:
        tracked = {("channel.raid", "new-target")}
        self.raid_bot.set_cog(
            SimpleNamespace(
                _eventsub_has_sub=lambda sub_type, broadcaster_user_id: (
                    sub_type,
                    str(broadcaster_user_id),
                )
                in tracked
            )
        )
        self.raid_bot._pending_raids["old-target"] = (
            "source_login",
            {"user_login": "old_target"},
            time.time(),
            True,
            2,
            None,
        )

        await self.raid_bot._register_pending_raid(
            from_broadcaster_login="source_login",
            to_broadcaster_id="new-target",
            to_broadcaster_login="new_target",
            target_stream_data={"user_login": "new_target"},
            is_partner_raid=True,
            viewer_count=4,
        )

        self.assertNotIn("old-target", self.raid_bot._pending_raids)
        self.assertIn(("new-target", "source_login"), self.raid_bot._pending_raids)

    async def test_execute_raid_pipeline_prepares_fresh_raid_subscription_before_start(self) -> None:
        call_order: list[str] = []
        tracked_subs: set[tuple[str, str]] = set()

        async def _ensure_ready(broadcaster_id: str, broadcaster_login: str, **_kwargs):
            call_order.append(f"ensure:{broadcaster_id}:{broadcaster_login}")
            tracked_subs.add(("channel.raid", str(broadcaster_id)))
            return True, "enabled"

        self.raid_bot.set_cog(
            SimpleNamespace(
                _eventsub_has_sub=lambda sub_type, broadcaster_user_id: (
                    sub_type,
                    str(broadcaster_user_id),
                )
                in tracked_subs,
                ensure_raid_target_dynamic_ready=AsyncMock(side_effect=_ensure_ready),
            )
        )

        target = {
            "user_id": "2002",
            "user_login": "targetlogin",
            "viewer_count": 3,
            "followers_total": 120,
            "started_at": "2026-03-10T18:00:00+00:00",
            "raid_enabled": True,
        }

        async def _start_raid(**kwargs):
            call_order.append(
                f"start:{kwargs['to_broadcaster_id']}:{kwargs['to_broadcaster_login']}"
            )
            return True, None

        with (
            patch.object(
                self.raid_bot,
                "_select_partner_candidate_by_score",
                new=AsyncMock(return_value=target),
            ),
            patch.object(
                self.raid_bot.raid_executor,
                "start_raid",
                new=AsyncMock(side_effect=_start_raid),
            ) as start_mock,
            patch.object(self.raid_bot, "_register_pending_raid", new=AsyncMock()),
        ):
            result = await self.raid_bot._execute_raid_pipeline(
                broadcaster_id="1001",
                broadcaster_login="source_login",
                viewer_count=7,
                stream_duration_sec=600,
                online_partners=[target],
                api=None,
                category_id=None,
                offline_trigger_ts=None,
                reason="auto_raid_on_offline",
            )

        self.assertEqual(result["status"], "started")
        self.assertEqual(
            call_order[:2],
            ["ensure:2002:targetlogin", "start:2002:targetlogin"],
        )
        ensure_ready_mock = self.raid_bot._cog.ensure_raid_target_dynamic_ready
        self.assertEqual(ensure_ready_mock.await_count, 1)
        start_mock.assert_awaited_once()


class ManualRaidFlowTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(self.conn)
        self.session = aiohttp.ClientSession()
        self.raid_bot = RaidBot(
            client_id="client-id",
            client_secret="client-secret",
            redirect_uri="http://localhost/raid/callback",
            session=self.session,
        )

    async def asyncTearDown(self) -> None:
        await self.raid_bot.cleanup()
        await self.session.close()
        self.conn.close()

    def _conn_patch(self):
        stack = contextlib.ExitStack()
        stack.enter_context(
            patch(
                "bot.raid.bot.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
            )
        )
        stack.enter_context(
            patch(
                "bot.raid.bot.transaction",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
            )
        )
        return stack

    def _insert_partner(self, login: str, user_id: str) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_streamer_identities (twitch_login, twitch_user_id)
            VALUES (?, ?)
            ON CONFLICT(twitch_user_id) DO UPDATE SET
                twitch_login = excluded.twitch_login
            """,
            (login, user_id),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_login,
                twitch_user_id,
                manual_verified_at,
                raid_bot_enabled,
                partnered_at,
                status
            ) VALUES (?, ?, ?, 1, ?, 'active')
            """,
            (
                login,
                user_id,
                "2026-03-10T19:00:00+00:00",
                "2026-03-10T19:00:00+00:00",
            ),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (
                twitch_user_id, twitch_login, token_expires_at, scopes, raid_enabled, authorized_at
            ) VALUES (?, ?, ?, ?, 1, ?)
            """,
            (
                user_id,
                login,
                "2026-03-11T19:00:00+00:00",
                "channel:manage:raids",
                "2026-03-10T19:00:00+00:00",
            ),
        )

    async def test_start_manual_raid_filters_partners_and_uses_source_metrics(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at,
                last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
            ) VALUES (?, ?, 1, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "source_login",
                "2026-03-10T19:55:00+00:00",
                "Deadlock",
                88,
                1,
                "2026-03-10T19:58:00+00:00",
            ),
        )
        self._insert_partner("deadlocker", "2002")
        self._insert_partner("variety", "3003")

        streams_by_login = {
            "deadlocker": {
                "user_id": "2002",
                "user_login": "deadlocker",
                "game_name": "Deadlock",
                "started_at": "2026-03-10T18:00:00+00:00",
                "viewer_count": 5,
            },
            "variety": {
                "user_id": "3003",
                "user_login": "variety",
                "game_name": "Fortnite",
                "started_at": "2026-03-10T19:30:00+00:00",
                "viewer_count": 1,
            },
        }

        with (
            self._conn_patch(),
            patch("bot.raid.bot.datetime", _FrozenDateTime),
            patch.object(
                self.raid_bot,
                "_fetch_streams_by_logins_for_raid",
                new=AsyncMock(return_value=streams_by_login),
            ),
            patch.object(self.raid_bot, "_create_twitch_api", return_value=None),
            patch.object(
                self.raid_bot,
                "_resolve_target_category_id",
                new=AsyncMock(return_value=None),
            ),
            patch.object(
                self.raid_bot,
                "_select_partner_candidate_by_score",
                new=AsyncMock(side_effect=lambda candidates, _source: candidates[0]),
            ) as select_mock,
            patch.object(
                self.raid_bot.raid_executor,
                "start_raid",
                new=AsyncMock(return_value=(True, None)),
            ) as start_mock,
            patch.object(self.raid_bot, "_register_pending_raid", new=AsyncMock()) as register_mock,
            patch.object(self.raid_bot, "mark_manual_raid_started") as suppression_mock,
        ):
            result = await self.raid_bot.start_manual_raid(
                broadcaster_id="1001",
                broadcaster_login="source_login",
            )

        self.assertEqual(result["status"], "started")
        self.assertEqual(result["target_login"], "deadlocker")

        selected_candidates = select_mock.await_args.args[0]
        self.assertEqual(len(selected_candidates), 1)
        self.assertEqual(selected_candidates[0]["user_login"], "deadlocker")

        start_kwargs = start_mock.await_args.kwargs
        self.assertEqual(start_kwargs["viewer_count"], 88)
        self.assertEqual(start_kwargs["stream_duration_sec"], 300)
        self.assertEqual(start_kwargs["to_broadcaster_login"], "deadlocker")
        self.assertEqual(start_kwargs["reason"], "manual_chat_command")

        register_kwargs = register_mock.await_args.kwargs
        self.assertEqual(register_kwargs["viewer_count"], 88)
        self.assertTrue(register_kwargs["is_partner_raid"])
        self.assertEqual(register_kwargs["to_broadcaster_login"], "deadlocker")

        suppression_mock.assert_called_once_with(
            broadcaster_id="1001",
            ttl_seconds=180.0,
        )

    async def test_start_manual_raid_rejects_stale_just_chatting_source(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at,
                last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
            ) VALUES (?, ?, 1, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "source_login",
                "2026-03-10T19:50:00+00:00",
                "Just Chatting",
                42,
                1,
                "2026-03-10T19:40:00+00:00",
            ),
        )

        with (
            self._conn_patch(),
            patch("bot.raid.bot.datetime", _FrozenDateTime),
            patch.object(
                self.raid_bot.raid_executor,
                "start_raid",
                new=AsyncMock(return_value=(True, None)),
            ) as start_mock,
        ):
            result = await self.raid_bot.start_manual_raid(
                broadcaster_id="1001",
                broadcaster_login="source_login",
            )

        self.assertEqual(result["status"], "source_not_eligible")
        self.assertEqual(result["reason"], "stale_deadlock_session")
        start_mock.assert_not_awaited()

    async def test_start_manual_raid_allows_live_deadlock_when_db_snapshot_is_stale(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at,
                last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
            ) VALUES (?, ?, 0, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "source_login",
                "2026-03-10T18:30:00+00:00",
                "Fortnite",
                13,
                0,
                None,
            ),
        )
        self._insert_partner("deadlocker", "2002")

        api = SimpleNamespace(
            get_streams_by_logins=AsyncMock(
                return_value=[
                    {
                        "user_id": "1001",
                        "user_login": "source_login",
                        "game_name": "Deadlock",
                        "started_at": "2026-03-10T19:57:00+00:00",
                        "viewer_count": 144,
                    }
                ]
            )
        )

        with (
            self._conn_patch(),
            patch("bot.raid.bot.datetime", _FrozenDateTime),
            patch.object(
                self.raid_bot,
                "_fetch_streams_by_logins_for_raid",
                new=AsyncMock(
                    return_value={
                        "deadlocker": {
                            "user_id": "2002",
                            "user_login": "deadlocker",
                            "game_name": "Deadlock",
                            "started_at": "2026-03-10T18:00:00+00:00",
                            "viewer_count": 5,
                        }
                    }
                ),
            ),
            patch.object(self.raid_bot, "_create_twitch_api", return_value=api),
            patch.object(
                self.raid_bot,
                "_resolve_target_category_id",
                new=AsyncMock(return_value=None),
            ),
            patch.object(
                self.raid_bot,
                "_select_partner_candidate_by_score",
                new=AsyncMock(side_effect=lambda candidates, _source: candidates[0]),
            ),
            patch.object(
                self.raid_bot.raid_executor,
                "start_raid",
                new=AsyncMock(return_value=(True, None)),
            ) as start_mock,
            patch.object(self.raid_bot, "_register_pending_raid", new=AsyncMock()),
            patch.object(self.raid_bot, "mark_manual_raid_started"),
        ):
            result = await self.raid_bot.start_manual_raid(
                broadcaster_id="1001",
                broadcaster_login="source_login",
            )

        self.assertEqual(result["status"], "started")
        api.get_streams_by_logins.assert_awaited_once_with(["source_login"])
        start_kwargs = start_mock.await_args.kwargs
        self.assertEqual(start_kwargs["viewer_count"], 144)
        self.assertEqual(start_kwargs["stream_duration_sec"], 180)

    async def test_start_manual_raid_rejects_live_just_chatting_without_deadlock_session(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at,
                last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
            ) VALUES (?, ?, 1, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "source_login",
                "2026-03-10T19:52:00+00:00",
                "Deadlock",
                42,
                0,
                None,
            ),
        )

        api = SimpleNamespace(
            get_streams_by_logins=AsyncMock(
                return_value=[
                    {
                        "user_id": "1001",
                        "user_login": "source_login",
                        "game_name": "Just Chatting",
                        "started_at": "2026-03-10T19:59:00+00:00",
                        "viewer_count": 50,
                    }
                ]
            )
        )

        with (
            self._conn_patch(),
            patch("bot.raid.bot.datetime", _FrozenDateTime),
            patch.object(self.raid_bot, "_create_twitch_api", return_value=api),
            patch.object(
                self.raid_bot.raid_executor,
                "start_raid",
                new=AsyncMock(return_value=(True, None)),
            ) as start_mock,
        ):
            result = await self.raid_bot.start_manual_raid(
                broadcaster_id="1001",
                broadcaster_login="source_login",
            )

        self.assertEqual(result["status"], "source_not_eligible")
        self.assertEqual(result["reason"], "just_chatting_without_deadlock_session")
        start_mock.assert_not_awaited()

    async def test_start_manual_raid_allows_live_just_chatting_with_recent_deadlock_session(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at,
                last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
            ) VALUES (?, ?, 1, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "source_login",
                "2026-03-10T19:52:00+00:00",
                "Deadlock",
                42,
                1,
                "2026-03-10T19:58:30+00:00",
            ),
        )
        self._insert_partner("deadlocker", "2002")

        api = SimpleNamespace(
            get_streams_by_logins=AsyncMock(
                return_value=[
                    {
                        "user_id": "1001",
                        "user_login": "source_login",
                        "game_name": "Just Chatting",
                        "started_at": "2026-03-10T19:56:00+00:00",
                        "viewer_count": 61,
                    }
                ]
            )
        )

        with (
            self._conn_patch(),
            patch("bot.raid.bot.datetime", _FrozenDateTime),
            patch.object(
                self.raid_bot,
                "_fetch_streams_by_logins_for_raid",
                new=AsyncMock(
                    return_value={
                        "deadlocker": {
                            "user_id": "2002",
                            "user_login": "deadlocker",
                            "game_name": "Deadlock",
                            "started_at": "2026-03-10T18:00:00+00:00",
                            "viewer_count": 5,
                        }
                    }
                ),
            ),
            patch.object(self.raid_bot, "_create_twitch_api", return_value=api),
            patch.object(
                self.raid_bot,
                "_resolve_target_category_id",
                new=AsyncMock(return_value=None),
            ),
            patch.object(
                self.raid_bot,
                "_select_partner_candidate_by_score",
                new=AsyncMock(side_effect=lambda candidates, _source: candidates[0]),
            ),
            patch.object(
                self.raid_bot.raid_executor,
                "start_raid",
                new=AsyncMock(return_value=(True, None)),
            ) as start_mock,
            patch.object(self.raid_bot, "_register_pending_raid", new=AsyncMock()),
            patch.object(self.raid_bot, "mark_manual_raid_started"),
        ):
            result = await self.raid_bot.start_manual_raid(
                broadcaster_id="1001",
                broadcaster_login="source_login",
            )

        self.assertEqual(result["status"], "started")
        start_kwargs = start_mock.await_args.kwargs
        self.assertEqual(start_kwargs["viewer_count"], 61)
        self.assertEqual(start_kwargs["stream_duration_sec"], 240)

    async def test_start_manual_raid_falls_back_to_db_when_source_api_refresh_fails(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at,
                last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
            ) VALUES (?, ?, 1, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "source_login",
                "2026-03-10T19:55:00+00:00",
                "Deadlock",
                88,
                1,
                "2026-03-10T19:58:00+00:00",
            ),
        )
        self._insert_partner("deadlocker", "2002")

        api = SimpleNamespace(
            get_streams_by_logins=AsyncMock(side_effect=RuntimeError("twitch-api-down"))
        )

        with (
            self._conn_patch(),
            patch("bot.raid.bot.datetime", _FrozenDateTime),
            patch.object(
                self.raid_bot,
                "_fetch_streams_by_logins_for_raid",
                new=AsyncMock(
                    return_value={
                        "deadlocker": {
                            "user_id": "2002",
                            "user_login": "deadlocker",
                            "game_name": "Deadlock",
                            "started_at": "2026-03-10T18:00:00+00:00",
                            "viewer_count": 5,
                        }
                    }
                ),
            ),
            patch.object(self.raid_bot, "_create_twitch_api", return_value=api),
            patch.object(
                self.raid_bot,
                "_resolve_target_category_id",
                new=AsyncMock(return_value=None),
            ),
            patch.object(
                self.raid_bot,
                "_select_partner_candidate_by_score",
                new=AsyncMock(side_effect=lambda candidates, _source: candidates[0]),
            ),
            patch.object(
                self.raid_bot.raid_executor,
                "start_raid",
                new=AsyncMock(return_value=(True, None)),
            ) as start_mock,
            patch.object(self.raid_bot, "_register_pending_raid", new=AsyncMock()),
            patch.object(self.raid_bot, "mark_manual_raid_started"),
        ):
            result = await self.raid_bot.start_manual_raid(
                broadcaster_id="1001",
                broadcaster_login="source_login",
            )

        self.assertEqual(result["status"], "started")
        api.get_streams_by_logins.assert_awaited_once_with(["source_login"])
        start_kwargs = start_mock.await_args.kwargs
        self.assertEqual(start_kwargs["viewer_count"], 88)
        self.assertEqual(start_kwargs["stream_duration_sec"], 300)

    async def test_start_manual_raid_marks_source_not_live_when_api_reports_offline(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at,
                last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
            ) VALUES (?, ?, 1, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "source_login",
                "2026-03-10T19:55:00+00:00",
                "Deadlock",
                88,
                1,
                "2026-03-10T19:58:00+00:00",
            ),
        )

        api = SimpleNamespace(get_streams_by_logins=AsyncMock(return_value=[]))

        with (
            self._conn_patch(),
            patch("bot.raid.bot.datetime", _FrozenDateTime),
            patch.object(self.raid_bot, "_create_twitch_api", return_value=api),
            patch.object(
                self.raid_bot.raid_executor,
                "start_raid",
                new=AsyncMock(return_value=(True, None)),
            ) as start_mock,
        ):
            result = await self.raid_bot.start_manual_raid(
                broadcaster_id="1001",
                broadcaster_login="source_login",
            )

        self.assertEqual(result["status"], "source_not_live")
        self.assertEqual(result["reason"], "api_offline")
        start_mock.assert_not_awaited()

    async def test_start_manual_raid_uses_deadlock_fallback_when_no_partner_is_eligible(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at,
                last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
            ) VALUES (?, ?, 1, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "source_login",
                "2026-03-10T19:54:00+00:00",
                "Deadlock",
                55,
                1,
                "2026-03-10T19:57:00+00:00",
            ),
        )
        self._insert_partner("variety", "3003")

        api = SimpleNamespace(
            get_streams_by_category=AsyncMock(
                return_value=[
                    {
                        "user_id": "4004",
                        "user_login": "fallbacker",
                        "game_name": "Deadlock",
                        "started_at": "2026-03-10T19:30:00+00:00",
                        "viewer_count": 7,
                    }
                ]
            )
        )

        with (
            self._conn_patch(),
            patch("bot.raid.bot.datetime", _FrozenDateTime),
            patch.object(
                self.raid_bot,
                "_fetch_streams_by_logins_for_raid",
                new=AsyncMock(
                    return_value={
                        "variety": {
                            "user_id": "3003",
                            "user_login": "variety",
                            "game_name": "Fortnite",
                            "started_at": "2026-03-10T19:40:00+00:00",
                            "viewer_count": 3,
                        }
                    }
                ),
            ),
            patch.object(self.raid_bot, "_create_twitch_api", return_value=api),
            patch.object(
                self.raid_bot,
                "_resolve_target_category_id",
                new=AsyncMock(return_value="deadlock-cat"),
            ),
            patch.object(
                self.raid_bot,
                "_select_partner_candidate_by_score",
                new=AsyncMock(return_value=None),
            ) as partner_select_mock,
            patch.object(
                self.raid_bot,
                "_select_fairest_candidate",
                new=AsyncMock(side_effect=lambda candidates, _source: candidates[0]),
            ) as fallback_select_mock,
            patch.object(
                self.raid_bot.raid_executor,
                "start_raid",
                new=AsyncMock(return_value=(True, None)),
            ) as start_mock,
            patch.object(self.raid_bot, "_register_pending_raid", new=AsyncMock()),
            patch.object(self.raid_bot, "mark_manual_raid_started"),
        ):
            result = await self.raid_bot.start_manual_raid(
                broadcaster_id="1001",
                broadcaster_login="source_login",
            )

        self.assertEqual(result["status"], "started")
        self.assertEqual(result["target_login"], "fallbacker")
        partner_select_mock.assert_not_awaited()

        fallback_candidates = fallback_select_mock.await_args.args[0]
        self.assertEqual(len(fallback_candidates), 1)
        self.assertEqual(fallback_candidates[0]["user_login"], "fallbacker")

        start_kwargs = start_mock.await_args.kwargs
        self.assertEqual(start_kwargs["to_broadcaster_login"], "fallbacker")
        self.assertEqual(start_kwargs["candidates_count"], 1)
        self.assertEqual(start_kwargs["reason"], "manual_chat_command")


class OfflineRaidSourceLoggingTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(self.conn)
        self.session = aiohttp.ClientSession()
        self.raid_bot = RaidBot(
            client_id="client-id",
            client_secret="client-secret",
            redirect_uri="http://localhost/raid/callback",
            session=self.session,
        )
        self.raid_bot.auth_manager.has_enabled_auth = lambda _user_id: True
        self.harness = _OfflineAutoRaidHarness(self.raid_bot)

    async def asyncTearDown(self) -> None:
        await self.raid_bot.cleanup()
        await self.session.close()
        self.conn.close()

    def _conn_patch(self):
        return patch(
            "bot.raid.mixin.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(self.conn),
        )

    async def test_handle_auto_raid_logs_stale_deadlock_recency_reason(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_streamers (twitch_login, twitch_user_id, raid_bot_enabled)
            VALUES (?, ?, 1)
            """,
            ("source_login", "1001"),
        )

        with (
            self._conn_patch(),
            patch("bot.raid.bot.datetime", _FrozenDateTime),
            self.assertLogs("TwitchStreams.RaidMixin", level="INFO") as captured_logs,
        ):
            await self.harness._handle_auto_raid_on_offline(
                login="source_login",
                twitch_user_id="1001",
                previous_state={
                    "last_game": "Just Chatting",
                    "had_deadlock_in_session": 1,
                    "last_deadlock_seen_at": "2026-03-10T19:40:00+00:00",
                    "last_viewer_count": 12,
                },
                streams_by_login={},
            )

        combined_logs = "\n".join(captured_logs.output)
        self.assertIn("letzter Deadlock > 360s her", combined_logs)
        self.assertNotIn("letzte Kategorie", combined_logs)


if __name__ == "__main__":
    unittest.main()
