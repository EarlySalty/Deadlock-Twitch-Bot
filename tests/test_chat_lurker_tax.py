from __future__ import annotations

import re
import sqlite3
import unittest
from contextlib import ExitStack
from datetime import UTC, datetime, timedelta
from types import SimpleNamespace
from unittest.mock import patch

from bot.chat.commands import RaidCommandsMixin
from bot.chat.constants import PROMO_OVERALL_COOLDOWN_MIN
from bot.chat.promos import PromoMixin


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    @staticmethod
    def _translate(sql: str) -> str:
        translated = str(sql)
        translated = translated.replace("%s", "?")
        translated = translated.replace(
            "NOW() - INTERVAL '30 days'",
            "datetime('now', '-30 days')",
        )
        translated = translated.replace(
            "NOW() - INTERVAL '7 days'",
            "datetime('now', '-7 days')",
        )
        translated = re.sub(
            r"EXTRACT\(EPOCH FROM\s*\(\s*sc\.last_seen_at\s*-\s*sc\.first_message_at\s*\)\s*\)\s*/\s*60\.0",
            "(julianday(sc.last_seen_at) - julianday(sc.first_message_at)) * 24.0 * 60.0",
            translated,
            flags=re.IGNORECASE,
        )
        return translated

    def execute(self, sql: str, params=()):
        return self._conn.execute(self._translate(sql), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


class _ConnCtx:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = _CompatConn(conn)

    def __enter__(self) -> _CompatConn:
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _DummyPromoChat(PromoMixin):
    def __init__(self) -> None:
        self.announcement_calls: list[dict[str, str]] = []
        self._last_promo_sent: dict[str, float] = {}

    async def _send_announcement(self, channel, text: str, color: str = "purple", source: str = ""):
        self.announcement_calls.append(
            {
                "login": str(getattr(channel, "name", "") or ""),
                "channel_id": str(getattr(channel, "id", "") or ""),
                "text": text,
                "color": color,
                "source": source,
            }
        )
        return True


class _DummyCtx:
    def __init__(self, *, is_moderator: bool = True, is_broadcaster: bool = False) -> None:
        self.author = SimpleNamespace(
            name="tester",
            is_moderator=is_moderator,
            moderator=is_moderator,
            is_broadcaster=is_broadcaster,
            broadcaster=is_broadcaster,
        )
        self.channel = SimpleNamespace(name="partner_one")
        self.sent_messages: list[str] = []

    async def send(self, message: str):
        self.sent_messages.append(message)


class _DummyLurkerTaxCommands(RaidCommandsMixin):
    def __init__(self, *, plan_id: str, enabled: bool) -> None:
        self.plan_id = plan_id
        self.enabled = enabled
        self.saved: list[dict[str, object]] = []

    def _get_streamer_by_channel(self, channel_name: str):
        if channel_name == "partner_one":
            return ("partner_one", "1001", 1)
        return None

    def _load_lurker_tax_settings(self, login: str):
        del login
        return {
            "plan_id": self.plan_id,
            "is_paid_plan": self.plan_id != "raid_free",
            "enabled": self.enabled,
        }

    def _set_lurker_tax_enabled(
        self,
        *,
        twitch_login: str,
        twitch_user_id: str = "",
        plan_id: str = "",
        enabled: bool,
    ) -> bool:
        self.saved.append(
            {
                "twitch_login": twitch_login,
                "twitch_user_id": twitch_user_id,
                "plan_id": plan_id,
                "enabled": enabled,
            }
        )
        self.enabled = enabled
        return True


def _iso(dt: datetime) -> str:
    return dt.astimezone(UTC).isoformat(timespec="seconds")


def _build_conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE twitch_streamers (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_streamer_identities (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE streamer_plans (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            lurker_tax_enabled INTEGER NOT NULL DEFAULT 0,
            manual_plan_id TEXT,
            manual_plan_expires_at TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_billing_subscriptions (
            stripe_subscription_id TEXT PRIMARY KEY,
            customer_reference TEXT,
            status TEXT,
            plan_id TEXT,
            updated_at TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT,
            twitch_login TEXT,
            scopes TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_live_state (
            twitch_user_id TEXT,
            streamer_login TEXT,
            is_live INTEGER,
            active_session_id INTEGER
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_stream_sessions (
            id INTEGER PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            started_at TEXT,
            ended_at TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_session_chatters (
            session_id INTEGER NOT NULL,
            streamer_login TEXT NOT NULL,
            chatter_login TEXT NOT NULL,
            chatter_id TEXT,
            first_message_at TEXT NOT NULL,
            messages INTEGER DEFAULT 0,
            seen_via_chatters_api INTEGER DEFAULT 0,
            last_seen_at TEXT,
            PRIMARY KEY (session_id, chatter_login)
        )
        """
    )
    return conn


def _insert_lurker_history(
    conn: sqlite3.Connection,
    *,
    chatter_login: str,
    session_id: int,
    started_at: datetime,
    watch_minutes: int,
    chatter_id: str | None = None,
) -> None:
    ended_at = started_at + timedelta(minutes=watch_minutes + 15)
    conn.execute(
        """
        INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
        VALUES (?, ?, ?, ?)
        """,
        (session_id, "partner_one", _iso(started_at), _iso(ended_at)),
    )
    conn.execute(
        """
        INSERT INTO twitch_session_chatters (
            session_id, streamer_login, chatter_login, chatter_id,
            first_message_at, messages, seen_via_chatters_api, last_seen_at
        )
        VALUES (?, ?, ?, ?, ?, 0, 1, ?)
        """,
        (
            session_id,
            "partner_one",
            chatter_login,
            chatter_id or f"id-{chatter_login}",
            _iso(started_at),
            _iso(started_at + timedelta(minutes=watch_minutes)),
        ),
    )


class ChatLurkerTaxReminderTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = _build_conn()
        now = datetime.now(UTC)
        self.now = now
        self.conn.execute(
            "INSERT INTO twitch_streamers (twitch_user_id, twitch_login) VALUES (?, ?)",
            ("1001", "partner_one"),
        )
        self.conn.execute(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES (?, ?)",
            ("1001", "partner_one"),
        )
        self.conn.execute(
            """
            INSERT INTO streamer_plans (twitch_user_id, twitch_login, lurker_tax_enabled)
            VALUES (?, ?, ?)
            """,
            ("1001", "partner_one", 1),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_billing_subscriptions (
                stripe_subscription_id, customer_reference, status, plan_id, updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("sub_1", "partner_one", "active", "raid_boost", _iso(now)),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes)
            VALUES (?, ?, ?)
            """,
            ("1001", "partner_one", "chat:read chat:edit moderator:read:chatters"),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id)
            VALUES (?, ?, 1, ?)
            """,
            ("1001", "partner_one", 9001),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, NULL)
            """,
            (9001, "partner_one", _iso(now - timedelta(minutes=45))),
        )

        for offset, minutes in enumerate((150, 120, 90), start=1):
            _insert_lurker_history(
                self.conn,
                chatter_login="alpha",
                session_id=100 + offset,
                started_at=now - timedelta(days=offset + 2, hours=offset),
                watch_minutes=minutes,
            )
        for offset, minutes in enumerate((110, 100, 95), start=1):
            _insert_lurker_history(
                self.conn,
                chatter_login="beta",
                session_id=200 + offset,
                started_at=now - timedelta(days=offset + 6, hours=offset),
                watch_minutes=minutes,
            )
        for offset, minutes in enumerate((100, 90, 80), start=1):
            _insert_lurker_history(
                self.conn,
                chatter_login="gamma",
                session_id=300 + offset,
                started_at=now - timedelta(days=offset + 10, hours=offset),
                watch_minutes=minutes,
            )
        for offset, minutes in enumerate((70, 60), start=1):
            _insert_lurker_history(
                self.conn,
                chatter_login="low_sessions",
                session_id=400 + offset,
                started_at=now - timedelta(days=offset + 14),
                watch_minutes=minutes,
            )
        for offset, minutes in enumerate((50, 50, 50), start=1):
            _insert_lurker_history(
                self.conn,
                chatter_login="low_watch",
                session_id=500 + offset,
                started_at=now - timedelta(days=offset + 20),
                watch_minutes=minutes,
            )

        fresh_seen = _iso(now - timedelta(minutes=1))
        stale_seen = _iso(now - timedelta(minutes=10))
        current_rows = [
            ("alpha", 0, 1, fresh_seen),
            ("beta", 0, 1, fresh_seen),
            ("gamma", 0, 1, fresh_seen),
            ("stale_user", 0, 1, stale_seen),
            ("chatter_user", 2, 1, fresh_seen),
            ("low_sessions", 0, 1, fresh_seen),
            ("low_watch", 0, 1, fresh_seen),
        ]
        for chatter_login, messages, seen_via_api, last_seen_at in current_rows:
            self.conn.execute(
                """
                INSERT INTO twitch_session_chatters (
                    session_id, streamer_login, chatter_login, chatter_id,
                    first_message_at, messages, seen_via_chatters_api, last_seen_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    9001,
                    "partner_one",
                    chatter_login,
                    f"id-{chatter_login}",
                    _iso(now - timedelta(minutes=20)),
                    messages,
                    seen_via_api,
                    last_seen_at,
                ),
            )
        self.conn.commit()
        self.handler = _DummyPromoChat()

    def tearDown(self) -> None:
        self.conn.close()

    def _conn_patch(self):
        stack = ExitStack()
        stack.enter_context(
            patch("bot.chat.promos.readonly_connection", return_value=_ConnCtx(self.conn))
        )
        stack.enter_context(
            patch("bot.chat.promos.transaction", return_value=_ConnCtx(self.conn))
        )
        return stack

    async def test_candidate_selection_filters_and_sorts(self) -> None:
        with self._conn_patch():
            candidates = self.handler._get_lurker_tax_candidates(
                login="partner_one",
                session_id=9001,
                now_utc=self.now,
            )

        self.assertEqual([c["chatter_login"] for c in candidates[:3]], ["alpha", "beta", "gamma"])
        self.assertNotIn("stale_user", [c["chatter_login"] for c in candidates])
        self.assertNotIn("chatter_user", [c["chatter_login"] for c in candidates])
        self.assertNotIn("low_sessions", [c["chatter_login"] for c in candidates])
        self.assertNotIn("low_watch", [c["chatter_login"] for c in candidates])

    async def test_candidate_selection_prefers_chatter_id_and_requires_last_seen(self) -> None:
        for offset, minutes in enumerate((130, 120, 110), start=1):
            _insert_lurker_history(
                self.conn,
                chatter_login=f"renamed_old_{offset}",
                chatter_id="shared-rename-id",
                session_id=600 + offset,
                started_at=self.now - timedelta(days=offset + 30),
                watch_minutes=minutes,
            )
            _insert_lurker_history(
                self.conn,
                chatter_login="recycled_login",
                chatter_id="old-recycled-id",
                session_id=700 + offset,
                started_at=self.now - timedelta(days=offset + 40),
                watch_minutes=minutes,
            )
            _insert_lurker_history(
                self.conn,
                chatter_login="missing_last_seen",
                chatter_id="id-missing-last-seen",
                session_id=800 + offset,
                started_at=self.now - timedelta(days=offset + 50),
                watch_minutes=minutes,
            )

        self.conn.execute(
            """
            INSERT INTO twitch_session_chatters (
                session_id, streamer_login, chatter_login, chatter_id,
                first_message_at, messages, seen_via_chatters_api, last_seen_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                9001,
                "partner_one",
                "renamed_now",
                "shared-rename-id",
                _iso(self.now - timedelta(minutes=15)),
                0,
                1,
                _iso(self.now - timedelta(minutes=1)),
            ),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_session_chatters (
                session_id, streamer_login, chatter_login, chatter_id,
                first_message_at, messages, seen_via_chatters_api, last_seen_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                9001,
                "partner_one",
                "recycled_login",
                "new-recycled-id",
                _iso(self.now - timedelta(minutes=15)),
                0,
                1,
                _iso(self.now - timedelta(minutes=1)),
            ),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_session_chatters (
                session_id, streamer_login, chatter_login, chatter_id,
                first_message_at, messages, seen_via_chatters_api, last_seen_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                9001,
                "partner_one",
                "missing_last_seen",
                "id-missing-last-seen",
                _iso(self.now - timedelta(minutes=1)),
                0,
                1,
                None,
            ),
        )
        self.conn.commit()

        with self._conn_patch():
            candidates = self.handler._get_lurker_tax_candidates(
                login="partner_one",
                session_id=9001,
                now_utc=self.now,
            )

        candidate_logins = [c["chatter_login"] for c in candidates]
        self.assertIn("renamed_now", candidate_logins)
        self.assertNotIn("recycled_login", candidate_logins)
        self.assertNotIn("missing_last_seen", candidate_logins)

    async def test_reminder_dedupes_mentions_within_same_session(self) -> None:
        with self._conn_patch():
            first_ok = await self.handler._maybe_send_lurker_tax_reminder(
                "partner_one",
                "1001",
                now=0.0,
            )
            second_ok = await self.handler._maybe_send_lurker_tax_reminder(
                "partner_one",
                "1001",
                now=3601.0,
            )
            third_ok = await self.handler._maybe_send_lurker_tax_reminder(
                "partner_one",
                "1001",
                now=7202.0,
            )

        self.assertTrue(first_ok)
        self.assertTrue(second_ok)
        self.assertFalse(third_ok)
        self.assertEqual(len(self.handler.announcement_calls), 2)
        self.assertIn("@alpha", self.handler.announcement_calls[0]["text"])
        self.assertIn("@beta", self.handler.announcement_calls[0]["text"])
        self.assertIn("@gamma", self.handler.announcement_calls[1]["text"])
        self.assertNotIn("@alpha", self.handler.announcement_calls[1]["text"])

    async def test_reminder_logs_channel_id_without_login(self) -> None:
        with self._conn_patch(), self.assertLogs("TwitchStreams.ChatBot", level="INFO") as captured:
            ok = await self.handler._maybe_send_lurker_tax_reminder(
                "partner_one",
                "1001",
                now=0.0,
            )

        self.assertTrue(ok)
        combined = "\n".join(captured.output)
        self.assertIn("channel_id=1001", combined)
        self.assertNotIn("partner_one", combined)

    async def test_reminder_requires_chatters_scope(self) -> None:
        self.conn.execute("UPDATE twitch_raid_auth SET scopes = 'chat:read chat:edit'")
        self.conn.commit()

        with self._conn_patch():
            ok = await self.handler._maybe_send_lurker_tax_reminder(
                "partner_one",
                "1001",
                now=0.0,
            )

        self.assertFalse(ok)
        self.assertEqual(self.handler.announcement_calls, [])

    async def test_reminder_requires_known_bot_chatters_scope(self) -> None:
        self.conn.execute("UPDATE twitch_raid_auth SET scopes = 'chat:read chat:edit'")
        self.conn.commit()
        self.handler._token_manager = SimpleNamespace(
            scopes=set(),
            bot_id=None,
            expires_at=None,
        )

        with self._conn_patch():
            ok = await self.handler._maybe_send_lurker_tax_reminder(
                "partner_one",
                "1001",
                now=0.0,
            )

        self.assertFalse(ok)
        self.assertEqual(self.handler.announcement_calls, [])

    async def test_login_only_lookup_ignores_unrelated_blank_user_id_rows(self) -> None:
        self.conn.execute(
            "INSERT INTO twitch_streamers (twitch_user_id, twitch_login) VALUES (?, ?)",
            ("", "legacy_partner"),
        )
        self.conn.execute(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES (?, ?)",
            ("", "legacy_partner"),
        )
        self.conn.execute(
            """
            INSERT INTO streamer_plans (twitch_user_id, twitch_login, lurker_tax_enabled, manual_plan_id)
            VALUES (?, ?, ?, ?)
            """,
            (None, "other_streamer", 1, "analysis_dashboard"),
        )
        self.conn.execute(
            """
            INSERT INTO streamer_plans (twitch_user_id, twitch_login, lurker_tax_enabled, manual_plan_id)
            VALUES (?, ?, ?, ?)
            """,
            (None, "legacy_partner", 0, None),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes)
            VALUES (?, ?, ?)
            """,
            (None, "other_streamer", "chat:read chat:edit moderator:read:chatters"),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes)
            VALUES (?, ?, ?)
            """,
            (None, "legacy_partner", "chat:read chat:edit"),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id)
            VALUES (?, ?, 1, ?)
            """,
            ("", "legacy_partner", 9002),
        )
        self.conn.commit()

        with self._conn_patch():
            settings = self.handler._load_lurker_tax_settings("legacy_partner")

        self.assertEqual(settings["login"], "legacy_partner")
        self.assertEqual(settings["plan_id"], "raid_free")
        self.assertFalse(settings["enabled"])
        self.assertFalse(settings["is_paid_plan"])
        self.assertFalse(settings["has_moderator_read_chatters"])
        self.assertEqual(settings["active_session_id"], 9002)

    async def test_reminder_respects_shared_60_minute_cooldown(self) -> None:
        self.assertEqual(PROMO_OVERALL_COOLDOWN_MIN, 60)

        with self._conn_patch():
            self.handler._mark_promo_sent("partner_one", 0.0, reason="promo")
            blocked = await self.handler._maybe_send_lurker_tax_reminder(
                "partner_one",
                "1001",
                now=3599.0,
            )
            allowed = await self.handler._maybe_send_lurker_tax_reminder(
                "partner_one",
                "1001",
                now=3600.0,
            )

        self.assertFalse(blocked)
        self.assertTrue(allowed)
        self.assertEqual(len(self.handler.announcement_calls), 1)

    async def test_manual_free_override_blocks_paid_subscription_fallback(self) -> None:
        self.conn.execute(
            """
            UPDATE streamer_plans
               SET manual_plan_id = ?, manual_plan_expires_at = ?
             WHERE twitch_user_id = ?
            """,
            ("raid_free", _iso(self.now + timedelta(days=1)), "1001"),
        )
        self.conn.commit()

        with self._conn_patch():
            settings = self.handler._load_lurker_tax_settings("partner_one")
            ok = await self.handler._maybe_send_lurker_tax_reminder(
                "partner_one",
                "1001",
                now=0.0,
            )

        self.assertEqual(settings["plan_id"], "raid_free")
        self.assertFalse(settings["is_paid_plan"])
        self.assertFalse(ok)
        self.assertEqual(self.handler.announcement_calls, [])


class ChatLurkerTaxCommandTests(unittest.IsolatedAsyncioTestCase):
    async def test_command_disables_paid_plan_feature(self) -> None:
        cog = _DummyLurkerTaxCommands(plan_id="raid_boost", enabled=True)
        ctx = _DummyCtx(is_moderator=False, is_broadcaster=True)

        await _DummyLurkerTaxCommands.cmd_lurkersteuer_off.callback(cog, ctx)

        self.assertEqual(
            cog.saved,
            [
                {
                    "twitch_login": "partner_one",
                    "twitch_user_id": "1001",
                    "plan_id": "raid_boost",
                    "enabled": False,
                }
            ],
        )
        self.assertEqual(
            ctx.sent_messages,
            [
                "@tester Lurker Steuer deaktiviert. Im Abo-Bereich kannst du sie später wieder aktivieren."
            ],
        )

    async def test_command_rejects_mod_for_persistent_disable(self) -> None:
        cog = _DummyLurkerTaxCommands(plan_id="raid_boost", enabled=True)
        ctx = _DummyCtx(is_moderator=True, is_broadcaster=False)

        await _DummyLurkerTaxCommands.cmd_lurkersteuer_off.callback(cog, ctx)

        self.assertEqual(cog.saved, [])
        self.assertEqual(
            ctx.sent_messages,
            [
                "@tester Nur der Broadcaster kann die Lurker Steuer dauerhaft deaktivieren."
            ],
        )

    async def test_command_rejects_free_plan(self) -> None:
        cog = _DummyLurkerTaxCommands(plan_id="raid_free", enabled=False)
        ctx = _DummyCtx(is_moderator=False, is_broadcaster=True)

        await _DummyLurkerTaxCommands.cmd_lurkersteuer_off.callback(cog, ctx)

        self.assertEqual(cog.saved, [])
        self.assertEqual(
            ctx.sent_messages,
            ["@tester Die Lurker Steuer ist nur in bezahlten Plänen verfügbar."],
        )


if __name__ == "__main__":
    unittest.main()
