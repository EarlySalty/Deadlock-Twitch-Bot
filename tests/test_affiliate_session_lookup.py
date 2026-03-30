from __future__ import annotations

import time
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from bot.dashboard.affiliate_mixin import _DashboardAffiliateMixin


class _AffiliateSessionLookupHarness(_DashboardAffiliateMixin):
    pass


class AffiliateSessionLookupTests(unittest.TestCase):
    def test_lookup_loads_single_session_by_cookie_id_and_caches_it(self) -> None:
        handler = _AffiliateSessionLookupHarness()
        request = SimpleNamespace(cookies={"twitch_affiliate_session": "session-123"})
        stored_session = {
            "twitch_login": "partner_one",
            "twitch_user_id": "1001",
            "display_name": "Partner One",
            "created_at": time.time() - 60,
            "expires_at": time.time() + 3600,
        }

        with patch(
            "bot.dashboard.affiliate_mixin.sessions_db.load_session",
            return_value=stored_session,
        ) as load_session, patch(
            "bot.dashboard.affiliate_mixin.sessions_db.load_valid_sessions"
        ) as load_valid_sessions, patch(
            "bot.dashboard.affiliate_mixin.sessions_db.delete_session"
        ) as delete_session:
            session = handler._get_affiliate_session(request)
            cached_session = handler._get_affiliate_session(request)

        self.assertEqual(session, stored_session)
        self.assertEqual(cached_session, stored_session)
        self.assertIn("session-123", handler._affiliate_sessions)
        load_session.assert_called_once()
        load_valid_sessions.assert_not_called()
        delete_session.assert_not_called()

    def test_lookup_skips_full_preload_when_cache_is_empty(self) -> None:
        handler = _AffiliateSessionLookupHarness()
        request = SimpleNamespace(cookies={"twitch_affiliate_session": "session-456"})

        with patch(
            "bot.dashboard.affiliate_mixin.sessions_db.load_session",
            return_value=None,
        ) as load_session, patch(
            "bot.dashboard.affiliate_mixin.sessions_db.load_valid_sessions",
            side_effect=AssertionError("full preload must not be used"),
        ) as load_valid_sessions:
            session = handler._get_affiliate_session(request)

        self.assertIsNone(session)
        load_session.assert_called_once()
        load_valid_sessions.assert_not_called()

    def test_lookup_removes_expired_cached_session_and_purges_db_row(self) -> None:
        handler = _AffiliateSessionLookupHarness()
        request = SimpleNamespace(cookies={"twitch_affiliate_session": "session-expired"})
        handler._affiliate_sessions = {
            "session-expired": {
                "twitch_login": "partner_one",
                "twitch_user_id": "1001",
                "display_name": "Partner One",
                "created_at": 100.0,
                "expires_at": 200.0,
            }
        }

        with patch("bot.dashboard.affiliate_mixin.time.time", return_value=500.0), patch(
            "bot.dashboard.affiliate_mixin.sessions_db.load_session"
        ) as load_session, patch(
            "bot.dashboard.affiliate_mixin.sessions_db.delete_session"
        ) as delete_session:
            session = handler._get_affiliate_session(request)

        self.assertIsNone(session)
        self.assertNotIn("session-expired", handler._affiliate_sessions)
        load_session.assert_not_called()
        delete_session.assert_called_once_with("session-expired")


if __name__ == "__main__":
    unittest.main()
