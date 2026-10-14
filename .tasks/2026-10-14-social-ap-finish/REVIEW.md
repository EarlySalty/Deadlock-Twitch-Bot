# AP independent acceptance review

Initial reviewed revision: fedc7515a, compared with origin/main d21e72c6a.
Reviewer: native rust-reviewer ab203daa1565e034d, source-only acceptance and targeted continuations.
Initial result: BLOCK. Final source acceptance: no blocking defect remains in the five reviewed lifecycle corrections. Final compiler, PostgreSQL/FFmpeg and gate results are tracked separately in EVIDENCE.md.

1. Fixed: schedule retiming excludes artifacts whose jobs started but remain unfinished. Both running and resumed queued jobs retain matching job/video reservations.
2. Fixed: skip expires unpublished review/rendering outputs and releases pending exact-file reservations atomically with cancellation. Other owners and executed publication history remain untouched.
3. Fixed: explicit processing resets unfinished metadata.ai_editor.attempts while preserving successful checkpoints and their counters. The AI pipeline was not edited.
4. Fixed follow-up: skipped static review outputs retain their review status with immediate expiry, preserving the existing publisher cleanup path.
5. Fixed follow-up: the original skip correction preserved AI review/rendering states for expiry-to-expired cleanup. Newly integrated upstream also supports rejected AI output cleanup. Final AP skip now rejects every unpublished AI rendering/review/approved/scheduled output and sets needs_review=true; static ready outputs retain review with immediate expiry. Both existing cleanup consumers remain supported.
6. Fixed integration finding: upstream can validate one AI variant without a review requirement before another variant fails. Skip now includes approved/scheduled outputs, releases their pending reservations and clears their slots. Rejection and needs_review=true defeat a promotion selected before the owner lock. Preselected slotless scheduling fails its subsequent readiness check. The reviewer confirmed no blocking defect in the corrected not-yet-executed skip path. No publishing implementation was edited.

Boundary: publication events already marked publication_executed_at remain untouched. Their platform effects can still complete after skip. The contract is cancellation of not-yet-executed outputs, not an unconditional no-upload-after-click guarantee. The reviewer found no binding completed-parent prerequisite for pre-skip promotion, so no such architecture change was introduced.

The reviewer checked the integrated expiry consumer and pipeline cleanup plus publisher candidate selection, owner-locked eligibility and scheduling readiness. No builds or tests were run by the reviewer in these source-only continuations.

No production accounts were modified and no AI or publication calls were made.

Repository-wide Rust formatting was compared independently before the latest upstream merge: untouched d21e72c6a and AP both had 802 formatter Diff sections, and normalized complete output was identical. AP introduced zero additional formatter failures.
