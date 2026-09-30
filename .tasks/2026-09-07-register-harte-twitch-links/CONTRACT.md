# Contract: Zuschauer-Register nimmt harte Twitch-Verknüpfungen aus dem Discord auf

status: aktiv
datum: 2026-09-07
klasse: medium
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Gegenstück in Deadlock-Bots:
`Deadlock-Bots/.tasks/2026-09-07-twitch-connection-discord-oauth/CONTRACT.md`
(liefert den Broker-Endpunkt `GET /internal/master/v1/discord/twitch-links`).

## Ziel

Das Zuschauer-Register (`rust/crates/tb-chat/src/zuschauer_register.rs`, live seit Release 9c128ed6) erkennt Zuschauer, die ihr Twitch-Konto im Discord verknüpft haben, sicher als Community-Mitglieder (Wahrscheinlichkeit 1,0) statt über Namensähnlichkeit. Bestandsaufnahme: Abschnitte 4 und 5 in `Deadlock-Bots/.tasks/2026-09-07-twitch-connection-discord-oauth/EVIDENCE.md`.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Datenweg: `tb-transport-discord` bekommt neben `list_members()` eine Funktion, die `GET /internal/master/v1/discord/twitch-links` vom Broker liest (Felder `discord_id`, `twitch_user_id`, `twitch_login`, `verified`, `updated_at`). Fehler oder ein fehlender Endpunkt (404, weil Deadlock-Bots noch nicht deployt ist) führen zu einer leeren Liste, nie zum Abbruch des Mitgliederindex.
- REQ-02 Signal: Der Mitgliederindex des Registers lädt die Twitch-Verknüpfungen im selben Fetch und hält sie als Map Twitch-User-ID zu Discord-ID. `score` behandelt einen Treffer dort wie den harten Treffer aus `twitch_streamer_identities` (Score 1,0, `discord_user_id` gesetzt, Signal-Stufe `hart_verknuepfung`). Nur Einträge mit `verified = true` zählen.
- REQ-03 Aktualisierung: Ein bestehender Registereintrag wird beim nächsten `ensure_current` neu gerechnet, sobald für seine Twitch-User-ID eine Verknüpfung vorliegt, auch wenn er jünger als 7 Tage ist. Das Backfill-Bin rechnet Verknüpfungen ebenfalls ein, damit ein einmaliger Lauf nach dem Deploy den Bestand korrigiert.
- REQ-04 Sichtbarkeit: Die Review-Karte des gezielten Pitches und die Reject-Gründe bleiben unverändert; das Signal-JSON im Register zeigt die Stufe `hart_verknuepfung`.

## Invarianten (darf sich nicht ändern)

- INV-01: Gate-Semantik, Schwellen, Priors, Limits und Fail-closed-Verhalten aus dem Contract `2026-09-06-zuschauer-register` bleiben unverändert.
- INV-02: Keine Helix-Aufrufe im Nachrichtenpfad, keine ENV-Config, kein neues Secret; Broker-Zugriff wie bei `list_members()`.
- INV-03: Bestehende Tests nicht löschen oder abschwächen; neue Tests für den harten Treffer, den Vorrang vor Namenssignalen, das Neurechnen unter 7 Tagen und die leere Liste bei Broker-404.
- INV-04: Migrationen nicht nötig; falls doch, nur additiv mit Zeitstempel ab 20260907100000.

## Nicht-Ziele

- Keine Änderung an Pitch-Pfaden, Limits oder FAQ.
- Kein Dashboard-Anteil.

## Erlaubter Änderungsbereich

- rust/crates/tb-transport-discord/src/relay.rs
- rust/crates/tb-transport-discord/src/lib.rs
- rust/crates/tb-chat/src/zuschauer_register.rs
- rust/crates/tb-chat/tests/
- rust/bin/tb-bot/src/zuschauer_register_backfill.rs
- rust/bin/tb-bot/src/chat_wiring.rs
- rust/.sqlx/
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt
- rust/migrations/
- .tasks/2026-09-07-register-harte-twitch-links/

## Verbotene Änderungen

- rust/crates/tb-chat/src/promos.rs
- rust/crates/tb-chat/src/pipeline.rs
- rust/knowledge/
- bestehende Migrationen
- Lint- und CI-Konfiguration

## Offene Produktfragen

- keine

## Amendments
