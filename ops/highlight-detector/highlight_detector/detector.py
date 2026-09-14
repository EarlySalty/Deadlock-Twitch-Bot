def fenster_score(events, gewichte, t0, t1):
    beitraege_wert = {}
    for t, typ, wert in events:
        if t0 <= t < t1:
            if wert > beitraege_wert.get(typ, 0.0):
                beitraege_wert[typ] = wert
    einzel = {}
    score = 0.0
    for typ, wert in beitraege_wert.items():
        g = gewichte.get(typ, 0.0)
        if g == 0.0:
            continue
        beitrag = g * wert
        einzel[typ] = beitrag
        score += beitrag
    return score, einzel


def finde_kandidaten(events, gewichte, fenster_s, schritt_s, nms_abstand_s, schwelle, dauer_s):
    fenster = []
    t0 = 0.0
    while t0 < dauer_s:
        t1 = t0 + fenster_s
        score, einzel = fenster_score(events, gewichte, t0, t1)
        if score >= schwelle:
            fenster.append(
                {
                    "start_s": round(t0, 2),
                    "end_s": round(t1, 2),
                    "mitte_s": round(t0 + fenster_s / 2.0, 2),
                    "score": round(score, 4),
                    "signale": {k: round(v, 4) for k, v in einzel.items()},
                }
            )
        t0 += schritt_s

    fenster.sort(key=lambda k: k["score"], reverse=True)
    behalten = []
    for kandidat in fenster:
        if all(abs(kandidat["mitte_s"] - b["mitte_s"]) >= nms_abstand_s for b in behalten):
            behalten.append(kandidat)
    behalten.sort(key=lambda k: k["start_s"])
    return behalten
