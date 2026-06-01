"""Engagement-Layer Partner-Gate.

Engagement darf ausschliesslich in operativ aktiven Partner-Channels feuern.
Departnerte / monitored-only / opt-out Channels muessen schweigen, selbst wenn
``twitch_engagement_settings.enabled`` (verwaist) noch TRUE ist.

Spiegelt das Gate, das Moderation/Promos/Join bereits via
``bot.core.partner_utils.is_operational_partner_channel`` nutzen.
"""

from __future__ import annotations

import unittest
from datetime import UTC, datetime
from unittest.mock import AsyncMock, patch

from bot.engagement.conversation import ConversationTurn
from bot.engagement.minimax_chat import ChatResponse
from bot.engagement.pipeline import (
    Decision,
    EngagementPipeline,
    EngagementSettings,
    IncomingMessage,
)


class _FakeConversation:
    def __init__(self) -> None:
        self.assistant_turns: list[str] = []

    async def append_user_turn(self, **_kwargs) -> None:
        return None

    async def append_assistant_turn(self, *, content: str, **_kwargs) -> None:
        self.assistant_turns.append(content)

    async def load_recent_buffer(self, **_kwargs) -> list[ConversationTurn]:
        return [
            ConversationTurn(
                role="user",
                twitch_user_id="42",
                twitch_login="viewer",
                content="was baust du gegen haze?",
                message_id="msg-1",
                ts=datetime.now(UTC),
            )
        ]


class _FakeRhythm:
    def note_user_post(self, _channel_login: str) -> None:
        return None

    def anti_flood_ok(self, _channel_login: str, *, now: datetime) -> bool:
        return True

    def anti_burst_ok(self, _channel_login: str, *, now: datetime) -> bool:
        return True

    def note_bot_post(self, _channel_login: str, *, now: datetime) -> None:
        return None


class _SpyMiniMax:
    def __init__(self) -> None:
        self.called = False

    async def generate(self, *, system_prompt: str, history: list, max_output_tokens: int):
        del system_prompt, history, max_output_tokens
        self.called = True
        return ChatResponse(
            text="gegen haze frueh metal skin einplanen.",
            model="MiniMax-M3",
            prompt_tokens=100,
            completion_tokens=12,
            latency_ms=50,
        )


def _make_pipeline(minimax: _SpyMiniMax) -> EngagementPipeline:
    pipeline = EngagementPipeline(
        conversation=_FakeConversation(),
        rhythm=_FakeRhythm(),
        minimax=minimax,
    )
    pipeline._load_settings = AsyncMock(  # type: ignore[method-assign]
        return_value=EngagementSettings(
            channel_login="somechannel",
            enabled=True,
            steam_id=None,
            persona_override=None,
            tabu_topics=[],
        )
    )
    pipeline._is_opted_out = AsyncMock(return_value=False)  # type: ignore[method-assign]
    return pipeline


def _incoming(channel: str) -> IncomingMessage:
    return IncomingMessage(
        channel_login=channel,
        twitch_user_id="42",
        twitch_login="viewer",
        content="was kann man da machen?",
        message_id="msg-2",
    )


class EngagementPartnerGateTests(unittest.IsolatedAsyncioTestCase):
    async def test_non_partner_channel_is_disabled_and_skips_model(self) -> None:
        """Kein aktiver Partner -> DISABLED, MiniMax wird nicht angefragt,
        obwohl enabled=TRUE und der Channel live Deadlock streamt."""
        minimax = _SpyMiniMax()
        pipeline = _make_pipeline(minimax)
        with (
            patch(
                "bot.core.partner_utils.is_operational_partner_channel",
                return_value=False,
            ),
            patch(
                "bot.engagement.stream_state.is_streaming_deadlock",
                new=AsyncMock(return_value=True),
            ),
        ):
            result = await pipeline.handle(_incoming("monitoredguy"))

        self.assertEqual(result.decision, Decision.DISABLED)
        self.assertFalse(minimax.called)

    async def test_active_partner_channel_reaches_model(self) -> None:
        """Operativ aktiver Partner + live + enabled -> Gate laesst durch,
        MiniMax wird angefragt (SPOKE)."""
        minimax = _SpyMiniMax()
        pipeline = _make_pipeline(minimax)
        with (
            patch(
                "bot.core.partner_utils.is_operational_partner_channel",
                return_value=True,
            ),
            patch(
                "bot.engagement.stream_state.is_streaming_deadlock",
                new=AsyncMock(return_value=True),
            ),
            patch("bot.engagement.pipeline.sample_tone", new=AsyncMock(side_effect=RuntimeError)),
            patch(
                "bot.engagement.pipeline.load_open_threads_for_user",
                new=AsyncMock(return_value=[]),
            ),
            patch(
                "bot.engagement.pipeline.known_regulars_currently_lurking",
                new=AsyncMock(return_value=[]),
            ),
            patch("bot.engagement.pipeline.get_match_state", new=AsyncMock(return_value=None)),
            patch(
                "bot.engagement.pipeline.load_recent_segments",
                new=AsyncMock(return_value=[]),
            ),
        ):
            result = await pipeline.handle(_incoming("earlysalty"))

        self.assertEqual(result.decision, Decision.SPOKE)
        self.assertTrue(minimax.called)


if __name__ == "__main__":
    unittest.main()
