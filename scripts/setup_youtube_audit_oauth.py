"""Einmaliges OAuth-Setup fuer den separaten YouTube-Audit-Account (Device-Flow).

Voraussetzung: Google-Cloud-Projekt mit aktivierter "YouTube Data API v3" und
einem OAuth-Client vom Typ "TVs und Eingabebeschraenkte Geraete" (Device-Flow).

Ablauf: Skript zeigt eine google.com/device-URL + Code, du bestaetigst im
Browser mit dem Audit-Account. Der Refresh-Token landet ausschliesslich in
~/.config/deadlock-twitch-bot/youtube_audit_oauth.json (0600) - nichts davon
wird ausgegeben. Alternativ koennen die Werte als YOUTUBE_AUDIT_CLIENT_ID/
-SECRET/-REFRESH_TOKEN in Infisical gepflegt werden (Env hat Vorrang).
"""

from __future__ import annotations

import getpass
import json
import sys
import time
from pathlib import Path

import requests

DEVICE_CODE_URL = "https://oauth2.googleapis.com/device/code"
TOKEN_URL = "https://oauth2.googleapis.com/token"
SCOPES = (
    "https://www.googleapis.com/auth/youtube.upload "
    "https://www.googleapis.com/auth/youtube.force-ssl"
)
TARGET = Path.home() / ".config" / "deadlock-twitch-bot" / "youtube_audit_oauth.json"


def main() -> int:
    print("YouTube-Audit-Account einrichten (Device-Flow)")
    print(f"Token-Ziel: {TARGET}")
    client_id = input("OAuth Client-ID: ").strip()
    client_secret = getpass.getpass("OAuth Client-Secret (Eingabe unsichtbar): ").strip()
    if not client_id or not client_secret:
        print("Abbruch: Client-ID und -Secret sind Pflicht", file=sys.stderr)
        return 2

    response = requests.post(
        DEVICE_CODE_URL,
        data={"client_id": client_id, "scope": SCOPES},
        timeout=30,
    )
    if response.status_code != 200:
        print(
            f"Device-Code-Anfrage fehlgeschlagen (HTTP {response.status_code}): "
            f"{response.text[:300]}",
            file=sys.stderr,
        )
        return 2
    device = response.json()
    print()
    print(f"1. Im Browser oeffnen: {device['verification_url']}")
    print(f"2. Mit dem AUDIT-Account anmelden und diesen Code eingeben: {device['user_code']}")
    print()
    print("Warte auf Bestaetigung ...")

    interval = float(device.get("interval") or 5)
    deadline = time.monotonic() + float(device.get("expires_in") or 1800)
    while time.monotonic() < deadline:
        time.sleep(interval)
        token_response = requests.post(
            TOKEN_URL,
            data={
                "client_id": client_id,
                "client_secret": client_secret,
                "device_code": device["device_code"],
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            },
            timeout=30,
        )
        payload = token_response.json()
        error = str(payload.get("error") or "")
        if token_response.status_code == 200:
            refresh_token = str(payload.get("refresh_token") or "")
            if not refresh_token:
                print("Fehler: Google lieferte keinen refresh_token", file=sys.stderr)
                return 2
            TARGET.parent.mkdir(parents=True, exist_ok=True)
            TARGET.write_text(
                json.dumps(
                    {
                        "client_id": client_id,
                        "client_secret": client_secret,
                        "refresh_token": refresh_token,
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )
            TARGET.chmod(0o600)
            print(f"OK - Credentials gespeichert: {TARGET}")
            _print_channel_name(client_id, client_secret, refresh_token)
            return 0
        if error == "authorization_pending":
            continue
        if error == "slow_down":
            interval += 2
            continue
        print(f"OAuth fehlgeschlagen: {error or token_response.text[:200]}", file=sys.stderr)
        return 2
    print("Abbruch: Bestaetigung nicht rechtzeitig erfolgt", file=sys.stderr)
    return 2


def _print_channel_name(client_id: str, client_secret: str, refresh_token: str) -> None:
    """Kurzer Funktionstest: Kanalname des verbundenen Accounts (kein Secret)."""
    try:
        token = requests.post(
            TOKEN_URL,
            data={
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
                "grant_type": "refresh_token",
            },
            timeout=30,
        ).json()["access_token"]
        channels = requests.get(
            "https://www.googleapis.com/youtube/v3/channels",
            params={"part": "snippet", "mine": "true"},
            headers={"Authorization": f"Bearer {token}"},
            timeout=30,
        ).json()
        title = channels["items"][0]["snippet"]["title"]
        print(f"Verbundener YouTube-Kanal: {title}")
    except Exception:  # noqa: BLE001 - reiner Komfort-Check
        print("Hinweis: Kanal-Check uebersprungen (Upload-Test folgt im Betrieb)")


if __name__ == "__main__":
    raise SystemExit(main())
