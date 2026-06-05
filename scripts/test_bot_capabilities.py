#!/usr/bin/env python3
"""Empirischer Test: hat der Bot-Token die Scopes für den Blacklist-Raid-Guard,
und kann er einen Whisper zustellen?

Holt mit dem Bot-Refresh-Token EIN frisches Access-Token (dieselbe Refresh-
Operation, die der Bot im Betrieb stündlich selbst ausführt) und liest daraus
die aktuellen Scopes. Ist `user:manage:whispers` vorhanden und ein Ziel-Login
übergeben, wird testweise ein echter Whisper an dieses Konto geschickt.

Gibt NUR Scope-Namen + HTTP-Status + Twitch-Fehlermeldungen aus, niemals einen
Token. Läuft über scripts/run_with_infisical.sh (Secrets aus der Umgebung):

    scripts/run_with_infisical.sh /usr/bin/python3 \\
        scripts/test_bot_capabilities.py earlysalty
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

NEED = ("channel:moderate", "user:manage:whispers")
TOKEN_URL = "https://id.twitch.tv/oauth2/token"
HELIX = "https://api.twitch.tv/helix"


def _refresh(refresh_token: str, client_id: str, client_secret: str) -> dict:
    body = urllib.parse.urlencode(
        {
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": client_id,
            "client_secret": client_secret,
        }
    ).encode()
    req = urllib.request.Request(TOKEN_URL, data=body, method="POST")
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read().decode("utf-8"))


def _helix_get(path: str, params: dict, token: str, client_id: str) -> dict:
    url = f"{HELIX}{path}"
    if params:
        url += "?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(
        url, headers={"Authorization": f"Bearer {token}", "Client-Id": client_id}
    )
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read().decode("utf-8"))


def main() -> int:
    refresh = (os.getenv("TWITCH_BOT_REFRESH_TOKEN") or "").replace("oauth:", "").strip()
    client_id = (os.getenv("TWITCH_BOT_CLIENT_ID") or os.getenv("TWITCH_CLIENT_ID") or "").strip()
    client_secret = (
        os.getenv("TWITCH_BOT_CLIENT_SECRET") or os.getenv("TWITCH_CLIENT_SECRET") or ""
    ).strip()
    if not (refresh and client_id and client_secret):
        miss = [
            n
            for n, v in (
                ("REFRESH_TOKEN", refresh),
                ("CLIENT_ID", client_id),
                ("CLIENT_SECRET", client_secret),
            )
            if not v
        ]
        print(f"FEHLER: fehlende Env-Variablen: {', '.join(miss)}")
        return 2

    # 1) Frisches Access-Token holen + Scopes lesen (normale Refresh-Operation)
    try:
        data = _refresh(refresh, client_id, client_secret)
    except urllib.error.HTTPError as exc:
        print(f"FEHLER: Token-Refresh fehlgeschlagen (HTTP {exc.code}).")
        return 1
    except Exception as exc:  # noqa: BLE001
        print(f"FEHLER: Token-Refresh-Request fehlgeschlagen: {type(exc).__name__}")
        return 1

    access = str(data.get("access_token") or "")
    scopes = sorted(str(s) for s in (data.get("scope") or []))
    print(f"Scopes im Bot-Token ({len(scopes)}):")
    for need in NEED:
        print(f"  {need:<26} {'vorhanden' if need in scopes else 'FEHLT'}")
    print("  ---")
    print("  " + ", ".join(scopes))

    has_whisper = "user:manage:whispers" in scopes
    has_moderate = "channel:moderate" in scopes
    missing = [s for s in NEED if s not in scopes]
    if missing:
        print(f"\n=> Re-Auth nötig (fehlt: {', '.join(missing)}).")

    # 2) Optionaler Whisper-Test (nur wenn Scope da + Ziel angegeben)
    target = (sys.argv[1] if len(sys.argv) > 1 else "").strip().lower()
    if target and has_whisper and access:
        try:
            me = _helix_get("/users", {}, access, client_id)
            bot_id = (me.get("data") or [{}])[0].get("id", "")
            tgt = _helix_get("/users", {"login": target}, access, client_id)
            tgt_id = (tgt.get("data") or [{}])[0].get("id", "")
        except Exception as exc:  # noqa: BLE001
            print(f"\nWhisper-Test: User-Auflösung fehlgeschlagen ({type(exc).__name__}).")
            bot_id = tgt_id = ""
        if not bot_id or not tgt_id:
            print(f"\nWhisper-Test: Bot- oder Ziel-ID ({target}) nicht auflösbar.")
        else:
            url = f"{HELIX}/whispers?" + urllib.parse.urlencode(
                {"from_user_id": bot_id, "to_user_id": tgt_id}
            )
            req = urllib.request.Request(
                url,
                method="POST",
                data=json.dumps(
                    {"message": "Test vom Deadlock-Bot: Blacklist-Raid-Guard Whisper-Check, bitte ignorieren."}
                ).encode(),
                headers={
                    "Authorization": f"Bearer {access}",
                    "Client-Id": client_id,
                    "Content-Type": "application/json",
                },
            )
            try:
                with urllib.request.urlopen(req, timeout=15) as resp:
                    print(
                        f"\nWhisper-Test an {target}: HTTP {resp.status} "
                        "(204 = angenommen; kann still verworfen sein – prüf deine Whispers)."
                    )
            except urllib.error.HTTPError as exc:
                hint = ""
                try:
                    hint = (json.loads(exc.read().decode()).get("message") or "")
                except Exception:  # noqa: BLE001
                    pass
                print(f"\nWhisper-Test an {target}: HTTP {exc.code} – {hint}")
    elif target and not has_whisper:
        print(f"\nWhisper-Test übersprungen: Scope user:manage:whispers fehlt.")

    if not missing:
        print("\n=> Beide Scopes vorhanden – Raid-Erkennung (channel.moderate) "
              f"und Whisper-Versand sind tokenseitig möglich. (moderate={has_moderate}, whisper={has_whisper})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
