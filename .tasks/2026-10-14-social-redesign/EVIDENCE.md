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

The changed repository file passes `/home/nathanael/.cargo/bin/rustfmt --edition 2021 --check`. The unpinned `/usr/bin/cargo fmt --all` attempt reported unrelated formatting in 137 files; none in the owned repository file. No unrelated formatting was applied.

Clippy current and committed baseline both fail in unchanged tb-raid signup_denylist.rs with one result_unit_err compiler error. Logs: `/tmp/rd-resume-tools/clippy.log`, `/tmp/rd-resume-tools/clippy-baseline.log`.

The first DB test command incorrectly requested a nonexistent testing feature. Neither tb-social-media nor tb-db declares it. Corrected clip_pool test is running through cargo-slot against owned throwaway PostgreSQL on loopback port 48419, with `--include-ignored --test-threads=1`; no result is claimed yet.

## Independent review
See REVIEW.md. Six confirmed frontend findings were corrected and independently rechecked. The added primary-publication regression also fixed future events with empty destination lists.

## Legacy schedule and safety
Legacy approval still delegates to posting_plan for platform selection, automatic approval and next-slot planning. ApprovalWorker and AutoPilotWorker both run. The effective legacy settings remain collapsed as Bisherige Pipeline; migrating their behavior into the shared schedule is outside RD.

A read-only production approval aggregate was blocked by the isolation hook on the prescribed Infisical eval command. No protection was bypassed, no secrets printed, and no aggregate obtained. This is not evidence of absent legacy usage.

No paid AI/STT, real publishing or real-account setting changes. Preview-only data URL thumbnails reuse the synthetic poster; production remains HTTPS-only. No screenshots or secrets are committed.

## Release state
No RD gate, merge or deployment yet. origin/main was refreshed to d915c030a and remains to be integrated. The dirty main checkout is untouched.
