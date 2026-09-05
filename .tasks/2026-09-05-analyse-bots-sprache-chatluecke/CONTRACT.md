# Contract: Analyse-Backend: Bot-Ausschluss, Sessionsprache, Chat-Lücken-Warnung

status: überholt
datum: 2026-09-05
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Bot-Konten und anonyme Twitch-Logins fallen aus allen Zuschauer-Auswertungen heraus, die Primärsprache eines Kanals ist bekannt statt "Unbekannt", und die Warnung zu fehlenden Chat-Nachrichten erscheint nur bei echten Lücken und in Nutzersprache.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: `own3d`, `kofistreambot` und jeder Login nach dem Muster `justinfan` plus Ziffern erscheinen in keiner Zuschauer-Auswertung mehr: Top Lurker, Lurker-Analyse, Viewer-Typen, Audience Demographics, Einzel-Viewer-Verzeichnis, Chat-Kennzahlen (Chat Penetration, Loyalitätsscore, Unique Viewer), Follower-Funnel-Stufen. Beleg: der Aufruf für `earlysalty` über 30 Tage listet die drei Logins nirgends mehr.
- REQ-02: Es gibt genau eine Bot-Liste und eine Anonym-Regel im Code (eine Funktion, ein Modul); `lurker_analysis.rs`, `viewer_exclusion.rs`, `chatter_tracking.rs` und `irc_lurker.rs` beziehen sie von dort. Die vier heutigen `KNOWN_CHAT_BOTS`-Kopien verschwinden.
- REQ-03: Die Primärsprache in Audience Demographics wird nur aus Sessions mit gesetzter Sprache berechnet; "Unbekannt" erscheint erst, wenn keine einzige Session im Zeitraum eine Sprache trägt. Für `earlysalty` über 30 Tage steht danach "Deutsch".
- REQ-04: Neue Sessions bekommen ihre Sprache beim Anlegen aus den Helix-Stream-Daten (Ursache der 1029 leeren `language`-Werte ist im Session-Schreiber belegt und behoben). Bestehende Sessions mit leerer Sprache werden per einmaligem SQL-Update aus `twitch_channel_updates` (jüngster Wert je `twitch_user_id`) nachgefüllt; das Update liegt als Datei `backfill.sql` im Task-Ordner und wird vom Orchestrator als `postgres` ausgeführt.
- REQ-05: Die Chat-Lücken-Warnung (`suspectedIngestionIssue`) zählt nur Sessions als Lücke, die Chat enthalten konnten: Dauer mindestens 10 Minuten und `unique_chatters > 0`. Geister-Sessions aus EventSub-Flaps (Dauer unter 10 Minuten oder ohne Chatter) lösen sie nicht mehr aus. Beleg: Regressionstest, der mit zwei kurzen chatlosen Sessions plus einer echten Session mit Chat vor dem Fix `true` liefert und danach `false`; roter Lauf mit Testname und Fehlermeldung in `PLAN.md` festgehalten.
- REQ-06: Die `note`-Texte in `raw_chat_status.rs` sind Nutzersprache ohne Roh-Chat, KPI, Presence, Rollup, Ingestion, Insert; z. B. "Für einige Streams in diesem Zeitraum liegen keine Chat-Nachrichten vor."
- REQ-07: `cargo test -p tb-dashboard-api -p tb-analytics -p tb-chat -p tb-monitoring` ist gegen die bekannte Baseline grün (Baseline vor dem ersten Edit messen und in PLAN.md notieren).

## Invarianten (darf sich nicht ändern)

- INV-01: Keine ENV-Variablen, keine neuen Secrets; bestehende `TWITCH_*`-Env-Auswertung in `viewer_exclusion.rs` bleibt unverändert.
- INV-02: Kein Schema-Wechsel, keine Migration; REQ-04 ist reines Daten-Update.
- INV-03: API-Feldnamen und Routen bleiben gleich (`suspectedIngestionIssue`, `primaryLanguage`, `peakHoursMethod` bleiben im JSON).
- INV-04: Bestehende Tests werden nicht gelöscht oder abgeschwächt; der Implementierer ändert den Regressionstest aus REQ-05 nach dem roten Lauf nicht mehr.
- INV-05: Keine Code-Kommentare; bestehende Kommentare in angefassten Dateien werden nicht erweitert.
- INV-06: Python-Legacy bleibt unangetastet; produktive Logik nur in Rust.

## Nicht-Ziele

- Frontend-Änderungen (eigener Contract `.tasks/2026-09-05-analyse-optik-zeitraum/`).
- Automatische Bot-Erkennung per Heuristik; nur die feste Liste plus Anonym-Muster.
- Sprache aus Chat-Nachrichten ableiten.

## Erlaubter Änderungsbereich

- rust/crates/tb-analytics/src/**
- rust/crates/tb-analytics/tests/**
- rust/crates/tb-dashboard-api/src/handlers/viewer_exclusion.rs
- rust/crates/tb-dashboard-api/src/handlers/lurker_analysis.rs
- rust/crates/tb-dashboard-api/src/handlers/audience_demographics.rs
- rust/crates/tb-dashboard-api/src/handlers/audience.rs
- rust/crates/tb-dashboard-api/src/handlers/viewers.rs
- rust/crates/tb-dashboard-api/src/handlers/chat_analytics.rs
- rust/crates/tb-dashboard-api/src/handlers/follower_funnel.rs
- rust/crates/tb-dashboard-api/Cargo.toml
- rust/crates/tb-chat/src/chatter_tracking.rs
- rust/crates/tb-chat/Cargo.toml
- rust/crates/tb-monitoring/src/**
- rust/crates/tb-monitoring/tests/**
- rust/crates/tb-monitoring/Cargo.toml
- rust/Cargo.lock
- .tasks/2026-09-05-analyse-bots-sprache-chatluecke/

## Verbotene Änderungen

- bot/** (Frontends und Python)
- rust/crates/tb-analytics/migrations/**, rust/migrations/**
- rust/scripts/**, systemd-Units, Caddy

## Offene Produktfragen

- keine

## Amendments

