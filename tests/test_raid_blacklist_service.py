from __future__ import annotations

from types import SimpleNamespace
import unittest

from bot.raid.raid_blacklist import RaidBlacklistCallbacks, RaidBlacklistConfig, RaidBlacklistService


class _BlacklistHarness:
    def __init__(self) -> None:
        self.blacklist_rows: list[object] = []
        self.due_recruitment_rows: list[object] = []
        self.due_ban_rows: list[object] = []
        self.stored_blacklist_entries: list[dict[str, object]] = []
        self.scheduled_recruitment_blacklists: list[dict[str, object]] = []
        self.deleted_recruitment_blacklists: list[str] = []
        self.deleted_target_ban_checks: list[str] = []
        self.rescheduled_target_ban_checks: list[tuple[str, int]] = []
        self.blacklisted_matches: set[tuple[str, str]] = set()
        self.partner_matches: set[tuple[str, str]] = set()
        self.chat_bot: object | None = object()
        self.part_calls: list[tuple[object, list[str]]] = []
        self.join_calls: list[tuple[object, str, str | None]] = []
        self.join_result: bool = True

    def load_blacklist_rows(self):
        return self.blacklist_rows

    def is_blacklisted(self, *, target_id: str, target_login: str) -> bool:
        return (target_id, target_login) in self.blacklisted_matches

    def store_blacklist_entry(self, *, target_id: str | None, target_login: str, reason: str) -> None:
        self.stored_blacklist_entries.append(
            {"target_id": target_id, "target_login": target_login, "reason": reason}
        )

    def load_due_external_recruitment_blacklist_pending(self):
        return self.due_recruitment_rows

    def schedule_external_recruitment_blacklist_pending(
        self,
        *,
        target_id: str,
        target_login: str,
        confirmed_raid_count: int,
        raid_flow_id: str | None,
        grace_seconds: int,
    ) -> None:
        self.scheduled_recruitment_blacklists.append(
            {
                "target_id": target_id,
                "target_login": target_login,
                "confirmed_raid_count": confirmed_raid_count,
                "raid_flow_id": raid_flow_id,
                "grace_seconds": grace_seconds,
            }
        )

    def delete_external_recruitment_blacklist_pending(self, target_id: str) -> None:
        self.deleted_recruitment_blacklists.append(target_id)

    def is_target_partner(self, *, target_id: str, target_login: str) -> bool:
        return (target_id, target_login) in self.partner_matches

    def load_due_external_target_ban_checks(self):
        return self.due_ban_rows

    def schedule_external_target_ban_check(
        self,
        *,
        target_id: str | None,
        target_login: str,
        source: str,
        delay_seconds: int,
    ) -> None:
        self.scheduled_ban_check = {
            "target_id": target_id,
            "target_login": target_login,
            "source": source,
            "delay_seconds": delay_seconds,
        }

    def delete_external_target_ban_check_pending(self, target_id: str) -> None:
        self.deleted_target_ban_checks.append(target_id)

    def reschedule_external_target_ban_check_pending(self, target_id: str, delay_seconds: int) -> None:
        self.rescheduled_target_ban_checks.append((target_id, delay_seconds))

    def get_chat_bot(self):
        return self.chat_bot

    async def part_chat_channels(self, chat_bot: object, channels: list[str]) -> None:
        self.part_calls.append((chat_bot, list(channels)))

    async def join_chat_channel(
        self,
        chat_bot: object,
        channel_login: str,
        channel_id: str | None,
    ) -> bool:
        self.join_calls.append((chat_bot, channel_login, channel_id))
        return self.join_result


class RaidBlacklistServiceTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.backend = _BlacklistHarness()
        self.service = RaidBlacklistService(
            RaidBlacklistCallbacks(
                load_blacklist_rows=self.backend.load_blacklist_rows,
                is_blacklisted=self.backend.is_blacklisted,
                store_blacklist_entry=self.backend.store_blacklist_entry,
                load_due_external_recruitment_blacklist_pending=self.backend.load_due_external_recruitment_blacklist_pending,
                schedule_external_recruitment_blacklist_pending=self.backend.schedule_external_recruitment_blacklist_pending,
                delete_external_recruitment_blacklist_pending=self.backend.delete_external_recruitment_blacklist_pending,
                is_target_partner=self.backend.is_target_partner,
                load_due_external_target_ban_checks=self.backend.load_due_external_target_ban_checks,
                schedule_external_target_ban_check=self.backend.schedule_external_target_ban_check,
                delete_external_target_ban_check_pending=self.backend.delete_external_target_ban_check_pending,
                reschedule_external_target_ban_check_pending=self.backend.reschedule_external_target_ban_check_pending,
                get_chat_bot=self.backend.get_chat_bot,
                join_chat_channel=self.backend.join_chat_channel,
                part_chat_channels=self.backend.part_chat_channels,
            ),
            config=RaidBlacklistConfig(
                external_recruitment_raid_limit=4,
                external_recruitment_blacklist_grace_seconds=172800,
                external_target_ban_check_delay_seconds=3600,
                external_target_ban_check_reschedule_seconds=900,
            ),
        )

    def test_load_raid_blacklist_normalizes_rows(self) -> None:
        self.backend.blacklist_rows = [
            ("123", "Alpha"),
            {"target_id": "456", "target_login": "Beta"},
            SimpleNamespace(target_id=" 789 ", target_login="Gamma"),
            (None, None),
        ]

        blacklisted_ids, blacklisted_logins = self.service.load_raid_blacklist()

        self.assertEqual(blacklisted_ids, {"123", "456", "789"})
        self.assertEqual(blacklisted_logins, {"alpha", "beta", "gamma"})

    def test_is_blacklisted_returns_false_on_error(self) -> None:
        service = RaidBlacklistService(
            RaidBlacklistCallbacks(
                load_blacklist_rows=self.backend.load_blacklist_rows,
                is_blacklisted=lambda **_: (_ for _ in ()).throw(RuntimeError("boom")),
                store_blacklist_entry=self.backend.store_blacklist_entry,
                load_due_external_recruitment_blacklist_pending=self.backend.load_due_external_recruitment_blacklist_pending,
                schedule_external_recruitment_blacklist_pending=self.backend.schedule_external_recruitment_blacklist_pending,
                delete_external_recruitment_blacklist_pending=self.backend.delete_external_recruitment_blacklist_pending,
                is_target_partner=self.backend.is_target_partner,
                load_due_external_target_ban_checks=self.backend.load_due_external_target_ban_checks,
                schedule_external_target_ban_check=self.backend.schedule_external_target_ban_check,
                delete_external_target_ban_check_pending=self.backend.delete_external_target_ban_check_pending,
                reschedule_external_target_ban_check_pending=self.backend.reschedule_external_target_ban_check_pending,
                get_chat_bot=self.backend.get_chat_bot,
                join_chat_channel=self.backend.join_chat_channel,
                part_chat_channels=self.backend.part_chat_channels,
            )
        )

        self.assertFalse(service.is_blacklisted(" 123 ", "Alpha"))

    def test_add_to_blacklist_normalizes_and_delegates(self) -> None:
        self.service.add_to_blacklist(" 123 ", "Alpha ", "reason text")

        self.assertEqual(
            self.backend.stored_blacklist_entries,
            [{"target_id": "123", "target_login": "alpha", "reason": "reason text"}],
        )

    def test_add_to_blacklist_ignores_blank_login(self) -> None:
        self.service.add_to_blacklist("123", "   ", "reason text")

        self.assertEqual(self.backend.stored_blacklist_entries, [])

    def test_schedule_external_recruitment_blacklist_pending_respects_threshold_and_partner_skip(self) -> None:
        self.service.schedule_external_recruitment_blacklist_pending(
            target_id="123",
            target_login="Alpha",
            confirmed_raid_count=3,
            raid_flow_id="raid-1",
        )
        self.assertEqual(self.backend.scheduled_recruitment_blacklists, [])

        self.backend.partner_matches.add(("123", "alpha"))
        self.service.schedule_external_recruitment_blacklist_pending(
            target_id="123",
            target_login="Alpha",
            confirmed_raid_count=4,
            raid_flow_id="raid-2",
        )
        self.assertEqual(self.backend.deleted_recruitment_blacklists, ["123"])
        self.assertEqual(self.backend.scheduled_recruitment_blacklists, [])

        self.backend.partner_matches.clear()
        self.service.schedule_external_recruitment_blacklist_pending(
            target_id="123",
            target_login="Alpha",
            confirmed_raid_count=4,
            raid_flow_id="raid-3",
        )
        self.assertEqual(
            self.backend.scheduled_recruitment_blacklists,
            [
                {
                    "target_id": "123",
                    "target_login": "alpha",
                    "confirmed_raid_count": 4,
                    "raid_flow_id": "raid-3",
                    "grace_seconds": 172800,
                }
            ],
        )

    def test_process_due_external_recruitment_blacklist_pending_adds_and_deletes(self) -> None:
        self.backend.due_recruitment_rows = [
            ("123", "Alpha", 4, "2026-03-27T12:00:00+00:00"),
        ]

        self.service.process_due_external_recruitment_blacklist_pending()

        self.assertEqual(
            self.backend.stored_blacklist_entries,
            [
                {
                    "target_id": "123",
                    "target_login": "alpha",
                    "reason": (
                        "confirmed_external_recruitment_limit_grace_expired:"
                        " count=4 limit=4 threshold_reached_at=2026-03-27T12:00:00+00:00"
                    ),
                }
            ],
        )
        self.assertEqual(self.backend.deleted_recruitment_blacklists, ["123"])

    def test_process_due_external_recruitment_blacklist_pending_deletes_if_already_blacklisted(self) -> None:
        self.backend.due_recruitment_rows = [
            ("123", "Alpha", 4, "2026-03-27T12:00:00+00:00"),
        ]
        self.backend.blacklisted_matches.add(("123", "alpha"))

        self.service.process_due_external_recruitment_blacklist_pending()

        self.assertEqual(self.backend.stored_blacklist_entries, [])
        self.assertEqual(self.backend.deleted_recruitment_blacklists, ["123"])

    def test_schedule_external_target_ban_check_normalizes_and_uses_delay(self) -> None:
        self.service.schedule_external_target_ban_check(
            target_id=" 123 ",
            target_login="Alpha",
            source="Recruitment",
        )

        self.assertEqual(
            self.backend.scheduled_ban_check,
            {
                "target_id": "123",
                "target_login": "alpha",
                "source": "recruitment",
                "delay_seconds": 3600,
            },
        )

    async def test_process_due_external_target_ban_checks_reschedules_without_bot(self) -> None:
        self.backend.due_ban_rows = [
            ("123", "Alpha", "Recruitment"),
        ]
        self.backend.chat_bot = None

        await self.service.process_due_external_target_ban_checks()

        self.assertEqual(self.backend.rescheduled_target_ban_checks, [("123", 900)])
        self.assertEqual(self.backend.join_calls, [])
        self.assertEqual(self.backend.part_calls, [])

    async def test_process_due_external_target_ban_checks_partitions_joins_and_deletes(self) -> None:
        self.backend.due_ban_rows = [
            ("123", "Alpha", "Recruitment"),
        ]

        await self.service.process_due_external_target_ban_checks()

        self.assertEqual(len(self.backend.part_calls), 1)
        self.assertEqual(len(self.backend.join_calls), 1)
        self.assertEqual(self.backend.deleted_target_ban_checks, ["123"])
        self.assertEqual(self.backend.rescheduled_target_ban_checks, [])
        self.assertEqual(self.backend.join_calls[0][1:], ("alpha", "123"))

    async def test_process_due_external_target_ban_checks_reschedules_on_join_failure(self) -> None:
        self.backend.due_ban_rows = [
            ("123", "Alpha", "Recruitment"),
        ]
        self.backend.join_result = False

        await self.service.process_due_external_target_ban_checks()

        self.assertEqual(self.backend.deleted_target_ban_checks, [])
        self.assertEqual(self.backend.rescheduled_target_ban_checks, [("123", 900)])

    async def test_process_due_external_target_ban_checks_deletes_when_join_false_but_blacklisted(self) -> None:
        self.backend.due_ban_rows = [
            ("123", "Alpha", "Recruitment"),
        ]
        self.backend.join_result = False
        self.backend.blacklisted_matches.add(("123", "alpha"))

        await self.service.process_due_external_target_ban_checks()

        self.assertEqual(self.backend.deleted_target_ban_checks, ["123"])
        self.assertEqual(self.backend.rescheduled_target_ban_checks, [])


if __name__ == "__main__":
    unittest.main()
