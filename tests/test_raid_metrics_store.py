from __future__ import annotations

import unittest
from unittest.mock import Mock

from bot.raid.raid_metrics_store import RaidMetricsStore


class _ContextManagerStub:
    def __init__(self, conn) -> None:
        self._conn = conn

    def __enter__(self):
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class RaidMetricsStoreTests(unittest.TestCase):
    def test_record_confirmed_external_recruitment_raid_falls_back_to_count_on_insert_failure(self) -> None:
        logger = Mock()
        transaction = Mock(side_effect=RuntimeError("boom"))
        readonly_conn = Mock()
        readonly_conn.execute.return_value.fetchone.return_value = (7,)
        store = RaidMetricsStore(
            readonly_connection=lambda: _ContextManagerStub(readonly_conn),
            transaction=transaction,
            normalize_broadcaster_login=lambda value: str(value or "").strip().lower(),
            is_partner_target_channel=lambda **_kwargs: False,
            next_raid_observability_flow_id=lambda **_kwargs: "flow-1",
            logger=logger,
        )

        result = store.record_confirmed_external_recruitment_raid(
            raid_flow_id=None,
            from_broadcaster_id="123",
            from_broadcaster_login="Source",
            to_broadcaster_id="456",
            to_broadcaster_login="Target",
            viewer_count=9,
            confirmation_signal="chat_notification",
        )

        self.assertEqual(result, 7)
        logger.exception.assert_called_once()

    def test_get_recent_raid_targets_normalizes_source_id_before_query(self) -> None:
        readonly_conn = Mock()
        readonly_conn.execute.return_value.fetchall.return_value = [("2001",), ("2002",), (None,)]
        store = RaidMetricsStore(
            readonly_connection=lambda: _ContextManagerStub(readonly_conn),
            transaction=lambda: _ContextManagerStub(Mock()),
            normalize_broadcaster_login=lambda value: str(value or "").strip().lower(),
            is_partner_target_channel=lambda **_kwargs: False,
            next_raid_observability_flow_id=lambda **_kwargs: "flow-1",
        )

        result = store.get_recent_raid_targets(" 1001 ", 7)

        self.assertEqual(result, {"2001", "2002"})
        execute_args = readonly_conn.execute.call_args[0][1]
        self.assertEqual(execute_args, ("1001", "7 days"))


if __name__ == "__main__":
    unittest.main()
