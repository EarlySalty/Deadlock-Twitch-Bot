from __future__ import annotations

import unittest

from bot.monitoring.embeds_mixin import _EmbedsMixin


class _DummyEmbeds(_EmbedsMixin):
    def __init__(self) -> None:
        self.bot = type("Bot", (), {"guilds": []})()
        self._notify_channel_id = 0
        self._referral_override = ""
        self._payload_override = None

    def _build_referral_url(self, login: str) -> str:
        if self._referral_override:
            return self._referral_override
        return f"https://www.twitch.tv/{login}"

    async def _ensure_live_ping_role(self, **kwargs):
        del kwargs
        return "<@&123456>", 123456

    def _render_live_announcement_payload(self, **kwargs):
        del kwargs
        return self._payload_override


class LiveAnnouncementIntegrationTests(unittest.IsolatedAsyncioTestCase):
    async def test_mentions_are_sanitized_and_allowed_mentions_are_restricted(
        self,
    ) -> None:
        dummy = _DummyEmbeds()
        dummy._payload_override = {
            "content": "@everyone <@&123456> ist live!",
            "embed": {
                "title": "Live",
                "description": "Beschreibung",
                "color": 0x9146FF,
                "fields": [],
                "author": {
                    "name": "LIVE: Tester",
                    "icon_mode": "none",
                    "link_enabled": False,
                },
                "footer": {"text": "Footer", "icon_mode": "none"},
                "thumbnail": {"mode": "none"},
                "image": {"use_stream_thumbnail": False, "custom_url": ""},
            },
            "button": {
                "enabled": True,
                "label": "Auf Twitch ansehen",
                "url": "https://evil.example",
            },
        }

        (
            content,
            _embed,
            _view,
            allowed_mentions,
            _token,
        ) = await dummy._build_live_announcement_message(
            login="tester",
            stream={"user_name": "Tester", "title": "Ranked", "viewer_count": 12},
            streamer_entry={},
            notify_channel=None,
        )

        self.assertIn("@\u200beveryone", content)
        self.assertFalse(allowed_mentions.everyone)
        self.assertFalse(allowed_mentions.users)
        self.assertNotEqual(allowed_mentions.roles, False)

    async def test_fallback_rendering_without_config_payload(self) -> None:
        dummy = _DummyEmbeds()
        dummy._payload_override = None

        (
            content,
            _embed,
            view,
            _allowed_mentions,
            _token,
        ) = await dummy._build_live_announcement_message(
            login="tester",
            stream={"user_name": "Tester", "title": "No Config", "viewer_count": 5},
            streamer_entry={"live_ping_enabled": 0},
            notify_channel=None,
        )

        self.assertIn("ist live", content)
        self.assertIsNotNone(view)
        button = view.children[0]
        self.assertEqual(getattr(button, "label", ""), "Auf Twitch ansehen")

    async def test_build_live_announcement_message_reuses_supplied_tracking_token(
        self,
    ) -> None:
        dummy = _DummyEmbeds()
        dummy._payload_override = None

        (
            _content,
            embed,
            _view,
            _allowed_mentions,
            tracking_token,
        ) = await dummy._build_live_announcement_message(
            login="tester",
            stream={
                "user_name": "Tester",
                "user_login": "tester",
                "title": "No Config",
                "game_name": "Deadlock",
                "viewer_count": 5,
                "thumbnail_url": "https://cdn.example/thumb-{width}x{height}.jpg",
                "started_at": "2026-03-15T11:30:00Z",
            },
            streamer_entry={"live_ping_enabled": 0},
            notify_channel=None,
            tracking_token="track-1",
        )

        self.assertEqual(tracking_token, "track-1")
        self.assertEqual(
            embed.to_dict()["image"]["url"],
            "https://cdn.example/thumb-1280x720.jpg?rand=90953515094fab84",
        )

    def test_config_rendering_uses_placeholders(self) -> None:
        class _ConfigDummy(_EmbedsMixin):
            def __init__(self) -> None:
                self.bot = type("Bot", (), {"guilds": []})()

            def _build_referral_url(self, login: str) -> str:
                return f"https://www.twitch.tv/{login}"

            def _load_live_announcement_config(self, login: str) -> dict:
                del login
                return {
                    "content_template": "{mention_role} {channel} {viewer_count}",
                    "title_template": "{channel} ist LIVE in {game}!",
                    "description_mode": "stream_title",
                    "description_template": "{title}",
                    "color": 0x9146FF,
                    "fields": [
                        {
                            "name_template": "Viewer",
                            "value_template": "{viewer_count}",
                            "inline": True,
                        }
                    ],
                    "author": {"name_template": "LIVE: {channel}", "icon_mode": "none"},
                    "footer": {
                        "text_template": "Footer",
                        "icon_mode": "none",
                        "timestamp_mode": "none",
                    },
                    "images": {"thumbnail_mode": "none", "image_mode": "none"},
                    "button": {
                        "label_template": "Watch",
                        "url_template": "{url}",
                        "force_stream_url": True,
                    },
                    "mentions": {
                        "use_streamer_ping_role": True,
                        "allowed_editor_role_ids": [123],
                    },
                }

        dummy = _ConfigDummy()
        payload = dummy._render_live_announcement_payload(
            login="tester",
            stream={
                "user_name": "Tester",
                "user_login": "tester",
                "viewer_count": 99,
                "game_name": "Deadlock",
                "title": "Grind",
            },
            mention_text="<@&123>",
        )
        assert isinstance(payload, dict)
        self.assertIn("Tester", payload.get("content", ""))
        self.assertIn("99", payload.get("content", ""))
        self.assertEqual(
            payload.get("embed", {}).get("title"), "Tester ist LIVE in Deadlock!"
        )

    async def test_button_url_is_forced_to_streamer_referral_url(self) -> None:
        dummy = _DummyEmbeds()
        dummy._referral_override = "https://www.twitch.tv/tester?ref=deadlock"
        dummy._payload_override = {
            "content": "test",
            "embed": {
                "title": "Live",
                "description": "Beschreibung",
                "color": 0x9146FF,
                "fields": [],
                "author": {
                    "name": "LIVE: Tester",
                    "icon_mode": "none",
                    "link_enabled": False,
                },
                "footer": {"text": "Footer", "icon_mode": "none"},
                "thumbnail": {"mode": "none"},
                "image": {"use_stream_thumbnail": False, "custom_url": ""},
            },
            "button": {
                "enabled": True,
                "label": "Custom",
                "url": "https://example.com/not-allowed",
            },
        }

        (
            _content,
            _embed,
            view,
            _allowed_mentions,
            _token,
        ) = await dummy._build_live_announcement_message(
            login="tester",
            stream={"user_name": "Tester", "title": "Ranked", "viewer_count": 12},
            streamer_entry={},
            notify_channel=None,
        )
        assert view is not None
        self.assertEqual(view.referral_url, "https://www.twitch.tv/tester?ref=deadlock")

    async def test_button_can_be_disabled_by_config_payload(self) -> None:
        dummy = _DummyEmbeds()
        dummy._payload_override = {
            "content": "test",
            "embed": {
                "title": "Live",
                "description": "Beschreibung",
                "color": 0x9146FF,
                "fields": [],
                "author": {
                    "name": "LIVE: Tester",
                    "icon_mode": "none",
                    "link_enabled": False,
                },
                "footer": {"text": "Footer", "icon_mode": "none"},
                "thumbnail": {"mode": "none"},
                "image": {"use_stream_thumbnail": False, "custom_url": ""},
            },
            "button": {
                "enabled": False,
                "label": "Hidden",
                "url": "https://example.com/not-used",
            },
        }

        (
            _content,
            _embed,
            view,
            _allowed_mentions,
            _token,
        ) = await dummy._build_live_announcement_message(
            login="tester",
            stream={"user_name": "Tester", "title": "Ranked", "viewer_count": 12},
            streamer_entry={},
            notify_channel=None,
        )
        self.assertIsNone(view)

    async def test_role_id_without_local_guild_still_preserves_ping_in_content(
        self,
    ) -> None:
        class _HeadlessRoleDummy(_DummyEmbeds):
            async def _ensure_live_ping_role(self, **kwargs):
                del kwargs
                return "", 987654

        dummy = _HeadlessRoleDummy()
        dummy._payload_override = None

        (
            content,
            _embed,
            _view,
            allowed_mentions,
            _token,
        ) = await dummy._build_live_announcement_message(
            login="tester",
            stream={"user_name": "Tester", "title": "Ranked", "viewer_count": 12},
            streamer_entry={},
            notify_channel=None,
        )

        self.assertIn("<@&987654>", content)
        role_ids = dummy._allowed_role_ids_from_allowed_mentions(allowed_mentions)
        self.assertEqual(role_ids, [987654])

    def test_live_tracking_view_spec_contains_required_fields(self) -> None:
        dummy = _DummyEmbeds()

        view_spec = dummy._build_twitch_live_tracking_view_spec(
            login="Tester",
            referral_url="https://www.twitch.tv/tester?ref=deadlock",
            tracking_token="abc123",
            button_label="Jetzt ansehen",
        )

        self.assertEqual(
            view_spec,
            {
                "type": "twitch_live_tracking",
                "streamer_login": "tester",
                "tracking_token": "abc123",
                "referral_url": "https://www.twitch.tv/tester?ref=deadlock",
                "button_label": "Jetzt ansehen",
            },
        )


if __name__ == "__main__":
    unittest.main()
