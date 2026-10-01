import re
from difflib import SequenceMatcher

import cv2
import numpy as np
import pytesseract


def _norm(text):
    return re.sub(r"[^a-z0-9]", "", text.lower())


def _aehnlich(kandidat, name):
    return SequenceMatcher(None, kandidat, name).ratio()


def _enthaelt_spieler(text, spieler_namen, schwelle):
    roh = _norm(text)
    if not roh:
        return False
    for name in spieler_namen:
        n = _norm(name)
        if not n:
            continue
        if n in roh:
            return True
        fenster = len(n)
        for start in range(0, max(1, len(roh) - fenster + 1)):
            teil = roh[start:start + fenster]
            if _aehnlich(teil, n) >= schwelle:
                return True
    return False


def parse_killfeed(killer_text, victim_text, spieler_namen, aehnlichkeit):
    ereignisse = []
    if _enthaelt_spieler(killer_text, spieler_namen, aehnlichkeit):
        ereignisse.append({"typ": "ocr_kill", "wert": 1.0})
    elif _enthaelt_spieler(victim_text, spieler_namen, aehnlichkeit):
        ereignisse.append({"typ": "ocr_tod", "wert": 1.0})
    return ereignisse


def parse_souls(text):
    ziffern = re.sub(r"[^0-9]", "", text or "")
    if not ziffern:
        return None
    try:
        return int(ziffern)
    except ValueError:
        return None


def parse_banner(text, multikill_woerter, objective_woerter):
    ereignisse = []
    unten = (text or "").lower()
    if any(w in unten for w in multikill_woerter):
        ereignisse.append({"typ": "ocr_multikill", "wert": 1.0})
    if any(w in unten for w in objective_woerter):
        ereignisse.append({"typ": "ocr_objective", "wert": 1.0})
    return ereignisse


def soul_spruenge(reihe, min_sprung):
    spruenge = []
    vorher = None
    for t, souls in reihe:
        if souls is None:
            continue
        if vorher is not None:
            delta = souls - vorher
            if delta >= min_sprung:
                spruenge.append((t, delta))
        vorher = souls
    return spruenge


def _region_pixel(breite, hoehe, region):
    x0 = int(region["x0"] * breite)
    y0 = int(region["y0"] * hoehe)
    x1 = int(region["x1"] * breite)
    y1 = int(region["y1"] * hoehe)
    return x0, y0, x1, y1


def _vorverarbeiten(crop, skalierung, invertieren):
    grau = cv2.cvtColor(crop, cv2.COLOR_BGR2GRAY)
    grau = cv2.resize(grau, None, fx=skalierung, fy=skalierung, interpolation=cv2.INTER_CUBIC)
    if invertieren:
        grau = cv2.bitwise_not(grau)
    return grau


def _tesseract(bild, sprache, psm, whitelist):
    cfg = f"--psm {psm} --oem 1"
    if whitelist:
        cfg += f" -c tessedit_char_whitelist={whitelist}"
    return pytesseract.image_to_string(bild, lang=sprache, config=cfg).strip()


def lies_regionen(frame, regionen, sprache):
    hoehe, breite = frame.shape[:2]
    ergebnis = {}
    for region in regionen:
        x0, y0, x1, y1 = _region_pixel(breite, hoehe, region)
        crop = frame[y0:y1, x0:x1]
        if crop.size == 0:
            ergebnis[region["name"]] = {"text": ""}
            continue
        skal = region.get("skalierung", 2.0)
        inv = region.get("invertieren", False)
        psm = region.get("psm", 6)
        wl = region.get("whitelist")
        if region["name"] == "killfeed" and "teilung" in region:
            teil = region["teilung"]
            grenze = int(crop.shape[1] * teil)
            killer = _tesseract(_vorverarbeiten(crop[:, :grenze], skal, inv), sprache, psm, wl)
            victim = _tesseract(
                _vorverarbeiten(crop[:, int(crop.shape[1] * 0.5):], skal, inv),
                sprache,
                psm,
                wl,
            )
            ergebnis[region["name"]] = {"killer": killer, "victim": victim}
        else:
            text = _tesseract(_vorverarbeiten(crop, skal, inv), sprache, psm, wl)
            ergebnis[region["name"]] = {"text": text}
    return ergebnis
