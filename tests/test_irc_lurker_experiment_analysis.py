import unittest
from datetime import UTC, datetime
from types import SimpleNamespace
from unittest.mock import patch

from bot.analytics.mixin import TwitchAnalyticsMixin


class _AnalyticsHarness(TwitchAnalyticsMixin):
    def __init__(self) -> None:
        pass


class IRCLurkerExperimentAnalysisTests(unittest.TestCase):
    def test_record_sample_aggregates_helix_and_irc_differences(self) -> None:
        harness = _AnalyticsHarness()
        harness._experimental_irc_lurker_enabled = True
        harness._experimental_irc_lurker_channels = {"earlysalty"}
        harness._irc_lurker_tracker = SimpleNamespace(
            get_chatters=lambda login: {"viewer_a", "viewer_c"}
        )
        harness._irc_lurker_experiment_session_stats = {}

        with patch("bot.analytics.mixin._IRC_EXPERIMENT_LOG.info") as log_info:
            harness._record_irc_lurker_experiment_sample(
                login="earlysalty",
                session_id=42,
                now_iso="2026-03-19T05:00:00+00:00",
                helix_chatters=[
                    {"user_login": "viewer_a"},
                    {"user_login": "viewer_b"},
                ],
            )

        stats = harness._irc_lurker_experiment_session_stats[42]
        self.assertEqual(stats["sample_count"], 1)
        self.assertEqual(stats["helix_led_sample_count"], 0)
        self.assertEqual(stats["irc_led_sample_count"], 0)
        self.assertEqual(stats["equal_sample_count"], 1)
        self.assertEqual(stats["distinct_helix_only"], {"viewer_b"})
        self.assertEqual(stats["distinct_irc_only"], {"viewer_c"})
        log_info.assert_called_once()

    def test_finalize_session_logs_summary_and_clears_state(self) -> None:
        harness = _AnalyticsHarness()
        harness._irc_lurker_experiment_session_stats = {
            77: {
                "login": "earlysalty",
                "first_sample_at": "2026-03-19T05:00:00+00:00",
                "last_sample_at": "2026-03-19T06:00:00+00:00",
                "sample_count": 2,
                "equal_sample_count": 1,
                "helix_led_sample_count": 1,
                "irc_led_sample_count": 0,
                "helix_total_sum": 9,
                "irc_total_sum": 7,
                "overlap_total_sum": 6,
                "helix_only_total_sum": 3,
                "irc_only_total_sum": 1,
                "max_helix_count": 5,
                "max_irc_count": 4,
                "max_overlap_count": 3,
                "max_helix_only_count": 2,
                "max_irc_only_count": 1,
                "distinct_helix": {"viewer_a", "viewer_b", "viewer_c"},
                "distinct_irc": {"viewer_a", "viewer_c"},
                "distinct_overlap": {"viewer_a", "viewer_c"},
                "distinct_helix_only": {"viewer_b"},
                "distinct_irc_only": set(),
            }
        }

        with patch("bot.analytics.mixin._IRC_EXPERIMENT_LOG.info") as log_info:
            harness._finalize_irc_lurker_experiment_session(
                login="earlysalty",
                session_id=77,
                reason="offline",
                ended_at=datetime(2026, 3, 19, 6, 30, tzinfo=UTC),
            )

        self.assertNotIn(77, harness._irc_lurker_experiment_session_stats)
        log_info.assert_called_once()


if __name__ == "__main__":
    unittest.main()
