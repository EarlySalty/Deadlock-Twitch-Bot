import asyncio
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from bot.base import TwitchBaseCog


class _FetchAllResult:
    def __init__(self, rows):
        self._rows = list(rows)

    def fetchall(self):
        return list(self._rows)


class _FetchOneResult:
    def __init__(self, row):
        self._row = row

    def fetchone(self):
        return self._row


class _ScoutConnection:
    def __init__(self) -> None:
        self.inserted: list[tuple[str, str | None, str]] = []
        self.commits = 0

    def execute(self, sql: str, params=(), *args, **kwargs):
        normalized_sql = " ".join(str(sql).split())
        normalized_sql = normalized_sql.replace("%s", "?")
        if "SELECT twitch_login FROM twitch_streamers WHERE is_monitored_only = 1" in normalized_sql:
            return _FetchAllResult([])
        if "SELECT 1 FROM twitch_streamers WHERE twitch_login = ?" in normalized_sql:
            return _FetchOneResult(None)
        if "INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only, created_at)" in normalized_sql:
            login, user_id, created_at = params
            self.inserted.append((str(login), str(user_id or "") or None, str(created_at)))
            return _FetchOneResult(None)
        raise AssertionError(f"Unexpected SQL in test_scout_monitored_session_bootstrap: {normalized_sql}")

    def commit(self) -> None:
        self.commits += 1


class _ExistingMonitoredScoutConnection:
    def __init__(self) -> None:
        self.commits = 0

    def execute(self, sql: str, params=(), *args, **kwargs):
        normalized_sql = " ".join(str(sql).split())
        normalized_sql = normalized_sql.replace("%s", "?")
        if "SELECT twitch_login FROM twitch_streamers WHERE is_monitored_only = 1" in normalized_sql:
            return _FetchAllResult([("mewgles",)])
        if "SELECT 1 FROM twitch_streamers WHERE twitch_login = ?" in normalized_sql:
            return _FetchOneResult((1,))
        raise AssertionError(
            f"Unexpected SQL in test_scout_monitored_session_bootstrap: {normalized_sql}"
        )

    def commit(self) -> None:
        self.commits += 1


class _MutableMonitoredScoutConnection:
    def __init__(self, monitored_logins: list[str]) -> None:
        self.commits = 0
        self.monitored_logins = list(monitored_logins)

    def execute(self, sql: str, params=(), *args, **kwargs):
        normalized_sql = " ".join(str(sql).split())
        normalized_sql = normalized_sql.replace("%s", "?")
        if "SELECT twitch_login FROM twitch_streamers WHERE is_monitored_only = 1" in normalized_sql:
            return _FetchAllResult([(login,) for login in self.monitored_logins])
        if "SELECT 1 FROM twitch_streamers WHERE twitch_login = ?" in normalized_sql:
            login = str(params[0]).lower()
            return _FetchOneResult((1,)) if login in self.monitored_logins else _FetchOneResult(None)
        if "UPDATE twitch_stream_sessions" in normalized_sql:
            return _FetchOneResult(None)
        if "DELETE FROM twitch_live_state WHERE streamer_login = ?" in normalized_sql:
            return _FetchOneResult(None)
        raise AssertionError(
            f"Unexpected SQL in test_scout_monitored_session_bootstrap: {normalized_sql}"
        )

    def commit(self) -> None:
        self.commits += 1


class _ScoutApi:
    async def get_streams_for_game(self, *, game_id, game_name, language, limit):
        return [
            {
                "id": "stream-1",
                "user_id": "1001",
                "user_login": "mewgles",
                "game_name": "Deadlock",
                "viewer_count": 42,
                "title": "Grinding ranked",
                "language": "de",
                "tags": ["ranked"],
            }
        ]


class _SequencedScoutApi:
    def __init__(self, batches: list[list[dict[str, object]]]) -> None:
        self._batches = list(batches)
        self._index = 0

    async def get_streams_for_game(self, *, game_id, game_name, language, limit):
        if self._index < len(self._batches):
            batch = self._batches[self._index]
            self._index += 1
            return batch
        return self._batches[-1] if self._batches else []


class _ScoutHarness:
    def __init__(self) -> None:
        self.bot = SimpleNamespace(wait_until_ready=self._wait_until_ready)
        self.api = _ScoutApi()
        self._category_id = "509658"
        self._target_game_name = "Deadlock"
        self.calls: list[tuple[str, object]] = []
        self._twitch_chat_bot = SimpleNamespace(
            set_monitored_channels=self._set_monitored_channels,
            join_channels=self._join_channels,
        )

    async def _wait_until_ready(self) -> None:
        return None

    async def _ensure_stream_session(self, *, login, stream, previous_state, twitch_user_id):
        self.calls.append(("ensure_session", login))
        return 123

    async def _join_channels(self, channels):
        self.calls.append(("join_channels", list(channels)))
        return len(channels)

    def _set_monitored_channels(self, channels):
        self.calls.append(("set_monitored_channels", list(channels)))

    async def _ensure_category_id(self):
        return self._category_id

    async def _prime_monitored_only_sessions(self, *, streams, logins):
        return await TwitchBaseCog._prime_monitored_only_sessions(
            self,
            streams=streams,
            logins=logins,
        )


class _HealingScoutHarness(_ScoutHarness):
    def __init__(self) -> None:
        super().__init__()
        self._twitch_chat_bot = SimpleNamespace(
            _monitored_streamers=set(),
            is_channel_subscription_ready=lambda login: False,
            set_monitored_channels=self._set_monitored_channels,
            join_channels=self._join_channels,
        )

    async def _ensure_stream_session(self, *, login, stream, previous_state, twitch_user_id):
        raise AssertionError("Existing monitored channels must not prime a new session")


class _RemovalScoutHarness(_ScoutHarness):
    def __init__(self, api) -> None:
        super().__init__()
        self.api = api
        self._twitch_chat_bot = SimpleNamespace(
            _monitored_streamers={"mewgles"},
            is_channel_subscription_ready=lambda login: False,
            set_monitored_channels=self._set_monitored_channels,
            join_channels=self._join_channels,
            part_channels=self._part_channels,
        )

    async def _part_channels(self, channels):
        self.calls.append(("part_channels", list(channels)))
        return len(channels)

    async def _ensure_stream_session(self, *, login, stream, previous_state, twitch_user_id):
        raise AssertionError("Removal harness must not prime a new session")


class ScoutMonitoredSessionBootstrapTests(unittest.IsolatedAsyncioTestCase):
    async def test_scout_primes_session_before_joining_new_monitored_channel(self) -> None:
        harness = _ScoutHarness()
        conn = _ScoutConnection()
        sleep_calls: list[float] = []

        async def _fake_sleep(delay: float) -> None:
            sleep_calls.append(delay)
            if len(sleep_calls) >= 2:
                raise asyncio.CancelledError

        class _TxCtx:
            def __init__(self, inner):
                self._inner = inner

            def __enter__(self):
                return self._inner

            def __exit__(self, exc_type, exc, tb):
                if exc_type is None:
                    self._inner.commit()
                return False

        with patch(
            "bot.base.storage.transaction",
            side_effect=lambda: _TxCtx(conn),
        ), patch("bot.base.asyncio.sleep", side_effect=_fake_sleep):
            with (
                self.assertLogs("TwitchStreams", level="INFO") as captured,
                self.assertRaises(asyncio.CancelledError),
            ):
                await TwitchBaseCog._scout_deadlock_channels(harness)

        self.assertEqual(conn.commits, 1)
        self.assertEqual(
            conn.inserted,
            [("mewgles", "1001", conn.inserted[0][2])],
        )
        self.assertEqual(
            harness.calls,
            [
                ("ensure_session", "mewgles"),
                ("set_monitored_channels", ["mewgles"]),
                ("join_channels", ["mewgles"]),
            ],
        )
        self.assertTrue(
            any(
                "scout_cycle_summary" in entry
                and "flow_id=" in entry
                and "new_logins=" in entry
                for entry in captured.output
            )
        )

    async def test_scout_does_not_heal_existing_monitored_only_channel_missing_from_runtime(self) -> None:
        harness = _HealingScoutHarness()
        conn = _ExistingMonitoredScoutConnection()
        sleep_calls: list[float] = []

        async def _fake_sleep(delay: float) -> None:
            sleep_calls.append(delay)
            if len(sleep_calls) >= 2:
                raise asyncio.CancelledError

        class _TxCtx:
            def __init__(self, inner):
                self._inner = inner

            def __enter__(self):
                return self._inner

            def __exit__(self, exc_type, exc, tb):
                if exc_type is None:
                    self._inner.commit()
                return False

        with patch(
            "bot.base.storage.transaction",
            side_effect=lambda: _TxCtx(conn),
        ), patch("bot.base.asyncio.sleep", side_effect=_fake_sleep):
            with self.assertRaises(asyncio.CancelledError):
                await TwitchBaseCog._scout_deadlock_channels(harness)

        self.assertEqual(conn.commits, 1)
        self.assertEqual(harness.calls, [])

    async def test_scout_removes_monitored_only_channel_after_two_missed_polls(self) -> None:
        harness = _RemovalScoutHarness(
            _SequencedScoutApi(
                [
                    [],
                    [],
                ]
            )
        )
        conn = _MutableMonitoredScoutConnection(["mewgles"])
        sleep_calls: list[float] = []
        deleted_logins: list[str] = []

        async def _fake_sleep(delay: float) -> None:
            sleep_calls.append(delay)
            if len(sleep_calls) >= 3:
                raise asyncio.CancelledError

        class _TxCtx:
            def __init__(self, inner):
                self._inner = inner

            def __enter__(self):
                return self._inner

            def __exit__(self, exc_type, exc, tb):
                if exc_type is None:
                    self._inner.commit()
                return False

        def _delete_streamer(_, login):
            deleted_logins.append(str(login))
            conn.monitored_logins = [item for item in conn.monitored_logins if item != login]

        with patch(
            "bot.base.storage.transaction",
            side_effect=lambda: _TxCtx(conn),
        ), patch(
            "bot.base.storage.delete_streamer",
            side_effect=_delete_streamer,
        ), patch("bot.base.asyncio.sleep", side_effect=_fake_sleep):
            with self.assertRaises(asyncio.CancelledError):
                await TwitchBaseCog._scout_deadlock_channels(harness)

        self.assertGreaterEqual(conn.commits, 2)
        self.assertEqual(deleted_logins, ["mewgles"])
        self.assertEqual(harness.calls, [("part_channels", ["mewgles"])])


if __name__ == "__main__":
    unittest.main()
