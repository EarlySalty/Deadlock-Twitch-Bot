import asyncio
import unittest
from unittest import mock

from bot.engagement import deadlock_wiki as dw


class DisplayNameTests(unittest.TestCase):
    def test_real_item_name_kept(self):
        self.assertEqual(
            dw._display_item_name({"name": "Headshot Booster", "class_name": "upgrade_hs"}),
            "Headshot Booster",
        )

    def test_internal_name_equals_class_dropped(self):
        self.assertIsNone(
            dw._display_item_name(
                {"name": "citadel_weapon_bosstier2_set", "class_name": "citadel_weapon_bosstier2_set"}
            )
        )

    def test_snake_case_name_dropped(self):
        self.assertIsNone(
            dw._display_item_name({"name": "some_internal_thing", "class_name": "other"})
        )

    def test_hero_name(self):
        self.assertEqual(dw._hero_name({"name": "Pocket"}), "Pocket")
        self.assertIsNone(dw._hero_name({"name": ""}))


class DetectEntityTests(unittest.TestCase):
    def setUp(self):
        # Index wie nach dem Laden: längster Name zuerst.
        self._orig = dw._ENTITIES
        dw._ENTITIES = sorted(
            [
                ("headshot booster", "Headshot Booster", "item"),
                ("pocket", "Pocket", "hero"),
                ("reach", "Reach", "item"),
            ],
            key=lambda t: len(t[0]),
            reverse=True,
        )

    def tearDown(self):
        dw._ENTITIES = self._orig

    def test_detects_hero(self):
        self.assertEqual(dw._detect_entity("welcher build für pocket?"), ("Pocket", "hero"))

    def test_detects_multiword_item_longest_match(self):
        self.assertEqual(
            dw._detect_entity("lohnt sich headshot booster noch?"),
            ("Headshot Booster", "item"),
        )

    def test_word_boundary_no_substring_false_positive(self):
        # "reach" darf nicht in "breach" triggern
        self.assertIsNone(dw._detect_entity("they breach the base"))

    def test_no_entity_returns_none(self):
        self.assertIsNone(dw._detect_entity("was genau machen deine stacks"))


class TrimExtractTests(unittest.TestCase):
    def test_cuts_trailing_sections_and_empty_headers(self):
        raw = (
            "Headshot Booster is a Tier 1 Weapon Item.\n\n\n== Weapon ==\n\n\n"
            "== Notes ==\nApplies to headshots.\n\n\n== Update history ==\nlots of noise"
        )
        out = dw._trim_extract(raw)
        self.assertIn("Tier 1 Weapon Item", out)
        self.assertIn("Applies to headshots.", out)
        self.assertNotIn("Update history", out)
        self.assertNotIn("== Weapon ==", out)

    def test_truncates_long_text(self):
        out = dw._trim_extract("x" * 2000)
        self.assertLessEqual(len(out), dw._MAX_EXTRACT_CHARS)
        self.assertTrue(out.endswith("…"))


class BuildFragmentTests(unittest.TestCase):
    def setUp(self):
        self._orig = dw._ENTITIES
        dw._ENTITIES = [("pocket", "Pocket", "hero")]

    def tearDown(self):
        dw._ENTITIES = self._orig

    def test_fragment_contains_belegblock(self):
        async def fake_extract(title):
            return "Pocket is a Hero in Deadlock with a shotgun."

        with mock.patch.object(dw, "_ensure_index", new=mock.AsyncMock()), mock.patch.object(
            dw, "_fetch_wiki_extract", new=fake_extract
        ):
            frag = asyncio.run(dw.build_grounding_fragment("hat wer nen pocket build?"))

        self.assertIn("Beleg aus dem Deadlock-Wiki", frag)
        self.assertIn("[Held: Pocket]", frag)
        self.assertIn("Pocket is a Hero", frag)

    def test_empty_when_no_entity(self):
        with mock.patch.object(dw, "_ensure_index", new=mock.AsyncMock()):
            frag = asyncio.run(dw.build_grounding_fragment("hallo zusammen wie gehts"))
        self.assertEqual(frag, "")

    def test_empty_when_extract_missing(self):
        async def no_extract(title):
            return None

        with mock.patch.object(dw, "_ensure_index", new=mock.AsyncMock()), mock.patch.object(
            dw, "_fetch_wiki_extract", new=no_extract
        ):
            frag = asyncio.run(dw.build_grounding_fragment("pocket?"))
        self.assertEqual(frag, "")


if __name__ == "__main__":
    unittest.main()
