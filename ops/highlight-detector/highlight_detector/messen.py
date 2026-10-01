import json
import os


def vod_info(vod_pfad):
    info_pfad = os.path.splitext(vod_pfad)[0] + ".info.json"
    if not os.path.exists(info_pfad):
        return None
    with open(info_pfad) as f:
        d = json.load(f)
    if not (d.get("id") and d.get("timestamp") and d.get("duration")):
        return None
    return {"vod_id": d["id"], "start_epoch": d["timestamp"], "dauer_s": float(d["duration"])}


def verorte_clips(clips, info):
    fenster = []
    vs = info["start_epoch"]
    ve = vs + info["dauer_s"]
    for c in clips:
        ce = c.get("created_epoch")
        if ce is None or not (vs <= ce <= ve):
            continue
        ende_off = float(ce - vs)
        start_off = max(0.0, ende_off - max(c["dauer_s"], 1.0))
        fenster.append(
            {
                "clip_id": c["clip_id"], "clip_url": c["clip_url"], "views": c["views"],
                "titel": c.get("titel", ""), "start_s": start_off, "end_s": ende_off,
            }
        )
    return fenster


def _naehe(a0, a1, b0, b1):
    if a1 >= b0 and b1 >= a0:
        return 0.0
    return min(abs(a0 - b1), abs(b0 - a1))


def messe(kandidaten, clip_fenster, toleranz_s=15.0):
    treffer = []
    verpasst = []
    genutzte_kandidaten = set()
    for cf in clip_fenster:
        bester = None
        for i, k in enumerate(kandidaten):
            d = _naehe(k["start_s"], k["end_s"], cf["start_s"], cf["end_s"])
            if d <= toleranz_s and (bester is None or d < bester[1]):
                bester = (i, d)
        if bester is not None:
            genutzte_kandidaten.add(bester[0])
            treffer.append({**cf, "abstand_s": round(bester[1], 1),
                            "kandidat": kandidaten[bester[0]]})
        else:
            verpasst.append(cf)
    falsch_positive = [k for i, k in enumerate(kandidaten) if i not in genutzte_kandidaten]
    return {
        "clips_verortet": len(clip_fenster),
        "treffer": treffer,
        "verpasst": verpasst,
        "falsch_positive": falsch_positive,
        "trefferquote": round(len(treffer) / len(clip_fenster), 3) if clip_fenster else None,
    }
