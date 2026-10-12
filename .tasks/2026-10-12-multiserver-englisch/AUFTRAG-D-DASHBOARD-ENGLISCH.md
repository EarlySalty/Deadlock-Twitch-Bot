# Auftrag D: Streamer-Dashboard vollständig zweisprachig (Deutsch/Englisch)

Rolle: **Teil-Orchestrator** für den Bereich „Dashboard-Englisch“. Du darfst native Claude-Code-Subagenten für Recherche, Bau und Abnahme einsetzen, mit getrennten Schreibbereichen. Keine weiteren T3-Threads, keine weiteren Orchestrator-Ebenen.

Auftraggeber und Haupt-Orchestrator: Claude-Session `5f7811c4-86ac-4398-83e4-f929f2139a47`.
Gesamtplan: `.tasks/2026-10-12-multiserver-englisch/PLAN.md` auf Branch `docs/multiserver-plan-20261012` (Worktree `/home/nathanael/.worktrees/ttb-multiserver-plan`). Dieser Auftrag ist Paket D, Entscheidungen E13 und E15.

## Arbeitsort
- Worktree: `/home/nathanael/.worktrees/ttb-dashboard-englisch`
- Branch: `feat/dashboard-englisch-20261012` (Basis `origin/main` 0085b717f)
- Bei großen Mengen in mehrere aufeinanderfolgende kleine Branches/Merges schneiden (Diff je Merge höchstens etwa 150 KB). Jeder Schnitt einzeln Gate, Merge, Deploy, live.

## Ziel
Das Streamer-Dashboard `bot/dashboard_v2` ist vollständig auf Englisch nutzbar, die deutsche Ansicht bleibt unverändert.

1. **Sprachwahl:** Ohne gespeicherte Wahl gilt die Browsersprache (`navigator.languages`/`navigator.language`): beginnt mit `de` → Deutsch, sonst Englisch. Eine manuell gewählte Sprache (bestehender Umschalter, `localStorage` `dashboard.language`) hat Vorrang. Datum und Zahlen folgen der Sprache (`LOCALES`), die Zeitzone kommt weiter aus dem Browser.
2. **Alle Seiten und Komponenten** aus `bot/dashboard_v2/src`, die Nutzer sehen, laufen über das bestehende i18n (`i18n/dictionary.ts`, `useT()`/`translate`). Heute nutzen es nur 12 von 42 Seiten und 8 von 72 Komponenten. Bestandsliste der unübersetzten Seiten steht im PLAN, Abschnitt 3.1.
3. **Sichtbare Backend-Texte** für das Dashboard (`rust/crates/tb-dashboard-api`, Fehlermeldungen, Labels, Urteile, die im Dashboard erscheinen) kommen in der Sprache des Nutzers an. Vorzugsweise liefert die API stabile Codes und das Frontend übersetzt; wo das unverhältnismäßig ist, wertet das Backend die mitgesendete Sprache aus. Logs und Tracing bleiben deutsch (E13).
4. Das Wörterbuch darf in Bereichsdateien aufgeteilt werden, damit Teilpakete nicht auf derselben Datei kollidieren. Der Mechanismus „deutscher Text ist der Schlüssel, fehlende Übersetzung fällt auf Deutsch zurück“ bleibt.

## Grenzen
- Nicht anfassen: `bot/admin_dashboard` (bleibt deutsch), `website/` (eigenes Paket W), KI-Ausgaben selbst (Prompts bleiben).
- **Parallel laufen Social-AI-Autopilot-Threads im selben Repo.** Dateien unter `components/socialmedia/`, `pages/SocialMedia*.tsx` und Social-Media-Backendhandler nicht ändern; diese Seiten sind bereits übersetzt. Vor jedem Merge auf aktuelles `origin/main` rebasen.
- Englisch in natürlicher Sprache, kein Maschinenstil; deutsche Texte nicht verändern. Fachbegriffe von Twitch (Raid, VOD, Clip, Follower) wie auf Twitch-Englisch.
- Keine Code-Kommentare, keine neuen Bibliotheken, keine neuen KI-Aufrufe.
- Rust-Prüfungen nur über `cargo-slot` (`--jobs 3`), keine konkurrierenden Release-Builds (siehe `claude-config/machines/v50671/RUST-BUILDS.md`).
- Keine Secrets ausgeben.

## Beweisziel
- Prüfung, dass keine sichtbaren deutschen Reste bleiben: Skript oder Test, der alle JSX-Textknoten und sichtbaren String-Props der Dashboard-Seiten gegen das Wörterbuch prüft, plus der bestehende `tests/i18n.test.ts`.
- `npm run build`, Lint und bestehende Tests im Dashboard grün; Rust fmt, clippy und betroffene Tests grün.
- **Browser-Sichtprüfung mit Moli** (`/home/nathanael/.local/bin/moli`, vorher `claude-config/wissen/agent-browser.md` lesen; kein Brave): jede Seite in Englisch (Browsersprache `en-US`, kein gespeicherter Wert) und in Deutsch, Screenshots angesehen, keine abgeschnittenen oder überlaufenden Texte, besonders bei 320 px Breite.
- Gate: `gate_hook.py --review` mit Skill `rolle-merge-schleuse`. Bei BLOCK je Runde frischer Fixer als nativer Subagent, bis ALLOW; nach fünf erfolglosen Runden Blocker melden.
- Danach Merge nach `main`, Push, Deploy des Dashboards nach Repo-Weg (Memory `streamer-website-deploy-weg`, Skill `deploy-restart-selbstdienst`), Live-Prüfung auf `https://deutsche-deadlock-community.de/twitch/dashboard` in beiden Sprachen.

## Bericht
An den Haupt-Orchestrator höchstens 5 Zeilen je Schnitt: SHA, Gate-Urteil, live ja/nein mit Beweis, offen. Am Ende Worktree und Branches entfernen und den eigenen Thread mit `python3 ~/Documents/tools/t3-thread.py settle --selbst` abschließen.
