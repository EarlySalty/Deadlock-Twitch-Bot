from __future__ import annotations

import unittest
from collections import deque

import bot.chat.promos as promo_module
import bot.chat.service_pitch_warning as warning_module
from bot.chat.promos import PromoMixin
from bot.chat.service_pitch_warning import ServicePitchWarningMixin


class _DummyServiceWarning(ServicePitchWarningMixin):
    def __init__(self) -> None:
        self._init_service_pitch_warning()


class _DummyPromo(PromoMixin):
    def __init__(self) -> None:
        self._promo_activity: dict[str, deque[tuple[float, str]]] = {}
        self._promo_chatter_dedupe: dict[str, dict[str, float]] = {}
        self._last_raw_chat_message_ts: dict[str, float] = {}
        self._raw_msg_count_since_promo: dict[str, int] = {}
        self._last_promo_sent: dict[str, float] = {}
        self._last_promo_attempt: dict[str, float] = {}
        self._last_promo_viewer_spike: dict[str, float] = {}


class ChatMemoryPruningTests(unittest.TestCase):
    def test_prune_simple_monotonic_cache_removes_stale_entries_below_capacity(self) -> None:
        cache = {"stale": 10.0, "fresh": 195.0}

        _DummyServiceWarning._prune_simple_monotonic_cache(
            cache,
            200.0,
            max_len=10,
            max_age_sec=20.0,
        )

        self.assertNotIn("stale", cache)
        self.assertIn("fresh", cache)

    def test_service_warning_buckets_are_bounded(self) -> None:
        handler = _DummyServiceWarning()
        key = ("channel", "viewer")

        history_bucket = handler._get_service_message_history_bucket(key)
        activity_bucket = handler._get_service_activity_bucket(key)
        for index in range(max(
            warning_module._SERVICE_WARNING_MESSAGE_HISTORY_MAXLEN,
            warning_module._SERVICE_WARNING_ACTIVITY_BUCKET_MAXLEN,
        ) + 20):
            history_bucket.append((float(index), f"msg-{index}", {"feature"}))
            activity_bucket.append((float(index), index))

        self.assertEqual(
            len(history_bucket),
            warning_module._SERVICE_WARNING_MESSAGE_HISTORY_MAXLEN,
        )
        self.assertEqual(
            len(activity_bucket),
            warning_module._SERVICE_WARNING_ACTIVITY_BUCKET_MAXLEN,
        )

    def test_service_warning_state_prune_removes_stale_outer_entries(self) -> None:
        handler = _DummyServiceWarning()
        now = 10_000.0
        key = ("channel", "viewer")
        very_stale_ts = now - float(warning_module._SERVICE_WARNING_USER_COOLDOWN_SEC) * 3.0

        handler._service_warning_message_history[key] = deque(
            [(now - 1000.0, "msg", {"feature"})],
            maxlen=warning_module._SERVICE_WARNING_MESSAGE_HISTORY_MAXLEN,
        )
        handler._service_warning_activity[key] = deque(
            [(now - 2000.0, 5)],
            maxlen=warning_module._SERVICE_WARNING_ACTIVITY_BUCKET_MAXLEN,
        )
        handler._service_warning_first_seen[key] = very_stale_ts
        handler._service_warning_channel_cd["channel"] = very_stale_ts
        handler._service_warning_user_cd[key] = very_stale_ts
        handler._service_warning_hint_cd[key] = very_stale_ts

        handler._prune_service_warning_state(now, force=True)

        self.assertEqual(handler._service_warning_message_history, {})
        self.assertEqual(handler._service_warning_activity, {})
        self.assertEqual(handler._service_warning_first_seen, {})
        self.assertEqual(handler._service_warning_channel_cd, {})
        self.assertEqual(handler._service_warning_user_cd, {})
        self.assertEqual(handler._service_warning_hint_cd, {})

    def test_promo_runtime_prune_removes_stale_login_state(self) -> None:
        handler = _DummyPromo()
        now = 20_000.0
        stale_ts = now - float(promo_module._PROMO_RUNTIME_STATE_MAX_AGE_SEC) - 5.0

        handler._promo_activity["stale"] = deque([(stale_ts, "viewer-one")], maxlen=64)
        handler._promo_chatter_dedupe["stale"] = {"viewer-one": stale_ts}
        handler._last_raw_chat_message_ts["stale"] = stale_ts
        handler._raw_msg_count_since_promo["stale"] = 7
        handler._last_promo_sent["stale"] = stale_ts
        handler._last_promo_attempt["stale"] = stale_ts
        handler._last_promo_viewer_spike["stale"] = stale_ts

        handler._promo_activity["fresh"] = deque([(now, "viewer-two")], maxlen=64)
        handler._last_raw_chat_message_ts["fresh"] = now
        handler._raw_msg_count_since_promo["fresh"] = 3

        handler._prune_promo_runtime_state(now, force=True)

        self.assertNotIn("stale", handler._promo_activity)
        self.assertNotIn("stale", handler._promo_chatter_dedupe)
        self.assertNotIn("stale", handler._last_raw_chat_message_ts)
        self.assertNotIn("stale", handler._raw_msg_count_since_promo)
        self.assertNotIn("stale", handler._last_promo_sent)
        self.assertNotIn("stale", handler._last_promo_attempt)
        self.assertNotIn("stale", handler._last_promo_viewer_spike)
        self.assertIn("fresh", handler._promo_activity)
        self.assertIn("fresh", handler._last_raw_chat_message_ts)
        self.assertIn("fresh", handler._raw_msg_count_since_promo)


if __name__ == "__main__":
    unittest.main()
