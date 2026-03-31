import contextlib
import unittest
from datetime import UTC, datetime
from unittest.mock import patch

from bot.monitoring.sessions_mixin import _SessionsMixin


class _EnsureSessionHarness(_SessionsMixin):
    def __init__(self) -> None:
        self.exp_start_called = False

    def _get_active_sessions_cache(self) -> dict[str, int]:
        return {}

    def _lookup_open_session_id(self, login: str) -> int | None:
        del login
        return None

    async def _fetch_followers_total_safe(self, **kwargs) -> int | None:
        del kwargs
        return None

    def _extract_stream_start(self, stream: dict | None, previous_state: dict) -> str | None:
        del stream, previous_state
        return None

    def _start_stream_session(self, **kwargs) -> int | None:
        del kwargs
        return None

    def _exp_on_session_start(self, **kwargs) -> int | None:
        del kwargs
        self.exp_start_called = True
        return 1


class _ReadonlyFinalizeConn:
    def execute(self, sql: str, params=()):
        normalized = " ".join(str(sql).split())
        if "SELECT * FROM twitch_stream_sessions WHERE id = %s" in normalized:
            return type(
                "_Cursor",
                (),
                {
                    "fetchone": lambda self: {
                        "id": 7,
                        "started_at": "2026-03-31T00:00:00+00:00",
                        "start_viewers": 1,
                        "end_viewers": 1,
                        "peak_viewers": 1,
                        "avg_viewers": 1.0,
                        "samples": 0,
                        "followers_start": None,
                    }
                },
            )()
        if "SELECT minutes_from_start, viewer_count FROM twitch_session_viewers" in normalized:
            return type("_Cursor", (), {"fetchall": lambda self: []})()
        if "FROM twitch_session_chatters" in normalized:
            return type("_Cursor", (), {"fetchone": lambda self: None})()
        if "FROM twitch_live_state WHERE streamer_login = %s" in normalized:
            return type(
                "_Cursor",
                (),
                {
                    "fetchone": lambda self: {
                        "twitch_user_id": "u1",
                        "last_game": "Deadlock",
                        "had_deadlock_in_session": 1,
                    }
                },
            )()
        raise AssertionError(normalized)


class _FinalizeFailureHarness(_SessionsMixin):
    def __init__(self) -> None:
        self._active_sessions = {"foo": 7}
        self.exp_finalize_called = False
        self.warning_cleared = False

    def _get_active_sessions_cache(self) -> dict[str, int]:
        return self._active_sessions

    def _get_exp_session_id(self, login: str) -> int | None:
        del login
        return 99

    def _exp_on_session_finalize(self, **kwargs) -> None:
        del kwargs
        self.exp_finalize_called = True

    def _clear_session_followers_user_fallback_warning(self, login: str) -> None:
        del login
        self.warning_cleared = True

    def _get_target_game_lower(self) -> str:
        return "deadlock"

    def _parse_dt(self, value: str | None) -> datetime | None:
        if not value:
            return None
        parsed = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            return parsed.replace(tzinfo=UTC)
        return parsed.astimezone(UTC)

    async def _fetch_followers_total_safe(self, **kwargs) -> int | None:
        del kwargs
        return None


class MonitoringSessionConsistencyTests(unittest.IsolatedAsyncioTestCase):
    async def test_exp_session_start_is_skipped_when_primary_session_creation_fails(self) -> None:
        harness = _EnsureSessionHarness()

        result = await harness._ensure_stream_session(
            login="foo",
            stream={"id": "stream-1", "viewer_count": 1},
            previous_state={},
            twitch_user_id="123",
        )

        self.assertIsNone(result)
        self.assertFalse(harness.exp_start_called)

    async def test_finalize_write_failure_keeps_active_session_and_skips_side_effects(self) -> None:
        harness = _FinalizeFailureHarness()

        with (
            patch(
                "bot.monitoring.sessions_mixin.storage.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_ReadonlyFinalizeConn()),
            ),
            patch(
                "bot.monitoring.sessions_mixin.storage.transaction",
                side_effect=RuntimeError("db write failed"),
            ),
        ):
            finalized = await harness._finalize_stream_session(login="foo", reason="offline")

        self.assertFalse(finalized)
        self.assertEqual(harness._active_sessions, {"foo": 7})
        self.assertFalse(harness.exp_finalize_called)
        self.assertFalse(harness.warning_cleared)


if __name__ == "__main__":
    unittest.main()
