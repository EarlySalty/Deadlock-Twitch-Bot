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
