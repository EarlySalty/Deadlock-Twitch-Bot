# AP independent acceptance review

Initial reviewed revision: fedc7515a, compared with origin/main d21e72c6a.
Reviewer: native rust-reviewer ab203daa1565e034d, source-only acceptance and targeted continuations.
Initial result: BLOCK. Final source acceptance: no blocking defect remains in the five reviewed lifecycle corrections. Final compiler, PostgreSQL/FFmpeg and gate results are tracked separately in EVIDENCE.md.

1. Fixed: schedule retiming excludes artifacts whose jobs started but remain unfinished. Both running and resumed queued jobs retain matching job/video reservations.
2. Fixed: skip expires unpublished review/rendering outputs and releases pending exact-file reservations atomically with cancellation. Other owners and executed publication history remain untouched.
3. Fixed: explicit processing resets unfinished metadata.ai_editor.attempts while preserving successful checkpoints and their counters. The AI pipeline was not edited.
4. Fixed follow-up: skipped static review outputs retain their review status with immediate expiry, preserving the existing publisher cleanup path.
5. Fixed follow-up: skipped AI review/rendering outputs retain their status with immediate expiry, preserving the existing AI expiry consumer's conversion to expired and subsequent output deletion. The concrete skip regression invokes the real database expiry claim with production_ai_job_id metadata.

The reviewer verified the last condition against produced_video.rs:298 and ai_editor/pipeline.rs:1220. No builds or tests were run by the reviewer for the final source-only continuation.

No production accounts were modified and no AI or publication calls were made.

Repository-wide Rust formatting was compared independently before the latest upstream merge: untouched d21e72c6a and AP both had 802 formatter Diff sections, and normalized complete output was identical. AP introduced zero additional formatter failures.
