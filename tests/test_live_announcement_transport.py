from __future__ import annotations

import unittest
from unittest.mock import patch

import discord

from bot.monitoring.monitoring import TwitchMonitoringMixin


class _DummyMonitoring(TwitchMonitoringMixin):
    def __init__(self) -> None:
        self.bot = type("Bot", (), {})()


class LiveAnnouncementTransportTests(unittest.IsolatedAsyncioTestCase):
    def test_live_announcement_retry_payload_reuses_first_snapshot_for_same_stream(self) -> None:
        dummy = _DummyMonitoring()
        first_stream = {
            "id": "stream-1",
            "started_at": "2026-03-15T11:40:00Z",
            "title": "First Title",
            "game_name": "Deadlock",
            "viewer_count": 12,
            "language": "de",
            "tags": ["first"],
        }

        first_snapshot, first_token, _first_render_now = dummy._resolve_live_announcement_retry_payload(
            login="Tester",
            stream=first_stream,
            previous_state={},
            stream_id="stream-1",
            started_at="2026-03-15T11:40:00Z",
            message_id=None,
            rendered_at="2026-03-15T11:40:05+00:00",
        )

        second_snapshot, second_token, second_render_now = dummy._resolve_live_announcement_retry_payload(
            login="Tester",
            stream={
                "id": "stream-1",
                "started_at": "2026-03-15T11:40:00Z",
                "title": "Second Title",
                "game_name": "Just Chatting",
                "viewer_count": 99,
                "language": "en",
                "tags": ["second"],
            },
            previous_state={
                "last_stream_id": "stream-1",
                "last_started_at": "2026-03-15T11:40:00Z",
                "last_title": "First Title",
                "last_game": "Deadlock",
                "last_viewer_count": 12,
            },
            stream_id="stream-1",
            started_at="2026-03-15T11:40:00Z",
            message_id=None,
            rendered_at="2026-03-15T11:41:00+00:00",
        )

        self.assertEqual(first_token, second_token)
        self.assertEqual(first_snapshot, second_snapshot)
        self.assertEqual(second_snapshot["title"], "First Title")
        self.assertEqual(second_snapshot["viewer_count"], 12)
        self.assertEqual(second_snapshot["language"], "de")
        self.assertEqual(second_snapshot["tags"], ["first"])
        self.assertEqual(
            second_render_now.isoformat(),
            "2026-03-15T11:40:05+00:00",
        )

    async def test_send_live_announcement_via_broker_forwards_rich_payload(self) -> None:
        dummy = _DummyMonitoring()
        captured: dict[str, object] = {}

        async def _fake_post_master_broker_json(**kwargs):
            captured.update(kwargs)
            return {"message_id": 4242}

        dummy._post_master_broker_json = _fake_post_master_broker_json  # type: ignore[method-assign]
        embed = discord.Embed(title="Live", description="Beschreibung")

        message_id = await dummy._send_live_announcement_via_broker(
            channel_id=123456,
            login="Tester",
            stream_id="stream-1",
            content="Testcontent",
            embed=embed,
            allowed_role_ids=[111, 222],
            view_spec={
                "type": "twitch_live_tracking",
                "streamer_login": "tester",
                "tracking_token": "track-1",
                "referral_url": "https://www.twitch.tv/tester?ref=deadlock",
                "button_label": "Jetzt ansehen",
            },
        )

        self.assertEqual(message_id, "4242")
        self.assertEqual(
            captured["path"],
            "/internal/master/v1/discord/send-rich-message",
        )
        expected_payload = {
            "channel_id": 123456,
            "content": "Testcontent",
            "embed": embed.to_dict(),
            "allowed_role_ids": [111, 222],
            "view_spec": {
                "type": "twitch_live_tracking",
                "streamer_login": "tester",
                "tracking_token": "track-1",
                "referral_url": "https://www.twitch.tv/tester?ref=deadlock",
                "button_label": "Jetzt ansehen",
            },
        }
        self.assertEqual(
            captured["idempotency_key"],
            dummy._build_announcement_payload_idempotency_key(
                action="live-send",
                login="Tester",
                payload=expected_payload,
            ),
        )
        self.assertEqual(
            captured["payload"],
            expected_payload,
        )

    def test_live_send_idempotency_key_changes_with_payload(self) -> None:
        dummy = _DummyMonitoring()
        base_payload = {
            "channel_id": 123456,
            "content": "Testcontent",
            "embed": {"title": "Live"},
            "allowed_role_ids": [111],
            "view_spec": None,
        }

        first_key = dummy._build_announcement_payload_idempotency_key(
            action="live-send",
            login="Tester",
            payload=base_payload,
        )
        second_key = dummy._build_announcement_payload_idempotency_key(
            action="live-send",
            login="Tester",
            payload={**base_payload, "content": "Geaenderter Content"},
        )

        self.assertNotEqual(first_key, second_key)

    async def test_send_live_announcement_via_broker_falls_back_to_link_button(self) -> None:
        dummy = _DummyMonitoring()
        calls: list[dict[str, object]] = []

        async def _fake_post_master_broker_json(**kwargs):
            calls.append(dict(kwargs))
            if len(calls) == 1:
                return None, "view_resolver_unavailable"
            return {"message_id": 9898}, None

        dummy._post_master_broker_json = _fake_post_master_broker_json  # type: ignore[method-assign]
        embed = discord.Embed(title="Live", description="Beschreibung")

        message_id = await dummy._send_live_announcement_via_broker(
            channel_id=123456,
            login="Tester",
            stream_id="stream-1",
            content="Testcontent",
            embed=embed,
            allowed_role_ids=[111],
            view_spec={
                "type": "twitch_live_tracking",
                "streamer_login": "tester",
                "tracking_token": "track-1",
                "referral_url": "https://www.twitch.tv/tester?ref=deadlock",
                "button_label": "Jetzt ansehen",
            },
        )

        self.assertEqual(message_id, "9898")
        self.assertEqual(len(calls), 2)
        first_payload = {
            "channel_id": 123456,
            "content": "Testcontent",
            "embed": embed.to_dict(),
            "allowed_role_ids": [111],
            "view_spec": {
                "type": "twitch_live_tracking",
                "streamer_login": "tester",
                "tracking_token": "track-1",
                "referral_url": "https://www.twitch.tv/tester?ref=deadlock",
                "button_label": "Jetzt ansehen",
            },
        }
        second_payload = {
            "channel_id": 123456,
            "content": "Testcontent",
            "embed": embed.to_dict(),
            "allowed_role_ids": [111],
            "view_spec": {
                "type": "link_button",
                "label": "Jetzt ansehen",
                "url": "https://www.twitch.tv/tester?ref=deadlock",
            },
        }
        self.assertEqual(
            calls[0]["idempotency_key"],
            dummy._build_announcement_payload_idempotency_key(
                action="live-send",
                login="Tester",
                payload=first_payload,
            ),
        )
        self.assertEqual(
            calls[1]["idempotency_key"],
            dummy._build_announcement_payload_idempotency_key(
                action="live-send-fallback",
                login="Tester",
                payload=second_payload,
            ),
        )
        self.assertEqual(calls[1]["payload"], second_payload)

    async def test_edit_live_announcement_via_broker_uses_edit_endpoint(self) -> None:
        dummy = _DummyMonitoring()
        captured: dict[str, object] = {}

        async def _fake_post_master_broker_json(**kwargs):
            captured.update(kwargs)
            return {"message_id": 4242}

        dummy._post_master_broker_json = _fake_post_master_broker_json  # type: ignore[method-assign]
        embed = discord.Embed(title="Offline", description="VOD")

        updated = await dummy._edit_live_announcement_via_broker(
            channel_id=123456,
            login="Tester",
            message_id="4242",
            content="Offline",
            embed=embed,
            view_spec={
                "type": "link_button",
                "label": "VOD ansehen",
                "url": "https://www.twitch.tv/videos/1",
            },
        )

        self.assertTrue(updated)
        self.assertEqual(
            captured["path"],
            "/internal/master/v1/discord/edit-rich-message",
        )
        expected_payload = {
            "channel_id": 123456,
            "message_id": "4242",
            "content": "Offline",
            "embed": embed.to_dict(),
            "view_spec": {
                "type": "link_button",
                "label": "VOD ansehen",
                "url": "https://www.twitch.tv/videos/1",
            },
        }
        self.assertEqual(
            captured["idempotency_key"],
            dummy._build_announcement_payload_idempotency_key(
                action="live-edit",
                login="Tester",
                payload=expected_payload,
            ),
        )
        self.assertEqual(
            captured["payload"],
            expected_payload,
        )

    def test_broker_token_fallback_uses_main_bot_internal_token(self) -> None:
        dummy = _DummyMonitoring()

        with patch.dict(
            "os.environ",
            {
                "MASTER_BROKER_TOKEN": "",
                "MAIN_BOT_INTERNAL_TOKEN": "fallback-token",
            },
            clear=False,
        ):
            self.assertEqual(dummy._master_broker_token(), "fallback-token")
            self.assertTrue(dummy._announcement_transport_prefers_master_broker())

    def test_broker_token_fallback_accepts_shared_internal_api_token(self) -> None:
        dummy = _DummyMonitoring()

        with patch.dict(
            "os.environ",
            {
                "MASTER_BROKER_TOKEN": "",
                "MAIN_BOT_INTERNAL_TOKEN": "",
                "TWITCH_INTERNAL_API_TOKEN": "shared-internal-token",
            },
            clear=False,
        ):
            self.assertEqual(dummy._master_broker_token(), "shared-internal-token")
            self.assertTrue(dummy._announcement_transport_prefers_master_broker())

    def test_master_broker_base_url_accepts_loopback_host(self) -> None:
        dummy = _DummyMonitoring()

        with patch.dict(
            "os.environ",
            {
                "MASTER_BROKER_BASE_URL": "http://127.0.0.1:8770/internal-root",
            },
            clear=False,
        ):
            self.assertEqual(
                dummy._master_broker_base_url(),
                "http://127.0.0.1:8770/internal-root",
            )

    def test_master_broker_base_url_rejects_remote_host(self) -> None:
        dummy = _DummyMonitoring()

        with patch.dict(
            "os.environ",
            {
                "MASTER_BROKER_BASE_URL": "https://broker.example.com:8770",
            },
            clear=False,
        ):
            with self.assertRaisesRegex(ValueError, "loopback"):
                dummy._master_broker_base_url()


if __name__ == "__main__":
    unittest.main()
