import unittest
from unittest.mock import patch

from bot.dashboard.auth.state_store import DashboardAuthStateRepository


class DashboardAuthStateRepositoryAffiliateTests(unittest.TestCase):
    def test_affiliate_oauth_and_connect_states_use_distinct_persisted_types(self) -> None:
        repo = DashboardAuthStateRepository()

        with patch(
            "bot.dashboard.auth.state_store.sessions_db.upsert_session"
        ) as upsert_session, patch(
            "bot.dashboard.auth.state_store.sessions_db.pop_session",
            side_effect=[
                {"created_at": 1.0, "redirect_uri": "https://example/a"},
                {"created_at": 2.0, "redirect_uri": "https://example/b"},
            ],
        ) as pop_session:
            repo.save_affiliate_oauth_state(
                state="oauth-state",
                payload={"redirect_uri": "https://example/a"},
                ttl_seconds=600,
                now=10.0,
            )
            repo.save_affiliate_connect_state(
                state="connect-state",
                payload={"redirect_uri": "https://example/b"},
                ttl_seconds=600,
                now=11.0,
            )

            oauth_state = repo.consume_affiliate_oauth_state("oauth-state", now=12.0)
            connect_state = repo.consume_affiliate_connect_state("connect-state", now=13.0)

        self.assertEqual(oauth_state["redirect_uri"], "https://example/a")
        self.assertEqual(connect_state["redirect_uri"], "https://example/b")
        self.assertEqual(upsert_session.call_count, 2)
        self.assertEqual(pop_session.call_count, 2)
        self.assertEqual(upsert_session.call_args_list[0].args[1], "oauth_state:affiliate")
        self.assertEqual(upsert_session.call_args_list[1].args[1], "oauth_state:affiliate_connect")


if __name__ == "__main__":
    unittest.main()
