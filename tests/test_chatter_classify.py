"""Prüft die Einordnung der Chat-Konten aus tools/chatter_classify.py.

Die Logik hier entscheidet, ob ein Konto als Bot benannt wird. Ein Fehler in die
eine Richtung beschuldigt echte Zuschauer, einer in die andere spricht einen
nachweislichen Bot frei — beide Richtungen sind hier abgedeckt.
"""
from __future__ import annotations

import importlib.util
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "chatter_classify",
    Path(__file__).resolve().parents[1] / "tools" / "chatter_classify.py",
)
cc = importlib.util.module_from_spec(_spec)
sys.modules["chatter_classify"] = cc
_spec.loader.exec_module(cc)

T0 = datetime(2026, 8, 3, 9, 15, tzinfo=timezone.utc)


class TestUrteil:
    def test_verhaltensbeweis_genuegt(self):
        k = cc.Konto(login="x", beweis={"taktnachricht"})
        assert k.urteil == "belegt"

    def test_kontomerkmale_allein_belegen_nichts(self):
        """Ein stiller Zuschauer erfüllt alle drei Merkmale, ohne ein Bot zu sein.
        Würde das schon 'belegt' ergeben, stünden 167 Unschuldige auf der Liste."""
        k = cc.Konto(login="x", merkmal={"welle", "kanalgebunden", "nie_gefolgt"})
        assert k.urteil == "gruppenverdacht"

    def test_entlastung_schlaegt_beweis(self):
        """Gemessener Fall: earlysalty liegt in der Registrierungswelle und ist bei
        92 Kanälen aktiv. Wer durch die Szene wandert, ist kein gekauftes Konto."""
        k = cc.Konto(login="earlysalty", beweis={"salve"},
                     merkmal={"welle", "kanalgebunden"}, entlastung={"wandert"})
        assert k.urteil == "entlastet"

    def test_wenige_merkmale_bleiben_unauffaellig(self):
        k = cc.Konto(login="x", merkmal={"nie_gefolgt", "kanalgebunden"})
        assert k.urteil == "unauffaellig"


class TestSalve:
    def test_streamende_welle_wird_erkannt(self):
        """Gemessener Fall: neun Konten verabschieden sich in 22 Sekunden."""
        msgs = [(T0 + timedelta(seconds=s), f"konto{i}")
                for i, s in enumerate([58, 64, 70, 70, 71, 77, 78, 79, 80])]
        assert len(cc.finde_salven(msgs)) == 9

    def test_normaler_chatverlauf_ist_keine_salve(self):
        """Vier Leute, die sich über eine Viertelstunde verteilt melden."""
        msgs = [(T0 + timedelta(seconds=s), f"konto{i % 4}")
                for i, s in enumerate([0, 120, 300, 480, 700, 900])]
        assert cc.finde_salven(msgs) == set()

    def test_zu_wenige_konten_sind_keine_salve(self):
        """Zwei Leute, die gleichzeitig 'gg' schreiben, sind kein Ring."""
        msgs = [(T0, "a"), (T0 + timedelta(seconds=2), "b"),
                (T0 + timedelta(seconds=4), "a")]
        assert cc.finde_salven(msgs) == set()


class TestStammgast:
    def test_wiederkehrender_zuschauer(self):
        zeiten = [T0, T0 + timedelta(days=9), T0 + timedelta(days=20)]
        assert cc.ist_stammgast(zeiten) is True

    def test_ein_einsatz_an_einem_abend_ist_kein_stammgast(self):
        """43 Nachrichten an einem einzigen Tag — genau das Bot-Profil."""
        zeiten = [T0 + timedelta(minutes=i) for i in range(43)]
        assert cc.ist_stammgast(zeiten) is False

    def test_zwei_tage_dicht_beieinander_reichen_nicht(self):
        """30.07. und 03.08. — derselbe Auftrag, nicht zwei Besuche."""
        zeiten = [T0, T0 + timedelta(days=4)]
        assert cc.ist_stammgast(zeiten) is False

    def test_einzelne_nachricht(self):
        assert cc.ist_stammgast([T0]) is False


class TestWellen:
    def test_median_als_bezug(self):
        from collections import Counter
        m = Counter({f"2024-{i:02d}": 2 for i in range(1, 13)})
        m["2024-08"] = 40
        assert cc.finde_wellen(m) == {"2024-08"}

    def test_gleichmaessig_keine_welle(self):
        from collections import Counter
        assert cc.finde_wellen(Counter({f"2024-{i:02d}": 9 for i in range(1, 13)})) == set()
