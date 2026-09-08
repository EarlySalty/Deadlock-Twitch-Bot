# Contract: Coaching-Audit nimmt nach Releasewechsel zuverlässig auf

status: aktiv
datum: 2026-09-08
klasse: mittel
repo: Deadlock-Twitch-Bot

## Ziel

Der laufende Audit-Dienst darf nach einem Releasewechsel nicht auf einen
schreibgeschützten Datenpfad wechseln. Eine Startmeldung bestätigt tatsächlich
aufgenommenes Material. Vorübergehende Aufnahmefehler heilen durch Wiederanlauf.

## Anforderungen

- REQ-01 Audit-Daten liegen unter einem stabilen StateDirectory-Pfad, unabhängig vom current-Symlink. Die systemd-Härtung bleibt erhalten.
- REQ-02 Die Start-DM wird erst nach einem erfolgreich aufgenommenen Block ausgelöst, nicht beim Start des Tasks. Schreibfehler verhindern eine ungesicherte Erfolgsmeldung.
- REQ-03 Zugestellte Startmeldungen werden atomar dauerhaft markiert. Fehlgeschlagene Zustellung bleibt wiederholbar mit demselben Idempotenzschlüssel. Es werden keine verspäteten Startmeldungen aus der allgemeinen Hinweiswarteschlange erzeugt.
- REQ-04 Der durchgehende Mitschnitt versucht nach vorübergehenden Fehlern auch nach fünf Anläufen mit begrenzter Frequenz erneut zu starten. Meldungen behaupten weder einen endgültigen Ausfall noch funktionierende Blockauswertung ohne Nachweis.
- REQ-05 Verzeichnisfehler beim Mitschnitt werden mit ihrer tatsächlichen Ursache protokolliert; die allgemeine Block-Ausfallmeldung behauptet nicht pauschal einen streamlink-Defekt.
- REQ-06 Rust-Regressionstests, gemeinsamer unabhängiger Review und Selbst-Gate prüfen den Fix. Der Dienst wird nach Integration neu gestartet und an echter Aufnahme geprüft.

## Invarianten

- INV-01 Keine Modellwechsel, keine neuen Secrets, keine neuen ENV-Konfigurationswerte.
- INV-02 Aufnahmegrenzen, Archivierung, Kanalwahl und bestehende Autorisierung bleiben erhalten.

## Erlaubter Änderungsbereich

- rust/bin/tb-stream-audit/src/main.rs
- ops/systemd/audit.conf
- ops/systemd/deadlock-twitch-stream-coaching-watch.service
- rust/scripts/run_stream_audit_service.sh
- docs/architecture/stream-coaching-audit.md
- .tasks/2026-09-08-coaching-audit-aufnahme/

## Amendments

- 2026-09-08 A1: docs/architecture/stream-coaching-audit.md gehört zusätzlich zum erlaubten Änderungsbereich, um den produktiven stabilen Datenpfad zu dokumentieren.
