import subprocess
import tempfile

import numpy as np
import requests

from . import video

STT_URL = "http://127.0.0.1:8791/v1/audio/transcriptions"
LACH_WORTE = ["haha", "hahaha", "lol", "lmao", "hehe", "ahaha"]


def pegel_spitzen(pfad, start=0.0, ende=None, fenster_s=1.0, faktor=2.2, mittel_s=60.0):
    reihe = video.lautstaerke_reihe(pfad, start=start, ende=ende, fenster_s=fenster_s)
    if not reihe:
        return [], []
    werte = np.array([r for _, r in reihe])
    zeiten = [t for t, _ in reihe]
    fenster_n = max(1, int(mittel_s / fenster_s))
    spitzen = []
    for i, (t, w) in enumerate(zip(zeiten, werte)):
        lo = max(0, i - fenster_n)
        mittel = float(np.mean(werte[lo:i])) if i > lo else float(np.mean(werte[: i + 1]))
        if mittel > 0 and w >= faktor * mittel:
            wert = min(1.0, (w / mittel - 1.0) / (faktor - 1.0))
            spitzen.append((round(t, 2), round(wert, 4)))
    return spitzen, reihe


def _wav_fenster(pfad, start, laenge):
    tmp = tempfile.NamedTemporaryFile(suffix=".wav", delete=False)
    tmp.close()
    subprocess.run(
        [video.FFMPEG, "-v", "error", "-y", "-ss", str(start), "-t", str(laenge),
         "-i", pfad, "-ac", "1", "-ar", "16000", "-f", "wav", tmp.name],
        check=True, capture_output=True,
    )
    return tmp.name


def transkribiere(pfad, start=0.0, ende=None, block_s=600.0):
    import os

    dauer = (ende if ende is not None else video.dauer_s(pfad)) - start
    segmente = []
    offset = 0.0
    while offset < dauer:
        laenge = min(block_s, dauer - offset)
        wav = _wav_fenster(pfad, start + offset, laenge)
        try:
            with open(wav, "rb") as f:
                resp = requests.post(
                    STT_URL,
                    files={"file": ("chunk.wav", f, "audio/wav")},
                    data={"response_format": "verbose_json"},
                    timeout=1200,
                )
            resp.raise_for_status()
            body = resp.json()
            for seg in body.get("segments", []):
                segmente.append(
                    {
                        "start": round(start + offset + seg["start"], 2),
                        "ende": round(start + offset + seg["end"], 2),
                        "text": seg["text"].strip(),
                    }
                )
        finally:
            os.unlink(wav)
        offset += laenge
    return segmente


def sprach_signale(segmente, schluesselwoerter):
    lachen = []
    schluessel = []
    for seg in segmente:
        text = seg["text"].lower()
        if any(w in text for w in LACH_WORTE):
            lachen.append((seg["start"], 1.0))
        if any(w in text.split() or w in text for w in schluesselwoerter):
            schluessel.append((seg["start"], 1.0))
    return lachen, schluessel
