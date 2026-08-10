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

    def test_welle_mitten_im_stream_ist_keine_salve(self):
        """Fünf Leute, die in 40 Sekunden auf ein Spielereignis reagieren.

        Die Legende nennt den Befund „ausschließlich in einer Nachrichtenwelle
        am Streamende". Ohne die Endfenster-Bedingung landet hier jeder
        Einmal-Chatter namentlich als belegter Bot im Bericht
        (Merge-Kritiker 10.08.2026)."""
        welle = [(T0 + timedelta(seconds=s), f"konto{i}")
                 for i, s in enumerate([0, 8, 15, 27, 40])]
        # Der Stream läuft danach zwei Stunden weiter.
        danach = [(T0 + timedelta(minutes=m), "stammgast") for m in (30, 60, 90, 120)]
        assert cc.finde_salven(welle + danach) == set()

    def test_gleiche_welle_am_streamende_zaehlt_weiter(self):
        """Gegenstück: dieselbe Welle, nur am Ende — muss anschlagen."""
        davor = [(T0 + timedelta(minutes=m), "stammgast") for m in (0, 30, 60, 90)]
        welle = [(T0 + timedelta(minutes=120, seconds=s), f"konto{i}")
                 for i, s in enumerate([0, 8, 15, 27, 40])]
        assert len(cc.finde_salven(davor + welle)) == 5


class TestTaktkonten:
    @staticmethod
    def _takt(konten: list[str], n: int = 24):
        """Fester 30-Sekunden-Takt, reihum über die Konten."""
        return [(T0 + timedelta(seconds=30 * i), konten[i % len(konten)]) for i in range(n)]

    def test_kanal_bots_bilden_keinen_ring(self):
        """scan_cadence filtert Kanal-Bots vor der Ring-Bestimmung, taktkonten
        muss dasselbe tun — sonst meldet jeder Kanal seinen eigenen Chatbot als
        belegten Viewbot (Merge-Kritiker 10.08.2026)."""
        msgs = self._takt(["nightbot", "streamelements", "moobot"])
        assert cc.taktkonten({1: msgs}, "earlysalty") == set()

    def test_echter_ring_wird_weiter_erkannt(self):
        """Gegenprobe: dieselbe Taktkurve mit drei normalen Konten schlägt an."""
        msgs = self._takt(["konto_a", "konto_b", "konto_c"])
        assert cc.taktkonten({1: msgs}, "earlysalty") == {"konto_a", "konto_b", "konto_c"}


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
