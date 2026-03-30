from __future__ import annotations

import unittest
from contextlib import contextmanager
from unittest.mock import Mock, patch

from bot import storage
from bot.storage import pg
from bot.storage import promo_cooldowns


class StoragePromoCooldownTests(unittest.TestCase):
    def test_public_reexports_point_to_dedicated_module(self) -> None:
        self.assertIs(storage.save_promo_cooldown, promo_cooldowns.save_promo_cooldown)
        self.assertIs(storage.load_promo_cooldowns, promo_cooldowns.load_promo_cooldowns)
        self.assertIs(
            storage.cleanup_stale_promo_cooldowns,
            promo_cooldowns.cleanup_stale_promo_cooldowns,
        )
        self.assertIs(pg.save_promo_cooldown, promo_cooldowns.save_promo_cooldown)

    def test_save_and_cleanup_use_storage_transactions(self) -> None:
        conn = Mock()

        @contextmanager
        def _txn():
            yield conn

        with patch("bot.storage.pg.transaction", side_effect=_txn):
            promo_cooldowns.save_promo_cooldown("Alpha", "sent", 123.0)
            promo_cooldowns.cleanup_stale_promo_cooldowns(24)

        self.assertEqual(conn.execute.call_count, 2)
        self.assertEqual(conn.execute.call_args_list[0].args[0], "INSERT INTO twitch_promo_cooldowns (login, cooldown_type, wall_ts, updated_at)\n                   VALUES (%s, %s, %s, now())\n                   ON CONFLICT (login, cooldown_type)\n                   DO UPDATE SET wall_ts = EXCLUDED.wall_ts, updated_at = now()")
        self.assertEqual(conn.execute.call_args_list[0].args[1], ("alpha", "sent", 123.0))
        self.assertEqual(conn.execute.call_args_list[1].args[0], "DELETE FROM twitch_promo_cooldowns WHERE wall_ts < %s")


if __name__ == "__main__":
    unittest.main()
