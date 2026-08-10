#!/usr/bin/env python3
"""Ordnet die Chat-Konten eines Kanals nach Belastbarkeit ein.

Kein Einzelmerkmal reicht für ein Urteil: Registrierungswellen treffen auch echte
Zuschauer, Abschieds-Salven kommen in echten Chats vor, und stumme Zuschauer sind
der Normalfall. Erst wenn mehrere voneinander unabhängige Merkmale gleichzeitig
zutreffen, wird aus einem Verdacht ein Befund — und Merkmale, die ein Mensch
belegen kann (Gespräch, Wanderung durch die Szene, Follow), heben ein Konto
sofort wieder heraus.

Aufruf:
    ./scripts/run_with_infisical.sh .venv/bin/python tools/chatter_classify.py <login>
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from datetime import datetime, timedelta

import psycopg

BATCH = 100

# Verhaltensbeweise: beobachtete Koordination mit anderen Konten. Nur diese
# tragen ein Urteil über ein einzelnes Konto, weil ein Mensch sie nicht
# unabsichtlich erzeugt.
BEWEIS = {
    "taktnachricht": "schreibt im festen Zeittakt mit anderen Konten",
    "salve": "schreibt ausschließlich in einer Nachrichtenwelle am Streamende",
}
# Kontomerkmale: als Gruppe aussagekräftig, für sich genommen aber schwach.
# Ein stiller Zuschauer, der zufällig im selben Monat registriert wurde wie ein
# Pool, erfüllt alle drei — und ist trotzdem ein Mensch.
MERKMAL = {
    "welle": "stammt aus einer Registrierungswelle des Kanals",
    "kanalgebunden": "taucht bei keinem anderen Kanal der Szene auf",
    "nie_gefolgt": "hat keinem Partnerkanal je gefolgt",
    "einmalnachricht": "genau eine Nachricht in der gesamten Aufzeichnung",
}
# Merkmale, die ein Konto herausnehmen. Sie schlagen jeden Verdacht.
ENTLASTUNG = {
    "wandert": "taucht bei mehreren Kanälen der Szene auf",
    "folgt": "folgt einem Partnerkanal",
    "stammgast": "schreibt an mehreren Tagen über einen längeren Zeitraum",
}
# Ein Stammgast kommt wieder: mehrere Schreibtage mit echtem Abstand dazwischen.
# Ein für einen Auftrag eingesetztes Konto schreibt an einem oder zwei Tagen am
# Stück und verschwindet.
STAMMGAST_MIN_TAGE = 2
STAMMGAST_MIN_SPANNE = timedelta(days=7)

# Eine Salve: viele verschiedene Konten in kurzer Folge, weit über der sonstigen
# Schlagzahl der Session.
SALVE_FENSTER = timedelta(seconds=45)
SALVE_MIN_KONTEN = 5
# Nur Wellen in diesem Schlussfenster der Session zählen. Mitten im Stream ist
# eine Welle aus fünf Konten normaler Chat, kein koordiniertes Verhalten.
SALVE_ENDFENSTER = timedelta(minutes=10)

# Zusammenhängendes Gespräch: mehrere Nachrichten desselben Kontos in kurzem
# Abstand — ein Mensch, der auf etwas antwortet, schreibt nach.


@dataclass
class Konto:
    login: str
    erstellt: datetime | None = None
    sessions: int = 0
    msgs: int = 0
    kanaele: int = 0
    beweis: set[str] = field(default_factory=set)
    merkmal: set[str] = field(default_factory=set)
    entlastung: set[str] = field(default_factory=set)

    @property
    def urteil(self) -> str:
        """Ein Urteil über ein einzelnes Konto braucht beobachtete Koordination.

        Kontomerkmale allein reichen nie: sie beschreiben eine Gruppe, in der ein
        einzelner echter Zuschauer nicht von einem Pool-Konto zu trennen ist.
        """
        if self.entlastung:
            return "entlastet"
        if self.beweis:
            return "belegt"
        if len(self.merkmal) >= 3:
            return "gruppenverdacht"
        return "unauffaellig"


def app_token(cid: str, secret: str) -> str:
    body = urllib.parse.urlencode({"client_id": cid, "client_secret": secret,
                                   "grant_type": "client_credentials"}).encode()
    req = urllib.request.Request("https://id.twitch.tv/oauth2/token", data=body)
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.load(r)["access_token"]


def fetch_users(logins: list[str], cid: str, token: str) -> dict[str, dict]:
    out: dict[str, dict] = {}
    headers = {"Client-Id": cid, "Authorization": f"Bearer {token}"}
    for i in range(0, len(logins), BATCH):
        url = ("https://api.twitch.tv/helix/users?"
               + "&".join(f"login={urllib.parse.quote(c)}" for c in logins[i:i + BATCH]))
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=headers),
                                        timeout=25) as r:
                for u in json.load(r).get("data", []):
                    out[u["login"].lower()] = u
        except Exception as exc:  # Netzfehler darf die Einordnung nicht abbrechen
            print(f"  Twitch-Abruf Block {i // BATCH + 1} fehlgeschlagen: "
                  f"{type(exc).__name__}", file=sys.stderr)
    return out


def finde_wellen(monate: Counter) -> set[str]:
    if not monate:
        return set()
    werte = sorted(monate.values())
    median = werte[len(werte) // 2]
    return {m for m, n in monate.items() if n >= 8 and n >= 4.0 * max(median, 1)}


def finde_salven(msgs: list[tuple[datetime, str]]) -> set[str]:
    """Konten, die in einer Nachrichtenwelle am Ende der Session geschrieben haben.

    Nur das Schlussfenster zählt. Eine Welle mitten im Stream ist normaler
    lebhafter Chat — fünf Leute, die binnen 40 Sekunden auf dasselbe
    Spielereignis reagieren, sind kein Ring, und ohne diese Bedingung landete
    jeder von ihnen namentlich als belegter Bot im Bericht.

    Die belastbare Beobachtung ist die Abschieds-Salve: koordiniertes
    Verabschieden, wenn der Stream endet.
    """
    if not msgs:
        return set()
    ende = msgs[-1][0]
    in_salve: set[str] = set()
    for i, (ts, _) in enumerate(msgs):
        if ende - ts > SALVE_ENDFENSTER:
            continue
        fenster: list[tuple[datetime, str]] = []
        for m in msgs[i:]:
            if m[0] - ts > SALVE_FENSTER:
                break  # msgs ist zeitlich sortiert, alles Weitere liegt später
            fenster.append(m)
        if len({w for _, w in fenster}) >= SALVE_MIN_KONTEN:
            in_salve.update(w for _, w in fenster)
    return in_salve


def ist_stammgast(zeiten: list[datetime]) -> bool:
    if len(zeiten) < 2:
        return False
    tage = {t.date() for t in zeiten}
    return (len(tage) >= STAMMGAST_MIN_TAGE
            and max(zeiten) - min(zeiten) >= STAMMGAST_MIN_SPANNE)


def taktkonten(pro_session: dict[int, list[tuple[datetime, str]]], login: str) -> set[str]:
    """Konten, die in einer Session gemeinsam einen festen Takt bilden.

    Nutzt dieselbe Kern-Ring-Bestimmung wie tools/viewbot_detect.py, damit beide
    Werkzeuge denselben Ring benennen — einschließlich des Bot-Filters, den
    `scan_cadence` vor `core_ring` legt. Ohne ihn bilden nightbot und die
    kanaleigenen Bots den Takt und stehen als belegte Viewbots im Bericht.
    """
    import importlib.util
    pfad = Path(__file__).resolve().parent / "viewbot_detect.py"
    spec = importlib.util.spec_from_file_location("viewbot_detect", pfad)
    vd = importlib.util.module_from_spec(spec)
    # Muss vor exec_module stehen: die @dataclass darin schlägt sonst beim
    # Auflösen ihres eigenen Moduls fehl.
    sys.modules["viewbot_detect"] = vd
    spec.loader.exec_module(vd)

    treffer: set[str] = set()
    for msgs in pro_session.values():
        pool = [(ts, who, "") for ts, who in msgs if not vd.is_channel_bot(who, login)]
        if len(pool) < vd.CADENCE_MIN_MSGS:
            continue
        ring = vd.core_ring(pool)
        stats = vd._gap_stats(ring) if ring else None
        if not stats:
            continue
        mean, _, cv = stats
        wer = {m[1] for m in ring}
        if cv <= vd.CADENCE_MAX_CV and mean <= vd.CADENCE_MAX_MEAN \
                and len(wer) >= vd.CADENCE_MIN_ACCOUNTS:
            treffer |= wer
    return treffer


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("login")
    args = ap.parse_args()
    login = args.login.lstrip("#").lower()
    dsn, cid = os.environ.get("TWITCH_ANALYTICS_DSN"), os.environ.get("TWITCH_CLIENT_ID")
    secret = os.environ.get("TWITCH_CLIENT_SECRET")
    if not (dsn and cid and secret):
        sys.exit("Zugangsdaten fehlen — über scripts/run_with_infisical.sh starten.")

    with psycopg.connect(dsn) as conn, conn.cursor() as cur:
        cur.execute(
            "SELECT lower(chatter_login), count(DISTINCT session_id), "
            "       sum(COALESCE(messages, 0)) "
            "FROM twitch_session_chatters WHERE streamer_login ILIKE %s "
            "  AND chatter_login IS NOT NULL GROUP BY 1",
            (login,),
        )
        konten = {r[0]: Konto(login=r[0], sessions=r[1], msgs=r[2] or 0)
                  for r in cur.fetchall()}
        alle = sorted(konten)
        cur.execute(
            "SELECT lower(chatter_login), count(DISTINCT streamer_login) "
            "FROM twitch_session_chatters WHERE lower(chatter_login) = ANY(%s) GROUP BY 1",
            (alle,),
        )
        for name, n in cur.fetchall():
            konten[name].kanaele = n
        cur.execute(
            "SELECT DISTINCT lower(follower_login) FROM twitch_follow_events "
            "WHERE lower(follower_login) = ANY(%s)", (alle,))
        folger = {r[0] for r in cur.fetchall()}
        cur.execute(
            "SELECT message_ts, lower(chatter_login), session_id FROM twitch_chat_messages "
            "WHERE streamer_login ILIKE %s ORDER BY message_ts", (login,))
        nachrichten = cur.fetchall()

    users = fetch_users(alle, cid, app_token(cid, secret))
    monate = Counter(u["created_at"][:7] for u in users.values())
    wellen = finde_wellen(monate)

    pro_session: dict[int, list[tuple[datetime, str]]] = defaultdict(list)
    pro_konto: dict[str, list[datetime]] = defaultdict(list)
    for ts, who, sid in nachrichten:
        pro_session[sid].append((ts, who))
        pro_konto[who].append(ts)
    salve_konten: set[str] = set()
    for msgs in pro_session.values():
        salve_konten |= finde_salven(msgs)
    takt_konten = taktkonten(pro_session, login)

    for name, k in konten.items():
        u = users.get(name)
        if u:
            k.erstellt = datetime.fromisoformat(u["created_at"].replace("Z", "+00:00"))
            if u["created_at"][:7] in wellen:
                k.merkmal.add("welle")
        if k.kanaele <= 1:
            k.merkmal.add("kanalgebunden")
        if name not in folger:
            k.merkmal.add("nie_gefolgt")
        zeiten = sorted(pro_konto.get(name, []))
        if len(zeiten) == 1:
            k.merkmal.add("einmalnachricht")
        if name in takt_konten:
            k.beweis.add("taktnachricht")
        if name in salve_konten and len(zeiten) <= 2:
            k.beweis.add("salve")
        if k.kanaele > 1:
            k.entlastung.add("wandert")
        if name in folger:
            k.entlastung.add("folgt")
        if ist_stammgast(zeiten):
            k.entlastung.add("stammgast")

    gruppen = defaultdict(list)
    for k in konten.values():
        gruppen[k.urteil].append(k)

    print(f"{login}: {len(konten)} Konten, {len(users)} bei Twitch auffindbar, "
          f"{len(wellen)} Registrierungswelle(n)\n")
    grenzen = {"belegt": 60, "entlastet": 20, "gruppenverdacht": 12}
    for urteil in ("belegt", "entlastet", "gruppenverdacht", "unauffaellig"):
        g = sorted(gruppen[urteil], key=lambda k: (-len(k.beweis), -k.msgs, k.login))
        print(f"{urteil.upper()}: {len(g)}")
        if urteil == "unauffaellig":
            print("  (weniger als drei Merkmale — nicht weiter betrachtet)\n")
            continue
        limit = grenzen[urteil]
        for k in g[:limit]:
            teile = sorted(k.beweis) + sorted(k.merkmal)
            ent = ("  [" + ", ".join(sorted(k.entlastung)) + "]") if k.entlastung else ""
            datum = f"{k.erstellt:%Y-%m-%d}" if k.erstellt else "unbekannt "
            print(f"  {datum}  {k.login:<30} {k.msgs:>3} Msg  {k.kanaele:>2} Kan  "
                  f"{', '.join(teile)}{ent}")
        if len(g) > limit:
            print(f"  ... und {len(g) - limit} weitere")
        print()

    print("Lesart:")
    print("  belegt          — beobachtete Koordination mit anderen Konten (fester")
    print("                    Zeittakt oder reine Streamende-Welle). Das kann ein")
    print("                    Mensch nicht unabsichtlich erzeugen.")
    print("  gruppenverdacht — nur Kontomerkmale. Als Gruppe eindeutig auffällig,")
    print("                    für das einzelne Konto aber KEIN Beleg: ein stiller")
    print("                    Zuschauer sieht in diesen Daten genauso aus.")
    print("  entlastet       — ein menschliches Signal genügt, um ein Konto aus der")
    print("                    Bewertung zu nehmen.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
