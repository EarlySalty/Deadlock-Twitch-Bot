"""Regression: der self-explainer Discord-Relay muss denselben Broker-Token-Fallback
nutzen wie der Master (`bot_core/master_bot.py`) und der Live-Announcement-Pfad
(`bot/monitoring/monitoring.py`).

Vor dem Fix prüfte der Relay nur `MASTER_BROKER_TOKEN` und gab mit 503
(`master_broker_token_missing`) auf — obwohl der Worker `TWITCH_INTERNAL_API_TOKEN`
hat, exakt den geteilten Token, den der Broker als Auth akzeptiert.
"""

from __future__ import annotations

import os
import unittest
from unittest import mock

from bot.internal_api.routes.discord_log import _idempotency_key, _master_broker_token


class MasterBrokerTokenFallbackTest(unittest.TestCase):
    def test_prefers_explicit_master_broker_token(self) -> None:
        with mock.patch.dict(
            os.environ,
            {
                "MASTER_BROKER_TOKEN": "broker-token",
                "MAIN_BOT_INTERNAL_TOKEN": "main-token",
                "TWITCH_INTERNAL_API_TOKEN": "shared-token",
            },
            clear=False,
        ):
            self.assertEqual(_master_broker_token(), "broker-token")

    def test_falls_back_to_main_bot_internal_token(self) -> None:
        with mock.patch.dict(
            os.environ,
            {
                "MASTER_BROKER_TOKEN": "",
                "MAIN_BOT_INTERNAL_TOKEN": "main-token",
                "TWITCH_INTERNAL_API_TOKEN": "shared-token",
            },
            clear=False,
        ):
            self.assertEqual(_master_broker_token(), "main-token")

    def test_falls_back_to_shared_internal_token(self) -> None:
        # Kern-Regression: Worker hat nur den geteilten Internal-Token -> kein 503 mehr.
        with mock.patch.dict(
            os.environ,
            {
                "MASTER_BROKER_TOKEN": "",
                "MAIN_BOT_INTERNAL_TOKEN": "",
                "TWITCH_INTERNAL_API_TOKEN": "shared-token",
            },
            clear=False,
        ):
            self.assertEqual(_master_broker_token(), "shared-token")

    def test_empty_when_nothing_configured(self) -> None:
        with mock.patch.dict(
            os.environ,
            {
                "MASTER_BROKER_TOKEN": "",
                "MAIN_BOT_INTERNAL_TOKEN": "",
                "TWITCH_INTERNAL_API_TOKEN": "",
            },
            clear=False,
        ):
            self.assertEqual(_master_broker_token(), "")


class IdempotencyKeyTest(unittest.TestCase):
    def test_deterministic_prefixed_and_bounded(self) -> None:
        payload = {"channel_id": 1374364800817303632, "embed": {"title": "Frage", "fields": []}}
        first = _idempotency_key(payload)
        second = _idempotency_key(dict(payload))
        self.assertEqual(first, second)
        self.assertTrue(first.startswith("self-explainer:"))
        self.assertLessEqual(len(first), 128)

    def test_distinct_payloads_yield_distinct_keys(self) -> None:
        a = _idempotency_key({"channel_id": 1, "embed": {"title": "a"}})
        b = _idempotency_key({"channel_id": 1, "embed": {"title": "b"}})
        self.assertNotEqual(a, b)


if __name__ == "__main__":
    unittest.main()
