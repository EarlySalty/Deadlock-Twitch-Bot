import unittest
from unittest.mock import AsyncMock, patch

from bot.chat.moderation import ModerationMixin


class _DummyTokenManager:
    async def get_valid_token(self, force_refresh: bool = False):
        del force_refresh
        return ("token", "refresh")


class _DummyRaidBot:
    def __init__(self) -> None:
        self.blacklist_calls: list[tuple[str | None, str, str]] = []

    def _add_to_blacklist(self, target_id: str | None, target_login: str, reason: str) -> None:
        self.blacklist_calls.append((target_id, target_login, reason))


class _DummyModerationChat(ModerationMixin):
    def __init__(self) -> None:
        self._client_id = "client"
        self.bot_id_safe = "sender-1"
        self.bot_id = "sender-1"
        self.prefix = "!"
        self._token_manager = _DummyTokenManager()
        self._raid_bot = _DummyRaidBot()

    @staticmethod
    def _normalize_channel_login(login: str) -> str:
        return str(login or "").strip().lower().lstrip("#")

    def _normalize_channel_login_safe(self, channel) -> str:
        return self._normalize_channel_login(getattr(channel, "name", ""))

    @staticmethod
    def _is_partner_channel_for_chat_tracking(login: str) -> bool:
        del login
        return False

    def _record_raw_chat_message(self, login: str) -> None:
        del login

    def _resolve_session_id(self, login: str) -> int | None:
        del login
        return 123

    def _is_target_game_live_for_chat(self, login: str, session_id: int | None) -> bool:
        del login, session_id
        return True


class _DummyResponse:
    def __init__(self, status: int, payload_text: str) -> None:
        self.status = status
        self._payload_text = payload_text

    async def text(self) -> str:
        return self._payload_text

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb) -> bool:
        del exc_type, exc, tb
        return False


class _DummyClientSession:
    def __init__(self, response: _DummyResponse) -> None:
        self._response = response

    def post(self, url, headers=None, json=None):
        del url, headers, json
        return self._response

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb) -> bool:
        del exc_type, exc, tb
        return False


class _DummyChannel:
    def __init__(self, name: str, channel_id: str) -> None:
        self.name = name
        self.id = channel_id


class _RecordingConn:
    def __init__(self) -> None:
        self.calls: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql, params=None):
        self.calls.append((str(sql), tuple(params or ())))

        class _Cursor:
            @staticmethod
            def fetchone():
                return None

        return _Cursor()


class _RecordingConnContext:
    def __init__(self, conn: _RecordingConn) -> None:
        self._conn = conn

    def __enter__(self) -> _RecordingConn:
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        del exc_type, exc, tb
        return False


class _DummyAuthor:
    def __init__(self, name: str, author_id: str) -> None:
        self.name = name
        self.id = author_id


class _DummyMessage:
    def __init__(self, *, channel_name: str, author_name: str, author_id: str, content: str) -> None:
        self.channel = _DummyChannel(channel_name, "channel-1")
        self.author = _DummyAuthor(author_name, author_id)
        self.content = content
        self.id = "msg-1"


class ChatMessageBlacklistTests(unittest.IsolatedAsyncioTestCase):
    async def test_opted_out_streamer_skips_announcement_without_fallback(self) -> None:
        handler = _DummyModerationChat()
        channel = _DummyChannel("jekoz42", "22482316")

        with patch.object(handler, "_is_manual_partner_opt_out_for_chat", return_value=True), patch.object(
            handler,
            "_send_chat_message",
            new=AsyncMock(return_value=True),
        ) as fallback_mock, patch("aiohttp.ClientSession") as client_session_mock:
            ok = await handler._send_announcement(channel, "hello", source="promo")

        self.assertFalse(ok)
        fallback_mock.assert_not_awaited()
        client_session_mock.assert_not_called()

    async def test_opted_out_streamer_skips_chat_message_http_send(self) -> None:
        handler = _DummyModerationChat()
        channel = _DummyChannel("jekoz42", "22482316")

        with patch.object(handler, "_is_manual_partner_opt_out_for_chat", return_value=True), patch(
            "aiohttp.ClientSession"
        ) as client_session_mock:
            ok = await handler._send_chat_message(channel, "hello", source="promo")

        self.assertFalse(ok)
        client_session_mock.assert_not_called()

    async def test_dropped_recruitment_message_blacklists_banned_phone_alias(self) -> None:
        handler = _DummyModerationChat()
        channel = _DummyChannel("cemo_336", "494921554")
        response = _DummyResponse(
            200,
            (
                '{"data":[{"is_sent":false,"drop_reason":{"code":"banned_phone_alias",'
                '"message":"Your message was not sent because your phone number is banned from this channel."}}]}'
            ),
        )

        with patch("aiohttp.ClientSession", return_value=_DummyClientSession(response)):
            ok = await handler._send_chat_message(channel, "hello", source="recruitment")

        self.assertFalse(ok)
        self.assertEqual(len(handler._raid_bot.blacklist_calls), 1)
        target_id, target_login, reason = handler._raid_bot.blacklist_calls[0]
        self.assertEqual(target_id, "494921554")
        self.assertEqual(target_login, "cemo_336")
        self.assertIn("recruitment_bot_banned", reason)
        self.assertIn("banned_phone_alias", reason)

    async def test_channel_settings_drop_records_outbound_chat_suppression(self) -> None:
        handler = _DummyModerationChat()
        channel = _DummyChannel("realclassik", "471205134")
        response = _DummyResponse(
            200,
            (
                '{"data":[{"is_sent":false,"drop_reason":{"code":"channel_settings",'
                '"message":"Your message wasn\'t posted due to conflicts with the channel\'s moderation settings."}}]}'
            ),
        )

        with patch.object(handler, "_set_outbound_chat_suppression") as suppression_mock:
            with patch("aiohttp.ClientSession", return_value=_DummyClientSession(response)):
                ok = await handler._send_chat_message(channel, "hello", source="recruitment")

        self.assertFalse(ok)
        suppression_mock.assert_called_once()
        self.assertEqual(suppression_mock.call_args.args[:2], (channel, "recruitment"))
        self.assertEqual(
            suppression_mock.call_args.kwargs["reason_code"],
            "channel_settings",
        )
        self.assertIn(
            "channel_settings",
            suppression_mock.call_args.kwargs["reason_detail"],
        )

    async def test_active_outbound_chat_suppression_skips_http_send(self) -> None:
        handler = _DummyModerationChat()
        channel = _DummyChannel("realclassik", "471205134")
        suppression = {
            "target_login": "realclassik",
            "reason_code": "channel_settings",
            "reason_detail": "channel_settings: moderation settings",
            "suppressed_until": "2026-03-22T12:00:00+00:00",
        }

        with patch.object(handler, "_get_outbound_chat_suppression", return_value=suppression):
            with patch("aiohttp.ClientSession") as client_session_mock:
                ok = await handler._send_chat_message(channel, "hello", source="recruitment")

        self.assertFalse(ok)
        client_session_mock.assert_not_called()

    def test_raw_chat_health_upsert_uses_boolean_case_flag(self) -> None:
        handler = _DummyModerationChat()
        conn = _RecordingConn()

        handler._upsert_raw_chat_ingest_health_row(
            conn,
            "partner_one",
            last_raw_chat_error="boom",
        )

        self.assertEqual(len(conn.calls), 1)
        _, params = conn.calls[0]
        self.assertIs(params[7], True)
        self.assertEqual(params[8], "boom")

    async def test_raw_chat_message_insert_uses_boolean_is_command_value(self) -> None:
        handler = _DummyModerationChat()
        conn = _RecordingConn()
        message = _DummyMessage(
            channel_name="partner_one",
            author_name="viewer_one",
            author_id="42",
            content="!ping",
        )

        with patch("bot.chat.moderation.transaction", return_value=_RecordingConnContext(conn)):
            with patch.object(handler, "_is_partner_channel_for_chat_tracking", return_value=True):
                self.assertIsNone(await handler._track_chat_health(message))

        chat_message_call = next(
            params for sql, params in conn.calls if "INSERT INTO twitch_chat_messages" in sql
        )
        session_chatter_call = next(
            params for sql, params in conn.calls if "INSERT INTO twitch_session_chatters" in sql
        )
        self.assertIs(chat_message_call[6], True)
        self.assertIs(session_chatter_call[6], True)
        self.assertIs(session_chatter_call[7], False)


if __name__ == "__main__":
    unittest.main()
