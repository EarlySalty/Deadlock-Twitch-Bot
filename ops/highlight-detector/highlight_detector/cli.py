import argparse
import json
import os
import sys
import tempfile

os.environ.setdefault("OMP_THREAD_LIMIT", "1")

import cv2

from . import cache, config, corpus, db, detector, messen, report, schnitt, signals, video

CONFIG_DIR = config.CONFIG_DIR
REPO_ROOT = os.path.dirname(os.path.dirname(config.BASIS))
TASK_DIR = os.path.join(REPO_ROOT, ".tasks", "2026-09-06-highlight-erkennung-vod")
KAL_DIR = os.path.join(TASK_DIR, "kalibrierung")


def _vod_id(pfad):
    return os.path.splitext(os.path.basename(pfad))[0]


def _regionen_pixel(region, breite, hoehe):
    return (int(region["x0"] * breite), int(region["y0"] * hoehe),
            int(region["x1"] * breite), int(region["y1"] * hoehe))


def cmd_kalibrieren(args):
    cfg = config.lade_regionen()
    breite, hoehe = video.aufloesung(args.vod)
    dauer = video.dauer_s(args.vod)
    ziel = KAL_DIR
    os.makedirs(ziel, exist_ok=True)
    vid = _vod_id(args.vod)
    schritt = max(60.0, args.intervall_s)
    t = schritt
    anzahl = 0
    while t < dauer:
        frame = video.frame_bei(args.vod, t, breite, hoehe)
        if frame is None:
            t += schritt
            continue
        frame = frame.copy()
        for region in cfg["regionen"]:
            x0, y0, x1, y1 = _regionen_pixel(region, breite, hoehe)
            cv2.rectangle(frame, (x0, y0), (x1, y1), (0, 215, 255), 3)
            cv2.putText(frame, region["name"], (x0, max(0, y0 - 8)),
                        cv2.FONT_HERSHEY_SIMPLEX, 0.9, (0, 215, 255), 2)
            if region["name"] == "killfeed" and "teilung" in region:
                gx = int(x0 + (x1 - x0) * region["teilung"])
                cv2.line(frame, (gx, y0), (gx, y1), (255, 0, 0), 2)
        out = os.path.join(ziel, f"{vid}_{int(t)}.png")
        cv2.imwrite(out, frame)
        anzahl += 1
        t += schritt
    print(f"{anzahl} Kalibrierungs-Frames in {ziel}")


def _dsn():
    dsn = os.environ.get("DEADLOCK_CENTRAL_DSN", "").strip()
    if not dsn:
        print("DEADLOCK_CENTRAL_DSN nicht gesetzt. Erst per Infisical eval'en:\n"
              '  eval "$(python3 /home/nathanael/Documents/Infisical/export_gpt_secret.py '
              '--secret DEADLOCK_CENTRAL_DSN)"', file=sys.stderr)
        sys.exit(2)
    return dsn


def _lade_configs():
    return config.lade_regionen(), config.lade_gewichte()


def cmd_analyse(args):
    regionen_cfg, gewichte_cfg = _lade_configs()
    vid = _vod_id(args.vod)
    dauer = video.dauer_s(args.vod)
    ende = args.ende if args.ende else None

    signals.extrahiere_roh(args.vod, vid, regionen_cfg, start=args.start, ende=ende,
                           force=args.force, kein_stt=args.kein_stt, worker=args.worker)
    if args.nur_signale:
        print(f"Signale extrahiert und gecacht fuer {vid}")
        return

    events = signals.baue_events(vid, regionen_cfg, gewichte_cfg)
    f = gewichte_cfg["fenster"]
    kandidaten = detector.finde_kandidaten(
        events, gewichte_cfg["gewichte"], f["fenster_s"], f["schritt_s"],
        f["nms_abstand_s"], f["schwelle"], dauer,
    )
    ausgabe = {"vod_id": vid, "dauer_s": dauer, "schwelle": f["schwelle"], "kandidaten": kandidaten}

    if args.json:
        print(json.dumps(ausgabe, ensure_ascii=False, indent=2))
    else:
        print(f"VOD {vid} ({dauer/3600:.1f} h) - {len(kandidaten)} Kandidaten ueber Schwelle {f['schwelle']}")
        print(f"{'Start':>9} {'Ende':>9} {'Score':>7}  Signale")
        for k in kandidaten:
            sig = ", ".join(f"{s}={v}" for s, v in sorted(k["signale"].items(), key=lambda x: -x[1]))
            print(f"{k['start_s']:>9.1f} {k['end_s']:>9.1f} {k['score']:>7.3f}  {sig}")

    if args.schneiden:
        s = gewichte_cfg["schnitt"]
        geschnitten = schnitt.schneide_kandidaten(
            args.vod, vid, kandidaten, s["vorlauf_s"], s["nachlauf_s"], dauer)
        print(f"{len(geschnitten)} Clips geschnitten nach {schnitt.ausgabe_dir(vid)}")

    if args.persist:
        streamer_id = args.streamer_id
        conn = db.verbinde(_dsn())
        zeilen = [
            {"streamer_twitch_id": streamer_id, "vod_id": vid, "start_s": k["start_s"],
             "end_s": k["end_s"], "score": k["score"], "signale": k["signale"], "status": "neu"}
            for k in kandidaten
        ]
        try:
            db.schreibe_highlights(conn, zeilen)
            print(f"{len(zeilen)} Highlights in twitch_vod_highlights geschrieben")
        except db.TabelleFehlt as e:
            lokal = cache.schreibe_json(vid, "highlights.json", zeilen)
            print(f"Tabelle {e} fehlt (Prod-Migration ausstehend); lokal abgelegt: {lokal}")


def _lade_korpus_datei(pfad):
    with open(pfad) as f:
        roh = json.load(f)
    clips = []
    for e in roh:
        clips.append(
            {
                "clip_id": e["clip_id"],
                "clip_url": e.get("url") or e.get("clip_url"),
                "views": int(e.get("views", 0)),
                "dauer_s": float(e.get("dauer", e.get("dauer_s", 0)) or 0),
                "streamer_login": e.get("ersteller") or e.get("streamer_login"),
                "streamer_twitch_id": e.get("streamer_twitch_id"),
                "game_name": "Deadlock",
            }
        )
    return clips


def cmd_lernen(args):
    regionen_cfg, gewichte_cfg = _lade_configs()
    conn = db.verbinde(_dsn())
    clips = db.korpus_clips(conn, args.limit, nur_deadlock=not args.alle_spiele)
    print(f"{len(clips)} Partner-Clips aus der DB")
    if args.korpus_datei:
        geerntet = _lade_korpus_datei(args.korpus_datei)
        bekannt = {c["clip_id"] for c in clips}
        neu = [c for c in geerntet if c["clip_id"] not in bekannt and c["clip_url"]]
        clips = clips + neu
        print(f"{len(neu)} geerntete Deadlock-Clips aus {args.korpus_datei} dazu, {len(clips)} gesamt")
    alle_merkmale = []
    with tempfile.TemporaryDirectory(dir=args.tmp) as tmp:
        ergebnisse = corpus.verarbeite_korpus(clips, regionen_cfg, gewichte_cfg, tmp, worker=args.worker, kein_stt=args.kein_stt)
    for erg in ergebnisse:
        alle_merkmale.extend(corpus.merkmal_zeilen(erg))

    print(f"{len(ergebnisse)} Clips verarbeitet, {len(alle_merkmale)} Merkmalszeilen")
    cache.schreibe_jsonl("korpus", "merkmale.jsonl", alle_merkmale)
    try:
        db.schreibe_merkmale(conn, alle_merkmale)
        print("Merkmale in twitch_clip_merkmale geschrieben")
    except db.TabelleFehlt as e:
        print(f"Tabelle {e} fehlt (Prod-Migration ausstehend); Merkmale lokal in cache/korpus/merkmale.jsonl")

    gewichte, schwelle, praev = corpus.leite_gewichte_ab(ergebnisse, gewichte_cfg)
    corpus.schreibe_gewichte_toml(os.path.join(CONFIG_DIR, "gewichte.toml"), gewichte, schwelle, gewichte_cfg)
    print(f"Gewichte abgeleitet -> {os.path.join(CONFIG_DIR, 'gewichte.toml')}")
    text = report.baue_report(ergebnisse, praev, gewichte, schwelle)
    with open(args.report, "w") as f:
        f.write(text)
    print(f"Report -> {args.report}")


def cmd_messen(args):
    regionen_cfg, gewichte_cfg = _lade_configs()
    conn = db.verbinde(_dsn())
    streamer_id = db.aufloesen_streamer_id(conn, args.streamer)
    if not streamer_id:
        print(f"Streamer {args.streamer} nicht in twitch_streamers", file=sys.stderr)
        sys.exit(1)
    clips = db.echte_clips(conn, streamer_id)
    print(f"Streamer {args.streamer} (ID {streamer_id}): {len(clips)} echte Clips")

    vods = args.vod if args.vod else _alle_vods(args.vod_dir)
    gesamt_treffer = gesamt_verortet = gesamt_fp = 0
    f = gewichte_cfg["fenster"]
    for vpfad in vods:
        info = messen.vod_info(vpfad)
        if not info:
            continue
        fenster = messen.verorte_clips(clips, info)
        if args.span_ende:
            fenster = [cf for cf in fenster
                       if cf["start_s"] >= args.span_start and cf["end_s"] <= args.span_ende]
        if not fenster:
            continue
        vid = info["vod_id"]
        if not cache.existiert(vid, "ocr.jsonl"):
            print(f"  {vid}: kein Signal-Cache, erst 'analyse --nur-signale' laufen lassen", file=sys.stderr)
            continue
        events = signals.baue_events(vid, regionen_cfg, gewichte_cfg)
        kandidaten = detector.finde_kandidaten(
            events, gewichte_cfg["gewichte"], f["fenster_s"], f["schritt_s"],
            f["nms_abstand_s"], f["schwelle"], video.dauer_s(vpfad))
        ergebnis = messen.messe(kandidaten, fenster)
        gesamt_treffer += len(ergebnis["treffer"])
        gesamt_verortet += ergebnis["clips_verortet"]
        gesamt_fp += len(ergebnis["falsch_positive"])
        print(f"\n== VOD {vid}: {len(ergebnis['treffer'])}/{ergebnis['clips_verortet']} Clips getroffen, "
              f"{len(ergebnis['falsch_positive'])} Kandidaten ohne echten Clip ==")
        for t in ergebnis["treffer"]:
            print(f"  TREFFER v={t['views']} clip@{t['start_s']:.0f}-{t['end_s']:.0f}s "
                  f"abstand={t['abstand_s']}s score={t['kandidat']['score']}")
        for v in ergebnis["verpasst"]:
            print(f"  VERPASST v={v['views']} clip@{v['start_s']:.0f}-{v['end_s']:.0f}s  {v['clip_url']}")

    quote = round(gesamt_treffer / gesamt_verortet, 3) if gesamt_verortet else None
    print(f"\nGESAMT: {gesamt_treffer}/{gesamt_verortet} echte Clips getroffen (Quote {quote}), "
          f"{gesamt_fp} Falsch-Positive")


def _alle_vods(vod_dir):
    return sorted(os.path.join(vod_dir, f) for f in os.listdir(vod_dir) if f.endswith(".mp4"))


def main(argv=None):
    p = argparse.ArgumentParser(prog="highlight-detector")
    sub = p.add_subparsers(dest="cmd", required=True)

    k = sub.add_parser("kalibrieren")
    k.add_argument("vod")
    k.add_argument("--intervall-s", type=float, default=600.0)
    k.set_defaults(func=cmd_kalibrieren)

    a = sub.add_parser("analyse")
    a.add_argument("vod")
    a.add_argument("--start", type=float, default=0.0)
    a.add_argument("--ende", type=float, default=0.0)
    a.add_argument("--nur-signale", action="store_true")
    a.add_argument("--json", action="store_true")
    a.add_argument("--schneiden", action="store_true")
    a.add_argument("--force", action="store_true")
    a.add_argument("--kein-stt", action="store_true")
    a.add_argument("--worker", type=int, default=8)
    a.add_argument("--persist", action="store_true")
    a.add_argument("--streamer-id", default="")
    a.set_defaults(func=cmd_analyse)

    l = sub.add_parser("lernen")
    l.add_argument("--limit", type=int, default=800)
    l.add_argument("--worker", type=int, default=8)
    l.add_argument("--kein-stt", action="store_true")
    l.add_argument("--korpus-datei", default="")
    l.add_argument("--alle-spiele", action="store_true")
    l.add_argument("--tmp", default="/home/nathanael/vod-archive/highlights")
    l.add_argument("--report", default=os.path.join(TASK_DIR, "korpus-report.md"))
    l.set_defaults(func=cmd_lernen)

    m = sub.add_parser("messen")
    m.add_argument("--streamer", default="earlysalty")
    m.add_argument("--vod", nargs="*")
    m.add_argument("--vod-dir", default="/home/nathanael/vod-archive/downloads")
    m.add_argument("--span-start", type=float, default=0.0)
    m.add_argument("--span-ende", type=float, default=0.0)
    m.set_defaults(func=cmd_messen)

    args = p.parse_args(argv)
    args.func(args)


if __name__ == "__main__":
    main()
