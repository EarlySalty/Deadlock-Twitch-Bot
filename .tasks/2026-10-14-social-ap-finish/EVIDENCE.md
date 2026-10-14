# AP verification evidence

## Checkpoints

- Implementation checkpoint: 2189ab9f1, pushed to feat/social-ap-finish-20261014.
- Edition-correct formatting checkpoint: fedc7515a, pushed.
- Latest integrated upstream: 21515ce1f43d4a486997dd65ce61ce4dfc3c1794.
- Attributed upstream merge checkpoints: 949b94143 and dd48554946fe7d1e52815081397e4054e7db9b45.
- Authenticated target/resolve regression checkpoint: fc9bec76a59a89cb984f505273729e9665cdd01f, pushed and confirmed on the remote feature branch. Lifecycle corrections are awaiting their final package checks and commit.

## Completed checks

- cargo-slot check for tb-social-media and tb-dashboard-api: exit 0 before the latest upstream merge and lifecycle corrections.
- Dashboard tsc -b and npm run build: exit 0.
- Targeted dashboard ESLint: exit 0.
- Dashboard schedule/import adapter contracts: 11 passed, 0 failed, 0 skipped.
- Changed UI mechanical design detector: no findings.
- Untouched origin/main d21e72c6a and AP Rust formatter outputs: 802 Diff sections each, identical after path normalization, zero new formatter files.
- The initial API filter social_media_autopilot_schedule selected zero tests, despite exit 0, and is not evidence. The actual test namespace is handlers::social_media::autopilot_schedule::tests. Its first real run executed 5 tests: 4 passed, 1 failed because the new connection fixture omitted required access_token_enc bytes in the real migration schema. After providing a one-byte synthetic value, the corrected run b950fv4ce passed: 5 passed, 0 failed, 0 ignored, 1398 filtered. It exercises the authenticated composed routes with all real migrations: connected-target normalization and persistence, rejection of disabled/foreign connections, foreign-owner resolution refusal, process audit and actual next claim, skip audit and cancellation, and conflict on repeated resolution. No platform token is decrypted or used.
- Package Clippy with -D warnings stopped at an unchanged tb-raid Result<_, ()> warning; no complete strict baseline is claimed. Default workspace-policy package Clippy bifkm99hl completed with exit 0, reporting 20 warnings outside AP-owned changes (tb-raid 1, tb-chat 1, tb-dashboard-api 18). Integrated-upstream package checks are tracked below.

## Independent source acceptance

The independent reviewer confirmed corrections for started-job artifact retiming, skipped review-output reservations, and exhausted unfinished AI-stage counters. Their subsequent static- and AI-artifact retention findings were addressed by immediate expiry while preserving each existing consumer's supported states. Static review outputs remain review. AI review/rendering outputs retain their current state for the existing AI expiry consumer, which converts them to expired and deletes their outputs. Audit metadata remains available. The independent source-only follow-up confirmed that no blocking defect remains in these five lifecycle corrections.

The first PostgreSQL/FFmpeg run executed 35 tests: 29 passed, 6 failed. Four failures were invalid new fixtures missing database-required fields. One existing queue check mixed a fixed planner date with the real claim clock. One existing immutable-version test still expected review after exhausting all attempts. Fixtures and expectations were corrected without removing tests or weakening immutable-file assertions. The next run executed 36 tests: 35 passed, 1 failed. Increasing lead time alone did not fix the stale planner date because claims explicitly reject past slots. The test now passes a database-clock-aligned planning time into the real planner, preserving the production no-catch-up guard. Corrected run bd4tda2q9 passed: 36 passed, 0 failed, 0 ignored, 442 filtered, including the final AI expiry correction and the real FFmpeg render/restart with zero-social_editor-usage assertions.

## Integrated upstream verification

- Upstream 21515ce1 was merged as dd4855494. Its protected AI/publishing paths were integrated unchanged, not edited by AP. The pipeline retains the ai_editor.checkpoints/attempts contract and now also deletes rejected AI outputs. Preserving review/rendering with immediate expiry remains compatible with both consumers. Manual AI revisions still allocate without a production job_id, so the backlog join excludes them.
- Dashboard production build, including tsc -b: exit 0 after integration. Vite reported unresolved font URLs and its chunk-size warning; no warning-free build is claimed.
- The first adapter invocation omitted tsconfig.app.json and failed to resolve @/api; no application code was changed to address this launcher mistake. The correct command, ./node_modules/.bin/tsx --tsconfig tsconfig.app.json --test src/components/socialmedia/autopilot/import-pool/contracts.test.ts src/components/socialmedia/autopilot/schedule-queue/api.test.ts, passed: 11 passed, 0 failed, 0 skipped.
- Final five-file edition-2021 rustfmt --check and git diff --check: exit 0.
- Initial integrated Rust launcher bcluqpd6t rejected a passwordless synthetic DSN with exit 78 before compiling or running tests. Confirmed the existing dl-central-test-postgres-48623 container's throwaway label and binding; bk1wdot3f uses the documented synthetic credentials and queues AP PostgreSQL/FFmpeg, authenticated route tests, then package Clippy. No production credentials or database are used.

## Gate

MERGEPROTOKOLL[MS-1]: Git operations performed individually | Anläufe: 1 | Gate: [gpt-6.1-sol] ALLOW on 8a23ca5ea7937e396b61832ca59ee093b2e6f605

That verdict does not cover the later lifecycle corrections. A fresh gate is required on the final committed revision.

## Browser evidence

Moli 1.1.14 was used exclusively against the isolated local preview with synthetic fixtures. No real account was used.

SICHTPRUEFUNG[SP-1]: 6 Aufnahmen | 0 mit Befund | Bilder unter /home/nathanael/.claude/sichtpruefung/social-ap-finish-20261014 | angesehen: plan-390.png, review-390.png, library-390.png, plan-1440.png, review-1440.png, library-1440-isolated.png

The last desktop library capture renders the actual ImportPoolPanel with its existing fixture API and actual studio CSS in the isolated fixture host. The image was opened and reviewed: priority explanation, normal oldest-first selector, clip status, and priority checkbox are readable with no overlap. Measured viewport width and document width were both 1440; component width was 1176; zero page errors.

The local Moli DOM-handler check passed: TikTok selection was saved, disconnected Instagram stayed disabled, and review processing stored the owner, process action, and explanation. Pointer-action compatibility is not claimed. Moli does not reliably emulate native form submission here; explicit DOM submit events were used on the local fixture.

Earlier whole-page library and static-DOM desktop rendering attempts timed out and are not passing evidence. The first isolated-component setup also failed: it imported React Query without the Vite version query and therefore provided a different context from the real component. Using the component's actual Vite module identity corrected the test setup and produced the settled desktop image above. This verifies the scoped import component, not whole-page desktop browser compatibility. No other browser was started. The original preview task reached its background limit and stopped; the owned replacement preview is bxwt316j6.

## Production safety and release preflight

- No paid product AI calls, publishing, or real streamer setting changes were performed.
- No AP migration was added.
- Read-only production migration metadata comparison found two missing migrations inherited from upstream: 20261014150000_social_ai_admin_console_limits.sql and 20261014150100_social_ai_skill_review.sql. No checksum mismatch was found. No migration was applied.
- Read-only release inspection found current d21e72c6a, active bot/dashboard processes on that revision, and the stream-coaching watcher still on 00e750557. The wrapper check returned exit 1 due to that mixed revision state.
- No AP merge to main or deploy has been performed.
- The early preflight found the two inherited upstream migrations unapplied. A later source inspection verified that the current installed deploy-twitch-release wrapper matches the repository wrapper and now starts deadlock-twitch-migrate.service before application restarts. Its required artifact list includes tb-stt-server in addition to the eight previously documented binaries. Follow this live verified wrapper, not the stale migration and artifact-count memory. No migration was manually applied.
- Refresh after the parallel upstream release: current is d915c030aa08fa65dd730bad4f4edd8feded36e7. The read-only wrapper check now passes for bot PID 2642229, dashboard PID 2642391 and coaching watcher PID 2643551, all on that revision, active with NRestarts 0 and no deleted executable. Migration metadata is current: zero missing migrations and zero checksum/success mismatches. The early pending-migration/mixed-release state above is historical, not the current blocker.
- Both AP-owned preview processes have stopped: the original reached its background limit; the replacement was explicitly stopped after the scoped desktop capture. Other services were not stopped.
