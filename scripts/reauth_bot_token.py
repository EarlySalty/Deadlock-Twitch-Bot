#!/usr/bin/env python3
"""Re-Auth des zentralen Bot-Tokens mit erweitertem Scope-Satz.

Führt den Twitch Authorization-Code-Flow aus und schreibt die neuen Tokens
zurück nach Infisical:

  1. Baut die Authorize-URL (du öffnest sie im Browser, eingeloggt als der
     BOT-Account, und klickst "Authorize").
  2. Fängt den Redirect-Code auf http://localhost:3000 ab.
  3. Tauscht den Code gegen Access- + Refresh-Token (inkl. Scope-Liste).
  4. Validiert, dass das Token wirklich dem Bot-Account gehört.
  5. Schreibt TWITCH_BOT_TOKEN + TWITCH_BOT_REFRESH_TOKEN nach Infisical.

Es werden NUR Login-/Scope-Namen + HTTP-Status ausgegeben, niemals ein Token.

Aufruf (lädt Client-Creds + Infisical-Config in die Env):

    scripts/run_with_infisical.sh /usr/bin/python3 scripts/reauth_bot_token.py

Voraussetzungen:
  - http://localhost:3000 ist als OAuth-Redirect-URI in der Twitch-App des Bots
    registriert (die Twitch-CLI nutzt dieselbe URL – war bei euch schon so).
  - Ein Browser AUF DEM SERVER (z. B. via RDP), eingeloggt als der Bot-Account.
  - INFISICAL_SERVICE_TOKEN mit Schreibrecht – ODER ein separater
    INFISICAL_WRITE_TOKEN (read+write), der nur zum Zurückschreiben genutzt
    wird, während das Standing-Token read-only bleibt.
"""

from __future__ import annotations

import http.server
import json
import os
import secrets
import sys
import threading
import urllib.error
import urllib.parse
import urllib.request

REDIRECT_URI = "http://localhost:3000"
AUTHORIZE_URL = "https://id.twitch.tv/oauth2/authorize"
TOKEN_URL = "https://id.twitch.tv/oauth2/token"
VALIDATE_URL = "https://id.twitch.tv/oauth2/validate"

# Erwarteter Bot-Login – Schutz davor, versehentlich ein fremdes Konto zu minten.
EXPECTED_LOGIN = (os.getenv("BOT_LOGIN") or "deutschedeadlockcommunity").strip().lower()

# Voller Scope-Satz: bisherige 30 (aus docs/BOT_TOKEN_SCOPES.md) + 3 neue.
SCOPES = [
    "moderator:manage:announcements",
    "moderator:manage:automod",
    "moderator:read:automod_settings",
    "moderator:manage:automod_settings",
    "moderator:read:banned_users",
    "moderator:manage:banned_users",
    "moderator:read:blocked_terms",
    "moderator:manage:blocked_terms",
    "moderator:read:chat_messages",
    "moderator:manage:chat_messages",
    "moderator:read:chat_settings",
    "moderator:manage:chat_settings",
    "moderator:read:chatters",
    "moderator:read:followers",
    "moderator:read:guest_star",
    "moderator:manage:guest_star",
    "moderator:read:moderators",
    "moderator:read:shield_mode",
    "moderator:manage:shield_mode",
    "moderator:read:shoutouts",
    "moderator:manage:shoutouts",
    "moderator:read:suspicious_users",
    "moderator:manage:suspicious_users",
    "moderator:read:unban_requests",
    "moderator:manage:unban_requests",
    "moderator:read:vips",
    "moderator:read:warnings",
    "moderator:manage:warnings",
    "user:bot",
    "user:read:chat",
    "user:write:chat",
    # NEU für den Blacklist-Raid-Guard:
    "channel:moderate",
    "user:manage:whispers",
]

_result: dict[str, str] = {}
_done = threading.Event()


class _CallbackHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802
        parsed = urllib.parse.urlparse(self.path)
        params = urllib.parse.parse_qs(parsed.query)
        _result["code"] = (params.get("code") or [""])[0]
        _result["state"] = (params.get("state") or [""])[0]
        _result["error"] = (params.get("error") or [""])[0]
        _result["error_description"] = (params.get("error_description") or [""])[0]
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.end_headers()
        msg = "Fehler – siehe Terminal." if _result.get("error") else "Fertig. Du kannst diesen Tab schliessen."
        self.wfile.write(f"<html><body><h2>{msg}</h2></body></html>".encode("utf-8"))
        _done.set()

    def log_message(self, *args):  # Stille HTTP-Logs.
        return


def _post_form(url: str, data: dict) -> tuple[int, dict]:
    body = urllib.parse.urlencode(data).encode()
    req = urllib.request.Request(url, data=body, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            return resp.status, json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        try:
            return exc.code, json.loads(exc.read().decode("utf-8"))
        except Exception:  # noqa: BLE001
            return exc.code, {}


def _validate(token: str) -> dict:
    req = urllib.request.Request(VALIDATE_URL, headers={"Authorization": f"OAuth {token}"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read().decode("utf-8"))


def _infisical_set(name: str, value: str) -> tuple[int, str]:
    base = (os.getenv("INFISICAL_API_URL") or "").rstrip("/")
    project = os.getenv("INFISICAL_PROJECT_ID") or ""
    env = os.getenv("INFISICAL_ENV") or ""
    # Bevorzugt einen separaten read+write Token; sonst das Standing-Token.
    service_token = (os.getenv("INFISICAL_WRITE_TOKEN") or os.getenv("INFISICAL_SERVICE_TOKEN") or "")
    path = (os.getenv("INFISICAL_SECRET_PATH") or "/").strip() or "/"
    payload = json.dumps(
        {"workspaceId": project, "environment": env, "secretPath": path, "secretValue": value}
    ).encode()
    url = f"{base}/api/v3/secrets/raw/{urllib.parse.quote(name)}"
    headers = {
        "Authorization": f"Bearer {service_token}",
        "Content-Type": "application/json",
        "Accept": "application/json",
    }
    # Erst PATCH (Update). Existiert das Secret nicht -> POST (Create).
    for method in ("PATCH", "POST"):
        req = urllib.request.Request(url, data=payload, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=20) as resp:
                return resp.status, method
        except urllib.error.HTTPError as exc:
            if method == "PATCH" and exc.code in (404,):
                continue  # Secret existiert nicht -> mit POST anlegen.
            return exc.code, method
        except Exception as exc:  # noqa: BLE001
            return -1, f"{method}:{type(exc).__name__}"
    return -1, "exhausted"


def main() -> int:
    client_id = (os.getenv("TWITCH_BOT_CLIENT_ID") or os.getenv("TWITCH_CLIENT_ID") or "").strip()
    client_secret = (
        os.getenv("TWITCH_BOT_CLIENT_SECRET") or os.getenv("TWITCH_CLIENT_SECRET") or ""
    ).strip()
    if not client_id or not client_secret:
        print("FEHLER: TWITCH_BOT_CLIENT_ID / TWITCH_BOT_CLIENT_SECRET fehlen in der Env.")
        return 2
    for key in ("INFISICAL_API_URL", "INFISICAL_PROJECT_ID", "INFISICAL_ENV", "INFISICAL_SERVICE_TOKEN"):
        if not (os.getenv(key) or "").strip():
            print(f"FEHLER: {key} fehlt in der Env (Infisical-Schreiben nicht möglich).")
            return 2

    state = secrets.token_urlsafe(16)
    authorize = AUTHORIZE_URL + "?" + urllib.parse.urlencode(
        {
            "response_type": "code",
            "client_id": client_id,
            "redirect_uri": REDIRECT_URI,
            "scope": " ".join(SCOPES),
            "state": state,
            "force_verify": "true",
        }
    )

    server = http.server.HTTPServer(("127.0.0.1", 3000), _CallbackHandler)
    threading.Thread(target=server.serve_forever, daemon=True).start()

    print("=" * 70)
    print("1) Stelle sicher, dass der Browser als BOT-Account eingeloggt ist:")
    print(f"   erwartet: {EXPECTED_LOGIN}")
    print("2) Öffne diese URL im Browser AUF DEM SERVER und klicke 'Authorize':\n")
    print(authorize)
    print("\nWarte auf den Redirect (max. 5 Min) ...")
    print("=" * 70)

    if not _done.wait(timeout=300):
        print("FEHLER: Timeout – kein Redirect erhalten.")
        return 1
    server.shutdown()

    if _result.get("error"):
        print(f"FEHLER bei der Autorisierung: {_result['error']} – {_result.get('error_description')}")
        return 1
    if _result.get("state") != state:
        print("FEHLER: State stimmt nicht (CSRF-Schutz). Abbruch.")
        return 1
    code = _result.get("code") or ""
    if not code:
        print("FEHLER: Kein Authorization-Code im Redirect.")
        return 1

    status, data = _post_form(
        TOKEN_URL,
        {
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
            "grant_type": "authorization_code",
            "redirect_uri": REDIRECT_URI,
        },
    )
    if status != 200 or not data.get("access_token"):
        print(f"FEHLER: Token-Tausch fehlgeschlagen (HTTP {status}): {data.get('message', '')}")
        return 1

    access = data["access_token"]
    refresh = data.get("refresh_token") or ""
    granted = sorted(str(s) for s in (data.get("scope") or []))

    # Sicherheitscheck: gehört das Token wirklich dem Bot-Account?
    try:
        info = _validate(access)
    except Exception as exc:  # noqa: BLE001
        print(f"FEHLER: Validate des neuen Tokens fehlgeschlagen ({type(exc).__name__}). Schreibe NICHTS.")
        return 1
    login = str(info.get("login") or "").strip().lower()
    print(f"\nNeues Token für Login: {login}")
    print(f"Scopes ({len(granted)}): {', '.join(granted)}")
    for need in ("user:bot", "channel:moderate", "user:manage:whispers"):
        print(f"  {need:<26} {'vorhanden' if need in granted else 'FEHLT'}")

    if login != EXPECTED_LOGIN:
        print(f"\nABBRUCH: Login '{login}' != erwartet '{EXPECTED_LOGIN}'. "
              "Falscher Account im Browser? Es wird NICHTS nach Infisical geschrieben.")
        return 1
    if not refresh:
        print("\nABBRUCH: Kein Refresh-Token erhalten. Es wird NICHTS geschrieben.")
        return 1

    # Nach Infisical schreiben.
    write_src = (
        "INFISICAL_WRITE_TOKEN (Override)"
        if (os.getenv("INFISICAL_WRITE_TOKEN") or "").strip()
        else "INFISICAL_SERVICE_TOKEN"
    )
    print(f"\nSchreibe nach Infisical mit: {write_src}")
    s1, m1 = _infisical_set("TWITCH_BOT_TOKEN", access)
    s2, m2 = _infisical_set("TWITCH_BOT_REFRESH_TOKEN", refresh)
    ok1 = s1 in (200, 201)
    ok2 = s2 in (200, 201)
    print(f"\nInfisical TWITCH_BOT_TOKEN        : HTTP {s1} ({m1}) {'OK' if ok1 else 'FEHLER'}")
    print(f"Infisical TWITCH_BOT_REFRESH_TOKEN: HTTP {s2} ({m2}) {'OK' if ok2 else 'FEHLER'}")

    if ok1 and ok2:
        print("\n=> Erfolg. Jetzt die Services neu starten:")
        print("   systemctl --user restart deadlock-twitch-bot.service deadlock-twitch-dashboard.service")
        return 0
    print("\n=> Schreiben fehlgeschlagen (evtl. Service-Token ohne Schreibrecht). "
          "Tokens NICHT gespeichert – bitte Infisical-Schreibrecht prüfen.")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
