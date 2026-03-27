from __future__ import annotations

import unittest

from bot.raid.pending_raids import PendingRaid, PendingRaidStore


class PendingRaidTests(unittest.TestCase):
    def test_legacy_tuple_payload_is_coerced_into_typed_pending_raid(self) -> None:
        raid = PendingRaid.from_payload(
            (
                "Source_Login",
                {"user_login": "target_login"},
                123.5,
                1,
                42,
                9.25,
            ),
            to_broadcaster_id=" 9009 ",
        )

        self.assertIsInstance(raid, PendingRaid)
        assert raid is not None
        self.assertEqual(raid.from_broadcaster_login, "source_login")
        self.assertEqual(raid.to_broadcaster_id, "9009")
        self.assertEqual(raid.target_stream_data, {"user_login": "target_login"})
        self.assertEqual(raid.registered_ts, 123.5)
        self.assertTrue(raid.is_partner_raid)
        self.assertEqual(raid.registered_viewer_count, 42)
        self.assertEqual(raid.offline_trigger_ts, 9.25)

    def test_normalized_tuple_key_migration_rewrites_backing_mapping(self) -> None:
        backing = {
            (" 9009 ", "Source_Login"): {
                "from_broadcaster_login": "Source_Login",
                "target_stream_data": {"user_login": "target_login"},
                "registered_ts": 123.5,
                "is_partner_raid": True,
                "registered_viewer_count": 42,
                "offline_trigger_ts": 9.25,
            }
        }
        store = PendingRaidStore(backing)

        entries = list(store.iter_entries())

        self.assertEqual(len(entries), 1)
        self.assertEqual(entries[0].key, ("9009", "source_login"))
        self.assertIn(("9009", "source_login"), backing)
        self.assertNotIn((" 9009 ", "Source_Login"), backing)
        self.assertIsInstance(backing[("9009", "source_login")], PendingRaid)

    def test_exact_get_and_pop_use_normalized_key(self) -> None:
        store = PendingRaidStore()
        store.store(
            {
                "from_broadcaster_login": "Source_Login",
                "to_broadcaster_id": "9009",
                "registered_ts": 123.5,
                "registered_viewer_count": 7,
            }
        )
        store.store(
            {
                "from_broadcaster_login": "Other_Source",
                "to_broadcaster_id": "9009",
                "registered_ts": 124.5,
                "registered_viewer_count": 8,
            }
        )

        exact = store.get(
            to_broadcaster_id="9009",
            from_broadcaster_login=" source_login ",
        )
        self.assertIsInstance(exact, PendingRaid)
        assert exact is not None
        self.assertEqual(exact.registered_viewer_count, 7)

        popped = store.pop(
            to_broadcaster_id="9009",
            from_broadcaster_login="source_login",
        )
        self.assertIsInstance(popped, PendingRaid)
        assert popped is not None
        self.assertEqual(popped.registered_viewer_count, 7)
        self.assertIsNone(
            store.get(to_broadcaster_id="9009", from_broadcaster_login="source_login")
        )
        self.assertIsNotNone(
            store.get(to_broadcaster_id="9009", from_broadcaster_login="other_source")
        )

    def test_supersede_from_source_removes_older_targets(self) -> None:
        store = PendingRaidStore()
        store.store(
            {
                "from_broadcaster_login": "source_login",
                "to_broadcaster_id": "old-target",
                "registered_ts": 123.5,
            }
        )
        store.store(
            {
                "from_broadcaster_login": "source_login",
                "to_broadcaster_id": "new-target",
                "registered_ts": 124.5,
            }
        )
        store.store(
            {
                "from_broadcaster_login": "other_source",
                "to_broadcaster_id": "other-target",
                "registered_ts": 125.5,
            }
        )

        removed = store.supersede_from_source(
            from_broadcaster_login="Source_Login",
            current_target_id="new-target",
        )

        self.assertEqual(len(removed), 1)
        self.assertEqual(removed[0].to_broadcaster_id, "old-target")
        self.assertIsNone(
            store.get(to_broadcaster_id="old-target", from_broadcaster_login="source_login")
        )
        self.assertIsNotNone(
            store.get(to_broadcaster_id="new-target", from_broadcaster_login="source_login")
        )
        self.assertIsNotNone(
            store.get(to_broadcaster_id="other-target", from_broadcaster_login="other_source")
        )

    def test_cleanup_stale_removes_expired_entries(self) -> None:
        store = PendingRaidStore()
        store.store(
            {
                "from_broadcaster_login": "source_login",
                "to_broadcaster_id": "old-target",
                "registered_ts": 10.0,
            }
        )
        store.store(
            {
                "from_broadcaster_login": "source_login",
                "to_broadcaster_id": "fresh-target",
                "registered_ts": 20.0,
            }
        )

        removed = store.cleanup_stale(timeout_seconds=5.0, now=20.0)

        self.assertEqual(len(removed), 1)
        self.assertEqual(removed[0].to_broadcaster_id, "old-target")
        self.assertIsNone(
            store.get(to_broadcaster_id="old-target", from_broadcaster_login="source_login")
        )
        self.assertIsNotNone(
            store.get(to_broadcaster_id="fresh-target", from_broadcaster_login="source_login")
        )

    def test_mapping_payload_with_invalid_numeric_values_is_normalized_without_raising(self) -> None:
        raid = PendingRaid.from_payload(
            {
                "from_broadcaster_login": "Source_Login",
                "to_broadcaster_id": "9009",
                "registered_ts": "abc",
                "registered_viewer_count": "not-a-number",
                "offline_trigger_ts": "still-bad",
            },
            now=321.5,
        )

        self.assertIsInstance(raid, PendingRaid)
        assert raid is not None
        self.assertEqual(raid.registered_ts, 321.5)
        self.assertEqual(raid.registered_viewer_count, 0)
        self.assertIsNone(raid.offline_trigger_ts)

    def test_mapping_payload_string_boolean_values_are_coerced_correctly(self) -> None:
        raid = PendingRaid.from_payload(
            {
                "from_broadcaster_login": "Source_Login",
                "to_broadcaster_id": "9009",
                "registered_ts": 123.5,
                "is_partner_raid": "false",
                "channel_raid_ready": "0",
            },
        )

        self.assertIsInstance(raid, PendingRaid)
        assert raid is not None
        self.assertFalse(raid.is_partner_raid)
        self.assertFalse(raid.channel_raid_ready)

    def test_normalize_in_place_tolerates_invalid_legacy_numeric_values(self) -> None:
        backing = {
            ("9009", "source_login"): (
                "Source_Login",
                {"user_login": "target_login"},
                "bad-ts",
                True,
                "bad-viewers",
                "bad-offline-ts",
            )
        }
        store = PendingRaidStore(backing)

        entries = list(store.iter_entries())

        self.assertEqual(len(entries), 1)
        raid = entries[0].raid
        self.assertIsInstance(raid.registered_ts, float)
        self.assertEqual(raid.registered_viewer_count, 0)
        self.assertIsNone(raid.offline_trigger_ts)


if __name__ == "__main__":
    unittest.main()
