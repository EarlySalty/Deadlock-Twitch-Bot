import unittest

from bot.raid.partner_resolution import (
    PartnerRaidArrivalResolution,
    classify_partner_raid_arrival,
    is_partner_target_channel,
    normalize_broadcaster_login,
)


class _PartnerLookup:
    def __init__(self, rows: dict[tuple[str | None, str | None], object]) -> None:
        self.rows = rows
        self.calls: list[dict[str, str | None]] = []

    def __call__(
        self,
        *,
        twitch_user_id: str | None = None,
        twitch_login: str | None = None,
    ) -> object:
        self.calls.append(
            {
                "twitch_user_id": twitch_user_id,
                "twitch_login": twitch_login,
            }
        )
        return self.rows.get((twitch_user_id, twitch_login))


class _KnownStreamerLookup:
    def __init__(self, row: object | None) -> None:
        self.row = row
        self.calls: list[dict[str, str | None]] = []

    def __call__(
        self,
        *,
        broadcaster_id: str | None = None,
        broadcaster_login: str | None = None,
    ) -> object | None:
        self.calls.append(
            {
                "broadcaster_id": broadcaster_id,
                "broadcaster_login": broadcaster_login,
            }
        )
        return self.row


class PartnerResolutionTests(unittest.TestCase):
    def test_normalize_broadcaster_login_strips_and_lowercases(self) -> None:
        self.assertEqual(normalize_broadcaster_login("  TeStLogin  "), "testlogin")

    def test_is_partner_target_channel_uses_normalized_inputs(self) -> None:
        lookup = _PartnerLookup(
            {
                ("9009", "targetlogin"): {"twitch_user_id": "9009"},
            }
        )

        self.assertTrue(
            is_partner_target_channel(
                broadcaster_id=" 9009 ",
                broadcaster_login=" TargetLogin ",
                partner_lookup=lookup,
            )
        )
        self.assertEqual(
            lookup.calls,
            [{"twitch_user_id": "9009", "twitch_login": "targetlogin"}],
        )

    def test_classify_partner_raid_arrival_marks_known_source_by_user_id(self) -> None:
        partner_lookup = _PartnerLookup(
            {
                ("9009", "targetlogin"): {"twitch_user_id": "9009"},
            }
        )
        known_lookup = _KnownStreamerLookup(
            {"twitch_user_id": "1001", "twitch_login": "source_login"}
        )

        result = classify_partner_raid_arrival(
            from_broadcaster_login="Source_Login",
            from_broadcaster_id="1001",
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            partner_lookup=partner_lookup,
            known_streamer_lookup=known_lookup,
        )

        self.assertEqual(
            result,
            PartnerRaidArrivalResolution(
                classification="ours_to_partner",
                source_resolution="known_streamer_id",
                target_is_partner=True,
                from_broadcaster_id="1001",
                from_broadcaster_login="source_login",
                to_broadcaster_id="9009",
                to_broadcaster_login="targetlogin",
            ),
        )
        self.assertEqual(
            known_lookup.calls,
            [{"broadcaster_id": "1001", "broadcaster_login": "source_login"}],
        )

    def test_classify_partner_raid_arrival_marks_external_source(self) -> None:
        partner_lookup = _PartnerLookup(
            {
                ("9009", "targetlogin"): {"twitch_user_id": "9009"},
            }
        )
        known_lookup = _KnownStreamerLookup(None)

        result = classify_partner_raid_arrival(
            from_broadcaster_login="external_login",
            from_broadcaster_id="",
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            partner_lookup=partner_lookup,
            known_streamer_lookup=known_lookup,
        )

        self.assertEqual(result.classification, "external_to_partner")
        self.assertEqual(result.source_resolution, "unmatched_source")
        self.assertTrue(result.target_is_partner)
        self.assertEqual(
            known_lookup.calls,
            [{"broadcaster_id": None, "broadcaster_login": "external_login"}],
        )

    def test_classify_partner_raid_arrival_skips_known_lookup_for_non_partner_target(self) -> None:
        partner_lookup = _PartnerLookup({})
        known_lookup = _KnownStreamerLookup({"twitch_user_id": "1001"})

        result = classify_partner_raid_arrival(
            from_broadcaster_login="source_login",
            from_broadcaster_id="1001",
            to_broadcaster_id="9009",
            to_broadcaster_login="TargetLogin",
            partner_lookup=partner_lookup,
            known_streamer_lookup=known_lookup,
        )

        self.assertIsNone(result.classification)
        self.assertEqual(result.source_resolution, "non_partner_target")
        self.assertFalse(result.target_is_partner)
        self.assertEqual(known_lookup.calls, [])


if __name__ == "__main__":
    unittest.main()
