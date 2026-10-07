# Übergabe an den Haupt-Orchestrator

ABWEICHUNG: Der Bauauftrag ist implementiert und geprüft, der Abschluss bleibt wegen des lokalen Merge-Gates offen. Zwei Aufrufe enden mit Exit 2, weil `bwrap` und `unshare` keine Namespace-Prüfumgebung anlegen können: `Cannot allocate memory`. Es gibt kein ALLOW und keinen inhaltlichen BLOCK. Die Schutz-Hooks wurden nicht geändert.

Codecommit: `0ee53ca2`, Branch `fix/social-token-ablauf`, Worktree `/home/nathanael/.worktrees/tb-social-token-ablauf`. Weiterarbeit erfolgt an diesen Artefakten. Der schmutzige Haupt-Checkout blieb unangetastet; keine zusätzlichen Threads.

TikTok-Verbindungsfrist, dauerhafter Neu-Verbinden-Zustand, strukturierte Anbieterfehler, ID-gebundene und entprellte Discord-DM sowie Dashboard/API/i18n und SQLx-Metadaten sind umgesetzt. Die vorhandene Verschlüsselung und der Discord-DM-Port bleiben erhalten. Die gespeicherte Nachricht bleibt pro Vorfall gleich, auch bei Eskalation. Eine negative Broker-Antwort wird nicht als Erfolg behandelt.

Prüfung: 351 Social-/DB-Tests, 2 Bot-DM-Tests und 26 Frontend-Tests bestanden. API-Nachlauf: 1309 bestanden, dieselben 22 Fehler wie vor dem Fix, keine neuen. Clippy und drei Frontend-Builds erfolgreich. Formatprüfung der geänderten Rust-Dateien bestanden; Workspace-Formatprüfung hat Abweichungen in 148 nicht geänderten Dateien. Die neue Anzeige wurde im gebauten Dashboard-Artefakt belegt, noch nicht in Produktion.

Nachweise: `EVIDENCE.md`, `REVIEW.md`, `CONTRACT.md`. Die Wiederaufnahme einschließlich manueller Produktionsmigration, Release-Verifikation, Live-Beweis und Cleanup steht in `TODO.md`. Produktionsmigration, Merge, Deploy und Neustart wurden nicht ausgeführt. Ein echter Zugang wurde nicht für eine DM-Probe entwertet. Branch, Worktree und die eigene Test-DB bleiben erhalten. Der Thread wird nicht als fertig gesettelt.

TESTNACHWEIS[TW-1]: 1688 passed, 0 ignored | Baseline: 22 rot
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Dashboard und Social-Media-DM
WIRKUNGSPRUEFUNG[WP-1]: 0 offene Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 7/7 geprüft

Kein LIVEBEWEIS: Produktion ist unverändert. Nächste Aktion für die Bereichsführung: `REVIEW.md` öffnen und den Hostfehler der Prüfumgebung beheben lassen, ohne den Gate zu umgehen.
