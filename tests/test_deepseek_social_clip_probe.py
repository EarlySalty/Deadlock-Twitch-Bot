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
          "main_moment": "Der Gegner lernt mitten im Clip dazu.",
          "content_angle": "Comedy",
          "title_options": ["Er hat gelernt, Digga!", "Wie verliert man so schnell?", "Der Gegner hat einfach gelernt", "Deadlock bestraft sofort", "Dieser Hero macht fertig", "Wenn dein Plan live zerlegt wird", "Der Chat wollte den Clip", "Er klippt es selbst", "Deadlock Fail mit Ansage", "Der schnellste Lernmoment"],
          "best_title": "Er hat gelernt, Digga! #Deadlock 😳",
          "best_title_reason": "Der Satz ist der Punchline-Moment des Clips.",
          "captions": {
            "tiktok": ["Er hat wirklich gelernt.", "Wer kennt den Moment?", "Clip oder Skill Issue?"],
            "instagram": ["Manchmal lernt der Gegner schneller.", "Der Chat wusste sofort Bescheid.", "Deadlock nimmt keine Ruecksicht."],
            "youtube": ["Deadlock Fail mit Punchline.", "earlysalty merkt, dass der Gegner gelernt hat.", "Kurzer Deadlock-Clip mit sauberem Comedy-Moment."]
          },
          "hashtag_groups": {
            "game_specific": ["#deadlock", "#deadlockgame", "#deadlockclip", "#valve", "#heroshooter"],
            "gaming_clip": ["#gaming", "#twitchclip", "#fail", "#moba", "#streamer"],
            "german": ["#deutsch", "#gamingdeutschland", "#zocken"]
          },
          "pin_comments": ["War das Skill Issue?", "Welcher Hero tiltet euch?", "Hat er wirklich gelernt?"],
          "calls_to_action": ["Schreib den Hero in die Kommentare.", "Folgen fuer mehr Deadlock-Clips.", "Speichern, wenn du den Moment kennst."],
          "video_hooks": ["Er dachte, er hat ihn gelesen.", "Der Gegner hat einfach gelernt.", "So schnell kippt ein Deadlock-Clip."],
          "youtube": {"title": "Unfassbar: Er hat gelernt #Deadlock 😳", "title_options": ["Unfassbar: Er hat gelernt #Deadlock 😳", "Der schnellste Throw"], "description": "Kurz und stark. #Deadlock #Fail", "hashtags": ["Deadlock", "gaming", "clip", "fail"]},
          "tiktok": {"title": "Deadlock clip", "title_options": ["Deadlock clip"], "description": "Sauberer Fight.", "hashtags": ["Deadlock", "twitch"]},
          "instagram": {"title": "Deadlock Highlight", "title_options": ["Deadlock Highlight"], "description": "Der Moment sitzt.", "hashtags": ["Deadlock", "reels"]}
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

        self.assertEqual(response.youtube.title, "Er hat gelernt")
        self.assertEqual(response.youtube.title_options, ("Er hat gelernt", "Der schnellste Throw"))
        self.assertEqual(response.youtube.description, "Kurz und stark.")
        self.assertEqual(response.youtube.hashtags, ("#Deadlock", "#gaming", "#fail"))
        self.assertEqual(response.raw_payload["best_title"], "Er hat gelernt, Digga! #Deadlock 😳")
        self.assertEqual(response.provider, "deepseek")
        self.assertEqual(client.completions.kwargs["response_format"], {"type": "json_object"})
        self.assertEqual(client.completions.kwargs["max_tokens"], 6500)
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
                    "editorial": {
                        "main_moment": "Moment",
                        "content_angle": "Comedy",
                        "title_options": ["Hook 1 #Deadlock 😳", "Hook 2"],
                        "best_title": "Hook 1 #Deadlock 😳",
                        "best_title_reason": "Trifft den Moment.",
                        "captions": {"tiktok": ["Kurz"], "instagram": [], "youtube": []},
                        "hashtag_groups": {
                            "game_specific": ["#Deadlock"],
                            "gaming_clip": ["#fail"],
                            "german": ["#deutsch"],
                        },
                        "pin_comments": ["War das Skill Issue?"],
                        "calls_to_action": ["Kommentier den Hero."],
                        "video_hooks": ["Er dachte, er gewinnt."],
                    },
                    "youtube": {
                        "title": "YT #Deadlock 😳",
                        "description": "D #Deadlock",
                        "hashtags": ["#Deadlock"],
                    },
                    "tiktok": {
                        "title": "TT",
                        "title_options": ["TT", "TikTok Hook"],
                        "description": "D",
                        "hashtags": ["#Deadlock"],
                    },
                    "instagram": {"title": "IG", "description": "D", "hashtags": ["#Deadlock"]},
                },
            }
        )
        self.assertIn("corrected", markdown)
        self.assertIn("Title: YT", markdown)
        self.assertNotIn("Title: YT #Deadlock", markdown)
        self.assertNotIn("D #Deadlock", markdown)
        self.assertIn("Bester Titel: Hook 1", markdown)
        self.assertNotIn("Bester Titel: Hook 1 #Deadlock", markdown)
        self.assertIn("- War das Skill Issue?", markdown)
        self.assertIn("- TikTok Hook", markdown)

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
