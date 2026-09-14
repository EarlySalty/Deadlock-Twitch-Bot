import os
import sys

import cv2

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from highlight_detector import config, ocr

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "frame_eigener_kill.jpg")


def test_echter_frame_ergibt_eigenen_kill_und_souls():
    cfg = config.lade_regionen()
    frame = cv2.imread(FIXTURE)
    assert frame is not None
    gelesen = ocr.lies_regionen(frame, cfg["regionen"], cfg["ocr"]["sprache"])

    kf = gelesen["killfeed"]
    ereignisse = ocr.parse_killfeed(
        kf.get("killer", ""),
        kf.get("victim", ""),
        cfg["spieler_namen"],
        cfg["parser"]["name_aehnlichkeit"],
    )
    assert any(e["typ"] == "ocr_kill" for e in ereignisse), (
        f"kein eigener Kill erkannt: killer={kf.get('killer')!r} victim={kf.get('victim')!r}"
    )

    souls = ocr.parse_souls(gelesen["souls"]["text"])
    assert souls == 3665, f"Souls falsch gelesen: {gelesen['souls']['text']!r} -> {souls}"
