from collections import Counter

from .corpus import FENSTER_ENDE_S, SIGNAL_TYPEN

SIGNAL_KLARTEXT = {
    "ocr_kill": "eigener Kill (Kill-Feed)",
    "ocr_tod": "eigener Tod (Kill-Feed)",
    "ocr_multikill": "Multikill-Einblendung",
    "ocr_objective": "Objective-Meldung",
    "ocr_soul_sprung": "Soul-Sprung",
    "sprache_pegel": "Lautstaerke-Spitze",
    "sprache_lachen": "Lachen",
    "sprache_schluesselwort": "Schluesselwort",
    "bild_szenenwechsel": "Szenenwechsel",
    "bild_death_screen": "Death-Screen (grau)",
    "bild_template": "Template-Treffer",
}


def baue_report(ergebnisse, praev_frac, gewichte, schwelle):
    n = len(ergebnisse)
    z = ["# Korpus-Report: Warum sind gute Clips gut", ""]
    z.append(f"- Verarbeitete Clips: **{n}**")
    gesamt_views = sum(e["clip"].get("views", 0) for e in ergebnisse)
    z.append(f"- Views gesamt im Korpus: {gesamt_views}")
    z.append(f"- Merkmalsfenster: letzte {int(FENSTER_ENDE_S)} s vor Clip-Ende")
    z.append(f"- Abgeleitete Score-Schwelle: **{schwelle}**")
    z.append("")

    z.append("## Signal-Verteilung (Anteil der Clips mit dem Signal im Endfenster)")
    z.append("")
    z.append("| Signal | Anteil Clips | abgeleitetes Gewicht |")
    z.append("|---|---|---|")
    for typ in sorted(SIGNAL_TYPEN, key=lambda t: praev_frac.get(t, 0), reverse=True):
        z.append(
            f"| {SIGNAL_KLARTEXT.get(typ, typ)} | {praev_frac.get(typ,0)*100:.0f}% | {gewichte.get(typ,0)} |"
        )
    z.append("")

    z.append("## Wann feuern die Signale relativ zum Clip-Ende")
    z.append("")
    for typ in SIGNAL_TYPEN:
        offsets = [m["offset_vom_ende_s"] for e in ergebnisse for m in e["merkmale"] if m["signal"] == typ]
        if not offsets:
            continue
        mittel = sum(offsets) / len(offsets)
        z.append(f"- {SIGNAL_KLARTEXT.get(typ, typ)}: {len(offsets)} Treffer, Mittel {mittel:.1f} s vor Ende")
    z.append("")

    z.append("## Top-Muster mit echten Beispiel-Clips")
    z.append("")
    muster = []
    for e in ergebnisse:
        typen = tuple(sorted({m["signal"] for m in e["merkmale"]
                              if m["signal"] in ("ocr_kill", "ocr_multikill", "sprache_lachen",
                                                 "sprache_pegel", "ocr_soul_sprung", "bild_death_screen")}))
        if typen:
            muster.append((typen, e["clip"]))
    zaehler = Counter(t for t, _ in muster)
    for typen, anzahl in zaehler.most_common(8):
        klar = " + ".join(SIGNAL_KLARTEXT.get(t, t) for t in typen)
        z.append(f"### {klar} ({anzahl} Clips)")
        beispiele = [c for t, c in muster if t == typen]
        beispiele.sort(key=lambda c: c.get("views", 0), reverse=True)
        for c in beispiele[:3]:
            z.append(f"- [{c.get('views',0)} Views] {c['clip_url']}")
        z.append("")

    if not muster:
        z.append("_Kein Muster mit Kernsignalen gefunden (Korpus zu klein oder OCR/Sprache leer)._")
        z.append("")

    return "\n".join(z)
