from __future__ import annotations

import contextlib
import unittest
from types import SimpleNamespace

from bot.raid.manual_raid_suppression import (
    ManualRaidSuppressionDependencies,
    ManualRaidSuppressionService,
)


class ManualRaidSuppressionServiceTests(unittest.TestCase):
    def test_mark_and_cleanup_expired_suppressions(self) -> None:
        now = [100.0]
        owner = SimpleNamespace(_manual_raid_suppression={})
        service = ManualRaidSuppressionService(
            owner,
            ManualRaidSuppressionDependencies(
                readonly_connection=lambda: contextlib.nullcontext(None),
                load_active_partner=lambda *_args, **_kwargs: None,
                now=lambda: now[0],
            ),
        )

        service.mark_manual_raid_started("1001", ttl_seconds=30.0)
        self.assertTrue(service.is_offline_auto_raid_suppressed("1001"))

        now[0] = 131.0
        service.cleanup_expired_manual_raid_suppressions()

        self.assertFalse(service.is_offline_auto_raid_suppressed("1001"))
        self.assertEqual(owner._manual_raid_suppression, {})


if __name__ == "__main__":
    unittest.main()
