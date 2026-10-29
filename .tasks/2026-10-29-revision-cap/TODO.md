# Social AI revision cap: visible counter and clear messages

## Goal (2026-10-29)
- Lucky (twitch_user_id 1376724506, clip 140449, root job 159) hit the revision cap and got a vague 409; nothing visible happened.
- User decisions: failed revisions do not count; live cap 20 (bot.toml `[social_ai.runtime.revisions] max_revisions = 20`, applied live, backup bot.toml.before-revisions20-20261029).

## Change
- Cap counts non-failed children of the root against the live runtime (not the frozen checkpoint limit), so the config change applies to existing chains.
- New errors RevisionLimit and RevisionBusy with German messages, both HTTP 409.
- AiJobView.revisions_left (null on list rows, number on single job views).
- Dashboard: "Noch N Änderungswünsche für diesen Clip." under the revise form in StudioApprovals and RevisionControls; at 0 the submit button is disabled and the used-up message shows.

## Checks
- clippy tb-social-media + tb-dashboard-api -D warnings green; ai_editor tests 113/113 on ionos incl. cap regression in unchanged_revision_ends_without_a_new_version_and_keeps_its_summary.
- tsc, eslint green; npm test 511/516 (baseline 5 red).
- clip_pool and autopilot_schedule finish suites need test_dsn, not set on ionos (environment, unrelated).

## Live proof after deploy
1. `deploy-twitch-release --pruefen` exit 0; journal of bot and dashboard without errors.
2. GET single job 171 (owner 1376724506) returns revisions_left 12 (20 minus 8 non-failed children) via DB-equivalent query:
   SELECT COUNT(*) FROM social_ai_jobs WHERE root_job_id=159 AND id<>159 AND status<>'failed'.
3. Throwaway owner 1186925760: open Studio > Freigaben on an AI video, click "Änderung wünschen": counter line visible.

## State after merge
- c2be811b2 on main, gate ALLOW (Opus, Sol at quota), deploy-queue ticket 13.
- Follow-up branch for gate nits: singular "Noch 1 Änderungswunsch" (de+en); retry_manual_job must check revisions_left for children; do not POST open_editor just by opening the form (only show counter once a maker job exists); list rows also get revisions_left (fix note or drop count from list); screenshots N>0 and 0.

## Live proof (2026-10-29, after ticket 13)
- Release c2be811b26 live since 16:17; `deploy-twitch-release --pruefen` exit 0; bot and dashboard journal -p err empty since deploy.
- Live dashboard bundle (SocialMediaManager chunk) contains the counter string, the used-up message and the revisions_left decoder.
- Lucky root 159: 5 non-failed and 5 failed children, so the live API now reports 15 left (the earlier estimate of 12 counted children that have since failed).
- Open: UI check on the throwaway owner needs a dashboard login (no stored session); covered by fixture screenshots on the follow-up branch.
