# Contract: Uplink-Warteliste im Admin-Modus

status: erledigt
datum: 2026-08-25
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Ein Admin kann auf der Uplink-Seite wartende Streamer sehen und direkt zur Teilnahme freischalten.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Bei aktivem Admin-Modus erscheint rechts im Uplink-Hauptbereich eine eigene Wartelistenbox.
- REQ-02: Die Box zeigt wartende Streamer mit Name oder Twitch-ID und dem Zeitpunkt ihrer Anfrage.
- REQ-03: Ein Klick auf `Freischalten` aktiviert den gewählten Streamer und entfernt ihn nach bestätigtem Erfolg aus der Warteliste.
- REQ-04: Ladezustand, leere Warteliste und Fehler sind in der Box eindeutig erkennbar.
- REQ-05: Ohne aktiven Admin-Modus ist die Box nicht sichtbar und die neuen Server-Endpunkte antworten mit 403.
- REQ-06: Der private Schlüsselhinweis in OBS-Schritt 2 ist als eigenständige Warnbox sichtbar.

## Invarianten (darf sich nicht ändern)

- INV-01: Das Relay-Secret bleibt serverseitig und gelangt nicht in das Frontend.
- INV-02: Der vom Relay erzeugte Ingest-Schlüssel wird bei einer Freischaltung nicht an den Browser zurückgegeben.
- INV-03: Die bestehende Nutzer-Warteliste und ihre Route bleiben unverändert.
- INV-04: Bestehende Tests werden nicht gelöscht oder abgeschwächt.

## Nicht-Ziele

- Wartelisteneinträge ablehnen oder löschen.
- Relay-Schema, Datenbankmigrationen oder Teilnehmer-Limits ändern.
- Einen zweiten Admin-Modus oder eine eigene Admin-Anmeldung bauen.

## Erlaubter Änderungsbereich

- bot/dashboard_v2/src/pages/Uplink.tsx
- bot/dashboard_v2/src/pages/Uplink.layout.test.tsx
- bot/dashboard_v2/src/api/uplink.ts
- bot/dashboard_v2/src/preview/fixtures.ts
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs
- rust/crates/tb-dashboard-api/src/lib.rs
- .tasks/2026-08-25-uplink-admin-waitlist/

## Verbotene Änderungen

- rs-relay Quellcode und Migrationen
- Authentifizierungs- und Admin-Modus-Verträge
- Lint- oder Testkonfiguration

## Offene Produktfragen

- keine

## Amendments
