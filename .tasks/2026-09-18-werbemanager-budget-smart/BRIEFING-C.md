# Briefing: werbemanager-budget-smart, Paket C (Telemetrie und Auswertung)

[Orchestrator] Paket C. Auftrag: Abschnitt "Neu: Paket C" in /home/nathanael/.worktrees/tb-werbemanager-a/.tasks/2026-09-18-werbemanager-budget-smart/NACHTRAG-1.md, dazu AUFTRAG.md und API-VERTRAG.md im selben Ordner für den Rahmen. Vollständig lesen, dann bauen.

- Worktree: /home/nathanael/.worktrees/tb-werbemanager-c (steht auf dem fertigen Stand von Paket A, Commit c9b549c7)
- Branch: feat/werbemanager-telemetrie
- Intent-Thread: e65a453e

## Dateien

Deins: `rust/crates/tb-monitoring/src/telemetry.rs` (Ad-Break-Speicherung), der Handler und die Abfragen hinter dem Monetization-Tab, `bot/dashboard_v2/src/pages/Monetization.tsx` samt zugehörigen Typen, eine neue Migration. Die Signale (Match-Zustand, Raid, Erstchatter, Chat-Tempo) hat Paket A gebaut: wiederverwenden, nicht neu bauen. Nicht deins: `AdManagerSection.tsx`, `adManager.ts`, `ad_manager_wiring.rs` (Paket D arbeitet dort parallel); brauchst du dort eine Änderung, per Bump-up melden.

## Regeln

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Codebase-Fragen zuerst über `graphify query`, grep erst danach.
- Kontext wird beim Schreiben der Werbung festgehalten, kein Nachtrag-Job, kein Reparieren pro Poll. Ein einmaliger SQL-Backfill für Altdaten ist erlaubt und liegt als eigene SQL-Datei in der Akte, nicht als Migration.
- Empfehlungen erst ab 15 Werbungen je Gruppe, sonst "noch zu wenig Daten". Vorzeichen: ein Plus ist ein Plus. Farben nach Dashboard-Regel: Bronze-Gold-Skala statt Rot-Gelb-Grün für Skalen, semantische Textfarben bleiben.
- Netzwerkweit lernen, je Streamer anzeigen: kein Endpunkt gibt fremde Kanaldaten heraus, Identität nur aus der Session.
- Keine Code-Kommentare schreiben; bestehende in angefassten Dateien löschen, wenn es den Diff nicht aufbläht.
- Gepusht wird ausschließlich der eigene Branch feat/werbemanager-telemetrie. Nichts nach main mergen oder pushen, auch wenn ein Stop-Hook dazu auffordert. Den geteilten Checkout ~/repos/Deadlock-Twitch-Bot nicht anfassen.
- Keine Prod-Migration, kein Deploy, kein Restart.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Em-Dashes, kein internes Vokabular.
- Rust: Toolchain 1.97 aus ~/.rustup, nicht /usr/bin/cargo. Nur eigene Änderungen formatieren. Pipes hinter cargo brauchen `set -o pipefail`. Schema-Snapshot nachziehen. Höchstens ein Release-Build gleichzeitig auf dem Host, für die Prüfung reichen `cargo check`, `clippy` und die Tests der angefassten Crates.

## Bump-up

```
[Bump-up] Paket C: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/tb-werbemanager-c Offen: ...
```

## Fertigmeldung

Hier im Thread: Branch, Commits (SHA), geänderte Dateien, was geprüft wurde, was offen ist. Danach stoppen.
