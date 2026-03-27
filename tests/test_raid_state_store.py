from __future__ import annotations

import unittest
from types import SimpleNamespace

from bot.raid.raid_state_store import RaidStateStore, RaidStateStoreConfig


class RaidStateStoreTests(unittest.TestCase):
    def test_promote_stale_orphan_keeps_payload_when_processing_fails(self) -> None:
        owner = SimpleNamespace(
            _pending_raids={},
            _recent_raid_arrivals={},
            _orphan_chat_raid_notifications={},
            _raid_readiness_by_flow_id={},
        )
        store = RaidStateStore(
            owner,
            config=RaidStateStoreConfig(
                orphan_chat_notification_grace_seconds=15.0,
                orphan_chat_notification_retention_seconds=900.0,
            ),
            now=lambda: 100.0,
        )
        store.store_orphan_chat_raid_notification(
            {
                "to_broadcaster_id": "9009",
                "to_broadcaster_login": "targetlogin",
                "from_broadcaster_login": "source_login",
                "viewer_count": 21,
                "observed_ts": 0.0,
            }
        )

        store.promote_stale_orphan_chat_raid_notifications(
            process_independent_partner_raid_arrival=lambda **_kwargs: False
        )

        self.assertEqual(len(owner._orphan_chat_raid_notifications), 1)

    def test_promote_stale_orphan_removes_payload_when_processing_succeeds(self) -> None:
        owner = SimpleNamespace(
            _pending_raids={},
            _recent_raid_arrivals={},
            _orphan_chat_raid_notifications={},
            _raid_readiness_by_flow_id={},
        )
        store = RaidStateStore(
            owner,
            config=RaidStateStoreConfig(
                orphan_chat_notification_grace_seconds=15.0,
                orphan_chat_notification_retention_seconds=900.0,
            ),
            now=lambda: 100.0,
        )
        store.store_orphan_chat_raid_notification(
            {
                "to_broadcaster_id": "9009",
                "to_broadcaster_login": "targetlogin",
                "from_broadcaster_login": "source_login",
                "viewer_count": 21,
                "observed_ts": 0.0,
            }
        )

        store.promote_stale_orphan_chat_raid_notifications(
            process_independent_partner_raid_arrival=lambda **_kwargs: True
        )

        self.assertEqual(owner._orphan_chat_raid_notifications, {})

    def test_lookup_recent_raid_arrival_expires_old_entries(self) -> None:
        owner = SimpleNamespace(
            _pending_raids={},
            _recent_raid_arrivals={},
            _orphan_chat_raid_notifications={},
            _raid_readiness_by_flow_id={},
        )
        store = RaidStateStore(
            owner,
            config=RaidStateStoreConfig(recent_raid_arrival_ttl_seconds=10.0),
            now=lambda: 25.0,
        )
        store.remember_recent_raid_arrival(
            to_broadcaster_id="9009",
            from_broadcaster_login="source_login",
            from_broadcaster_id="1001",
            to_broadcaster_login="targetlogin",
            viewer_count=21,
            classification="external_to_partner",
            confirmation_signals={"channel.raid"},
            arrival_tracking_id=123,
            raid_flow_id="flow-1",
        )
        owner._recent_raid_arrivals[("9009", "source_login")]["confirmed_ts"] = 0.0

        result = store.lookup_recent_raid_arrival(
            to_broadcaster_id="9009",
            from_broadcaster_login="source_login",
        )

        self.assertIsNone(result)
        self.assertEqual(owner._recent_raid_arrivals, {})


if __name__ == "__main__":
    unittest.main()
