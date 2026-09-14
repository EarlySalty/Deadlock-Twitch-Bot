import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from highlight_detector import ocr


SPIELER = ["earlysalty", "salty"]


def test_killfeed_eigener_kill_trotz_ocr_fehler():
    ereignisse = ocr.parse_killfeed("carLysaLty", "stringindexoutofbounds", SPIELER, 0.6)
    typen = {e["typ"] for e in ereignisse}
    assert "ocr_kill" in typen
    assert "ocr_tod" not in typen


def test_killfeed_eigener_tod():
    ereignisse = ocr.parse_killfeed("gegnername", "earivsaiiy", SPIELER, 0.6)
    typen = {e["typ"] for e in ereignisse}
    assert "ocr_tod" in typen
    assert "ocr_kill" not in typen


def test_killfeed_fremder_kill_ist_kein_ereignis():
    ereignisse = ocr.parse_killfeed("spielerx", "spielery", SPIELER, 0.6)
    assert ereignisse == []


def test_souls_liest_zahl_mit_dollar_und_komma():
    assert ocr.parse_souls("$3,665") == 3665
    assert ocr.parse_souls("1,407") == 1407
    assert ocr.parse_souls("239") == 239


def test_souls_leer_gibt_none():
    assert ocr.parse_souls("") is None
    assert ocr.parse_souls("....") is None


def test_banner_multikill():
    woerter = ["double", "triple", "rampage"]
    obj = ["guardian", "walker"]
    ereignisse = ocr.parse_banner("DOUBLE KILL", woerter, obj)
    assert any(e["typ"] == "ocr_multikill" for e in ereignisse)


def test_banner_objective():
    woerter = ["double"]
    obj = ["guardian", "walker"]
    ereignisse = ocr.parse_banner("Guardian destroyed", woerter, obj)
    assert any(e["typ"] == "ocr_objective" for e in ereignisse)


def test_soul_spruenge_erkennt_grossen_zuwachs():
    reihe = [(0.0, 1000), (1.0, 1010), (2.0, 1300), (3.0, 1305)]
    spruenge = ocr.soul_spruenge(reihe, 120)
    assert len(spruenge) == 1
    t, wert = spruenge[0]
    assert t == 2.0
    assert wert >= 120
