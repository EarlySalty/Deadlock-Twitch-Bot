import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from highlight_detector import detector


GEWICHTE = {
    "ocr_kill": 0.5,
    "ocr_multikill": 0.5,
    "ocr_soul_sprung": 0.35,
    "sprache_lachen": 0.3,
    "sprache_pegel": 0.3,
    "bild_death_screen": 0.15,
}


def test_fenster_score_summiert_gewichtete_signale():
    events = [
        (100.0, "ocr_kill", 1.0),
        (101.0, "sprache_lachen", 1.0),
        (140.0, "ocr_kill", 1.0),
    ]
    score, einzel = detector.fenster_score(events, GEWICHTE, 98.0, 106.0)
    assert einzel["ocr_kill"] == 0.5
    assert einzel["sprache_lachen"] == 0.3
    assert abs(score - 0.8) < 1e-9


def test_fenster_score_nimmt_maximum_je_typ():
    events = [
        (100.0, "sprache_pegel", 0.4),
        (101.0, "sprache_pegel", 0.9),
    ]
    score, einzel = detector.fenster_score(events, GEWICHTE, 99.0, 103.0)
    assert abs(einzel["sprache_pegel"] - 0.3 * 0.9) < 1e-9


def test_finde_kandidaten_liefert_bekanntes_fenster():
    events = [
        (200.0, "ocr_kill", 1.0),
        (201.0, "ocr_multikill", 1.0),
        (201.5, "sprache_lachen", 1.0),
        (2000.0, "bild_death_screen", 1.0),
    ]
    kandidaten = detector.finde_kandidaten(
        events,
        GEWICHTE,
        fenster_s=8.0,
        schritt_s=2.0,
        nms_abstand_s=25.0,
        schwelle=0.6,
        dauer_s=2100.0,
    )
    assert len(kandidaten) == 1
    k = kandidaten[0]
    assert k["start_s"] <= 200.0 <= k["end_s"]
    assert k["score"] >= 0.6
    assert "ocr_kill" in k["signale"]


def test_finde_kandidaten_unterdrueckt_ueberlappung():
    events = [
        (100.0, "ocr_kill", 1.0),
        (101.0, "ocr_multikill", 1.0),
        (103.0, "ocr_kill", 1.0),
        (104.0, "sprache_lachen", 1.0),
    ]
    kandidaten = detector.finde_kandidaten(
        events,
        GEWICHTE,
        fenster_s=8.0,
        schritt_s=2.0,
        nms_abstand_s=25.0,
        schwelle=0.6,
        dauer_s=200.0,
    )
    assert len(kandidaten) == 1
