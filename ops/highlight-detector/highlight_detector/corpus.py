import os
import tempfile
import traceback

from . import detector, signals, video

FENSTER_ENDE_S = 30.0
SIGNAL_TYPEN = [
    "ocr_kill", "ocr_tod", "ocr_multikill", "ocr_objective", "ocr_soul_sprung",
    "sprache_pegel", "sprache_lachen", "sprache_schluesselwort",
    "bild_szenenwechsel", "bild_death_screen", "bild_template",
]


def _clip_events(clip_pfad, regionen_cfg, gewichte_cfg):
    dauer = video.dauer_s(clip_pfad)
    events = signals.extrahiere_events_direkt(clip_pfad, regionen_cfg, gewichte_cfg)
    merkmale = []
    for t, typ, wert in events:
        offset_vom_ende = round(dauer - t, 2)
        if 0.0 <= offset_vom_ende <= FENSTER_ENDE_S:
            merkmale.append({"signal": typ, "offset_vom_ende_s": offset_vom_ende, "wert": wert})
    return dauer, merkmale


def verarbeite_clip(clip, regionen_cfg, gewichte_cfg, tmp_dir):
    ziel = os.path.join(tmp_dir, f"{clip['clip_id']}.mp4")
    try:
        video.lade_clip_fenster(clip["clip_url"], ziel)
        dateien = [p for p in os.listdir(tmp_dir) if p.startswith(clip["clip_id"])]
        if not dateien:
            return None
        pfad = os.path.join(tmp_dir, dateien[0])
        dauer, merkmale = _clip_events(pfad, regionen_cfg, gewichte_cfg)
        return {"clip": clip, "dauer_s": dauer, "merkmale": merkmale}
    except Exception:
        traceback.print_exc()
        return None
    finally:
        for p in os.listdir(tmp_dir):
            if p.startswith(clip["clip_id"]):
                try:
                    os.unlink(os.path.join(tmp_dir, p))
                except OSError:
                    pass


def merkmal_zeilen(ergebnis):
    clip = ergebnis["clip"]
    zeilen = []
    for m in ergebnis["merkmale"]:
        zeilen.append(
            {
                "clip_id": clip["clip_id"],
                "streamer_twitch_id": clip.get("streamer_twitch_id"),
                "views": clip.get("views", 0),
                "dauer_s": ergebnis["dauer_s"],
                "ersteller": clip.get("streamer_login"),
                "signal": m["signal"],
                "offset_vom_ende_s": m["offset_vom_ende_s"],
                "wert": m["wert"],
            }
        )
    return zeilen


def leite_gewichte_ab(ergebnisse, gewichte_cfg):
    n = len(ergebnisse)
    praevalenz = {typ: 0 for typ in SIGNAL_TYPEN}
    for e in ergebnisse:
        typen = {m["signal"] for m in e["merkmale"]}
        for typ in typen:
            if typ in praevalenz:
                praevalenz[typ] += 1
    praev_frac = {typ: (praevalenz[typ] / n if n else 0.0) for typ in SIGNAL_TYPEN}

    max_frac = max(praev_frac.values()) if praev_frac else 0.0
    gewichte = {}
    for typ in SIGNAL_TYPEN:
        gewichte[typ] = round(0.5 * praev_frac[typ] / max_frac, 4) if max_frac > 0 else 0.0

    peaks = []
    for e in ergebnisse:
        events = [(FENSTER_ENDE_S - m["offset_vom_ende_s"], m["signal"], m["wert"])
                  for m in e["merkmale"]]
        kand = detector.finde_kandidaten(
            events, gewichte,
            fenster_s=gewichte_cfg["fenster"]["fenster_s"],
            schritt_s=gewichte_cfg["fenster"]["schritt_s"],
            nms_abstand_s=gewichte_cfg["fenster"]["nms_abstand_s"],
            schwelle=0.0, dauer_s=FENSTER_ENDE_S + 1.0,
        )
        peaks.append(max((k["score"] for k in kand), default=0.0))
    peaks.sort()
    if peaks:
        idx = max(0, int(0.25 * len(peaks)) - 1)
        schwelle = round(max(0.15, peaks[idx]), 4)
    else:
        schwelle = gewichte_cfg["fenster"]["schwelle"]

    return gewichte, schwelle, praev_frac


def schreibe_gewichte_toml(pfad, gewichte, schwelle, gewichte_cfg):
    f = gewichte_cfg["fenster"]
    s = gewichte_cfg["schnitt"]
    a = gewichte_cfg["signale_aktiv"]
    zeilen = ["[gewichte]"]
    for k, v in gewichte.items():
        zeilen.append(f"{k} = {v}")
    zeilen += [
        "",
        "[signale_aktiv]",
        f"ocr = {str(a.get('ocr', True)).lower()}",
        f"sprache = {str(a.get('sprache', True)).lower()}",
        f"bild = {str(a.get('bild', True)).lower()}",
        "",
        "[fenster]",
        f"fenster_s = {f['fenster_s']}",
        f"schritt_s = {f['schritt_s']}",
        f"nms_abstand_s = {f['nms_abstand_s']}",
        f"schwelle = {schwelle}",
        "",
        "[schnitt]",
        f"vorlauf_s = {s['vorlauf_s']}",
        f"nachlauf_s = {s['nachlauf_s']}",
        "",
        "[schluesselwoerter]",
        "begriffe = [" + ", ".join(f'"{w}"' for w in gewichte_cfg["schluesselwoerter"]) + "]",
    ]
    with open(pfad, "w") as fh:
        fh.write("\n".join(zeilen) + "\n")
