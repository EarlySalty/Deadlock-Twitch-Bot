#!/usr/bin/env python3
"""Holt Erstellungsdatum und Profilzustand der Chat-Konten eines Kanals.

Ein eingekaufter Konten-Pool stammt meist aus einer Registrierungswelle: die
Konten wurden am selben Tag oder in derselben Woche angelegt, tragen kein
Profilbild und keine Beschreibung. Diese Merkmale liegen nicht in unserer DB,
sondern nur in der Twitch-API — deshalb dieses eigene Werkzeug.

Aufruf:
    ./scripts/run_with_infisical.sh .venv/bin/python tools/chatter_account_age.py <login>
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from datetime import datetime, timezone

import psycopg

HELIX = "https://api.twitch.tv/helix/users"
BATCH = 100

# Ein Kanal sammelt sein Publikum über Jahre ein; die Registrierungsmonate seiner
# Zuschauer verteilen sich entsprechend breit. Ein eingekaufter Pool stammt aus
# einer einzigen Registrierungswelle und drängt sich in wenigen Monaten.
KOHORTE_MIN_KONTEN = 8
KOHORTE_MIN_FAKTOR = 4.0       # Vielfaches des mittleren Monatsanteils

# Echte Zuschauer einer Szene tauchen auch bei benachbarten Kanälen auf. Ein Pool,
# der für einen Kanal gekauft wurde, kennt nur diesen einen.
WANDER_VERDACHT_MAX = 0.10     # Anteil der Kohorte, der bei weiteren Kanälen auftaucht


def app_token(client_id: str, client_secret: str) -> str:
    body = urllib.parse.urlencode({
        "client_id": client_id, "client_secret": client_secret,
        "grant_type": "client_credentials",
    }).encode()
    req = urllib.request.Request("https://id.twitch.tv/oauth2/token", data=body)
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.load(r)["access_token"]


def fetch_users(logins: list[str], client_id: str, token: str) -> dict[str, dict]:
    out: dict[str, dict] = {}
    headers = {"Client-Id": client_id, "Authorization": f"Bearer {token}"}
    for i in range(0, len(logins), BATCH):
        chunk = logins[i:i + BATCH]
        url = HELIX + "?" + "&".join(f"login={urllib.parse.quote(c)}" for c in chunk)
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=headers),
                                        timeout=25) as r:
                for u in json.load(r).get("data", []):
                    out[u["login"].lower()] = u
        except urllib.error.HTTPError as exc:
            print(f"  API-Fehler bei Block {i // BATCH + 1}: HTTP {exc.code}", file=sys.stderr)
    return out


def finde_kohorten(monate: Counter) -> list[tuple[str, int, float]]:
    """Registrierungsmonate, die weit über dem Schnitt des Kanals liegen.

    Verglichen wird gegen den Median der belegten Monate, nicht gegen den
    Mittelwert: sonst hebt die Welle selbst die Latte, gegen die sie geprüft wird.
    """
    if not monate:
        return []
    werte = sorted(monate.values())
    median = werte[len(werte) // 2]
    gesamt = sum(monate.values())
    treffer = []
    for m, n in monate.items():
        if n >= KOHORTE_MIN_KONTEN and n >= KOHORTE_MIN_FAKTOR * max(median, 1):
            treffer.append((m, n, n / gesamt))
    return sorted(treffer)


def wanderquote(conn: psycopg.Connection, logins: list[str], kanal: str) -> tuple[int, int]:
    """Wie viele dieser Konten tauchen auch bei anderen Kanälen auf?"""
    if not logins:
        return 0, 0
    with conn.cursor() as cur:
        cur.execute(
            """
            SELECT count(*) FILTER (WHERE n > 1), count(*)
            FROM (SELECT lower(chatter_login) l, count(DISTINCT streamer_login) n
                  FROM twitch_session_chatters
                  WHERE lower(chatter_login) = ANY(%s) GROUP BY 1) x
            """,
            (logins,),
        )
        return cur.fetchone()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("login")
    ap.add_argument("--min-sessions", type=int, default=1)
    args = ap.parse_args()
    login = args.login.lstrip("#").lower()

    dsn = os.environ.get("TWITCH_ANALYTICS_DSN")
    cid = os.environ.get("TWITCH_CLIENT_ID")
    secret = os.environ.get("TWITCH_CLIENT_SECRET")
    if not (dsn and cid and secret):
        sys.exit("DSN oder Twitch-Zugangsdaten fehlen — über run_with_infisical.sh starten.")

    with psycopg.connect(dsn) as conn, conn.cursor() as cur:
        cur.execute(
            """
            SELECT chatter_login, count(DISTINCT session_id), sum(COALESCE(messages, 0)),
                   min(COALESCE(first_message_at, last_seen_at))::date
            FROM twitch_session_chatters WHERE streamer_login ILIKE %s
            GROUP BY 1 HAVING count(DISTINCT session_id) >= %s
            """,
            (login, args.min_sessions),
        )
        local = {r[0].lower(): {"sessions": r[1], "msgs": r[2] or 0, "erstmals": r[3]}
                 for r in cur.fetchall() if r[0]}

    logins = sorted(local)
    print(f"{login}: {len(logins)} Konten aus der Aufzeichnung, frage Twitch ab ...")
    users = fetch_users(logins, cid, app_token(cid, secret))
    fehlend = [c for c in logins if c not in users]
    print(f"  {len(users)} beantwortet, {len(fehlend)} nicht auffindbar "
          f"(gelöscht, gesperrt oder anonym)")
    if fehlend:
        print(f"  nicht auffindbar: {', '.join(fehlend[:20])}"
              + (" ..." if len(fehlend) > 20 else ""))

    rows = []
    for c, u in users.items():
        created = datetime.fromisoformat(u["created_at"].replace("Z", "+00:00"))
        rows.append({
            "login": c,
            "erstellt": created,
            "alter_tage": (datetime.now(timezone.utc) - created).days,
            "bild": bool(u.get("profile_image_url")
                         and "user-default-pictures" not in u["profile_image_url"]),
            "text": bool((u.get("description") or "").strip()),
            **local.get(c, {}),
        })
    rows.sort(key=lambda r: r["erstellt"])

    monate = Counter(r["erstellt"].strftime("%Y-%m") for r in rows)
    kohorten = finde_kohorten(monate)
    kohorten_monate = {m for m, _, _ in kohorten}
    print(f"\nRegistrierungsmonate ({len(rows)} Konten):")
    for m, n in sorted(monate.items()):
        bar = "#" * min(n, 60)
        print(f"  {m}  {n:3d}  {bar}" + ("  <<< Welle" if m in kohorten_monate else ""))

    if kohorten:
        anteil = sum(n for _, n, _ in kohorten) / len(rows)
        print(f"\nRegistrierungswellen: {len(kohorten)} Monat(e), zusammen "
              f"{sum(n for _, n, _ in kohorten)} Konten ({anteil:.0%} des Kanals)")
        in_welle = sorted(r["login"] for r in rows
                          if r["erstellt"].strftime("%Y-%m") in kohorten_monate)
        uebrige = sorted(r["login"] for r in rows
                         if r["erstellt"].strftime("%Y-%m") not in kohorten_monate)
        with psycopg.connect(dsn) as conn:
            w_mehr, w_ges = wanderquote(conn, in_welle, login)
            r_mehr, r_ges = wanderquote(conn, uebrige, login)
        w_q = w_mehr / max(w_ges, 1)
        r_q = r_mehr / max(r_ges, 1)
        print(f"  Konten der Welle bei mehr als einem Kanal: {w_mehr}/{w_ges} ({w_q:.0%})")
        print(f"  übrige Konten des Kanals:                  {r_mehr}/{r_ges} ({r_q:.0%})")
        if w_q <= WANDER_VERDACHT_MAX and r_q > w_q * 2:
            print("  → Die Welle bleibt an diesem Kanal hängen, während das übrige\n"
                  "    Publikum durch die Szene wandert. Das spricht für einen Pool,\n"
                  "    der für genau diesen Kanal eingesetzt wird.")
        else:
            print("  → Die Welle wandert wie das übrige Publikum. Kein Hinweis auf\n"
                  "    einen kanalgebundenen Pool.")
    else:
        print("\nKeine Registrierungswelle: die Konten verteilen sich über die Monate.")

    tage = Counter(r["erstellt"].strftime("%Y-%m-%d") for r in rows)
    haeufig = [(d, n) for d, n in tage.items() if n >= 3]
    if haeufig:
        print("\nKonten mit identischem Registrierungstag (ab drei):")
        for d, n in sorted(haeufig):
            wer = sorted(r["login"] for r in rows if r["erstellt"].strftime("%Y-%m-%d") == d)
            print(f"  {d}  {n:3d}  {', '.join(wer)}")
    else:
        print("\nKein Registrierungstag mit drei oder mehr Konten.")

    ohne = [r for r in rows if not r["bild"] and not r["text"]]
    print(f"\nOhne Profilbild und ohne Beschreibung: {len(ohne)} von {len(rows)} "
          f"({100 * len(ohne) / max(len(rows), 1):.0f} %)")

    print("\nÄlteste zehn:")
    for r in rows[:10]:
        print(f"  {r['erstellt']:%Y-%m-%d}  {r['login']:<32} "
              f"{r['sessions']:>2} Sessions, {r['msgs']:>3} Nachrichten")
    print("Jüngste zehn:")
    for r in rows[-10:]:
        print(f"  {r['erstellt']:%Y-%m-%d}  {r['login']:<32} "
              f"{r['sessions']:>2} Sessions, {r['msgs']:>3} Nachrichten")
    return 0


if __name__ == "__main__":
    sys.exit(main())
