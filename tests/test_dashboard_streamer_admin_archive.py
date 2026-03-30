from __future__ import annotations

import contextlib
import unittest
from unittest import mock

from bot.dashboard.streamer_admin_mixin import _dashboard_archive_sync


@contextlib.contextmanager
def _dummy_transaction():
    yield object()


class DashboardArchiveSyncTests(unittest.TestCase):
    def test_legacy_archived_partner_is_reactivated_instead_of_treated_as_departnered(self) -> None:
        history_row = {
            "status": "archived",
            "departnered_at": "2026-03-20T10:00:00+00:00",
            "admin_archived_at": None,
        }

        with (
            mock.patch(
                "bot.dashboard.streamer_admin_mixin.storage.transaction",
                return_value=_dummy_transaction(),
            ),
            mock.patch(
                "bot.dashboard.streamer_admin_mixin.storage.load_active_partner",
                return_value=None,
            ),
            mock.patch(
                "bot.dashboard.streamer_admin_mixin.storage.load_latest_partner_history",
                return_value=history_row,
            ),
            mock.patch(
                "bot.dashboard.streamer_admin_mixin.storage.reactivate_partner",
                return_value={"twitch_login": "alpha", "twitch_user_id": "1001"},
            ) as mocked_reactivate,
        ):
            result = _dashboard_archive_sync("alpha", "unarchive")

        self.assertEqual(result, "alpha ent-archiviert")
        mocked_reactivate.assert_called_once()


if __name__ == "__main__":
    unittest.main()
