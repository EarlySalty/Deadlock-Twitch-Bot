import os

from . import video

AUSGABE_BASIS = "/home/nathanael/vod-archive/highlights"


def ausgabe_dir(vod_id):
    pfad = os.path.join(AUSGABE_BASIS, vod_id)
    os.makedirs(pfad, exist_ok=True)
    return pfad


def schneide_kandidaten(vod_pfad, vod_id, kandidaten, vorlauf_s, nachlauf_s, dauer_s):
    ziel_dir = ausgabe_dir(vod_id)
    ergebnisse = []
    for k in kandidaten:
        start = max(0.0, k["start_s"] - vorlauf_s)
        ende = min(dauer_s, k["end_s"] + nachlauf_s)
        name = f"{vod_id}_{int(start)}-{int(ende)}_score{k['score']:.2f}.mp4"
        ziel = os.path.join(ziel_dir, name)
        video.schneide(vod_pfad, start, ende, ziel)
        ergebnisse.append({**k, "datei": ziel})
    return ergebnisse
