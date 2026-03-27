from __future__ import annotations

import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock

from bot.raid.services.recruitment_messaging import (
    RecruitmentMessagingDependencies,
    RecruitmentMessagingService,
)


class RecruitmentMessagingTests(unittest.IsolatedAsyncioTestCase):
    def _make_service(self, **overrides):
        kwargs = {
            "create_twitch_api": lambda session: SimpleNamespace(session=session),
            "resolve_bot_oauth_context": AsyncMock(
                return_value=("bot-token", "bot-id", {"moderator:read:followers"})
            ),
            "resolve_valid_token": AsyncMock(return_value="streamer-token"),
            "get_followers_total_result": AsyncMock(
                return_value={"ok": True, "data": 123, "http_status": 200}
            ),
            "build_followers_runtime_state": MagicMock(return_value={"runtime": "state"}),
            "increment_counter": MagicMock(return_value=1),
            "log_followers_decision": MagicMock(),
            "next_flow_id": lambda prefix: f"{prefix}-1",
            "warn_user_scope_fallback_once": MagicMock(),
            "clear_user_scope_fallback_warning": MagicMock(),
            "get_chat_bot": MagicMock(return_value=SimpleNamespace()),
            "fetch_users": AsyncMock(return_value=[SimpleNamespace(id="9009")]),
            "lookup_outbound_chat_suppression": MagicMock(return_value=None),
            "join_chat_channel": AsyncMock(return_value=True),
            "follow_channel": AsyncMock(return_value=None),
            "send_chat_message": AsyncMock(return_value=True),
            "count_recent_raids": MagicMock(return_value=2),
            "count_confirmed_external_recruitment_raids": MagicMock(return_value=4),
            "schedule_external_target_ban_check": MagicMock(),
            "load_deadlock_stats": MagicMock(return_value=(10, 20)),
            "sleep": AsyncMock(),
        }
        kwargs.update(overrides)
        deps = RecruitmentMessagingDependencies(**kwargs)
        return RecruitmentMessagingService(deps)

    def test_parse_nonnegative_int(self) -> None:
        service = self._make_service()
        self.assertEqual(service.parse_nonnegative_int("4"), 4)
        self.assertEqual(service.parse_nonnegative_int(0), 0)
        self.assertIsNone(service.parse_nonnegative_int(-1))
        self.assertIsNone(service.parse_nonnegative_int("x"))

    async def test_resolve_recruitment_followers_total_uses_cached_value(self) -> None:
        service = self._make_service()
        payload = {"followers_total": 77}

        result = await service.resolve_recruitment_followers_total(
            login="target",
            target_id="9009",
            target_stream_data=payload,
            session=object(),
        )

        self.assertEqual(result, 77)
        self.assertEqual(payload["followers_total"], 77)
        service._deps.get_followers_total_result.assert_not_awaited()

    async def test_resolve_recruitment_followers_total_returns_none_without_session(self) -> None:
        service = self._make_service()

        result = await service.resolve_recruitment_followers_total(
            login="target",
            target_id="9009",
            target_stream_data={},
            session=None,
        )

        self.assertIsNone(result)
        service._deps.log_followers_decision.assert_called_once()

    async def test_resolve_recruitment_followers_total_falls_back_to_streamer_token(self) -> None:
        service = self._make_service(
            get_followers_total_result=AsyncMock(
                side_effect=[
                    {"ok": False, "error_code": "bot_denied", "http_status": 403},
                    {"ok": True, "data": 321, "http_status": 200},
                ]
            ),
        )

        result = await service.resolve_recruitment_followers_total(
            login="target",
            target_id="9009",
            target_stream_data={},
            session=object(),
        )

        self.assertEqual(result, 321)
        self.assertEqual(service._deps.get_followers_total_result.await_count, 2)

    async def test_send_recruitment_message_now_skips_on_suppression(self) -> None:
        service = self._make_service(
            lookup_outbound_chat_suppression=MagicMock(
                return_value={"reason_code": "channel_settings", "suppressed_until": "later"}
            )
        )

        result = await service.send_recruitment_message_now(
            from_broadcaster_login="source",
            to_broadcaster_login="target",
            target_stream_data={"user_id": "9009"},
            session=object(),
            chat_bot=SimpleNamespace(),
        )

        self.assertEqual(result.status, "blocked")
        self.assertEqual(result.reason, "outbound_chat_suppressed")
        service._deps.join_chat_channel.assert_not_awaited()
        service._deps.send_chat_message.assert_not_awaited()

    async def test_send_recruitment_message_now_sends_and_schedules_ban_check(self) -> None:
        service = self._make_service()

        result = await service.send_recruitment_message_now(
            from_broadcaster_login="source",
            to_broadcaster_login="target",
            target_stream_data={"user_id": "9009"},
            session=object(),
            chat_bot=SimpleNamespace(),
        )

        self.assertEqual(result.status, "sent")
        self.assertIsNotNone(result.message)
        self.assertIn("Dauersupport fuer @target", result.message or "")
        service._deps.join_chat_channel.assert_awaited_once()
        service._deps.send_chat_message.assert_awaited_once()
        service._deps.schedule_external_target_ban_check.assert_called_once_with(
            target_id="9009",
            target_login="target",
            source="recruitment",
        )

    async def test_send_recruitment_message_now_fetches_target_id_via_users(self) -> None:
        service = self._make_service()

        result = await service.send_recruitment_message_now(
            from_broadcaster_login="source",
            to_broadcaster_login="target",
            target_stream_data={},
            session=object(),
            chat_bot=SimpleNamespace(),
        )

        self.assertEqual(result.status, "sent")
        service._deps.fetch_users.assert_awaited_once()


if __name__ == "__main__":
    unittest.main()
