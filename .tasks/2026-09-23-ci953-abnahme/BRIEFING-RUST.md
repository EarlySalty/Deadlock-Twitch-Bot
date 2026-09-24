# PR #953: Rust-Fortsetzung

Auftraggeber ist der laufende ChatGPT-Auftrag über codex-mcp, kein T3-Intent-Thread. Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.

## Arbeitsort und Grenzen

Worktree: `/home/nathanael/.worktrees/tb-deterministic-pr-gate-20260922`
Branch: `ci/deterministic-pr-gate-20260922`
Übernommener HEAD: `eb1ee5e9c529e6a65e1fc1fe47c5567dfe28717e`
PR: https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/953

Lies die geltenden Regeln einschließlich PR-FIRST-TESTBETRIEB. PR-Testbetrieb: kein Merge, main-Push, Auto-Merge, Deploy, Dienst-Neustart, Cleanup, Hook-Bypass oder Produktionsdatenbankzugriff. Keine anderen Repositories verändern. Keine Geheimnisse laden. Testdatenbank nur nach Prüfung der vorhandenen isolierten CI-Testinstanz. Builds mit höchstens zwei Jobs. Keine globalen Cargo-Konfigurationsänderungen, keine Löschung vorhandener Build-Artefakte.

Du besitzt schreibend ausschließlich die Rust-Quelldateien unter `rust/` und deinen Bericht `RUST-NACHWEIS.md` in diesem Task-Verzeichnis. Keine Workflow-, Frontend-, Policy-, Security-CI- oder allgemeinen Statusdateien ändern. Keine Git-Commits, Git-Pushes oder Änderungen am Index in diesem geteilten Arbeitsauftrag: Der Orchestrator übernimmt verifizierte Änderungen mit einzelnen Git-Schritten und sofortigem Push. Er darf währenddessen geprüfte Frontend-Dateien committen. Melde deshalb deinen geprüften Stand auch als Dateiliste und Diff, nicht als vermeintlich unveränderlichen HEAD.

Im Worktree liegen übernommene, noch uncommittete Vorarbeiten genau dieses CI-Auftrags in `promos.rs`, `zuschauer_register.rs`, `handlers/community/matching.rs`, `handlers/self_explainer.rs`, `outreach_shadow.rs` sowie ein neuer SQLx-Cacheeintrag. Nichts zurücksetzen oder durch alte Dateiversionen ersetzen. Prüfe den vorhandenen Diff; Änderungen außerhalb deines Paketbesitzes bleiben unberührt. Beim Start wurden außer den lesenden codex-mcp-Kommandos keine aktiven Prozesse in diesem Worktree gefunden.

## Aufgabe

1. Den aktuellen vollständigen Clippy-Lauf unter Rust 1.98.0 reproduzieren und verbleibende Ursachen ohne zusätzliche Lint-Ausnahmen beheben. Ausgangslogs: `.ci-resume-clippy-fixed.log`, `.ci-resume-workspace.log`; frühere Fehler können durch den uncommitteten Diff bereits korrigiert sein. Die echte CI verwendet `cargo clippy --workspace --all-targets --locked -- -D warnings`.
2. Workspace-/DB-/Integrationstests tatsächlich ausführen, soweit das isolierte echte Schema bereitsteht. Vorhandene Fixture-/Migrationspfade verwenden. Keine produktiven Migrationen ändern, keine HTTP-Verträge zum Grünmachen abschwächen. Bestehende frisch angelegte CI-Testdatenbank steht in `.ci-test-db-port`; Port und Instanz zuerst prüfen. Fremde Dienste/Container unverändert lassen. DB-Prozessausgaben müssen redigiert beziehungsweise stumm sein.
3. Rustfmt-Ursache untersuchen. Der globale Gate bleibt `cargo fmt --all --check`. Kein riesiger sachfremder Formatierungsdiff, keine breite Ausnahme und kein Abschwächen auf changed files. Gezielt angefasste Dateien eng formatieren. Falls eine echte vorhandene Formatkonfiguration die Ursache ist, Beleg mit Optionsvergleich liefern; keine Konfiguration bloß zur Fehlerunterdrückung erfinden. Bis zur Entscheidung nur den nachweislich minimalen Patch machen.
4. Neu auftauchende echte Testfehler beheben, sofern sie innerhalb Rust dieses Auftrags liegen. Keine Cross-Repo-Korrekturen. Die Brain-Schema-Beschaffung und CI-Workflow-Verknüpfung bearbeitet der Orchestrator separat; benötigte Änderungen dort im Bericht nennen, nicht selber schreiben.

## Nachweis

Bericht: `.tasks/2026-09-23-ci953-abnahme/RUST-NACHWEIS.md` mit konkreten Commands, Exitcodes, Testanzahlen, Skips, roten Baselines, korrigierten Ursachen, noch offenen Punkten und Liste eigener Dateien. Keine Erfolgsaussage für einen nicht abgeschlossenen Lauf. Nach Fertigstellung keine weitere Dateiänderung. Keine Zeitabschätzung, kein Abbruch nach dem ersten roten Test, wenn die Ursache in diesem Paket behebbar ist.
