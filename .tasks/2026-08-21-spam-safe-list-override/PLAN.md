status: aktiv
datum: 2026-08-21
klasse: mittel
research: .tasks/2026-08-21-spam-safe-list-override/RESEARCH.md

## Ziel

Die AI-Entscheidung bleibt der automatische Lern-/Aktionspfad. Ein Discord-Klick
korrigiert ausschließlich die Gegenrichtung: Spam-Nachlernen bei AI-Harmlos oder
Safe-Listen-Lernen plus Spam-Muster-Entfernung bei AI-Spam.

## Milestones

### M1 — Rote Vertragstests (erledigt)

Tests für exakten Safe-Volltextmatch, Safe-DB-Laden, transaktionale API-Korrektur
und invertierte Discord-Buttons zuerst rot ergänzen.

### M2 — Safe-Filter vor Spam-Judge (erledigt)

Safe-Patterns laden, exakt matchen und den Pfad in `run_spam_check` vor der ersten
Score-/Mention-/Judge-Arbeit mit einem `SAFE_PATTERN`-Review abbrechen.

### M3 — Korrektur-API (erledigt)

Die Korrektur der AI-Spam-Row liest `source_message`, schreibt daraus ein manuelles
Safe-Pattern, auditert `clean` und entfernt die falsche Spam-Row atomar.

### M4 — Discord-Vertrag (erledigt)

Bei `safe` wird `Als Spam korrigieren` erzeugt. Bei `spam` wird die bestehende
Row-ID als `Als harmlos korrigieren` verwendet. Erfolgstexte und interne Doku
werden entsprechend angepasst.

### M5 — Prüfung (erledigt)

Gezielte Rust-Tests, Paket-Tests, Clippy mit `-D warnings`, Formatter und
`git diff --check`; danach Wirkungsprüfung gegen beide Repositories.

## Nicht-Ziele

- Kein Safe-Lernen aus einem AI-Harmlosurteil allein.
- Kein negativer Safe-Score und kein Substring-Match.
- Kein neues Modell und kein direkter Gemini-Aufruf im Buttonpfad.
