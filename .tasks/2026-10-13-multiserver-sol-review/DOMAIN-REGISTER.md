# Native Sol domain register

Integration owner: native session `5c00dffd-02da-4a1f-a6ee-019ccc515fca`, branch `integration/multiserver-sol-review-20261013`, initial HEAD `a3e7a40cf631e4f9ab3e24fb58128bc0a6c21378`. Coordinator `REGISTER.md` remains untouched.

## Availability

UltraCode activation is exposed by the parent runtime reminder. Workflow `wf_7573a16c-c64` executed one native worker successfully, with 3 tool calls and no errors. Worker runtime explicitly identifies `gpt-6.1-sol[1m]`; no model override or provider substitution was requested. The harness does not independently expose provider routing. The worker has no redelegation tool, which is consistent with the ordinary-worker contract, not a workflow availability failure.

## Read-only inventory ownership

Workflow `wf_1673ffc3-68f`, task `w12yakrij`. All workers inherit Sol; no worker may mutate files, build, publish, merge, or touch secrets in this phase.

| Worker | Scope |
| --- | --- |
| integration-gaps | Candidate ancestry and all ten issue/PR facts |
| hub-guild-db | H/G, Hub, DB schema, membership and backfill |
| tenant-admin-security | A/G centralized tenant and credential boundaries |
| oauth-domains | O trusted origins, OAuth state/redirect/cookies, Caddy |
| ai-ledger | K/L central AI guard and actual usage ledger |
| raids-ads | R language/network selection and home-server ads |
| dashboard-translation | D candidate and A frontend compatibility |
| chat-website-translation | C candidate and W website translations |

Raw reports: `/home/nathanael/.claude/projects/-home-nathanael--worktrees-ttb-multiserver-sol-review-20261013/5c00dffd-02da-4a1f-a6ee-019ccc515fca/subagents/workflows/wf_1673ffc3-68f/journal.jsonl`.

## Verified integration facts

Fetched `origin/main`: `643553a133ddb4efd6644b1224532011286e0fe9`. All six draft PR heads #1071 through #1076 are ancestors of initial integration HEAD; do not merge duplicate diffs. #1077 is the initial integration candidate, with an obsolete review-only body superseded by this task.

Candidate D: `origin/feat/dashboard-englisch-pr-20261013` at `304f92bc49652486e2a5a000ae6852fbfe127f3d`. Candidate C: `origin/feat/multiserver-c-texts-20261013` at `93e55ce383b28b2f8412ce871a7e4b43d11de66f`. Local committed G advanced to `3453e5ee9377ba5ce3f4277f5c8f9699c3d1836a`; no matching remote ref was present at fetch. Foreign worktrees remain read-only.

Acceptance and live status: not established. No merge gate, deployment, live mutation, or completion claim yet. Issue #1069 is closed and board Done, subject to independent actual-behavior verification.

## Candidate assembly and operations

Workflow `wf_ba6ac5f4-614`, task `wx1gtl1ys`, owns a fresh isolated feature worktree for mechanical D/G/C integration and conflict correction. It may not merge main or deploy. Parent remains sole integration owner. Raw report lives in that workflow transcript.

Workflow `wf_b247f22f-352`, task `wt9221sm6`, owns read-only H/O/G/A/W operational prerequisite inspection. No secret values, source/config/DB mutation, build, restart, or external messages authorized in that phase.

Read-only production baseline: `/usr/local/bin/deploy-twitch-release --pruefen` returned the consistent release `5c50fe47c3a74f7b38e865bb434445b79701eebf` for system units `deadlock-twitch-bot-rust`, `deadlock-twitch-dashboard-rust`, `deadlock-twitch-stream-coaching-watch`, all active and no deleted executable. Targeted system and user service listings showed no Discord Hub unit. This is baseline evidence, not roadmap completion.

Gate located at `/home/nathanael/Documents/.claude/gpt-workers/gate_hook.py`. Its CLI supports an explicit single-model `--model` review; the live pyramid includes `gpt-6.1-sol` in review_1 and review_5. A real Sol verdict may be reused by the protected push hook for the exact SHA, without requesting an unapproved fallback.

## Inventory outcome and follow-up ownership

Inventory `wf_1673ffc3-68f` completed. Board candidate advanced to `79a3376a8c51c89e51592bd023f3aaf4c999d14f`, now containing exact D/C/G committed content. Four existing D backend follow-ups remain required: `339ddfcab`, `c4230000d`, `a4e7a4f9a`, `0b6a9353d`. Never rebuild those translations.

Three typed Rust reviewers unexpectedly executed broad check/Clippy validation and stopped before their substantive reviews. Check passed; strict Clippy failed at unchanged-main `signup_denylist.rs:71` (`result_unit_err`). No green review or baseline test claim follows from those runs. Static recovery workflow `wf_9cf872ae-854` owns R/K/L/C/W review plus independent finding verification, with no compiler runs. Core findings are independently checked by `wf_f91f3e39-d5b` against board79a3376a and the four D follow-ups.

Original assembly task `wx1gtl1ys` and native agent `a87fd8927d891d607` were explicitly stopped after an ambiguous overlapping index/reset report. Preserve its dirty owned worktree `/home/nathanael/repos/Deadlock-Twitch-Bot/.claude/worktrees/wf_ba6ac5f4-614-1`, branch `feat/multiserver-sol-candidate-integration-20261013`, committed HEAD `7df682a3efd306e970fe45765f785774ed9d54b4`; do not reset, delete, or treat its staged sources as verified. No foreign actor is established by the report.

Replacement `wf_d5418f97-fdf` exclusively owns precreated `/home/nathanael/.worktrees/ttb-sol-assembly-v2-20261013`, branch `feat/multiserver-sol-assembly-v2-20261013`, initial79a3376a. It imports current main and the four D commits only; parent will create a clean release tree excluding feature task records. No concurrent writer or resume while active.

Urgent fixes `wf_6130ec2d-a92` have non-overlapping ownership: Hub gateway REST log/error redaction (`gateway/rest.rs` plus its test), and typed signup-denylist error (`signup_denylist.rs` only unless caller ownership is explicitly extended). No source secrets or real leaked tokens were read. Possible token-bearing log paths were reported to the user before deployment.

Operations implementation `wf_76bf0133-723` owns missing Hub release installer/launcher/unit support and Hub config default only, not Caddy, Cargo, relay, or gateway REST. App activation remains blocked: Infisical name inventory contains none of the three required Hub credential names, Hub unit/binary absent, old default8771 occupied. International public streamer URL still returns308, callback404. These are actual unmet prerequisites, not permission to publish or enable a broken service.

Read-only migration/shadow proof `wf_77b32055-f7e` completed without writes. Production guild/shadow tables are absent. Applied version metadata is NOT confirmed: the selected credential's migration SELECT was denied, exit3, despite schema/aggregate queries passing. The earlier operations claim of three unapplied versions needs reconciliation before any migration file is renamed or edited.

Anonymous projected shadow:77 existing streamers,61 canonical active partners. G-only projection matches; R-only and R-then-G produce8 false positives. R's inactive-membership constraint is incompatible with G's16 proposed inactive tuples without deactivated_at. These are projections, not post-migration/runtime parity proof. Correct the combined path and require actual zero-mismatch runtime evidence before cutover.
