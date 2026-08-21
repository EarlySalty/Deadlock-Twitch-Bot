# Plan: Twitch-Meldegrund in der Audit-DM

status: überholt
datum: 2026-08-21
klasse: kritisch
research: .tasks/2026-08-21-twitch-report-copy/RESEARCH.md
ersetzt durch: .tasks/2026-08-21-twitch-report-baukasten/PLAN.md

## Ziel

Fertig, wenn eine interne Audit-DM jeden Twitch-relevanten Fund in einer kopierbaren Struktur mit `Gesagt`, absoluter ungefähre Uhrzeit, Stream-Zeitfenster und `Kopierfertiger Twitch-Meldegrund` zeigt. Der Meldesatz stammt aus dem konfigurierten DeepSeek-V4-Flash-Pfad, liegt für Zustellwiederholungen im Bericht und wird bei fehlender Modellantwort sichtbar als Fallback markiert. Eine automatische Twitch-Meldung bleibt ausgeschlossen.

## Nicht-Ziele

- Keine automatische Meldung, Sanktion oder Interaktion mit dem gemeldeten Twitch-Konto.
- Kein dauerhafter Rohwortlaut in JSON, Markdown oder an einem entfernten LLM.
- Keine neue Dashboard-Route und kein neues globales Feature-Flag.

## Milestones

### M1: Zeitbasis und Datenvertrag

Status: erledigt.

Änderungen: `plan.rs`, `main.rs`, `lib.rs`, Testdaten.

Erwarteter Zwischenzustand: `started_at` und ein Fallback-Aufnahmebeginn werden über Zettel, Block und Bericht transportiert; alte Berichte bleiben deserialisierbar.

Validierung: `cargo test --manifest-path /home/nathanael/Documents/Deadlock-Twitch-Bot/rust/Cargo.toml -p tb-stream-audit --lib`

Stop-Regel: Bei einem Fehler im Rückwärtskompatibilitätstest keine Report- oder LLM-Änderung beginnen.

### M2: LLM-Aufbereitung und DM-Ausgabe

Status: erledigt.

Änderungen: `llm.rs`, `main.rs`, `report.rs`, `lib.rs`.

Erwarteter Zwischenzustand: Ein meldewürdiger Fund erhält aus geschwärzten Fakten einen persistierten Satz. Die DM zeigt Originalzitat, absolute ungefähre Uhrzeit, Stream-Zeitfenster und den Satz unter einer eigenen Überschrift.

Validierung: gezielte Unit-Tests für Promptvertrag, Modellantwort, Fallback, Zeitformat und `dm_text`; danach `cargo fmt --check` und der Repo-Verifizierer.

Stop-Regel: Bei fehlendem LLM-Satz darf die DM nicht wie eine erfolgreiche Aufbereitung aussehen.

### M3: echte DeepSeek-Gegenprobe und Live-Beweis

Status: erledigt.

Änderungen: keine weiteren fachlichen Änderungen nach grüner Validierung.

Erwarteter Zwischenzustand: Der konfigurierte Fireworks-Endpunkt beantwortet eine synthetische, geschwärzte Aufbereitungsanfrage strukturell gültig; danach läuft der Dienst mit der neuen Binärdatei.

Validierung: API-Gegenprobe ohne Secret-Ausgabe, Release-Build mit `-j 2`, Neustart des Audit-Dienstes, PID-/Binary-/Journal-Beweis und ein sauberer Dienstanlauf.

Stop-Regel: Bei HTTP-, Parse- oder Zustellfehlern keinen Erfolg behaupten; der Bericht bleibt liegen und der Fehler wird mit Kontext geloggt.

## Verlauf

- 2026-08-21: Research und Plan angelegt, Bestand teilweise vorhanden.
- 2026-08-21: Zeitanker, rückwärtskompatible Felder, LLM-Aufbereitung und DM-Format umgesetzt.
- 2026-08-21: 98 Bibliothekstests und 16 Binärtests bestanden; Clippy für den gesamten Workspace bestanden.
- 2026-08-21: DeepSeek-V4-Flash-Gegenprobe mit HTTP 200 und gültigem JSON bestanden.
- 2026-08-21: Release-Binary gebaut und Audit-Dienst mit PID-Wechsel neu gestartet.
- 2026-08-21: Die vollständige Workspace-Suite blieb an sieben unabhängigen tb-chat-Tests rot, davon sechs wegen fehlender Test-DB und einer wegen eines bereits geänderten Chat-Katalogs.
