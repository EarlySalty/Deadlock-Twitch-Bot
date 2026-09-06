import cv2
import numpy as np

from . import audio, cache, ocr, video


def _mittel_saettigung(frame):
    hsv = cv2.cvtColor(frame, cv2.COLOR_BGR2HSV)
    return float(np.mean(hsv[:, :, 1]))


def extrahiere_roh(vod_pfad, vod_id, regionen_cfg, start=0.0, ende=None, force=False):
    breite = regionen_cfg["frame_breite"]
    hoehe = regionen_cfg["frame_hoehe"]
    ist_breite, ist_hoehe = video.aufloesung(vod_pfad)
    breite, hoehe = ist_breite, ist_hoehe
    fps = regionen_cfg["ocr"]["fps"]
    sprache = regionen_cfg["ocr"]["sprache"]

    if not force and cache.existiert(vod_id, "ocr.jsonl") and cache.existiert(vod_id, "bild.jsonl"):
        pass
    else:
        ocr_zeilen = []
        bild_zeilen = []
        for t, frame in video.frame_strom(vod_pfad, fps, breite, hoehe, start=start, ende=ende):
            gelesen = ocr.lies_regionen(frame, regionen_cfg["regionen"], sprache)
            kf = gelesen.get("killfeed", {})
            ocr_zeilen.append(
                {
                    "t": round(t, 2),
                    "killer": kf.get("killer", ""),
                    "victim": kf.get("victim", ""),
                    "souls": ocr.parse_souls(gelesen.get("souls", {}).get("text", "")),
                    "banner": gelesen.get("banner", {}).get("text", ""),
                }
            )
            bild_zeilen.append({"t": round(t, 2), "sat": round(_mittel_saettigung(frame), 2)})
        cache.schreibe_jsonl(vod_id, "ocr.jsonl", ocr_zeilen)
        cache.schreibe_jsonl(vod_id, "bild.jsonl", bild_zeilen)

    if force or not cache.existiert(vod_id, "scene.json"):
        cache.schreibe_json(vod_id, "scene.json", video.szenenwechsel(vod_pfad, start=start, ende=ende))

    if force or not cache.existiert(vod_id, "audio.jsonl"):
        spitzen, reihe = audio.pegel_spitzen(vod_pfad, start=start, ende=ende)
        cache.schreibe_jsonl(vod_id, "audio.jsonl", [{"t": t, "rms": r} for t, r in reihe])
        cache.schreibe_json(vod_id, "audio_spitzen.json", spitzen)

    if force or not cache.existiert(vod_id, "transcript.json"):
        cache.schreibe_json(
            vod_id, "transcript.json", audio.transkribiere(vod_pfad, start=start, ende=ende)
        )


def _death_screen(bild_zeilen, faktor=0.55):
    if not bild_zeilen:
        return []
    sats = np.array([z["sat"] for z in bild_zeilen])
    median = float(np.median(sats))
    grenze = median * faktor
    treffer = []
    for z in bild_zeilen:
        if z["sat"] <= grenze:
            treffer.append((z["t"], 1.0))
    return treffer


def extrahiere_events_direkt(pfad, regionen_cfg, gewichte_cfg):
    ist_breite, ist_hoehe = video.aufloesung(pfad)
    fps = regionen_cfg["ocr"]["fps"]
    sprache = regionen_cfg["ocr"]["sprache"]
    parser = regionen_cfg["parser"]
    aktiv = gewichte_cfg["signale_aktiv"]
    schluessel = gewichte_cfg["schluesselwoerter"]
    events = []
    bild_zeilen = []
    souls_reihe = []

    for t, frame in video.frame_strom(pfad, fps, ist_breite, ist_hoehe):
        gelesen = ocr.lies_regionen(frame, regionen_cfg["regionen"], sprache)
        kf = gelesen.get("killfeed", {})
        souls_reihe.append((t, ocr.parse_souls(gelesen.get("souls", {}).get("text", ""))))
        bild_zeilen.append({"t": t, "sat": _mittel_saettigung(frame)})
        if aktiv.get("ocr", True):
            for e in ocr.parse_killfeed(kf.get("killer", ""), kf.get("victim", ""),
                                        regionen_cfg["spieler_namen"], parser["name_aehnlichkeit"]):
                events.append((t, e["typ"], e["wert"]))
            for e in ocr.parse_banner(gelesen.get("banner", {}).get("text", ""),
                                      parser["multikill_woerter"], parser["objective_woerter"]):
                events.append((t, e["typ"], e["wert"]))

    if aktiv.get("ocr", True):
        for t, delta in ocr.soul_spruenge(souls_reihe, parser["soul_sprung_min"]):
            events.append((t, "ocr_soul_sprung", min(1.0, delta / (parser["soul_sprung_min"] * 5.0))))

    if aktiv.get("bild", True):
        for t in video.szenenwechsel(pfad):
            events.append((t, "bild_szenenwechsel", 1.0))
        for t, w in _death_screen(bild_zeilen):
            events.append((t, "bild_death_screen", w))

    if aktiv.get("sprache", True):
        spitzen, _ = audio.pegel_spitzen(pfad)
        for t, w in spitzen:
            events.append((t, "sprache_pegel", w))
        segmente = audio.transkribiere(pfad)
        lachen, schluessel_tr = audio.sprach_signale(segmente, schluessel)
        for t, w in lachen:
            events.append((t, "sprache_lachen", w))
        for t, w in schluessel_tr:
            events.append((t, "sprache_schluesselwort", w))

    events.sort(key=lambda e: e[0])
    return events


def baue_events(vod_id, regionen_cfg, gewichte_cfg):
    parser = regionen_cfg["parser"]
    aktiv = gewichte_cfg["signale_aktiv"]
    schluessel = gewichte_cfg["schluesselwoerter"]
    events = []

    ocr_zeilen = cache.lies_jsonl(vod_id, "ocr.jsonl")
    if aktiv.get("ocr", True):
        souls_reihe = [(z["t"], z["souls"]) for z in ocr_zeilen]
        for z in ocr_zeilen:
            for e in ocr.parse_killfeed(
                z.get("killer", ""), z.get("victim", ""),
                regionen_cfg["spieler_namen"], parser["name_aehnlichkeit"],
            ):
                events.append((z["t"], e["typ"], e["wert"]))
            for e in ocr.parse_banner(
                z.get("banner", ""), parser["multikill_woerter"], parser["objective_woerter"]
            ):
                events.append((z["t"], e["typ"], e["wert"]))
        for t, delta in ocr.soul_spruenge(souls_reihe, parser["soul_sprung_min"]):
            wert = min(1.0, delta / (parser["soul_sprung_min"] * 5.0))
            events.append((t, "ocr_soul_sprung", wert))

    if aktiv.get("sprache", True):
        for z in cache.lies_json(vod_id, "audio_spitzen.json") or []:
            events.append((z[0], "sprache_pegel", z[1]))
        segmente = cache.lies_json(vod_id, "transcript.json") or []
        lachen, schluessel_tr = audio.sprach_signale(segmente, schluessel)
        for t, w in lachen:
            events.append((t, "sprache_lachen", w))
        for t, w in schluessel_tr:
            events.append((t, "sprache_schluesselwort", w))

    if aktiv.get("bild", True):
        for t in cache.lies_json(vod_id, "scene.json") or []:
            events.append((t, "bild_szenenwechsel", 1.0))
        for t, w in _death_screen(cache.lies_jsonl(vod_id, "bild.jsonl")):
            events.append((t, "bild_death_screen", w))

    events.sort(key=lambda e: e[0])
    return events
