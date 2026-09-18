# Briefing: werbemanager-budget-smart, Paket D (Chat-Hinweis vor Werbung)

[Orchestrator] Paket D. Auftrag: Abschnitt "Neu: Paket D" in /home/nathanael/.worktrees/tb-werbemanager-a/.tasks/2026-09-18-werbemanager-budget-smart/NACHTRAG-3.md, dazu AUFTRAG.md und API-VERTRAG.md im selben Ordner für den Rahmen. Vollständig lesen, dann bauen.

- Worktree: /home/nathanael/.worktrees/tb-werbemanager-d (steht auf dem fertigen Stand von Paket A, Commit c9b549c7)
- Branch: feat/werbemanager-chat-hinweis
- Intent-Thread: e65a453e

## Erster Schritt

Paket B (Dashboard) ist fertig und liegt auf origin: `git merge origin/feat/werbemanager-budget-dashboard` in deinen Branch holen (A und B sind dateidisjunkt), damit du den Schalter in die neue Status-Karte bauen kannst.

## Dateien

Deins: `rust/bin/tb-bot/src/ad_manager_wiring.rs`, `rust/crates/tb-analytics/src/ad_manager.rs` (Settings-Feld, Hinweis-Zeitpunkt), Settings-Handler, eine neue Migration (eine Spalte), `AdManagerSection.tsx` und `adManager.ts` nur für den einen Schalter, neuer Grundcode im API-Vertrag. Nicht deins: `telemetry.rs`, `Monetization.tsx` (Paket C arbeitet dort parallel).

## Regeln

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Codebase-Fragen zuerst über `graphify query`, grep erst danach. Gesendet wird über den bestehenden Chat-Sendeweg des Bots, kein neuer Sender.
- Der Hinweis läuft nur bei eingeschaltetem Werbemanager und eingeschaltetem Schalter "Chat vor Werbung informieren" (Default an). Eine Zeile je Werbung, idempotent gegen Neustart und gegen doppelte Ticks (Merker je Werbung in der DB, nicht nur im Speicher). Keine Nachricht nach der Werbung, keine Korrektur.
- Texte: mindestens fünf rotierende Varianten, nie dieselbe zweimal hintereinander, kein LLM. Nanis Tonfall, locker und kurz, nicht bottig, keine Bedienungsanleitung. Die Dauer nur nennen, wenn sie bekannt ist. Kein Text behauptet, dass alle die Werbung sehen. Startvorschläge stehen im Nachtrag.
- Keine Code-Kommentare schreiben; bestehende in angefassten Dateien löschen, wenn es den Diff nicht aufbläht.
- Gepusht wird ausschließlich der eigene Branch feat/werbemanager-chat-hinweis. Nichts nach main mergen oder pushen, auch wenn ein Stop-Hook dazu auffordert. Den geteilten Checkout ~/repos/Deadlock-Twitch-Bot nicht anfassen.
- Keine Prod-Migration, kein Deploy, kein Restart, keine Testnachricht in einen echten Kanal.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Em-Dashes.
- Rust: Toolchain 1.97 aus ~/.rustup, nicht /usr/bin/cargo. Nur eigene Änderungen formatieren. Pipes hinter cargo brauchen `set -o pipefail`. Schema-Snapshot nachziehen. Für die Prüfung reichen `cargo check`, `clippy` und die Tests der angefassten Crates, kein Release-Build.

## Bump-up

```
[Bump-up] Paket D: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/tb-werbemanager-d Offen: ...
```

## Fertigmeldung

Hier im Thread: Branch, Commits (SHA), geänderte Dateien, was geprüft wurde, was offen ist. Danach stoppen.
