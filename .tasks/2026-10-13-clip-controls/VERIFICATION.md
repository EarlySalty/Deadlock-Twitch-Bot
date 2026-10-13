# Clip controls verification

## Contract and boundary

Official Twitch reference: https://dev.twitch.tv/docs/api/reference/#create-clip and https://dev.twitch.tv/docs/api/reference/#create-clip-from-vod. Native clips accept title and duration query parameters, with duration from 5 to 60 seconds in 0.1-second increments. Creation is asynchronous. The transport sends the non-idempotent creation POST once; the runtime confirms publication using GET before reporting a completed clip.

The optional multi-minute VOD feature is not implemented. AUFTRAG.md explicitly permits shipping native improvements when existing infrastructure cannot provide the requested longer video. Existing whole-VOD archiving does not provide bounded chat-selected range jobs with a private expiring download. Recommended decision: private dashboard downloads with bounded jobs, retention and authorization, not automatic publication. No placeholder command or timestamp-link substitute was added.

## Completed checks

- Package check and regular Clippy, using cargo-slot with three jobs: tb-domain, tb-transport-twitch, tb-chat, tb-dashboard-api, tb-bot. Both completed with exit 0.
- Strict Clippy failed once on feature and once on unchanged origin/main 643553a13, both with the same result_unit_err in tb-raid/src/signup_denylist.rs:71. This is a measured baseline, not a new suppression.
- Formatter check passed for the changed Rust files other than auth/streamer_scope.rs. That file has unchanged production formatting failures at lines 178, 186 and 195 on both baseline and feature; the added macro was formatted separately. No unrelated production formatter rewrite was retained.
- Dashboard TypeScript/Vite build and ESLint on the three owned frontend files passed. Frontend tests: 16 passed, 0 failed, 0 skipped across clipCommand, commandNames and verwaltungTabs.
- Rust focused results completed: domain 25 passed; Twitch clips transport 8 passed; session-ID ownership test 1 passed; clip-command filter 10 passed, including the real command-to-HTTP request contract; settings unit tests 6 passed; three existing clip/quota fixtures passed; full migrations and schema snapshot assertion 1 passed without update mode. All these groups had 0 failed and 0 ignored.
- Affected plan fixtures used empty Twitch IDs and login-only social access. Clip-related calls now carry valid stable IDs, seeded ID-bound access and a trial-ineligible fixture. Production auth, publication and quota policies were not weakened. The unchanged full-schema baseline independently failed with exactly the same 78 social-AI columns absent from its snapshot; regeneration adds those existing columns plus the new clip-duration column. No applied migration was edited.
- Final formatter and regular Clippy for dashboard tests passed after the fixture corrections.
- PostgreSQL constraint check ran against the exact new migration SQL in a rolled-back temporary table: 5 valid values accepted, 5 invalid values rejected, absent durations stayed NULL and the disabled toggle stayed disabled.

TESTNACHWEIS[TW-1]: 70 passed, 0 ignored | Baseline: 2 tests red, 1 strict Clippy error

## Browser and text

Moli only, own isolated CDP context and preview port 4177. Persisted fetch fixture verifies default 60, save 30, reload, disable/re-enable and preservation of duration. This fixture is UI evidence, not production database evidence. Both screenshots were opened with Read. At 390 and 1440, document width equaled viewport width and no page errors were recorded. Moli and the preview later ended at their configured time limit after these checks; no foreign browser or preview process was stopped.

SICHTPRUEFUNG[SP-1]: 2 Aufnahmen | 0 mit Befund | Bilder unter /tmp | angesehen: /tmp/clip-controls-390.png, /tmp/clip-controls-1440.png
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: dashboard and clip command messages

## Non-green and infrastructure evidence

The earlier broad Rust run was stopped with 36 reported failures, including unrelated watchtime, contest and live-judge paths. It is not a successful suite. The new command-to-HTTP contract and four existing clip-command tests passed within it. Not all broad failures have a measured baseline and they must not be described collectively as pre-existing.

A focused unchanged-main baseline of watchtime_db_fehler_ist_keine_null subsequently ran locally: 0 passed, 1 failed, 0 ignored, panic at watchtime_tests.rs:107. That establishes only this individual old failure. The first remote attempt never ran a test because compilation failed on disk space.

Remote ionos root filesystem was 464G/464G used, approximately 2.9M free. Its build workspaces occupied 195G; no foreign workspace, job, data, log or compiler cache was deleted. Automatic stale-workspace cleanup had not resolved the condition. Remaining verification uses the documented per-call CARGO_SLOT_REMOTE=0 fallback and existing shared local sccache/artifact directories.

## Independent review

A second independent inherited-model native reviewer explicitly accepted the entire 17-file patch with no verified blocking findings. Reviewed patch SHA256: 04fece5331581f2b6115df4fc5afe6e943208ff4f251dda194aa9d954d9ba007. The clean integration diff against origin/main has exactly the same hash at ac46bdf447d9e10544d8e5a77537f8d2f65643d1. Its pending schema/baseline boundaries were subsequently closed by the parent's successful full-schema assertion and measured 78-column unchanged-main drift; the opened browser screenshots were already available. Live and real Twitch creation remain distinct boundaries.

WIRKUNGSPRUEFUNG[WP-1]: 0 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 2/2 geprüft

## Integration and live evidence

Code commit 9c98f4451 is pushed on feat/clip-controls-20261013. Its exact code diff SHA256 is 04fece5331581f2b6115df4fc5afe6e943208ff4f251dda194aa9d954d9ba007, the supplied full independent acceptance patch. Clean integration ac46bdf447d9e10544d8e5a77537f8d2f65643d1 is pushed to main and independently confirmed with git ls-remote. Local gate using the selected gpt-6.1-sol returned exactly `ALLOW: No grounded blocking defect found.` Its API-spec NIT is covered by the directly verified official reference in AUFTRAG.md and independent review, not by mock tests alone.

MERGEPROTOKOLL[MS-1]: 8 Git-Schritte einzeln | Anläufe: 2 | Gate: ALLOW: No grounded blocking defect found.

Main phase: fetch; inspect origin/main; cherry-pick code-only commit; identify exact SHA; compare full diff hash; verify clean integration; push HEAD:main; verify remote main. One pretool push call was blocked by R10, which reported that the merge role was not loaded. Reading the canonical merge skill resolved that prerequisite; no Git command ran on the blocked call and no gate was bypassed.

Standalone clean release clone matches origin/main SHA. Required Rust binaries and all three installer-required frontends are being built. No production migration, settings write, clip creation or public post has occurred. Task notes and the foreign REGISTER.md change remain outside main. Pending wrapper deploy and live verification.
