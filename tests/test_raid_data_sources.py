from __future__ import annotations

import logging
import unittest
from contextlib import contextmanager
from datetime import UTC, datetime, timedelta

from bot.raid.services.raid_data_sources import RaidDataSourceService


class _Cursor:
    def __init__(self, rows):
        self._rows = rows

    def fetchall(self):
        return list(self._rows)

    def fetchone(self):
        return self._rows[0] if self._rows else None


class _Connection:
    def __init__(self, handlers):
        self._handlers = handlers

    def execute(self, sql, params=()):
        sql_text = str(sql)
        for matcher, rows in self._handlers:
            if matcher in sql_text:
                resolved = rows(params) if callable(rows) else rows
                return _Cursor(resolved)
        raise AssertionError(f"Unexpected SQL: {sql_text}")


@contextmanager
def _connection_factory(handlers):
    yield _Connection(handlers)


class _Api:
    def __init__(self, *, streams=None, category_id=None, stream_error=None) -> None:
        self.streams = streams or []
        self.category_id = category_id
        self.stream_error = stream_error
        self.stream_calls: list[tuple[list[str], str | None]] = []

    async def get_streams_by_logins(self, logins, *, language=None):
        self.stream_calls.append((list(logins), language))
        if self.stream_error is not None:
            raise self.stream_error
        return list(self.streams)

    async def get_category_id(self, game_name):
        if isinstance(self.category_id, Exception):
            raise self.category_id
        return self.category_id


class RaidDataSourceServiceTests(unittest.IsolatedAsyncioTestCase):
    def test_load_partner_roster_filters_missing_auth(self) -> None:
        service = RaidDataSourceService(
            readonly_connection_factory=lambda: _connection_factory(
                [
                    (
                        "FROM twitch_streamers_partner_state",
                        [
                            {
                                "twitch_login": "alpha",
                                "twitch_user_id": "1001",
                                "raid_enabled": 1,
                                "authorized_at": None,
                            },
                            {
                                "twitch_login": "bravo",
                                "twitch_user_id": "1002",
                                "raid_enabled": 0,
                                "authorized_at": "2026-03-01T10:00:00+00:00",
                            },
                            {
                                "twitch_login": "charlie",
                                "twitch_user_id": "1003",
                                "raid_enabled": 0,
                                "authorized_at": None,
                            },
                        ],
                    )
                ]
            ),
        )

        result = service.load_partner_roster_for_raid("source-id")

        self.assertEqual(
            result,
            [
                {"twitch_login": "alpha", "twitch_user_id": "1001", "raid_enabled": True},
                {"twitch_login": "bravo", "twitch_user_id": "1002", "raid_enabled": True},
            ],
        )

    def test_filter_deadlock_candidates_prefers_active_target_game(self) -> None:
        last_seen = (datetime.now(UTC) - timedelta(minutes=2)).isoformat()
        service = RaidDataSourceService(
            readonly_connection_factory=lambda: _connection_factory(
                [
                    (
                        "FROM twitch_live_state",
                        [
                            {
                                "streamer_login": "alpha",
                                "had_deadlock_in_session": 1,
                                "last_game": "Deadlock",
                                "last_deadlock_seen_at": last_seen,
                            },
                            {
                                "streamer_login": "bravo",
                                "had_deadlock_in_session": 1,
                                "last_game": "Deadlock",
                                "last_deadlock_seen_at": last_seen,
                            },
                        ],
                    )
                ]
            ),
            target_game_name="Deadlock",
        )
        online_partners = [
            {"user_login": "alpha", "game_name": "Deadlock"},
            {"user_login": "bravo", "game_name": "Just Chatting"},
            {"user_login": "charlie", "game_name": "VALORANT"},
        ]

        eligible, filtered_out = service.filter_deadlock_eligible_partner_candidates(
            online_partners
        )

        self.assertEqual(eligible, [{"user_login": "alpha", "game_name": "Deadlock"}])
        self.assertEqual(len(filtered_out), 1)
        self.assertIn("charlie", filtered_out[0])

    async def test_fetch_streams_prefers_shared_fetch(self) -> None:
        shared_calls: list[list[str]] = []

        async def _shared_fetch(logins: list[str]) -> dict[str, dict]:
            shared_calls.append(list(logins))
            return {"alpha": {"user_login": "alpha", "viewer_count": 12}}

        api = _Api(streams=[{"user_login": "alpha", "viewer_count": 1}])
        service = RaidDataSourceService(
            shared_stream_fetch=_shared_fetch,
            language_filter_getter=lambda: ["de"],
        )

        result = await service.fetch_streams_by_logins_for_raid(
            ["Alpha", "alpha"],
            api=api,
        )

        self.assertEqual(result["alpha"]["viewer_count"], 12)
        self.assertEqual(shared_calls, [["alpha"]])
        self.assertEqual(api.stream_calls, [])

    async def test_resolve_manual_source_state_uses_api_match(self) -> None:
        service = RaidDataSourceService(
            readonly_connection_factory=lambda: _connection_factory(
                [
                    (
                        "FROM twitch_live_state",
                        [
                            {
                                "twitch_user_id": "1001",
                                "streamer_login": "source_login",
                                "is_live": 0,
                                "last_started_at": "",
                                "last_game": "Just Chatting",
                                "last_viewer_count": 0,
                                "had_deadlock_in_session": 0,
                                "last_deadlock_seen_at": "",
                            }
                        ],
                    )
                ]
            ),
        )
        api = _Api(
            streams=[
                {
                    "user_id": "1001",
                    "user_login": "source_login",
                    "started_at": "2026-03-27T12:00:00+00:00",
                    "game_name": "Deadlock",
                    "viewer_count": 33,
                }
            ]
        )

        result = await service.resolve_manual_raid_source_state(
            broadcaster_id="1001",
            broadcaster_login="Source_Login",
            api=api,
        )

        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["state_source"], "api_live")
        self.assertEqual(result["live_state"]["last_game"], "Deadlock")
        self.assertEqual(result["live_state"]["last_viewer_count"], 33)

    async def test_resolve_manual_source_state_falls_back_to_db_live_state(self) -> None:
        service = RaidDataSourceService(
            readonly_connection_factory=lambda: _connection_factory(
                [
                    (
                        "FROM twitch_live_state",
                        [
                            {
                                "twitch_user_id": "1001",
                                "streamer_login": "source_login",
                                "is_live": 1,
                                "last_started_at": "2026-03-27T12:00:00+00:00",
                                "last_game": "Deadlock",
                                "last_viewer_count": 44,
                                "had_deadlock_in_session": 1,
                                "last_deadlock_seen_at": "2026-03-27T12:10:00+00:00",
                            }
                        ],
                    )
                ]
            ),
            logger=logging.getLogger("test.raid_data_sources"),
        )
        api = _Api(stream_error=RuntimeError("boom"))

        result = await service.resolve_manual_raid_source_state(
            broadcaster_id="1001",
            broadcaster_login="source_login",
            api=api,
        )

        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["state_source"], "db")
        self.assertEqual(result["live_state"]["last_viewer_count"], 44)

    async def test_resolve_target_category_id_prefers_cache_then_api(self) -> None:
        service = RaidDataSourceService(
            target_game_name="Deadlock",
            cached_category_id_getter=lambda: "cached-id",
        )
        api = _Api(category_id="deadlock-id")

        cached = await service.resolve_target_category_id(api=api)
        service.cached_category_id_getter = lambda: None
        fetched = await service.resolve_target_category_id(api=api)

        self.assertEqual(cached, "cached-id")
        self.assertEqual(fetched, "deadlock-id")


if __name__ == "__main__":
    unittest.main()
