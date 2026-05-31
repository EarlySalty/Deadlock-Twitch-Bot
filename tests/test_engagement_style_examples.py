import asyncio
import unittest
from unittest import mock

from bot.engagement import style_examples as se


class IsGoodExampleTests(unittest.TestCase):
    def test_accepts_casual_line(self):
        self.assertTrue(se._is_good_example("warum baut er kein spirit lol"))

    def test_rejects_too_short(self):
        self.assertFalse(se._is_good_example("hi"))

    def test_rejects_no_space_single_emote(self):
        self.assertFalse(se._is_good_example("KEKW"))

    def test_rejects_command(self):
        self.assertFalse(se._is_good_example("!drop neon prime"))

    def test_rejects_link(self):
        self.assertFalse(se._is_good_example("schau mal http://twitch.tv/x"))

    def test_rejects_caps_spam(self):
        self.assertFalse(se._is_good_example("WAS IST DAS LOL ALTER"))

    def test_rejects_too_long(self):
        self.assertFalse(se._is_good_example("a " + "x" * 120))


class SelectExamplesTests(unittest.TestCase):
    def test_dedupes_and_limits(self):
        texts = [
            "ggs leute war nice",
            "ggs leute war nice",  # exaktes Duplikat (case-insensitiv)
            "GGS LEUTE WAR NICE",
            "warum baut der kein resilienz",
            "!drop xy",            # gefiltert
            "kp lol",              # zu kurz? len('kp lol')=6 < 8 → gefiltert
            "der heal build ist schon stark grad",
            "lass mal push gehen mid",
            "echt? den hab ich nie gebaut",
            "bin afk kurz brb gleich wieder",
        ]
        out = se._select_examples(texts, max_n=3)
        self.assertEqual(len(out), 3)
        self.assertEqual(out[0], "ggs leute war nice")
        # keine zwei identischen
        self.assertEqual(len(out), len(set(o.lower() for o in out)))

    def test_empty_when_nothing_good(self):
        self.assertEqual(se._select_examples(["hi", "!cmd", "KEKW"]), [])


class BuildFragmentTests(unittest.TestCase):
    def test_fragment_has_guard_and_lines(self):
        frag = se._build_fragment(["warum kein spirit", "ggs war nice"])
        self.assertIn("Stilvorlage", frag)
        self.assertIn("IGNORIERST", frag)  # Inhalt-ignorieren-Guard
        self.assertIn("- warum kein spirit", frag)
        self.assertIn("- ggs war nice", frag)

    def test_empty_examples_empty_fragment(self):
        self.assertEqual(se._build_fragment([]), "")


class BuildStyleFragmentTests(unittest.TestCase):
    def setUp(self):
        se._cache.clear()

    def tearDown(self):
        se._cache.clear()

    def test_end_to_end_with_mocked_db(self):
        rows = [
            "warum baut er kein spirit lol",
            "ggs leute das war clean",
            "!drop neon",
            "bin gleich wieder afk kurz",
        ]
        with mock.patch.object(se, "_sync_load_user_turns", return_value=rows):
            frag = asyncio.run(se.build_style_fragment("somechannel"))
        self.assertIn("- warum baut er kein spirit lol", frag)
        self.assertIn("- ggs leute das war clean", frag)
        self.assertNotIn("!drop", frag)  # Command gefiltert

    def test_empty_when_no_material(self):
        with mock.patch.object(se, "_sync_load_user_turns", return_value=["hi", "!x"]):
            frag = asyncio.run(se.build_style_fragment("quietchannel"))
        self.assertEqual(frag, "")


if __name__ == "__main__":
    unittest.main()
