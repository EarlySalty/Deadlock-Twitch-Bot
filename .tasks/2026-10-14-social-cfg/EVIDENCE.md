# CFG phase 1 evidence

## Completed checks

- `cargo-slot check -p tb-config -p tb-llm -p tb-social-media --jobs 3`: exit 0 before the checkpoint-pricing correction.
- Explicit rustfmt check of the owned Rust files: exit 0 for the original phase-1 change. The crate-wide formatter still reports three unchanged baseline files: reliability.rs, runtime.rs, global_file.rs. Their diff against origin/main is empty.
- Installer shell syntax check: exit 0.
- Existing deployment regression suite: 35 passed, 2 existing skips, exit 0. Fresh origin/main gives the same 35 passed and 2 skips.
- First Rust suite: 81 passed, 1 failed, 0 ignored before later targets could run. The failure is `discord_verbraucher_behalten_getrennte_guild_defaults_und_leere_guard_listen` in global_file.rs. Isolated origin/main measurement: 30 passed, the same 1 failed, 0 ignored. No test expectation was changed or skipped.
- Previous release editor and pricing settings match the inline fixture structurally. Brain remains absent.
- Production migration metadata comparison after integrating fresh main: 196 applied, no checksum/success differences. Two unapplied main migrations remain: 20261014150000 and 20261014150100. Repeat against the deploy SHA before activation.
- Migration preview using `rust/target/verify/tb-config-check`: `SOCIAL_AI_MIGRATION_READY previous_values_preserved=true brain_absent=true`. No live write performed.
- Before-deploy public admin probes for `/twitch/api/admin/social-ai/usage`, `/limits`, `/skills` and `/twitch/api/admin/config/operating` on `https://admin.deutsche-deadlock-community.de`: each unauthenticated response is 401 with application/json. Local partner AI jobs and consent routes also return 401. Loopback operating access returns 200, while the public admin endpoint correctly requires authentication.

## Independent read-only review

Reviewer found a real resumed-checkpoint pricing regression: startup validation discarded the trusted price table, so the later reservation path only enforced numeric bounds. Corrected by retaining private, nonserialized trust in EditorConfig. Deserialized checkpoints regain trust from a validated current runtime; model, pricing version and rates must match. The complete path validates again before reservation. Existing checkpoint JSON remains unchanged.

Independent follow-up: previous BLOCK resolved, no newly verified regression. Added config and media-runtime contracts inspected, execution pending.

The existing bot load call site now emits `SOCIAL_AI_WORKER_STARTED` inside the spawned manual worker future. This gives the requested positive journal proof of entering the worker, rather than inferring startup from absent warnings. It prints no configuration contents.

WIRKUNGSPRUEFUNG[WP-1]: 1 finding corrected | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 1/1 checked

## Latest verification and gate

- Check including tb-bot after the pricing correction: exit 0, verify profile compilation 2m32s.
- Clippy including dependencies fails at unchanged tb-raid/src/signup_denylist.rs:71 with result_unit_err. Isolated origin/main remote measurement reproduces the same single diagnostic and exit 101. The redundant local retry was stopped. Scoped selected-crate Clippy with --no-deps is queued; warnings remain errors.
- Correction committed and published as 09e14b30b. Fresh main d915c030a integrated as bf796e4d0f063b348de5f82e482ebff638d879ae and published.
- Formal gate task blhtkh5u6: exit 0, `[gpt-6.1-sol] ALLOW: No merge-blocking defect established by the supplied diff.` Nonblocking semantic-validation note is recorded in REVIEW.md and must be addressed by phase 2 candidate validation.
- Current live release has moved to d21e72c6a82949ee746758848c42f4a6dc4b80be for bot PID 2354089 and dashboard PID 2354395. Coaching-watch still uses 00e750557de1dd93571e66285fbb01178605b14c, PID 3422664. Reading the wrapper returned exit 1 for that release mismatch; all three have NRestarts=0 and nondeleted executables. No own deployment has occurred.

## Pending

- Queued scoped Clippy and remaining Rust suites, including the newly added checkpoint contracts.
- Verified main publication after checks.
- Build the verified main release, hash-checked live configuration migration with backup, deploy, worker-start and route proof. Latest integrated main contains admin frontend changes, so do not copy its preceding admin bundle unchanged.
- Phase 2 implementation must wait until phase 1 is live.

## Current safe boundary

Feature checkpoint bf796e4d0f063b348de5f82e482ebff638d879ae is published and has formal ALLOW. Neither CFG phase is live. No paid AI/STT calls, real publishing, streamer setting changes, Brain activation, production restart or secret output occurred.
