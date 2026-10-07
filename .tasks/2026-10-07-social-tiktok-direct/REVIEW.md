# Merge-Gate, Runde 1

- Basis: origin/main, e0b0dbaf662d7680c4ceaa210bf15f1443693cd8
- Head: 601142ae, feat/social-tiktok-direct-post
- Befehl: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-social-tiktok-direct --base origin/main --head feat/social-tiktok-direct-post`
- Exit: 1
- Modell: gpt-6.1-sol
- Urteil: BLOCK

## Befundliste

[gpt-6.1-sol] BLOCK: Legacy recovery, terminal-failure retries, and rejected scheduling requests are broken.

1. rust/crates/tb-dashboard-api/src/handlers/social_media.rs:1366 | BLOCKING: Choices commit before schedule validation. `save_choice` writes the clip and existing pending jobs at lines 260 and 266 before an invalid schedule returns 400 at line 1400. | A rejected rescheduling request still changes the caption/privacy of a post that remains scheduled.

2. rust/crates/tb-social-media/src/upload_worker.rs:846 | BLOCKING: Legacy recovery mistakes JSON null for present options. Both JSON-field reads at lines 391 and 816 produce `Some(Value::Null)` for SQL NULL; recovery consequently selects `resolve_tiktok_uploader` instead of the legacy fallback. | Pre-migration inbox jobs fail options parsing and stop receiving publication-status checks.

3. rust/crates/tb-dashboard-api/src/handlers/social_media_tiktok_direct.rs:254 | BLOCKING: Any historical publish ID permanently blocks another approval, including confirmed `FAILED` operations. `rejected` preserves that ID. Both callers, `queue_upload_handler` at line 1366 and `approval_decision_handler` at line 3032, inherit the lock. | Users cannot correct and reschedule definitively rejected clips, despite the recovery messages and retry button directing them to do so.

4. bot/dashboard_v2/src/components/socialmedia/TikTokPostDialog.tsx:60 | NIT: Appearance is unverified; this repository has no configured visual-review project. | Desktop and mobile appearance were not assessed in this review.

## Übergabe

Keine Eigenkorrektur durch den Implementierer. Haupt-Orchestrator weist einen neuen Fixer zu. Folgeprüfung muss dasselbe Modell wie Runde 1 verwenden. Die vorhandene eigene Sichtprüfung ist in EVIDENCE.md und unter `/home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/` dokumentiert; sie ersetzt kein unabhängiges Urteil des Gate.

Kein Merge, kein Push nach main, keine produktive Migration, kein Deploy und kein echter Post. Worktree und Branch bleiben für den neuen Fixer erhalten.

MERGEPROTOKOLL[MS-1]: 9 Git-Schritte einzeln | Anläufe: 1 | Gate: gpt-6.1-sol BLOCK, drei blockierende Befunde

Die neun verändernden Schritte dieser Integrationsrunde sind Fetch, Add, Commit, Attribution-Amend, Rebase, Konflikt-Add, Rebase-Continue, Dokumentations-Add und Dokumentationscommit. Reine lesende Git-Abfragen sind nicht mitgezählt.

## Folgeprüfung, Runde 2

- Modell: gpt-6.1-sol, ausdrücklich mit `--model` gewählt.
- Head: 59c904d8.
- Exit: 0.
- Urteil: ALLOW: All previous blockers are fixed; no blocking regression found in the supplied fix-diff.
- Die drei ursprünglichen Befunde wurden als FIXED bestätigt.
- Log: /tmp/tb-tiktok-gate-round-2.log.

## Integrierte Folgeprüfung, Runde 3

Origin/main hatte sich danach auf 07f511a3 weiterbewegt. D2 wurde durch Rebase integriert und beide Verarbeitungswege erhalten. Runde 2 galt nicht als Freigabe dieses neuen Standes.

- Modell: gpt-6.1-sol, ausdrücklich mit `--model` gewählt.
- Basis: origin/main 07f511a3.
- Head: 9315b3cf7e4a6feeffc32b603db26eca78b03a2c.
- Exit: 0.
- Urteil: ALLOW: No blocking defect is established by the supplied source.
- Log: /tmp/tb-tiktok-gate-round-3.log.

Nicht blockierende Hinweise: Ein fehlgeschlagener Statusschreibvorgang lässt einen failed-Job ohne bestätigtes FAILED konservativ gesperrt. Der nicht mitgelieferte waiting-to-pending-Pfad wurde in refresh_waiting_connections nachgelesen; die integrierten Recovery- und Fairness-Tests bestanden.

Nach ALLOW erfolgten Fast-Forward in /home/nathanael/.worktrees/tb-social-tiktok-merge und normaler Push origin HEAD:main, beide Exit 0. Eine erste Pushform mit Logumleitung wurde vom Hook wegen seiner RefSpec-Auswertung abgewiesen; der einzelne literale Push ohne Umleitung wurde zugelassen. Kein Hook wurde umgangen. Frisch geholtes origin/main bestätigte den vollständigen Head-SHA.

MERGEPROTOKOLL[MS-1]: 27 Git-Schritte einzeln | Anläufe: 2 | Gate: gpt-6.1-sol ALLOW in Runde 2 und Runde 3

Zählstand vor dem Dokumentationsabschluss: Das aus dem tatsächlichen Sessiontranskript abgeleitete Protokoll `/tmp/tb-tiktok-git-audit-before-docs.jsonl` enthält 27 einzelne Git-Aufrufe. Darin sind der abgewiesene Main-Push und der reine Clean-Dry-Run enthalten, nicht als erfolgreiche Änderungen gezählt. Die zwei Main-Push-Anläufe gehören zur Implementierungsintegration. Die nachfolgende Dokumentationsprüfung und das Cleanup werden getrennt protokolliert; der tatsächlich ausgelieferte Anwendungsstand bleibt 0452e03cb7eab42d9e08ee5d39bde380514f1cd3.
