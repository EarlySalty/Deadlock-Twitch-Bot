status: erledigt
datum: 2026-08-21
klasse: mittel
research: .tasks/2026-08-21-spam-safe-feedback/RESEARCH.md

## Ziel

Fertig, wenn ein `AI: harmlos`-Alert den Button `Als harmlos bestätigen` zeigt, der Klick als `clean`-Feedback im bestehenden Review-Log landet, der Discord-Button danach deaktiviert wird und die bestehende Spam-Korrektur für echte Spam-Urteile weiter funktioniert.

## Nicht-Ziele

- Keine Safe-Liste, kein negatives Scoring und keine automatische Umklassifizierung zukünftiger Nachrichten.
- Keine neue externe Route, kein neues Modell und kein direkter Gemini-Aufruf im Klickpfad.
- Keine Änderungen an bestehenden Mod-/Ban-Rechten oder an alten bereits gesendeten Buttons.

## Milestones

### M1 — Vertragstests für Safe-Feedback

Änderungen: `tb-internal-api`-Handler-Tests, `dl-changelog`-Button-Tests, `dl-bridges`-custom_id-/Klicktests.

Erwarteter Zwischenzustand: Die neuen Tests sind zunächst rot, weil `safe`-Payload, API und Action noch fehlen.

Validierung: `tb-internal-api` 5 Tests, `dl-changelog` 3 Tests und der gezielte
`dl-bridges`-Klicktest grün.

Stop-Regel: Bei einer nicht reproduzierbaren Testumgebung keine Produktivdatei ändern.

### M2 — Twitch-Internal-API und Alert-Payload

Änderungen: Safe-Feedback als `clean`-Zeile in `twitch_spam_review_decisions` speichern; `safe_pattern` nur bei Safe-Urteilen im Payload senden.

Erwarteter Zwischenzustand: Neue Safe-Feedback-Requests persistieren nachvollziehbar, ohne den aktiven Spam-Filter zu verändern.

Validierung: Handler-Unit-/DB-Tests grün; `rustfmt --check` auf den eigenen Rust-Dateien grün.

Stop-Regel: Kein Safe-Pattern-Insert und kein direkter LLM-Aufruf.

### M3 — Discord-Button und Ergebniszustand

Änderungen: `dl-changelog` zeigt bei `verdict=safe` `Als harmlos bestätigen`; `dl-bridges` validiert und sendet den Klick, prüft Rechte und deaktiviert die Zeile nach Erfolg.

Erwarteter Zwischenzustand: Der Screenshot-Fall bekommt den passenden Button; nach Klick bleibt nur ein deaktivierter Erfolgsstatus.

Validierung: gezielte `dl-changelog`-/`dl-bridges`-Tests und Rust-Formatter grün.

Stop-Regel: Bei fehlender Rechteprüfung oder Erfolg ohne Message-Update nicht weitergehen.

### M4 — Gesamtprüfung

Änderungen: keine neuen Features; Vertrag-Doku aktualisieren und Artefakt auf `erledigt` setzen.

Erwarteter Zwischenzustand: beide Repos kompilieren/linten im betroffenen Scope, Tests zählen den echten Lauf.

Validierung: betroffene Pakettests, `clippy -D warnings`, `rustfmt --check` und
`git diff --check` grün; anschließend Wirkungsreview gegen Diff und Cross-Repo-Vertrag.

Stop-Regel: Jede rote Validierung wird vor Commit behoben.

## Verlauf

- 2026-08-21: Research und Plan erstellt; Bestand bestätigt.
- 2026-08-21: Safe-Feedback-API, Payload, Discord-Button und Vertrag-Doku umgesetzt.
- 2026-08-21: Betroffene Tests und Clippy grün; Status auf `erledigt` gesetzt.
