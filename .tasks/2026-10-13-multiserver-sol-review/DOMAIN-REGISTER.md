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

Read-only migration/shadow proof `wf_77b32055-f7e` completed without writes. Production guild/shadow tables are absent. Its restricted runtime credential could not read the migration ledger, exit3. Parent subsequently used the existing local postgres-peer migrator access, without credentials or privilege changes: read-only transaction on canonical `twitch_analytics`, port5432, returned 0 rows for versions20261012120000,20261012140000,20261012140100,20261012150000,20261013120000, exit0. These incoming versions are confirmed unapplied there. Fetch fresh origin/main and check version collisions before assigning a replacement migration identity.

Anonymous projected shadow:77 existing streamers,61 canonical active partners. G-only projection matches; R-only and R-then-G produce8 false positives. R's inactive-membership constraint is incompatible with G's16 proposed inactive tuples without deactivated_at. These are projections, not post-migration/runtime parity proof. Correct the combined path and require actual zero-mismatch runtime evidence before cutover.

## Stable assembly and repair ownership

Assembly V2 is clean, committed and pushed at `7a15931f34fa8d0e40e85e5581eceb1504ded2ac`, branch `feat/multiserver-sol-assembly-v2-20261013`. It contains board79a3376a, main643553a and the four existing D follow-ups. Mechanical verification: dashboard Rust check, TypeScript, scoped Rustfmt and ESLint exited0;17 existing tests passed,0ignored. This is not semantic acceptance.

Hub log redaction `5d6b8ade2faed7420272a87f8362a81f242952f7` and optional deployment support `1b21f0f0c844ceb4f349d49c492704898c0601ab` are clean commits pushed to their own feature branches; remote SHAs confirmed. Independent exact-commit review `wf_7e897b16-337` completed. Rust reviewer ACCEPTed the log fix. Python reviewer ACCEPTed the test-tool scope; shell/unit reviewer BLOCKed whitespace-only activation credentials that Rust treats as missing. Fresh isolated fixer/review workflow `wf_1e044105-48c`, task `w48hiz45h`, owns launcher and its tests, without role SQL or production mutation. Original Ops SHA is not accepted for integration.

Accepted log fix was mechanically merged without conflicts into parent-owned `/home/nathanael/.worktrees/ttb-sol-integrate-v2-20261013`, branch `integration/multiserver-sol-verified-20261013`, SHA `85c2df379beeceefcbd87b105b84db6b43ba9181`, and pushed. Its gateway/rest.rs blob is identical to the independently reviewed source. This is a feature integration checkpoint, not combined acceptance, a merge gate or a live release.

PR1077 obsolete no-merge/no-deploy body and missing D/G/C inventory were corrected through the authorized PR maintenance. `gh pr edit` failed on deprecated classic-project GraphQL; GET confirmed no mutation, then REST PATCH succeeded at2026-10-13T06:41:09Z. All seven roadmap PRs remain linked to this T3 thread. No package status was falsely changed to Done.

Stopped core pipeline `wf_1ca91233-373`, task `w9hvonbwc`, after the membership continuation reported changes appearing during its edits. No external actor is established. The workflow was explicitly stopped; targeted agent-stop calls then reported no registered task. Preserve all original dirty UI, membership and Hub-effects worktrees, including the complete typed-denylist draft. Do not resume these writers. Recovery uses fresh writers and fresh worktrees, reconciling drafts read-only instead of overwriting them.

| Workflow | Exclusive repair ownership |
| --- | --- |
| `wf_35a331a5-0aa`, `wr55t9c67` | UI language state and ServerAdmin localization; Hub guild effects; membership service, canonical partner schema/backfill, runtime roles and typed-denylist caller fix |
| `wf_886391d7-8ca`, `w8hd7tgk2` | Dashboard OAuth/onboarding/tenant authorization; central AI runtime guard and contributor provenance |
| `wf_50f6349b-f7b`, `wi00qhpct` | Raid policy; chat/DM language and home advertising; international Caddy/FAQ/pricing; language-aware analytics/discovery |

All writers inherit requested Sol and start from exact assembly7a15931f. Parent alone integrates and deploys. Pipelines independently review each finished branch. No same-file writers across scopes: bot main belongs to Hub until handover; bot raid_oauth_impl belongs to membership for typed error only until handover; Auth owns shared dashboard routing/session handlers; UI owns App and dictionary indices; DB owns existing incoming migrations and new versions13130000/13130100; Auth owns new version13140000. Discovery startup, bot OAuth domain wiring and any further shared edits are serialized after these owners finish.

Static recovery `wf_9cf872ae-854` completed32 agents. Findings were independently verified, not assumed from initial drafts. Refuted claims excluded from fixes: foreign dashboard AI page hiding is unresolved product policy, and stated non-partner recruitment delivery is blocked by the actual ChatApi. All surviving findings are tracked in native journals and exact repair briefs, not duplicated as code structure here. Gate, combined acceptance, main merge, deployment and live mutation remain unperformed.

Fresh read-only L runtime projection from canonical production ledger: last30days `channel_profile` has1755attempts,0successful,0successful-with-missing-usage. Last24hours216attempts, all failed with HTTP412 and none unfinished; latest attempt2026-10-13T03:24:26+00:00. No prompts, identities, request IDs or secret values were selected, no AI call made. This confirms failure recording, not successful actual-token persistence. HTTP412 alone does not establish billing/account cause. Closed issue1069 remains source-verified but positive live usage proof is unavailable under these actual failures. Do not rebuild L or change provider/model/cost without authority.

Remaining-contract read-only scout `wf_e8ee639f-0f3`, task `wnh283t8g`, completed. D has concrete remaining Social AI presentation, historical clip-import dynamic-key/date and expanded static Uplink-help translation gaps. Existing imported language-keyed recommendation/cache work is correct and must not be rebuilt. Advertising scout found actual PromoEngine delivery already assigned to chat owner; dedicated mentor offers/separate public Discord engine were not found, occasion pitches are dormant and DiscordPitchReviewSink is review-only. Do not activate or invent these paths. Internal chat_command localization remains a candidate without an identified production consumer, not a delivered defect.

UI writer completed clean at `b0a94f995012b38874706f1740d582972d271ea8`, own branch pushed by parent, independent UI review pending. Reported66passed/0ignored across scoped checks; one additional existing shell test still fails and must be resolved before final combined checks, not silently omitted. Build/TS/ESLint/i18nCoverage exit0,14Moli screenshots inspected; absent font assets/fallback-font limitation explicitly reported. New D-tail workflow `wf_df372b0f-9b2`, task `w3610wvck`, starts from this handover with separate Social/import presentation and Uplink-help writers. Only Social writer now owns en/index.ts; Uplink writer owns existing en/uplink.ts/help loader/assets, no shared-file overlap.

Language-data writer needs serialized follow-up in `rust/crates/tb-internal-api/src/handlers/market_share.rs`, not yet granted to an active writer. It provides `market_share_series_for_language` and `MarketStreamRow::matches_language`; internal API must use the same language-selected historical/current metrics. Start that adapter only after its source branch handover so checks can use committed helpers. Discovery store can be injected with existing-style constructor wiring and automatic refresh; main.rs stays Hub-owned until handover.

Auth writer reports first check exit0, strict Clippy/test compilation still running, no final handover. Its new confirmed throwaway container `dl-central-test-postgres-50013` cannot start. Parent inspected only state/label keys, not environment/secrets; one targeted retry returned exit1 with OCI/runc `can't get final child's PID from pipe: EOF`. No daemon restart, foreign-container action, permission weakening or production write. Cause is not established. Existing reusable `rust/test-support/postgres.rs::TestPostgres` starts isolated native PostgreSQL16 with a private0700 Unix socket and no credentials/live DB; it is a valid existing test mechanism for serialized fallback, not permission to fake cargo-test-db container confirmation. Local production extension metadata advertises timescaledb2.29.1, pgcrypto1.3 and pg_trgm1.6, but native16 library compatibility is not yet proven. Prefer actual existing native fixtures where supported rather than skipping DB tests or changing protection mechanisms.

## Accepted feature checkpoint and current fix rounds

Launcher round1 `76a41e08165e4ff29df098fa52cecdad67ec6f36` rejects ASCII and Unicode blank activation credentials while preserving valid contents. General and Python reviewers independently ACCEPTed it. Actual matching tests:56passed,0ignored; Bash syntax, ShellCheck and diff-check exit0. Ruff still has15 inherited findings and exit1, no new findings. Parent merged only this accepted Ops source after the accepted log fix into `integration/multiserver-sol-verified-20261013` at `e54185251d7f8e3e74b60b2302aa29623aaad2c8`. The clean local HEAD and exact remote branch SHA were independently rechecked. This is not combined acceptance, ALLOW or production.

UI reviewer BLOCKed `b0a94f995012b38874706f1740d582972d271ea8`: normalized invite save acknowledgements leave the form dirty; failed language-triggered `/me` refetch unmounts cached forms and discards three drafts. Membership reviewer BLOCKed `4c46078549b34ad8af9c8643b48fa600b3a67b21`: committed Guild admission clears global operational exclusions; established German technical-reauth recovery can be blocked by inactive migration membership; offline eligibility and role entitlement still read raw partner status. The original42-line safety draft remains uncommitted and preserved read-only, not covered by earlier54passing tests.

Fresh workflow `wf_e3a0e553-fe3`, task `w7zkuaol6`, owns round1 UI and membership fixes with independent reviewers. UI writer `afd8ed69fc9fdbb72` owns fresh `/home/nathanael/.worktrees/ttb-sol-ui-r1-20261013`, branch `fix/multiserver-sol-ui-r1-20261013`, baseb0a94f, ServerAdmin/forms/tests and its dictionary only. Social still exclusively owns en/index.ts. Membership writer `a23fa1c87a8174580` owns fresh `/home/nathanael/.worktrees/ttb-sol-membership-r1-20261013`, branch `fix/multiserver-sol-membership-r1-20261013`, base4c460785, partner_setup and tests, offline_eligibility and canonical eligibility only in streamer_role_sync. Default-Guild entitlement must require active installed membership in that particular Guild, not any foreign membership. Full guild-scoped role reconciliation/oauth_followups changes remain serialized after handover. No other writer was authorized in these paths.

Chat writer and independent reviewer completed with BLOCK: original `/home/nathanael/.worktrees/ttb-sol-chat-20261013` has nine uncommitted files,94444-byte diff, fingerprint `26512572fbc64725fb00be80ca125f6435fbd834443d4d150918c4cfa43c7778`, no compiler/test proof. Reviewer confirms incompatible i64/u64 home-Guild types, unscoped private transport, untranslated actual rank/stat/error responses and promo overrides, wrong alias recipient locale, rejected legitimate Guild-associated invites and reminders marked sent despite skipped delivery. Fresh workflow `wf_449b73e9-a18`, task `wv51jnv6k`, exclusively owns a new chat-r1 tree from7a15931f plus recipient-only raid_greeting and token_lifecycle_wiring adapters. Old drafts remain intact. Partner_setup remains membership-owned; ModAlerter sections and tb-chat/pipeline.rs remain reserved for serialized work after AI/chat handover.

## Build resource blocker and remaining Discord contracts

Read-only ionos measurements confirm root volume464Gused/464Gtotal,0available,100%. Available RAM11769MiB, so current evidence is disk exhaustion, not RAM exhaustion. Remote sccache18G and cargo-remote workspaces195G. Metadata census:105workspaces,0eligible under the existing three-day cleanup cutoff. A further read-only du measured home372G, var11G and opt3.1G; exit1 means partial access, not a complete capacity accounting. No source, cache, logs, foreign workspace, locks or process was deleted or changed. Guarded cleanup is not currently eligible. Existing local build-slot2/3/4 files were checked with nonblocking read-only flock and all are busy; no compiler was started or lock retained by this diagnosis. Missing/unstarted checks remain unverified, not failed code checks or green tests.

Completed static Discord-effect scout `wf_1f7895c5-35f`, task `wd7ze3x0f`, confirms three remaining production gaps on7a15931f: default-only/global streamer-role reconciliation including oauth_followups; rule-based ModAlerter bypasses foreign Hub/configured alert destinations; private lifecycle/requirements transport defaults to Community without a Guild header. Existing foreign REST role/channel/DM ownership and mentionability checks were inspected and not duplicated. Chat-r1 now owns lifecycle transport; canonical default-role eligibility is membership-r1-owned. Full per-Guild role effects, ModAlerter after AI/chat handover and requirements-only raid_oauth_impl after Auth/membership handover remain pending exclusive assignments. No live Discord mutation or safety bypass was performed.

Auth's original workflow journal has no final writer result or handover. A separately resumed read-only acknowledgement is not a release of ownership, and background checks may still notify. Its own native throwaway PostgreSQL SELECT1 is reported successful. Foreign reconnect uses the existing dashboard Discord onboarding identify/guilds flow with sanitized return on the stored start domain, not an invented Hub initiate/consume API; German broker remains unchanged. Relinking identity must invalidate memberships no longer verified. Auth owns find_partner_for_login in auth/session.rs and must apply canonical active-installed membership plus global restrictions. No source writes/builds/process starts are authorized in message-only continuations.

Language-data writer reports both frontend builds, strict own-file lint and twelve isolated PostgreSQL assertions using SQL extracted from production sources. This is not Rust compilation or suite acceptance. Its first foreign installation historical cutoff still needs independent product review. Internal market-share adapter and shared discovery startup remain pending serialized work. Main, running release and services remain unchanged by this task; final combined acceptance and gate are still unperformed.
