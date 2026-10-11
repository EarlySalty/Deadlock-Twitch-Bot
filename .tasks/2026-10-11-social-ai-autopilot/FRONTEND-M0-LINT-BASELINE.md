# Measured M0 dashboard lint baseline

Parent integration worktree is rebased to verified exact M0 `33656f2c2d2154a8ce888ed1a39180935209ea6e`, without application changes. Matching `npm --prefix /home/nathanael/.worktrees/tb-social-ai/bot/dashboard_v2 run lint` with the existing pinned frontend dependencies fails with six errors and seven warnings. This independently matches P1's M0 proof, rather than relying on unchanged blobs.

Error locations:
- UplinkYouTubeLive.tsx:62,75,80, mixed component/constant exports.
- uplinkBetrieb.ts:104, no-control-regex.
- tests/community-visual.tsx:20, missing component export.
- tests/community-visual.tsx:21, impure Date.now during render.

Other seven warnings are existing hook/dependency or unused-disable findings in unrelated Uplink/Verwaltung/StreamReports/ViewerTimeline files. These are not AI timing changes. Do not suppress rules or repair foreign frontend files in the small P2 timing package. Report full lint red and separately run matching owned-file lint/build/tests and review the owned diff. This does not waive a real merge gate or claim the full frontend suite green.

The first parent attempt failed with exit 2 because this integration tree had no node_modules and npm selected system ESLint 6.4.0. No source/config change was made. Parent linked its own ignored node_modules location to the already existing UI-worktree dependencies, then repeated the actual command with the pinned tools and obtained exit 1, six errors/seven warnings. No dependency installation or cache copy.

Evidence: task `bfhka6y6x`, output `/tmp/claude-1000/-home-nathanael--worktrees-tb-social-ai/78e43a4b-d7c4-4910-adb3-26be78165791/tasks/bfhka6y6x.output`. Wrong-runner attempt: `b6vd3fbog`. No production changes.
