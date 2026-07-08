from __future__ import annotations

import unittest
from types import SimpleNamespace
from unittest import mock

from bot.social_media.llm.base import LLMProviderUnavailable, LLMRequest
from bot.social_media.llm.deepseek import DeepSeekProvider
from scripts.probe_social_clip_deepseek import is_url, render_markdown, safe_stem


class _FakeCompletions:
    def __init__(self) -> None:
        self.kwargs = None

    async def create(self, **kwargs):
        self.kwargs = kwargs
        content = """
        {
          "youtube": {"title": "Big Deadlock Moment", "description": "Kurz und stark.", "hashtags": ["Deadlock", "gaming"]},
          "tiktok": {"title": "Deadlock clip", "description": "Sauberer Fight.", "hashtags": ["Deadlock", "twitch"]},
          "instagram": {"title": "Deadlock Highlight", "description": "Der Moment sitzt.", "hashtags": ["Deadlock", "reels"]}
        }
        """
        usage = SimpleNamespace(
            prompt_tokens=1000,
            prompt_cache_hit_tokens=100,
            prompt_cache_miss_tokens=900,
            completion_tokens=200,
            total_tokens=1200,
        )
        return SimpleNamespace(
            choices=[SimpleNamespace(message=SimpleNamespace(content=content))],
            usage=usage,
        )


class _FakeClient:
    def __init__(self) -> None:
        self.completions = _FakeCompletions()
        self.chat = SimpleNamespace(completions=self.completions)


class DeepSeekProviderTests(unittest.IsolatedAsyncioTestCase):
    def test_uses_fireworks_alias_and_endpoint(self) -> None:
        calls = []

        class FakeAsyncOpenAI:
            def __init__(self, **kwargs) -> None:
                calls.append(kwargs)

        with (
            mock.patch.dict("os.environ", {"FIREWORK_API_KEY": "test-key"}, clear=True),
            mock.patch.dict(
                "sys.modules",
                {"openai": SimpleNamespace(AsyncOpenAI=FakeAsyncOpenAI)},
            ),
        ):
            provider = DeepSeekProvider()

        self.assertEqual(provider.model, "accounts/fireworks/models/deepseek-v4-pro")
        self.assertEqual(provider.base_url, "https://api.fireworks.ai/inference/v1")
        self.assertEqual(
            calls,
            [{"api_key": "test-key", "base_url": "https://api.fireworks.ai/inference/v1"}],
        )

    def test_legacy_deepseek_key_is_not_enough_for_fireworks_default(self) -> None:
        with mock.patch.dict("os.environ", {"DEEPSEEK_API_KEY": "legacy-key"}, clear=True):
            with self.assertRaisesRegex(
                LLMProviderUnavailable,
                "FIREWORKS_API_KEY/FIREWORK_API_KEY not set",
            ):
                DeepSeekProvider()

    async def test_generate_requests_json_and_parses_platforms(self) -> None:
        client = _FakeClient()
        provider = DeepSeekProvider(client=client, model="deepseek-v4-pro")

        response = await provider.generate(LLMRequest(transcript="Pocket gewinnt den Fight."))

        self.assertEqual(response.youtube.title, "Big Deadlock Moment")
        self.assertEqual(response.provider, "deepseek")
        self.assertEqual(client.completions.kwargs["response_format"], {"type": "json_object"})
        self.assertNotIn("extra_body", client.completions.kwargs)


class SocialClipProbeHelpersTests(unittest.TestCase):
    def test_safe_stem_and_url_detection(self) -> None:
        self.assertTrue(is_url("https://clips.twitch.tv/foo"))
        self.assertFalse(is_url("/tmp/foo.mp4"))
        self.assertEqual(safe_stem("https://clips.twitch.tv/Foo Bar!?"), "Foo-Bar")

    def test_render_markdown_contains_transcript_and_titles(self) -> None:
        markdown = render_markdown(
            {
                "source": "clip",
                "transcript": {"text": "raw"},
                "correction": {"text": "corrected"},
                "deepseek": {
                    "youtube": {"title": "YT", "description": "D", "hashtags": ["#Deadlock"]},
                    "tiktok": {"title": "TT", "description": "D", "hashtags": ["#Deadlock"]},
                    "instagram": {"title": "IG", "description": "D", "hashtags": ["#Deadlock"]},
                },
            }
        )
        self.assertIn("corrected", markdown)
        self.assertIn("Title: YT", markdown)

    def test_render_markdown_keeps_transcript_without_deepseek(self) -> None:
        markdown = render_markdown(
            {
                "source": "clip",
                "transcript": {"text": "raw"},
                "correction": {"text": "corrected"},
                "deepseek_error": "FIREWORKS_API_KEY/FIREWORK_API_KEY not set",
            }
        )
        self.assertIn("corrected", markdown)
        self.assertIn("FIREWORKS_API_KEY/FIREWORK_API_KEY not set", markdown)


if __name__ == "__main__":
    unittest.main()
