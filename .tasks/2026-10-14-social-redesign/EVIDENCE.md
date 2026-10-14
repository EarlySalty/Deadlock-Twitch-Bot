# RD evidence

## Frontend checks
- Production build passed after the palette correction. Log: `/tmp/rd-resume-tools/build.log`.
- ESLint on all modified frontend files passed; the subsequently changed schematic passed separately. Logs: `/tmp/rd-resume-tools/eslint.log`, `/tmp/rd-resume-tools/schematic-lint.log`.
- Social, shell and translation contracts: 96 passed, 0 failed, 0 skipped. Import, schedule and revision component contracts: 14 passed, 0 failed, 0 skipped. Commands use `npx --no-install tsx --tsconfig tsconfig.app.json --test`; logs: `/tmp/rd-resume-tools/frontend-tests.log`, `/tmp/rd-resume-tools/component-tests.log`.
- Shell baseline: 12 passed, 1 failed. The existing assertion overreached into unrelated routes; its slice now stops at the next top-level function without weakening the width assertion.
- Brand-palette baseline and current: both 2 passed, 3 failed, 0 skipped, with identical 91 source-violation lines. Baseline uses committed HEAD sources exported into `/tmp/rd-resume-tools/palette-baseline`. New schematic colors were corrected to the existing palette; no new violations remain. Logs: `/tmp/rd-resume-tools/palette-baseline.log`, `/tmp/rd-resume-tools/palette-current.log`.

## Browser checks
Owned inherited Moli PID 84018, CDP 9297, layout/image/font enabled. Owned inherited fixture Vite PID 70000, port 4191. No Chrome/Brave or personal profile was started or attached.

Fixture URL: `http://localhost:4191/social-media?streamer=midcore_live&twitch_user_id=123456789&view=plan`. The earlier screenshot without both channel parameters is not valid evidence.

Successful synthetic functional run: mobile navigation, keyboard Auto-Pilot switch and save with overview refresh, selectable-only bulk selection and clearing, historical import, review shortcut clearing incompatible search. Result: `functional-check.json`. No page errors; document width 390 at viewport 390. All mutations use local fixtures, no paid AI or publishing.

Moli limitation measured: getBoundingClientRect top and pointer hit testing use document coordinates after scrolling. Playwright visibility/fill/click checks therefore failed on scrolled controls. The successful run uses focus plus native keyboard events and attached-state waits. This is keyboard/DOM evidence, not pointer compatibility or Chrome/Firefox/Safari/media evidence. Diagnostic: `/tmp/rd-resume-tools/input-diagnostic.mjs`.

Final settled confirmation: all six tabs plus the Analytics dialog captured at 390 and 1440; measured document widths equal their viewport widths, no overflowing elements, page errors or network-request failures. The 34-image viewport series plus three targeted control captures is in `screens/`; images are never staged. Viewed final files:
- plan-1440-2-small.png, plan-390-2-small.png
- library-1440-3-small.png, library-390-3-small.png, library-390-controls-small.png
- pool-1440-2-small.png, pool-390-2-small.png
- konten-1440-2-small.png, konten-390-2-small.png
- ai-1440-2-small.png, ai-390-2-small.png, ai-1440-controls-small.png, ai-390-controls-small.png
- archiv-1440-2-small.png, archiv-390-2-small.png
- analytics-1440-1-small.png, analytics-390-1-small.png

The first mobile library anchor did not capture the cards; the targeted controls capture replaces it as card evidence. AI option checkboxes were still bare in the confirmation; adding the existing studio-switch class preserves input/query behavior and both corrected control images were viewed. Schematics now paint their camera/game/HUD composition. Moli reports synthetic SVG image natural widths as zero before lazy capture, but the card captures visibly paint the thumbnails. Analytics dialog and German text render at both widths; the preview has no analytics response fixture, so chart series/report generation remain unverified. No paid report button was pressed.

## Rust checks
The only backend change exposes existing nullable Twitch view counts in the pool response. Independent static Rust review found no confirmed defects.

The changed repository file passes `/home/nathanael/.cargo/bin/rustfmt --edition 2021 --check`; the entire affected crate also passes the configured stable `/home/nathanael/.cargo/bin/cargo fmt --manifest-path /home/nathanael/.worktrees/tb-social-rd/rust/Cargo.toml -p tb-social-media -- --check`. An earlier unpinned `/usr/bin/cargo fmt --all` attempt reported unrelated differences in 137 files; none were applied.

Clippy current and committed baseline both fail in unchanged tb-raid signup_denylist.rs with one result_unit_err compiler error. Logs: `/tmp/rd-resume-tools/clippy.log`, `/tmp/rd-resume-tools/clippy-baseline.log`.

The first DB test command incorrectly requested a nonexistent testing feature; neither tb-social-media nor tb-db declares it. The corrected real-database run compiled successfully and initially passed 9 of 10 pool tests; the full-migration test failed because the owned throwaway container lacked the migration-required twitch_analytics database. Creating that database in the same confirmed disposable container fixed the environment. The repeat passed 10, failed 0, ignored 0, with 496 filtered across library/integration targets, in 10.51 seconds of library test execution. Logs: `/tmp/rd-resume-tools/pool-tests.log`, `/tmp/rd-resume-tools/pool-tests-fixed.log`. Command: `TB_TEST_DATABASE_URL=postgres://postgres:tbtest-throwaway@127.0.0.1:48419/twitch_test TB_TEST_REQUIRE_DB=1 TURNIER_TEST_DB_CONFIRM=throwaway-only SQLX_OFFLINE=true cargo-slot test --manifest-path /home/nathanael/.worktrees/tb-social-rd/rust/Cargo.toml -p tb-social-media --jobs 3 clip_pool -- --include-ignored --test-threads=1`.

After integrating the landed AI package through origin/main 29011c43b, the repeated real PostgreSQL pool suite passed 10, failed 0, ignored 0, with 499 filtered across targets. Strict selected-crate Clippy passed with `SQLX_OFFLINE=true cargo-slot clippy --manifest-path /home/nathanael/.worktrees/tb-social-rd/rust/Cargo.toml -p tb-social-media --no-deps --jobs 3 -- -D warnings`; actual compiler execution took 1m30s. The configured stable formatter also passed for the merged crate. Logs: `/tmp/rd-resume-tools/ai-integration-pool-tests.log`, `/tmp/rd-resume-tools/ai-integration-clippy.log`, `/tmp/rd-resume-tools/ai-integration-fmt.log`. The earlier ordinary-Clippy dependency baseline remains separately documented, not suppressed.

## Independent review
See REVIEW.md. Six confirmed frontend findings were corrected and independently rechecked. The added primary-publication regression also fixed future events with empty destination lists.

## Landed AI package integration
origin/main advanced to 29011c43b during verification. The integration preserves MetadataPanel, language selection, removed timeline sections, synchronized playback, the publication revision entry and draft recovery. Only the EditorOptionsPanel label conflicted; the landed options and language behavior were retained with RD's switch styling.

The new language label inherited two grid tracks, compressing mobile label/help text to 29.59 px. A scoped full-width grid rule and the existing studio-select class corrected this without changing its API or settings semantics. Fresh ai-1440-controls-small.png and ai-390-controls-small.png were viewed again. Repeated production build and targeted ESLint pass; social/shell/translation/component contracts now pass 111, fail 0, skip 0.

Post-integration functional checks pass for the five existing synthetic workflows and the AI workspace mutation boundary. The generic workspace fixture intentionally returns 409 for AI option writes; the test verifies the visible save error and retained stored language, not a successful production save. Moli's selectOption did not dispatch the needed event, so the boundary check uses a DOM change event on the real rendered select. No fixture mutation guard or production API was altered to make the test pass.

## Merge-gate follow-up
The first local gate returned ALLOW with nonblocking observations. Both scheduling observations were verified and corrected before release: all legacy destinations now use the same queued-posting eligibility as the queue count, and the next-appointment strip includes legacy appointments with their own timezone. Its value remains unknown if any required source fails.

Two new concrete regressions cover a blocked TikTok destination alongside an eligible YouTube destination and an earlier legacy appointment in a different timezone. The affected overview/workspace run passes 13, fails 0, skips 0. Production build and targeted ESLint pass. The full synthetic functional check passes again. Final overview viewport captures at 1440 and 390 were opened: final-overview-1440-small.png and final-overview-390-small.png; measured widths match their viewports, with no page errors. Logs: /tmp/rd-resume-tools/gate-nits-{tests,build,lint,browser,functional}.log.

The push-time review subsequently blocked the legacy overview shortcuts: a clip actively posting to YouTube can still belong to the review stage because TikTok awaits approval. Both the current-publishing and upcoming-legacy handlers now navigate to the clip's actual queue stage. Four real Moli interactions cover both handlers with waiting_tiktok_approval and waiting_schedule; each returns the review filter and retains the target clip through the real queue filter. The repeated affected suite passes 13, fails 0, skips 0; production build and targeted ESLint pass. Logs: /tmp/rd-resume-tools/gate-fix-{shortcuts,tests,build,lint}.log. The hook requested a single inline corrective round without more workers, which was followed. No visual structure changed in this correction.

## Legacy schedule and safety
Legacy approval still delegates to posting_plan for platform selection, automatic approval and next-slot planning. ApprovalWorker and AutoPilotWorker both run. The effective legacy settings remain collapsed as Bisherige Pipeline; migrating their behavior into the shared schedule is outside RD.

A read-only production approval aggregate was blocked by the isolation hook on the prescribed Infisical eval command. No protection was bypassed, no secrets printed, and no aggregate obtained. This is not evidence of absent legacy usage.

No paid AI/STT, real publishing or real-account setting changes. Preview-only data URL thumbnails reuse the synthetic poster; production remains HTTPS-only. No screenshots or secrets are committed.

## Release state
Frontend checkpoint ff0513e11 and main-integration checkpoint 3bfb60c62 are pushed on feat/social-rd-redesign-20261014. Integration preserves main's publication translations and error-message keys, RD's heading/switch controls, and onChanged overview invalidation. Post-integration build, targeted ESLint and 110 contracts pass, 0 failed/skipped. The synthetic functional run also passed after integration. Additional viewed images: screens/integration/plan-1440-3-small.png, screens/integration/plan-390-3-small.png, screens/plan-1440-controls-small.png, screens/plan-390-controls-small.png; the fresh mobile control capture replaces the misplaced multi-anchor image as publication-layout evidence.

No RD gate, main merge or deployment at the time of this verification record. The nullable Rust response extension has compiler and real-database verification. A separate clean main release clone was created at /home/nathanael/repos/twitch-release-social-rd-20261014 because the shared main checkout is foreign and dirty. Locked dependencies for all three release frontends are installed there. Live read-only wrapper proof identifies d915c030aa08fa65dd730bad4f4edd8feded36e7, with bot PID 2642229, dashboard PID 2642391 and audit PID 2643551, each active and without deleted executables. These are pre-deploy readings, not evidence that RD is live. Public manager URL /twitch/social-media returns 303 unauthenticated; public hashed JavaScript assets return 200 application/javascript. The dirty main checkout remains untouched.
