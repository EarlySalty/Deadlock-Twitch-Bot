import contextlib
import unittest
from datetime import UTC, datetime
from unittest.mock import AsyncMock, patch

from bot.analytics.mixin import TwitchAnalyticsMixin
from bot.monitoring.eventsub_mixin import _EventSubMixin
from bot.raid.mixin import TwitchRaidMixin


class _RecordingConnection:
    def __init__(self) -> None:
        self.executed: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql: str, params=(), *args, **kwargs):
        del args, kwargs
        self.executed.append((sql, tuple(params or ())))
        return self

    def fetchone(self):
        return None


class _FrozenDateTime(datetime):
    @classmethod
    def now(cls, tz=None):
        current = cls(2026, 3, 31, 12, 5, 0, tzinfo=UTC)
        if tz is None:
            return current.replace(tzinfo=None)
        return current.astimezone(tz)


class _FakeRaidBot:
    def __init__(self) -> None:
        self.auth_manager = type(
            "_AuthManager",
            (),
            {"has_enabled_auth": staticmethod(lambda twitch_user_id: bool(twitch_user_id))},
        )()
        self.handle_streamer_offline = AsyncMock(return_value="targetlogin")
        self.loaded_rosters: list[tuple[str, ...]] = []
        self.online_candidate_batches: list[list[dict[str, object]]] = []

    def is_offline_auto_raid_suppressed(self, twitch_user_id: str) -> bool:
        del twitch_user_id
        return False

    def _get_target_game_lower(self) -> str:
        return "deadlock"

    def _evaluate_deadlock_raid_source(
        self,
        *,
        current_game: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> dict[str, object]:
        del current_game, last_deadlock_seen_at
        return {"eligible": bool(had_deadlock_session), "reason": "active_deadlock"}

    def _load_partner_roster_for_raid(self, twitch_user_id: str) -> list[dict[str, object]]:
        roster = [
            {
                "twitch_user_id": "2002",
                "twitch_login": "targetlogin",
                "raid_bot_enabled": 1,
            }
        ]
        self.loaded_rosters.append(tuple(str(row["twitch_user_id"]) for row in roster))
        return roster

    def _build_online_partner_candidates(
        self,
        partner_rows: list[dict[str, object]],
        streams_by_login: dict[str, dict[str, object]],
    ) -> list[dict[str, object]]:
        del partner_rows
        return [dict(streams_by_login["targetlogin"])]

    def _filter_deadlock_eligible_partner_candidates(
        self,
        online_partners: list[dict[str, object]],
    ) -> tuple[list[dict[str, object]], list[str]]:
        normalized = [dict(partner) for partner in online_partners]
        self.online_candidate_batches.append(normalized)
        return normalized, []


class _EventSubAutoRaidHarness(TwitchAnalyticsMixin, _EventSubMixin, TwitchRaidMixin):
    def __init__(self) -> None:
        self._raid_bot = _FakeRaidBot()
        self.api = object()
        self._category_id = "deadlock-cat"
        self.went_live_calls: list[tuple[str, str]] = []
        self.refresh_events: list[dict[str, object]] = []
        self.finalize_calls: list[dict[str, object]] = []

    async def _handle_stream_went_live(self, broadcaster_user_id: str, broadcaster_login: str) -> None:
        self.went_live_calls.append((broadcaster_user_id, broadcaster_login))

    async def _request_partner_raid_score_refresh(self, **kwargs):
        self.refresh_events.append(dict(kwargs))
        return True

    async def _finalize_stream_session(self, **kwargs):
        self.finalize_calls.append(dict(kwargs))
        return True

    def _load_live_state_row(self, login_lower: str) -> dict:
        del login_lower
        return {
            "is_live": 1,
            "last_game": "Deadlock",
            "had_deadlock_in_session": 1,
            "last_deadlock_seen_at": "2026-03-31T12:04:00+00:00",
            "last_started_at": "2026-03-31T11:55:00+00:00",
            "last_viewer_count": 77,
        }

    def _get_tracked_logins_for_eventsub(self) -> list[str]:
        return ["targetlogin"]

    async def _fetch_streams_by_logins_quick(self, tracked_logins: list[str]) -> dict[str, dict]:
        if tracked_logins != ["targetlogin"]:
            raise AssertionError(f"unexpected tracked logins: {tracked_logins!r}")
        return {
            "targetlogin": {
                "user_id": "2002",
                "user_login": "targetlogin",
                "game_name": "Deadlock",
                "started_at": "2026-03-31T11:00:00+00:00",
                "viewer_count": 12,
                "raid_enabled": True,
            }
        }

    async def _is_fully_authed(self, twitch_user_id: str) -> bool:
        return bool(twitch_user_id)

    def _load_auto_raid_partner_sync(self, twitch_user_id: str):
        del twitch_user_id
        return {"raid_bot_enabled": 1}


class EventSubAutoRaidFlowTests(unittest.IsolatedAsyncioTestCase):
    async def test_stream_online_then_offline_triggers_auto_raid_flow(self) -> None:
        harness = _EventSubAutoRaidHarness()
        conn = _RecordingConnection()

        with (
            patch(
                "bot.analytics.mixin.storage.transaction",
                side_effect=lambda: contextlib.nullcontext(conn),
            ),
            patch(
                "bot.monitoring.eventsub_mixin.storage.transaction",
                side_effect=lambda: contextlib.nullcontext(conn),
            ),
            patch(
                "bot.raid.mixin.asyncio.to_thread",
                new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
            ),
            patch("bot.raid.mixin.datetime", _FrozenDateTime),
        ):
            await harness._handle_stream_online(
                "1001",
                "source_login",
                {
                    "id": "stream-1",
                    "started_at": "2026-03-31T11:55:00+00:00",
                },
            )
            await harness._on_eventsub_stream_offline("1001", "source_login")

        self.assertEqual(harness.went_live_calls, [("1001", "source_login")])
        self.assertEqual(
            harness.finalize_calls,
            [{"login": "source_login", "reason": "offline"}],
        )
        self.assertEqual(
            harness.refresh_events,
            [
                {
                    "twitch_user_id": "1001",
                    "login": "source_login",
                    "trigger": "eventsub_stream_online",
                },
                {
                    "twitch_user_id": "1001",
                    "login": "source_login",
                    "trigger": "eventsub_stream_offline",
                },
            ],
        )
        harness._raid_bot.handle_streamer_offline.assert_awaited_once()
        raid_kwargs = harness._raid_bot.handle_streamer_offline.await_args.kwargs
        self.assertEqual(raid_kwargs["broadcaster_id"], "1001")
        self.assertEqual(raid_kwargs["broadcaster_login"], "source_login")
        self.assertEqual(raid_kwargs["viewer_count"], 77)
        self.assertEqual(raid_kwargs["stream_duration_sec"], 600)
        self.assertEqual(
            raid_kwargs["online_partners"],
            [
                {
                    "user_id": "2002",
                    "user_login": "targetlogin",
                    "game_name": "Deadlock",
                    "started_at": "2026-03-31T11:00:00+00:00",
                    "viewer_count": 12,
                    "raid_enabled": True,
                }
            ],
        )
        self.assertEqual(harness._raid_bot.loaded_rosters, [("2002",)])
        self.assertEqual(len(harness._raid_bot.online_candidate_batches), 1)

        insert_live_state = [
            sql for sql, _ in conn.executed if "INSERT INTO twitch_live_state" in sql
        ]
        offline_live_state = [
            sql for sql, _ in conn.executed if "UPDATE twitch_live_state" in sql
        ]
        self.assertTrue(insert_live_state)
        self.assertTrue(offline_live_state)
        self.assertIn("active_session_id = NULL", offline_live_state[0])


if __name__ == "__main__":
    unittest.main()
