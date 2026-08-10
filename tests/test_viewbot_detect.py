"""Prüft die Bot-Erkennung aus tools/viewbot_detect.py.

Schwerpunkt sind die Fälle, die KEIN Befund sein dürfen: Stream-Neustarts, Raids
und normales Abwandern erzeugen dieselben Rohsignale wie ein Bot-Netz. Eine
Erkennung, die diese nicht ausschließt, meldet vor allem sich selbst.
"""
from __future__ import annotations

import importlib.util
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "viewbot_detect", Path(__file__).resolve().parents[1] / "tools" / "viewbot_detect.py"
)
vd = importlib.util.module_from_spec(_spec)
sys.modules["viewbot_detect"] = vd
_spec.loader.exec_module(vd)

T0 = datetime(2026, 6, 2, 20, 0, tzinfo=timezone.utc)


def session(sid: int, start_offset_min: int, curve: list[tuple[int, int]],
            duration_min: int = 120) -> vd.Session:
    """curve = [(minute_ab_start, viewer), ...] mit 20-s-Abtastung dazwischen."""
    start = T0 + timedelta(minutes=start_offset_min)
    s = vd.Session(id=sid, started_at=start, ended_at=start + timedelta(minutes=duration_min),
                   duration_min=duration_min, peak=max(v for _, v in curve),
                   avg=sum(v for _, v in curve) / len(curve), title=None)
    s.samples = [(start + timedelta(minutes=m), v) for m, v in curve]
    return s


def plateau(from_min: int, to_min: int, value: int, step: int = 1) -> list[tuple[int, int]]:
    return [(m, value) for m in range(from_min, to_min, step)]


class TestKollaps:
    def test_schlagartiger_einbruch_wird_erkannt(self):
        """49 → 3 in einer Minute. Ein Publikum kann das nicht, ein Prozess schon."""
        curve = plateau(0, 40, 49) + [(40, 3)] + plateau(41, 60, 3)
        got = vd.scan_viewers([session(1, 0, curve)], [])
        kollaps = [f for f in got if f.kind == "kollaps"]
        assert len(kollaps) == 1
        assert "49 → 3" in kollaps[0].detail

    def test_langsames_abwandern_ist_kein_befund(self):
        """Dieselbe Spanne 49 → 3, aber über 46 Minuten verteilt: normales Ende."""
        curve = [(m, max(3, 49 - m)) for m in range(0, 60)]
        got = vd.scan_viewers([session(2, 0, curve)], [])
        assert [f for f in got if f.kind == "kollaps"] == []

    def test_kleiner_absoluter_ruecksprung_ist_kein_befund(self):
        """Von 8 auf 2 ist prozentual dramatisch, absolut aber Rauschen kleiner Kanäle."""
        curve = plateau(0, 30, 8) + [(30, 2)] + plateau(31, 60, 2)
        got = vd.scan_viewers([session(3, 0, curve)], [])
        assert [f for f in got if f.kind == "kollaps"] == []

    def test_abfall_am_streamende_ist_kein_befund(self):
        """Beim Beenden gehen alle gleichzeitig — das ist der Normalfall, kein Bot."""
        curve = plateau(0, 59, 40) + [(59, 1)]
        got = vd.scan_viewers([session(4, 0, curve, duration_min=60)], [])
        assert [f for f in got if f.kind == "kollaps"] == []


class TestEinspeisung:
    def test_sprung_ohne_raid_wird_erkannt(self):
        curve = plateau(0, 30, 16) + plateau(30, 60, 62)
        got = vd.scan_viewers([session(5, 0, curve)], [])
        assert [f.kind for f in got if f.kind == "einspeisung"] == ["einspeisung"]

    def test_sprung_mit_raid_wird_unterdrueckt(self):
        """Ein Raid bringt legitim 46 Zuschauer auf einmal."""
        curve = plateau(0, 30, 16) + plateau(30, 60, 62)
        raid = T0 + timedelta(minutes=30)
        got = vd.scan_viewers([session(6, 0, curve)], [raid])
        assert [f for f in got if f.kind == "einspeisung"] == []

    def test_startphase_wird_unterdrueckt(self):
        """In den ersten Minuten trudelt das Stammpublikum ein — immer ein Sprung."""
        curve = [(0, 1), (1, 3), (2, 30)] + plateau(3, 60, 30)
        got = vd.scan_viewers([session(7, 0, curve)], [])
        assert got == []

    def test_zulauf_auf_kleiner_basis_ist_kein_befund(self):
        """2 → 28 ist prozentual gewaltig, aber bei zwei Zuschauern erklärt das schon
        ein geteilter Link. Erst ein etabliertes Publikum macht den Sprung auffällig."""
        curve = plateau(0, 20, 2) + plateau(20, 60, 28)
        got = vd.scan_viewers([session(8, 0, curve)], [])
        assert [f for f in got if f.kind == "einspeisung"] == []

    def test_kleiner_relativer_zuwachs_auf_hohem_niveau_ist_rauschen(self):
        """+12 auf 104 sind 12 % — die Zuschauerzahl der API schwankt so."""
        curve = plateau(0, 30, 104) + plateau(30, 60, 116)
        got = vd.scan_viewers([session(9, 0, curve)], [])
        assert [f for f in got if f.kind == "einspeisung"] == []


class TestFortsetzung:
    def test_neustart_kurz_nach_streamende_wird_als_fortsetzung_markiert(self):
        """Twitch vergibt beim Neustart eine neue Session; die Zuschauer bleiben.
        Ohne diese Markierung liest sich jeder Werbe-/Absturz-Neustart als
        Einspeisung von 90 Zuschauern."""
        erste = session(10, 0, plateau(0, 60, 93), duration_min=60)
        zweite = session(11, 62, plateau(0, 60, 93), duration_min=60)
        vd.markiere_fortsetzungen([erste, zweite])
        assert zweite.continuation_of == erste.id

    def test_langer_abstand_ist_keine_fortsetzung(self):
        erste = session(12, 0, plateau(0, 60, 93), duration_min=60)
        zweite = session(13, 120, plateau(0, 60, 93), duration_min=60)
        vd.markiere_fortsetzungen([erste, zweite])
        assert zweite.continuation_of is None

    def test_fortsetzung_wird_nicht_auf_einspeisung_geprueft(self):
        """Der Zuwachs einer Fortsetzung ist zurückkehrendes Publikum."""
        erste = session(14, 0, plateau(0, 60, 93), duration_min=60)
        # Sprung, den scan_viewers ohne den Ausschluss als Einspeisung meldet.
        sprung = plateau(0, 20, 40) + plateau(20, 60, 400)
        zweite = session(15, 62, sprung, duration_min=60)
        vd.markiere_fortsetzungen([erste, zweite])
        assert zweite.continuation_of == erste.id, "Vorbedingung: markiert"

        befunde = vd.scan_viewers([erste, zweite], [])
        assert [f for f in befunde if f.session_id == 15 and f.kind == "einspeisung"] == []
        # Gegenprobe: dieselbe Kurve ohne Fortsetzungs-Markierung schlägt an,
        # sonst prüft der Test nur eine Kurve, die ohnehin nichts auslöst.
        allein = session(16, 200, sprung, duration_min=60)
        assert [f for f in vd.scan_viewers([allein], []) if f.kind == "einspeisung"]

    def test_fortsetzung_wird_weiter_auf_kollaps_geprueft(self):
        """Der Ausschluss gilt nur dem Zustrom. Eine Fortsetzung ganz zu
        überspringen und sie danach als 'unauffällig' auszuweisen, behauptet
        einen Prüfumfang, den der Bericht nicht hat (Merge-Kritiker 10.08.2026)."""
        erste = session(17, 0, plateau(0, 60, 93), duration_min=60)
        einbruch = plateau(0, 40, 60) + [(40, 4)] + plateau(41, 60, 4)
        zweite = session(18, 62, einbruch, duration_min=60)
        vd.markiere_fortsetzungen([erste, zweite])
        assert zweite.continuation_of == erste.id, "Vorbedingung: markiert"

        befunde = vd.scan_viewers([erste, zweite], [])
        assert [f.kind for f in befunde if f.session_id == 18] == ["kollaps"]


class TestChatTakt:
    @staticmethod
    def _msgs(offsets_sec: list[float], konten: list[str]):
        return [(T0 + timedelta(seconds=o), konten[i % len(konten)], f"nachricht {i}")
                for i, o in enumerate(offsets_sec)]

    def test_fester_takt_ueber_mehrere_konten_wird_erkannt(self):
        """52-Sekunden-Takt, reihum über fünf Konten — ein Zeitplan, kein Gespräch."""
        konten = ["alpha", "beta", "gamma", "delta", "epsilon"]
        msgs = self._msgs([i * 52 + (i % 3) for i in range(40)], konten)
        s = session(20, 0, plateau(0, 60, 30))
        found, accounts = vd.scan_cadence(msgs, [s])
        assert len(found) == 1
        assert set(accounts) == set(konten)

    def test_menschlicher_chat_wird_nicht_erkannt(self):
        """Echte Abstände schwanken zwischen Sekunden und Minuten."""
        konten = ["mensch_a", "mensch_b"]
        gaps = [2, 180, 5, 900, 3, 45, 600, 1, 320, 8, 1200, 4, 90, 15, 700,
                2, 400, 30, 1100, 6, 250, 12, 850, 3, 500, 40, 60, 900, 7, 200]
        cum, acc = [], 0.0
        for g in gaps:
            acc += g
            cum.append(acc)
        found, accounts = vd.scan_cadence(self._msgs(cum, konten), [session(21, 0, plateau(0, 90, 30), 180)])
        assert found == []
        assert accounts == {}

    def test_ring_wird_aus_gemischtem_chat_herausgeloest(self):
        """Der entscheidende Fall: Bots und Menschen schreiben gleichzeitig. Gemeldet
        werden dürfen nur die Konten des Rings — sonst steht ein echter Zuschauer
        mit im Befund und die ganze Meldung wird wertlos."""
        ring = ["alpha", "beta", "gamma"]
        msgs = self._msgs([i * 52 for i in range(36)], ring)
        # Zwei Menschen dazwischen, unregelmäßig.
        for off, who in [(31, "mensch_a"), (33, "mensch_a"), (410, "mensch_b"),
                         (412, "mensch_b"), (900, "mensch_a"), (1500, "mensch_b")]:
            msgs.append((T0 + timedelta(seconds=off), who, "echt"))
        msgs.sort(key=lambda m: m[0])
        found, accounts = vd.scan_cadence(msgs, [session(24, 0, plateau(0, 60, 30))])
        assert len(found) == 1
        assert set(accounts) == set(ring), f"Menschen im Befund: {set(accounts) - set(ring)}"

    def test_zu_wenige_nachrichten_ergeben_kein_urteil(self):
        """Aus fünf Nachrichten lässt sich kein Takt ableiten — lieber nichts sagen."""
        msgs = self._msgs([i * 52 for i in range(5)], ["alpha", "beta"])
        found, _ = vd.scan_cadence(msgs, [session(22, 0, plateau(0, 60, 30))])
        assert found == []

    def test_eigene_bots_zaehlen_nicht_als_befund(self):
        """Unser Bot postet planmäßig — er darf sich nicht selbst melden."""
        msgs = self._msgs([i * 52 for i in range(40)], sorted(vd.OWN_ACCOUNTS))
        found, accounts = vd.scan_cadence(msgs, [session(23, 0, plateau(0, 60, 30))])
        assert found == []
        assert accounts == {}

    def test_kanal_eigener_bot_zaehlt_nicht(self):
        """miracleg_bot im Kanal miracleghost9 ist der Bot des Streamers, kein Ring.
        Ohne diese Regel meldet der Detektor bei jedem Kanal dessen eigenen Chatbot."""
        msgs = self._msgs([i * 30 for i in range(40)], ["miracleg_bot", "nightbot", "stammgast"])
        found, accounts = vd.scan_cadence(msgs, [session(25, 0, plateau(0, 60, 30))],
                                          "miracleghost9")
        assert found == []

    def test_lebhafter_schneller_chat_ist_kein_takt(self):
        """Aus der Gegenprobe: 11 Zuschauer schrieben im Schnitt alle 7 s. Absolut
        streute das nur um 8,7 s — nach einem Sekundenmaß wäre das ein 'Takt'.
        Gemessen am mittleren Abstand ist es das Gegenteil von regelmäßig."""
        cum, acc = [], 0.0
        for g in [1, 2, 14, 3, 1, 22, 5, 2, 1, 18, 9, 3, 1, 2, 27, 4, 1, 11, 6, 2]:
            acc += g
            cum.append(acc)
        konten = [f"gast{i}" for i in range(11)]
        found, _ = vd.scan_cadence(self._msgs(cum, konten),
                                   [session(27, 0, plateau(0, 60, 30))])
        assert found == []

    def test_zwei_konten_im_gleichtakt_reichen_nicht(self):
        """Ein Bot plus der Stammgast, der ihn bedient, ergibt zwangsläufig Takt."""
        msgs = self._msgs([i * 40 for i in range(30)], ["helferlein", "stammgast"])
        found, _ = vd.scan_cadence(msgs, [session(26, 0, plateau(0, 60, 30))])
        assert found == []


class TestRender:
    def test_report_nennt_auch_geprueft_ohne_befund(self):
        """Nur Treffer zu zeigen verschweigt den Prüfumfang."""
        sessions = [session(30, 0, plateau(0, 60, 20)), session(31, 200, plateau(0, 60, 20))]
        out = vd.render("testkanal", sessions, [], {}, [], 0)
        assert "unauffällig" in out
        assert "2 von 2 Sessions" in out
