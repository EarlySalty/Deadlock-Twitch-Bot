from __future__ import annotations

import unittest
from collections import deque
from contextlib import ExitStack
from unittest.mock import patch

from bot.chat.promos import PromoMixin


class _DummyPromoGate(PromoMixin):
    def __init__(self) -> None:
        self._raw_msg_count_since_promo: dict[str, int] = {}
        self._last_promo_sent: dict[str, float] = {}
        self._promo_activity: dict = {}
        self._promo_seen_chatters: dict = {}
        self._promo_seen_chatters_ts: dict = {}


class PromoActivityGateTests(unittest.TestCase):
    """Regression: Targeted-Promo darf nicht am Session-Start feuern.

    _promo_activity_ready bündelt die Aktivitäts-Schwellen, die jetzt AUCH die
    Targeted-Promo gaten. Frischer Kanal = noch keine Roh-Nachrichten -> kein
    Werben, bis genug echte Chat-Aktivität da war.
    """

    def setUp(self) -> None:
        self.handler = _DummyPromoGate()
        stack = ExitStack()
        self.addCleanup(stack.close)
        stack.enter_context(
            patch("bot.chat.promos.PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO", 16)
        )
        stack.enter_context(patch("bot.chat.promos.PROMO_ACTIVITY_MIN_MSGS", 3))
        stack.enter_context(patch("bot.chat.promos.PROMO_ACTIVITY_MIN_CHATTERS", 1))
        stack.enter_context(patch("bot.chat.promos.PROMO_NEW_CHATTERS_MIN", 2))

    def test_blocks_when_no_activity_at_session_start(self) -> None:
        # Brandneuer Kanal: 0 Roh-Nachrichten < Mindestschwelle -> geblockt.
        self.assertFalse(self.handler._promo_activity_ready("partner_one", 1000.0))

    def test_blocks_when_raw_messages_below_threshold(self) -> None:
        self.handler._raw_msg_count_since_promo = {"partner_one": 5}
        self.handler._promo_activity = {
            "partner_one": deque([(1000.0, "a"), (1000.0, "b"), (1000.0, "c")])
        }
        self.assertFalse(self.handler._promo_activity_ready("partner_one", 1000.0))

    def test_ready_when_enough_activity(self) -> None:
        now = 1000.0
        self.handler._raw_msg_count_since_promo = {"partner_one": 20}
        self.handler._promo_activity = {
            "partner_one": deque([(now, "a"), (now, "b"), (now, "c")])
        }
        self.assertTrue(self.handler._promo_activity_ready("partner_one", now))


if __name__ == "__main__":
    unittest.main()
