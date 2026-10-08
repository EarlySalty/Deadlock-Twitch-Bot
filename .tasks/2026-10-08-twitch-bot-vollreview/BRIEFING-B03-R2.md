# B03: zweite Fixrunde

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min als Orientierung, keine Abbruchfrist für Kompilierung | Worktree: /home/nathanael/.worktrees/tb-vollreview-audit-akteur

## Auftrag und Eigentum

Grundauftrag und bestätigte Freigabekette: BRIEFING-B03.md im selben Taskverzeichnis. Frischer Sol-Fixer nach abgeschlossenem Gate und Kritiker der ersten Runde. Einziger Schreibpfad bleibt `rust/crates/tb-dashboard-api/src/admin_audit.rs`. Die Auth-Dateien und lib.rs gehören anderen aktiven Paketen. Zusätzlichen Schreibbedarf mit Begründung zurückgeben, diese Dateien nicht parallel ändern. Keine neue Bewertung eines fremden Befunds, sondern vollständige Erfüllung des bereits bestätigten B03-Vertrags.

Erhaltener Branch `fix/vollreview-audit-akteur`, lokaler Head `1ce4fae5cf1e91bb2d7aa42beaf6724d3c3e4a5b`, damalige Basis `51c8a674a371d0e623687940e6b8ca3492f96c92`. Eigenen Zustand vor Änderung prüfen, aktuelles origin/main holen und ausschließlich eigene Commits abgleichen. Fremden Hauptcheckout unverändert lassen. Der erste Fixer und der erste Kritiker sind beendet.

## Zwei belegte Mängel

1. Die neue Auswahl erkennt lokal gespiegelte gültige Cookies, aber nicht die tatsächliche Auswahl über den zentralen Broker. `admin_audit.rs:118-120` bestimmt den Akteur vor dem nachgelagerten Sitzungsimport. Mit abgelaufenem ersten Cookie und zentral gültigem zweiten Cookie ohne lokalen Spiegel bleibt der Akteur `admin`. Ist das erste Cookie zentral für Discord-ID 99 gültig und das zweite lokal für ID 42, erfasst der neue Pfad `discord:42`, obwohl die Authentifizierung ID 99 auswählt. Der Akteur muss die Sitzung abbilden, die die erfolgreiche Aktion tatsächlich legitimiert. Vorhandene Schutzketten verwenden; keine neue konkurrierende Auswahlregel, keine Änderung von Berechtigungen oder erlaubten Routen.
2. Der neue gewöhnliche Tokio-Test ruft an `admin_audit.rs:206-208` bedingungslos expect auf, obwohl pool_or_skip bei fehlender optionaler TB_TEST_DATABASE_URL None zurückgibt. Der bestehende Test überspringt diesen Fall. Diesen bisherigen Opt-in-Vertrag erhalten; konfigurierte Testläufe nicht durch abgeschwächte Assertions als erfolgreich ausgeben.

Fundstellen am ersten Fixstand: `auth/level.rs:408-474,872-917` belegen zentrale Validierung, Import und gewählte Sitzungsmarkierung. `lib.rs:1287-1298,2245-2248` belegt Middleware-Reihenfolge. `auth/csrf.rs:55-67,130-144` verwendet die gewählte Sitzung. `docs/HOST_ROUTING_CONTRACT.md` erlaubt direkte Admin-API-Zugriffe ohne vorausgesetzte lokale Spiegelung. Diese Dateien nur lesen. Regressionen brauchen den zentralen Auswahl-/Importfall, einschließlich verschiedener IDs, und den bisherigen Lauf ohne optionale Testdatenbank. Keine echten Konten oder externen Anbieteraufrufe.

## Ergebnis der ersten Runde

- Fixer `a1db7f98212fff43f`: 137 echte Sol-Datensätze, Transcript-SHA256 `88aed074134da8635c373f6787d62c4e6795a4eb7cc613ddaee3c330706c5677`.
- Kritiker `a10e0a2727e07f614`: BLOCK, 40 echte Sol-Datensätze, SHA256 `5657dbf1da800047c1fa930e427280e8c5b7b6d7279e097e2a122510a0654ea7`.
- Astra hat die Modelle und Hashes der fertigen Transcripts unter Workflow `wf_586b3f73-0dc` geprüft. Vollständige Rückgaben stehen in dessen journal.jsonl.
- Gate Runde 1: BLOCK wegen des neuen bedingungslosen Test-expect, Exit 1. Log `/tmp/b03-sol-gate.log`.
- Gemeldete Pakettests: Baseline 1333 bestanden und 35 fehlgeschlagen; Fix 1334 bestanden und dieselben 35 fehlgeschlagen, jeweils Exit 101. Zwei gezielte Audit-Tests bestanden, Exit 0. Keine unabhängige Ausführung durch den Kritiker.
- Eigene Datei formatiert; Paketformatierung hatte 265 identische Bestandsabweichungen. Clippy scheiterte vor und nach Änderung an derselben fremden tb-chat-Diagnose. Keine Nebenfixes.

## Prüfungen und Grenzen

Vor Codesuche code-suche/Graphify. Toolchain `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, `/home/nathanael/.local/bin/cargo-slot`, Kompilierung mit `--jobs 1`. Bestehende Crate-Tests, Paket-fmt und Clippy durchführen, fremde Fehler gegen die passende unveränderte Basis vergleichen. Testdatenbank wie im Grundbriefing. Bisheriger Baseline-Worktree `/home/nathanael/.worktrees/tb-vollreview-audit-akteur-baseline` gehört diesem Paket; vorhandenen Zustand zuerst prüfen. Kein Dateialter als Artefaktnachweis, keine fremden Prozesse beenden. Fehlende Prüfungen ausdrücklich melden.

Nur eigener minimaler Rust-Diff, keine Kommentare, Umbauten, neue Konfiguration, Abhängigkeiten oder Migrationen. Keine Produktionsdatenbank, Secrets/ENV-Dateien, Browser, Kontenaktionen oder ai-coach. Kein ListAgents/SendMessage, keine weiteren Agenten oder Threads, keine Nutzerfragen. Astra pflegt die Taskdokumentation; strukturierte Rückgabe genügt.

Ein Git-Schritt je Bash-Aufruf mit literalen absoluten Pfaden. Eigener Commit erlaubt; kein Merge oder Push nach main, kein Release, Deploy oder Restart. Abschließend denselben Sol-only-Gate aus BRIEFING-B03.md auf frischer Basis ausführen. Bei BLOCK abgeben, keine dritte Runde im eigenen Kontext. Danach prüft ein neuer Sol-Kritiker den festen Gesamtdiff, beide Mängel und mögliche Regressionen.

Rückgabe: Basis und Head, tatsächliche Schreibpfade, Sollabdeckung, Prüfungen mit Exit-Codes, Baselinevergleich, Gate und Belege, eigener Worktreezustand und verbleibende Blocker.
