#!/usr/bin/env python3
"""Erkennt Viewbot- und Chatbot-Muster in den aufgezeichneten Stream-Daten.

Liest twitch_session_viewers / twitch_chat_messages / twitch_session_chatters aus
der Analytics-DB und schreibt einen HTML-Report mit allen Einzelbefunden.

Aufruf:
    ./scripts/run_with_infisical.sh .venv/bin/python tools/viewbot_detect.py <login> [--out DATEI]

Grundsatz: jede geprüfte Session taucht im Report auf, auch die unauffälligen.
Nur die Treffer zu zeigen würde verschweigen, wie breit überhaupt geprüft wurde.
"""
from __future__ import annotations

import argparse
import html
import os
import statistics
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timedelta

import psycopg

# --- Schwellwerte, in Realwelt-Einheiten benannt -----------------------------

# Ein Publikum kann nicht schlagartig verschwinden: fällt die Zuschauerzahl
# innerhalb einer Minute um mehr als die Hälfte, hat eine Instanz die
# Verbindung getrennt, nicht ein Mensch den Tab geschlossen.
COLLAPSE_WINDOW = timedelta(seconds=90)
COLLAPSE_MIN_ABS = 12          # Zuschauer
COLLAPSE_MIN_FRAC = 0.40       # Anteil des Vorwerts

# Umgekehrt: ein Zustrom in unter 90 s ohne Raid ist eine Einspeisung. Nur zählt
# er erst, wenn der Kanal seine Anlaufphase hinter sich hat — bis dahin trudelt
# das Stammpublikum ein und erzeugt dieselben Sprünge.
INJECT_WINDOW = timedelta(seconds=90)
INJECT_MIN_ABS = 12
INJECT_MIN_FRAC = 0.25         # Anteil des Vorwerts; darunter ist es API-Rauschen
# Unter zehn Zuschauern erklärt schon ein geteilter Link jeden Sprung. Erst ab
# einem etablierten Publikum ist ein Zustrom dieser Größe erklärungsbedürftig.
INJECT_MIN_BASE = 10

# Ein Raid erklärt Zu- wie Abstrom in diesem Umkreis.
RAID_GRACE = timedelta(minutes=6)

# Stream-Neustart: Twitch vergibt eine neue Session-ID, die Zuschauer bleiben.
# Alles innerhalb dieser Spanne nach Sessionbeginn ist übernommenes Publikum.
SESSION_WARMUP = timedelta(minutes=8)
# ... und eine Session, die kurz nach dem Ende der vorigen beginnt, ist deren
# Fortsetzung — ihr hoher Startwert ist kein Spike.
CONTINUATION_GAP = timedelta(minutes=15)

# Ein Mensch tippt unregelmäßig. Ein Scheduler nicht. Gemessen wird die Streuung
# im Verhältnis zum mittleren Abstand: ein zufälliger Ankunftsprozess — also ein
# lebhafter Chat — liegt bei rund 1,0, ein fester Zeitplan geht gegen 0. Ein
# absolutes Sekundenmaß taugt nicht, weil ein Chat mit 7-Sekunden-Takt
# zwangsläufig kleiner streut als einer mit 52.
CADENCE_MIN_MSGS = 15
CADENCE_MAX_CV = 0.25          # Streuung geteilt durch mittleren Abstand
CADENCE_MAX_MEAN = 300.0       # Sekunden
# Zwei Konten im Gleichtakt sind meistens ein Kanal-Bot plus ein Stammgast, der
# ihn bedient. Ein eingekaufter Ring besteht aus mehreren Konten.
CADENCE_MIN_ACCOUNTS = 3

# Unsere eigenen Bots und die üblichen Kanal-Bots posten planmäßig — sie dürfen
# den Ring nicht bilden, sonst meldet jeder Kanal seinen eigenen Chatbot.
OWN_ACCOUNTS = {"deutschedeadlockcommunity", "nehringgg"}
KNOWN_CHANNEL_BOTS = {
    "nightbot", "streamelements", "streamlabs", "moobot", "fossabot", "wizebot",
    "sery_bot", "own3d", "streamholics", "botrixoficial", "kofistreambot",
    "soundalerts", "creatisbot", "pretzelrocks", "commanderroot", "lurxx",
}


@dataclass
class Session:
    id: int
    started_at: datetime
    ended_at: datetime | None
    duration_min: int
    peak: int
    avg: float
    title: str | None
    samples: list[tuple[datetime, int]] = field(default_factory=list)
    continuation_of: int | None = None


@dataclass
class Finding:
    kind: str
    at: datetime
    session_id: int
    detail: str
    severity: str


def connect() -> psycopg.Connection:
    dsn = os.environ.get("TWITCH_ANALYTICS_DSN")
    if not dsn:
        sys.exit("TWITCH_ANALYTICS_DSN fehlt — über scripts/run_with_infisical.sh starten.")
    return psycopg.connect(dsn)


def load_sessions(conn: psycopg.Connection, login: str) -> list[Session]:
    with conn.cursor() as cur:
        cur.execute(
            """
            SELECT id, started_at, ended_at, COALESCE(duration_seconds, 0) / 60,
                   COALESCE(peak_viewers, 0), COALESCE(avg_viewers, 0), stream_title
            FROM twitch_stream_sessions
            WHERE lower(streamer_login) = %s
            ORDER BY started_at
            """,
            (login,),
        )
        sessions = [
            Session(id=r[0], started_at=r[1], ended_at=r[2], duration_min=r[3],
                    peak=r[4], avg=float(r[5]), title=r[6])
            for r in cur.fetchall()
        ]
        by_id = {s.id: s for s in sessions}
        cur.execute(
            """
            SELECT v.session_id, v.ts_utc, v.viewer_count
            FROM twitch_session_viewers v
            JOIN twitch_stream_sessions s ON s.id = v.session_id
            WHERE lower(s.streamer_login) = %s
            ORDER BY v.session_id, v.ts_utc
            """,
            (login,),
        )
        for sid, ts, vc in cur.fetchall():
            if sid in by_id:
                by_id[sid].samples.append((ts, vc))

    markiere_fortsetzungen(sessions)
    return sessions


def markiere_fortsetzungen(sessions: list[Session]) -> None:
    """Markiert jede Session, die kurz nach der vorigen beginnt.

    Sonst liest sich jeder Werbe- oder Absturz-Neustart als Einspeisung: Twitch
    vergibt eine neue Session-ID, das Publikum bleibt. `scan_viewers` wertet in
    den Markierten keinen Zuwachs, prüft sie aber weiter auf Einbrüche.
    """
    for prev, cur_s in zip(sessions, sessions[1:]):
        prev_end = prev.ended_at or (prev.started_at + timedelta(minutes=prev.duration_min))
        if timedelta(0) <= cur_s.started_at - prev_end <= CONTINUATION_GAP:
            cur_s.continuation_of = prev.id


def load_raids(conn: psycopg.Connection, login: str) -> list[datetime]:
    with conn.cursor() as cur:
        cur.execute(
            "SELECT executed_at FROM twitch_raid_history "
            "WHERE lower(to_broadcaster_login) = %s AND success ORDER BY executed_at",
            (login,),
        )
        return [r[0] for r in cur.fetchall()]


def load_messages(conn: psycopg.Connection, login: str) -> list[tuple[datetime, str, str]]:
    with conn.cursor() as cur:
        cur.execute(
            "SELECT message_ts, chatter_login, COALESCE(content, '') "
            "FROM twitch_chat_messages WHERE lower(streamer_login) = %s ORDER BY message_ts",
            (login,),
        )
        return [(r[0], (r[1] or "").lower(), r[2]) for r in cur.fetchall()]


def near_raid(ts: datetime, raids: list[datetime]) -> bool:
    return any(abs(ts - r) <= RAID_GRACE for r in raids)


def scan_viewers(sessions: list[Session], raids: list[datetime]) -> list[Finding]:
    """Sucht Sprünge, die kein Publikum erzeugen kann."""
    out: list[Finding] = []
    for s in sessions:
        if len(s.samples) < 10:
            continue
        # Fortsetzung eines Neustarts: das Publikum der Vorsession ist noch da,
        # jede Rückkehr sähe sonst wie eine Einspeisung aus. Der Ausschluss gilt
        # deshalb nur für den Zustrom — ein Einbruch bleibt ein Einbruch, egal
        # woher das Publikum kam, und eine übersprungene Session als "geprüft"
        # zu zählen wäre eine Umfangsbehauptung ohne Deckung.
        zustrom_erklaert = s.continuation_of is not None
        end = s.ended_at or (s.started_at + timedelta(minutes=s.duration_min))
        for (t0, v0), (t1, v1) in zip(s.samples, s.samples[1:]):
            dt = t1 - t0
            # Warmup und Sessionende erzeugen systembedingt Sprünge.
            if t1 - s.started_at < SESSION_WARMUP or end - t1 < timedelta(minutes=2):
                continue
            if near_raid(t1, raids):
                continue
            delta = v1 - v0
            if delta <= -COLLAPSE_MIN_ABS and dt <= COLLAPSE_WINDOW and v0 > 0 \
                    and (-delta) / v0 >= COLLAPSE_MIN_FRAC:
                out.append(Finding(
                    "kollaps", t1, s.id,
                    f"{v0} → {v1} Zuschauer in {int(dt.total_seconds())} s "
                    f"({delta}, {100 * -delta / v0:.0f} %)",
                    "hoch",
                ))
            elif (not zustrom_erklaert
                  and delta >= INJECT_MIN_ABS and dt <= INJECT_WINDOW
                  and v0 >= INJECT_MIN_BASE
                  and delta / v0 >= INJECT_MIN_FRAC):
                out.append(Finding(
                    "einspeisung", t1, s.id,
                    f"{v0} → {v1} Zuschauer in {int(dt.total_seconds())} s "
                    f"(+{delta}, {100 * delta / v0:.0f} %), kein Raid",
                    "mittel",
                ))
    return out


def _gap_stats(msgs: list[tuple[datetime, str, str]]) -> tuple[float, float, float] | None:
    """Liefert mittleren Abstand, Streuung und Variationskoeffizient."""
    gaps = [(b[0] - a[0]).total_seconds() for a, b in zip(msgs, msgs[1:])]
    gaps = [g for g in gaps if g > 0]
    if len(gaps) < CADENCE_MIN_MSGS - 1:
        return None
    mean = statistics.mean(gaps)
    sd = statistics.pstdev(gaps)
    return mean, sd, sd / mean if mean else 1.0


def core_ring(pool: list[tuple[datetime, str, str]]) -> list[tuple[datetime, str, str]]:
    """Schält aus dem Chat die Konten heraus, die gemeinsam den festen Takt bilden.

    In einer echten Session schreiben Bots und Menschen durcheinander; über alle
    zusammen gemittelt sieht der Takt unregelmäßiger aus, als er ist. Deshalb wird
    das Konto, dessen Wegfall die Streuung am stärksten senkt, so lange entfernt,
    wie das die Streuung überhaupt noch senkt. Übrig bleibt der Kern.
    """
    best = list(pool)
    konten = {m[1] for m in best}
    while len(konten) > 2:
        stats = _gap_stats(best)
        if stats is None:
            return []
        cur_cv = stats[2]
        cand: tuple[float, str, list] | None = None
        for k in konten:
            sub = [m for m in best if m[1] != k]
            if len(sub) < CADENCE_MIN_MSGS:
                continue
            st = _gap_stats(sub)
            if st is not None and (cand is None or st[2] < cand[0]):
                cand = (st[2], k, sub)
        if cand is None or cand[0] >= cur_cv:
            break
        _, entfernt, best = cand
        konten.discard(entfernt)
    return best


def is_channel_bot(account: str, streamer: str) -> bool:
    """Kanal-eigene Bots heißen fast immer nach ihrem Kanal (miracleg_bot bei
    miracleghost9) oder stehen in der Liste der verbreiteten Dienste."""
    if account in OWN_ACCOUNTS or account in KNOWN_CHANNEL_BOTS:
        return True
    if account == streamer:
        return True
    stem = streamer.rstrip("0123456789_")
    return "bot" in account and len(stem) >= 4 and stem[:6] in account


def scan_cadence(messages: list[tuple[datetime, str, str]], sessions: list[Session],
                 streamer: str = "") -> tuple[list[Finding], dict[str, dict]]:
    """Ein fester Takt über mehrere Konten hinweg ist ein Scheduler."""
    findings: list[Finding] = []
    per_session: dict[int, list[tuple[datetime, str, str]]] = defaultdict(list)
    for ts, who, content in messages:
        for s in sessions:
            end = s.ended_at or (s.started_at + timedelta(minutes=s.duration_min))
            if s.started_at <= ts <= end + timedelta(minutes=5):
                per_session[s.id].append((ts, who, content))
                break

    accounts: dict[str, dict] = {}
    for sid, msgs in sorted(per_session.items()):
        pool = [m for m in msgs if not is_channel_bot(m[1], streamer)]
        if len(pool) < CADENCE_MIN_MSGS:
            continue
        ring = core_ring(pool)
        stats = _gap_stats(ring) if ring else None
        if stats is None:
            continue
        mean, sd, cv = stats
        if cv > CADENCE_MAX_CV or mean > CADENCE_MAX_MEAN:
            continue
        who = sorted({m[1] for m in ring})
        if len(who) < CADENCE_MIN_ACCOUNTS:
            continue
        findings.append(Finding(
            "chat-takt", ring[0][0], sid,
            f"{len(ring)} Nachrichten von {len(who)} Konten im Takt "
            f"{mean:.0f} s ± {sd:.1f} s (Streuungsmaß {cv:.2f}) — {', '.join(who)}",
            "hoch",
        ))
        for ts, w, content in ring:
            acc = accounts.setdefault(w, {"msgs": 0, "sessions": set(), "beispiele": []})
            acc["sessions"].add(sid)
            acc["msgs"] += 1
            if len(acc["beispiele"]) < 6 and content.strip():
                acc["beispiele"].append(content.strip()[:90])
    return findings, accounts


def sparkline(samples: list[tuple[datetime, int]], width: int = 900, height: int = 90) -> str:
    if len(samples) < 2:
        return ""
    vals = [v for _, v in samples]
    hi = max(vals) or 1
    t0 = samples[0][0]
    span = (samples[-1][0] - t0).total_seconds() or 1
    pts = " ".join(
        f"{(ts - t0).total_seconds() / span * width:.1f},{height - v / hi * (height - 6):.1f}"
        for ts, v in samples
    )
    return (f'<svg viewBox="0 0 {width} {height}" preserveAspectRatio="none" class="spark">'
            f'<polyline points="{pts}"/></svg>')


def render(login: str, sessions: list[Session], findings: list[Finding],
           accounts: dict[str, dict], raids: list[datetime], msg_count: int) -> str:
    e = html.escape
    by_session: dict[int, list[Finding]] = defaultdict(list)
    for f in findings:
        by_session[f.session_id].append(f)

    kollaps = [f for f in findings if f.kind == "kollaps"]
    einspeisung = [f for f in findings if f.kind == "einspeisung"]
    takt = [f for f in findings if f.kind == "chat-takt"]

    rows = []
    for s in sorted(sessions, key=lambda x: x.started_at, reverse=True):
        fs = by_session.get(s.id, [])
        if not s.samples:
            state, cls = "keine Messwerte", "leer"
        elif any(f.severity == "hoch" for f in fs):
            state, cls = "auffällig", "hoch"
        elif fs:
            state, cls = "prüfen", "mittel"
        else:
            state, cls = "unauffällig", "ok"
        note = " · ".join(f"{f.kind} {f.at:%H:%M}: {f.detail}" for f in fs) or "—"
        cont = f' <span class="cont">Fortsetzung von #{s.continuation_of}</span>' if s.continuation_of else ""
        rows.append(
            f'<tr class="{cls}"><td>{s.started_at:%Y-%m-%d %H:%M}</td><td>#{s.id}{cont}</td>'
            f'<td>{s.duration_min} min</td><td>{s.peak}</td><td>{s.avg:.0f}</td>'
            f'<td><span class="badge {cls}">{state}</span></td><td class="note">{e(note)}</td></tr>'
        )

    acc_rows = []
    for who, data in sorted(accounts.items(), key=lambda kv: -kv[1]["msgs"]):
        bsp = "<br>".join(e(b) for b in data["beispiele"])
        acc_rows.append(
            f'<tr><td class="mono">{e(who)}</td><td>{data["msgs"]}</td>'
            f'<td>{len(data["sessions"])}</td><td class="bsp">{bsp}</td></tr>'
        )

    charts = []
    for s in sorted(sessions, key=lambda x: x.started_at, reverse=True):
        fs = by_session.get(s.id, [])
        if not fs or not s.samples:
            continue
        marks = "".join(
            f'<li><b>{f.at:%H:%M:%S}</b> {e(f.kind)} — {e(f.detail)}</li>' for f in fs)
        charts.append(
            f'<section class="chart"><h3>#{s.id} · {s.started_at:%d.%m.%Y %H:%M} · '
            f'{s.duration_min} min · Peak {s.peak}</h3>'
            f'{sparkline(s.samples)}<ul>{marks}</ul></section>'
        )

    checked = sum(1 for s in sessions if s.samples)
    return f"""<!doctype html>
<html lang="de"><head><meta charset="utf-8">
<title>Bot-Analyse {e(login)}</title>
<style>
:root {{ color-scheme: dark; }}
body {{ background:#12100c; color:#e8e2d4; font:15px/1.55 ui-sans-serif,system-ui,sans-serif;
        margin:0; padding:2.5rem clamp(1rem,4vw,4rem); }}
h1 {{ font-size:1.7rem; margin:0 0 .2rem; color:#e6b64c; }}
h2 {{ font-size:1.15rem; margin:2.5rem 0 .8rem; color:#e6b64c;
      border-bottom:1px solid #3a3427; padding-bottom:.35rem; }}
h3 {{ font-size:.95rem; margin:0 0 .5rem; font-weight:600; }}
.sub {{ color:#9a927f; margin:0 0 2rem; }}
.cards {{ display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:.8rem; }}
.card {{ background:#1b1811; border:1px solid #332e22; border-radius:10px; padding:.9rem 1rem; }}
.card b {{ display:block; font-size:1.7rem; color:#e6b64c; font-weight:600; }}
.card span {{ color:#9a927f; font-size:.82rem; }}
table {{ width:100%; border-collapse:collapse; font-size:.85rem; }}
th {{ text-align:left; color:#9a927f; font-weight:500; padding:.4rem .6rem;
      border-bottom:1px solid #332e22; }}
td {{ padding:.4rem .6rem; border-bottom:1px solid #221f18; vertical-align:top; }}
tr.hoch td:first-child {{ box-shadow:inset 3px 0 0 #d4574a; }}
tr.mittel td:first-child {{ box-shadow:inset 3px 0 0 #e6b64c; }}
.badge {{ font-size:.75rem; padding:.1rem .5rem; border-radius:99px; white-space:nowrap; }}
.badge.hoch {{ background:#3d1f1b; color:#f0897c; }}
.badge.mittel {{ background:#3a301a; color:#e6b64c; }}
.badge.ok {{ background:#1f2a1f; color:#8fbf7f; }}
.badge.leer {{ background:#26241d; color:#7d7566; }}
.note {{ color:#a89f8b; font-size:.8rem; }}
.cont {{ color:#7d7566; font-size:.75rem; }}
.mono {{ font-family:ui-monospace,monospace; }}
.bsp {{ color:#a89f8b; font-size:.78rem; }}
.chart {{ background:#1b1811; border:1px solid #332e22; border-radius:10px;
          padding:1rem 1.2rem; margin-bottom:1rem; }}
.spark {{ width:100%; height:90px; display:block; }}
.spark polyline {{ fill:none; stroke:#e6b64c; stroke-width:1.5; vector-effect:non-scaling-stroke; }}
.chart ul {{ margin:.6rem 0 0; padding-left:1.1rem; color:#a89f8b; font-size:.8rem; }}
.method {{ background:#1b1811; border:1px solid #332e22; border-radius:10px; padding:1rem 1.4rem; }}
.method li {{ margin:.35rem 0; color:#c3bba7; }}
</style></head><body>
<h1>Bot-Analyse: {e(login)}</h1>
<p class="sub">{checked} von {len(sessions)} Sessions mit Messwerten geprüft ·
{msg_count} erfasste Chat-Nachrichten · {len(raids)} bestätigte Raids ·
erstellt {datetime.now():%d.%m.%Y %H:%M}</p>

<div class="cards">
  <div class="card"><b>{len(kollaps)}</b><span>Zuschauer-Einbrüche</span></div>
  <div class="card"><b>{len(einspeisung)}</b><span>Einspeisungen ohne Raid</span></div>
  <div class="card"><b>{len(takt)}</b><span>Sessions mit Chat-Takt</span></div>
  <div class="card"><b>{len(accounts)}</b><span>Takt-Konten</span></div>
</div>

<h2>Erkannte Chat-Konten</h2>
<table><thead><tr><th>Konto</th><th>Nachrichten</th><th>Sessions</th><th>Beispiele</th></tr></thead>
<tbody>{"".join(acc_rows) or '<tr><td colspan="4">keine</td></tr>'}</tbody></table>

<h2>Auffällige Sessions im Verlauf</h2>
{"".join(charts) or "<p>keine</p>"}

<h2>Alle Sessions</h2>
<table><thead><tr><th>Beginn</th><th>Session</th><th>Dauer</th><th>Peak</th><th>Ø</th>
<th>Bewertung</th><th>Befund</th></tr></thead><tbody>{"".join(rows)}</tbody></table>

<h2>Wie geprüft wurde</h2>
<div class="method"><ul>
<li><b>Zuschauer-Einbruch:</b> Rückgang um mindestens {COLLAPSE_MIN_ABS} Zuschauer
    <i>und</i> {COLLAPSE_MIN_FRAC:.0%} des Vorwerts in höchstens
    {int(COLLAPSE_WINDOW.total_seconds())} Sekunden. Echtes Publikum geht nach und nach,
    getrennte Verbindungen gehen gemeinsam.</li>
<li><b>Einspeisung:</b> Zuwachs von mindestens {INJECT_MIN_ABS} Zuschauern in
    {int(INJECT_WINDOW.total_seconds())} Sekunden ohne Raid im Umkreis von
    {int(RAID_GRACE.total_seconds() // 60)} Minuten.</li>
<li><b>Chat-Takt:</b> mindestens {CADENCE_MIN_MSGS} Nachrichten von mindestens
    {CADENCE_MIN_ACCOUNTS} Konten, deren Abstände um weniger als
    {CADENCE_MAX_CV:.0%} des mittleren Abstands streuen. Ein lebhafter Chat kommt
    in Wellen und liegt bei rund 100 %, ein Zeitplan geht gegen null. Aus dem Chat
    einer Session wird dabei so lange das Konto entfernt, dessen Wegfall die
    Streuung senkt — übrig bleiben nur die Konten, die gemeinsam den Takt bilden.</li>
<li><b>Ausgeschlossen:</b> die ersten {int(SESSION_WARMUP.total_seconds() // 60)} Minuten jeder
    Session, die letzten zwei Minuten vor Streamende sowie alle bestätigten Raids.
    In Sessions, die innerhalb von {int(CONTINUATION_GAP.total_seconds() // 60)} Minuten an die
    vorige anschließen, zählt zusätzlich kein Zuwachs (Twitch vergibt beim Neustart eine
    neue Session, das Publikum bleibt) — auf Einbrüche werden auch sie geprüft.</li>
</ul></div>
</body></html>"""


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("login")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()
    login = args.login.lstrip("#").lower()
    out = args.out or f"security-reports/botanalyse-{login}.html"

    with connect() as conn:
        sessions = load_sessions(conn, login)
        if not sessions:
            print(f"Keine Sessions für {login} aufgezeichnet.")
            return 1
        raids = load_raids(conn, login)
        messages = load_messages(conn, login)

    viewer_findings = scan_viewers(sessions, raids)
    chat_findings, accounts = scan_cadence(messages, sessions, login)
    findings = sorted(viewer_findings + chat_findings, key=lambda f: f.at)

    # Erst hier, nach der gesamten DB-Arbeit, wird geschrieben: ein fehlendes
    # Zielverzeichnis darf den Lauf nicht am Ende noch wegwerfen.
    zielordner = os.path.dirname(out)
    if zielordner:
        os.makedirs(zielordner, exist_ok=True)
    with open(out, "w", encoding="utf-8") as fh:
        fh.write(render(login, sessions, findings, accounts, raids, len(messages)))

    # Jede Entscheidung sichtbar machen, nicht nur die Treffer.
    checked = sum(1 for s in sessions if s.samples)
    print(f"{login}: {checked}/{len(sessions)} Sessions mit Messwerten geprüft, "
          f"{len(messages)} Chat-Nachrichten, {len(raids)} Raids ausgeklammert.")
    for kind in ("kollaps", "einspeisung", "chat-takt"):
        hits = [f for f in findings if f.kind == kind]
        print(f"  {kind:12s} {len(hits):3d}")
        for f in hits:
            print(f"      {f.at:%Y-%m-%d %H:%M:%S} #{f.session_id}  {f.detail}")
    print(f"Report: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
