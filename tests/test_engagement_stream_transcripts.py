from __future__ import annotations

import unittest
from datetime import UTC, datetime, timedelta
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.engagement.background import _transcribe_capture
from bot.engagement.conversation import ConversationTurn
from bot.engagement.minimax_chat import ChatResponse
from bot.engagement.pipeline import (
    Decision,
    EngagementPipeline,
    EngagementSettings,
    IncomingMessage,
)
from bot.engagement.stream_transcripts import (
    StreamTranscriptSegment,
    segments_to_prompt_fragment,
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


class _FakeMiniMax:
    def __init__(self) -> None:
        self.system_prompt = ""

    async def generate(self, *, system_prompt: str, history: list, max_output_tokens: int):
        del history, max_output_tokens
        self.system_prompt = system_prompt
        return ChatResponse(
            text="gegen haze frueh metal skin einplanen.",
            model="MiniMax-M3",
            prompt_tokens=100,
            completion_tokens=12,
            latency_ms=50,
        )


class EngagementStreamTranscriptTests(unittest.IsolatedAsyncioTestCase):
    def test_segments_to_prompt_fragment_keeps_recent_voice_context(self) -> None:
        ended_at = datetime(2026, 5, 25, 18, 30, tzinfo=UTC)
        fragment = segments_to_prompt_fragment(
            [
                StreamTranscriptSegment(
                    channel_login="earlysalty",
                    started_at=ended_at - timedelta(seconds=40),
                    ended_at=ended_at,
                    text="Streamer fragt nach dem neuen Patch und Kelvin Build.",
                    engine="openai_api",
                    model="gpt-4o-mini-transcribe",
                )
            ],
            max_chars=500,
        )

        self.assertIn("Stream-Audio-Kontext", fragment)
        self.assertIn("Kelvin Build", fragment)

    async def test_pipeline_injects_voice_context_into_minimax_prompt(self) -> None:
        conversation = _FakeConversation()
        minimax = _FakeMiniMax()
        pipeline = EngagementPipeline(
            conversation=conversation,
            rhythm=_FakeRhythm(),
            minimax=minimax,
        )
        pipeline._load_settings = AsyncMock(  # type: ignore[method-assign]
            return_value=EngagementSettings(
                channel_login="earlysalty",
                enabled=True,
                steam_id=None,
                persona_override=None,
                tabu_topics=[],
            )
        )
        pipeline._is_opted_out = AsyncMock(return_value=False)  # type: ignore[method-assign]

        segment = StreamTranscriptSegment(
            channel_login="earlysalty",
            started_at=datetime.now(UTC) - timedelta(seconds=45),
            ended_at=datetime.now(UTC),
            text="Ich habe gerade Probleme gegen Haze im Midgame.",
            engine="openai_api",
            model="gpt-4o-mini-transcribe",
        )

        with (
            patch(
                "bot.core.partner_utils.is_operational_partner_channel",
                return_value=True,
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
                new=AsyncMock(return_value=[segment]),
            ),
        ):
            result = await pipeline.handle(
                IncomingMessage(
                    channel_login="earlysalty",
                    twitch_user_id="42",
                    twitch_login="viewer",
                    content="was kann man da machen?",
                    message_id="msg-2",
                )
            )

        self.assertEqual(result.decision, Decision.SPOKE)
        self.assertIn("Stream-Audio-Kontext", minimax.system_prompt)
        self.assertIn("Probleme gegen Haze", minimax.system_prompt)
        self.assertEqual(conversation.assistant_turns, ["gegen haze frueh metal skin einplanen."])

    async def test_transcribe_capture_persists_clean_segment_and_cleans_temp_file(self) -> None:
        cleanup = unittest.mock.Mock()
        capture_result = SimpleNamespace(
            media_path="/tmp/fake-audio.ts",
            actual_duration_seconds=42.0,
            requested_duration_seconds=45,
            cleanup=cleanup,
        )
        transcript = SimpleNamespace(
            text="  Push kommt   gleich mid.  ",
            duration_seconds=40.0,
            engine="openai_api",
            model="gpt-4o-mini-transcribe",
        )

        with (
            patch(
                "bot.community.voice_reaction.audio_capture.capture",
                new=AsyncMock(return_value=capture_result),
            ) as capture,
            patch(
                "bot.social_media.transcription.whisper.transcribe_clip",
                new=AsyncMock(return_value=transcript),
            ) as transcribe,
            patch("bot.engagement.background.append_segment", new=AsyncMock()) as append_segment,
        ):
            await _transcribe_capture("earlysalty", object())

        capture.assert_awaited_once()
        transcribe.assert_awaited_once_with("/tmp/fake-audio.ts", engine=unittest.mock.ANY)
        append_segment.assert_awaited_once()
        saved_segment = append_segment.await_args.args[0]
        self.assertEqual(saved_segment.channel_login, "earlysalty")
        self.assertEqual(saved_segment.text, "Push kommt gleich mid.")
        self.assertEqual(saved_segment.engine, "openai_api")
        self.assertEqual(saved_segment.model, "gpt-4o-mini-transcribe")
        cleanup.assert_called_once()


if __name__ == "__main__":
    unittest.main()
