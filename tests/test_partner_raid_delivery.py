from __future__ import annotations

import unittest
from unittest.mock import AsyncMock, Mock

from bot.raid.services.partner_raid_delivery import (
    PartnerRaidDeliveryConfig,
    PartnerRaidDeliveryDependencies,
    PartnerRaidDeliveryService,
    PartnerRaidDeliveryRequest,
    plan_partner_raid_delivery,
)


class PartnerRaidDeliveryTests(unittest.TestCase):
    def test_viewer_wording_switches_between_singular_and_plural(self) -> None:
        singular = plan_partner_raid_delivery(
            PartnerRaidDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                to_broadcaster_id="9009",
                viewer_count=1,
                received_raid_count=7,
            )
        )
        plural = plan_partner_raid_delivery(
            PartnerRaidDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                to_broadcaster_id="9009",
                viewer_count=2,
                received_raid_count=7,
            )
        )

        self.assertEqual(singular.status, "ready")
        self.assertEqual(singular.viewer_word, "Viewer")
        self.assertIn("mit 1 Viewer geraidet", singular.message or "")
        self.assertIn("Raid Nr. 7", singular.message or "")

        self.assertEqual(plural.status, "ready")
        self.assertEqual(plural.viewer_word, "Viewern")
        self.assertIn("mit 2 Viewern geraidet", plural.message or "")
        self.assertIn("Raid Nr. 7", plural.message or "")

    def test_preserves_configured_delay_metadata(self) -> None:
        plan = plan_partner_raid_delivery(
            PartnerRaidDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                to_broadcaster_id="9009",
                viewer_count=3,
                received_raid_count=12,
            ),
            config=PartnerRaidDeliveryConfig(delay_seconds=8.5),
        )

        self.assertEqual(plan.status, "ready")
        self.assertEqual(plan.delay_seconds, 8.5)
        self.assertIn("delay_elapsed", plan.prerequisites)

    def test_blocks_when_chat_bot_is_unavailable(self) -> None:
        plan = plan_partner_raid_delivery(
            PartnerRaidDeliveryRequest(
                from_broadcaster_login="source_login",
                to_broadcaster_login="target_login",
                to_broadcaster_id="9009",
                viewer_count=3,
                received_raid_count=12,
                chat_bot_available=False,
            )
        )

        self.assertEqual(plan.status, "blocked")
        self.assertEqual(plan.reason, "chat_bot_unavailable")
        self.assertIsNone(plan.message)
        self.assertIsNone(plan.viewer_word)
        self.assertIn("chat_bot_available", plan.prerequisites)


class PartnerRaidDeliveryServiceTests(unittest.IsolatedAsyncioTestCase):
    class _CustomAwaitable:
        def __init__(self, value) -> None:
            self._value = value

        def __await__(self):
            async def _inner():
                return self._value

            return _inner().__await__()

    async def test_rebinds_chat_bot_after_delay_before_sending(self) -> None:
        initial_bot = object()
        replacement_bot = object()
        state = {"bot": initial_bot}
        join_calls: list[object] = []
        send_calls: list[object] = []

        async def sleep(_seconds: float) -> None:
            state["bot"] = replacement_bot

        async def join_chat_channel(chat_bot, channel_login: str, channel_id: str | None) -> None:
            self.assertEqual(channel_login, "target_login")
            self.assertEqual(channel_id, "9009")
            join_calls.append(chat_bot)

        async def send_chat_message(chat_bot, channel, message: str, source: str) -> bool | None:
            self.assertEqual(source, "partner_raid")
            self.assertIn("@target_login", message)
            send_calls.append(chat_bot)
            return True

        service = PartnerRaidDeliveryService(
            PartnerRaidDeliveryDependencies(
                get_chat_bot=lambda: state["bot"],
                count_received_network_raids=lambda _target_id: 4,
                lookup_outbound_chat_suppression=lambda **_kwargs: None,
                join_chat_channel=join_chat_channel,
                send_chat_message=send_chat_message,
                sleep=sleep,
                logger=Mock(),
            )
        )

        await service.send_partner_raid_message(
            from_broadcaster_login="source_login",
            to_broadcaster_login="target_login",
            to_broadcaster_id="9009",
            viewer_count=5,
        )

        self.assertEqual(join_calls, [initial_bot, replacement_bot])
        self.assertEqual(send_calls, [replacement_bot])

    async def test_missing_send_capability_is_logged_as_debug_skip(self) -> None:
        logger = Mock()
        join_chat_channel = AsyncMock()
        send_chat_message = AsyncMock(return_value=None)
        service = PartnerRaidDeliveryService(
            PartnerRaidDeliveryDependencies(
                get_chat_bot=lambda: object(),
                count_received_network_raids=lambda _target_id: 2,
                lookup_outbound_chat_suppression=lambda **_kwargs: None,
                join_chat_channel=join_chat_channel,
                send_chat_message=send_chat_message,
                sleep=AsyncMock(),
                logger=logger,
            )
        )

        await service.send_partner_raid_message(
            from_broadcaster_login="source_login",
            to_broadcaster_login="target_login",
            to_broadcaster_id="9009",
            viewer_count=5,
        )

        logger.warning.assert_not_called()
        logger.debug.assert_called()

    async def test_accepts_custom_awaitables_for_join_and_send(self) -> None:
        chat_bot = object()
        join_calls: list[object] = []
        send_calls: list[object] = []

        service = PartnerRaidDeliveryService(
            PartnerRaidDeliveryDependencies(
                get_chat_bot=lambda: chat_bot,
                count_received_network_raids=lambda _target_id: 3,
                lookup_outbound_chat_suppression=lambda **_kwargs: None,
                join_chat_channel=lambda chat_bot, _channel_login, _channel_id: (
                    join_calls.append(chat_bot) or self._CustomAwaitable(True)
                ),
                send_chat_message=lambda chat_bot, _channel, _message, _source: (
                    send_calls.append(chat_bot) or self._CustomAwaitable(True)
                ),
                sleep=AsyncMock(),
                logger=Mock(),
            )
        )

        await service.send_partner_raid_message(
            from_broadcaster_login="source_login",
            to_broadcaster_login="target_login",
            to_broadcaster_id="9009",
            viewer_count=5,
        )

        self.assertEqual(len(join_calls), 1)
        self.assertEqual(len(send_calls), 1)


if __name__ == "__main__":
    unittest.main()
