[Orchestrator] Coordinator decisions, binding from now on. Throughput is far too low (57 workflows, about 45 BLOCK verdicts, only two inactive merges in 9 hours). Keep quality, cut ceremony.

1. Baseline rule: a finding that also exists unchanged on current origin/main (result_unit_err clippy, existing formatter diffs, pre-existing failing tests, translation-coverage failure, Moli layout stall on one page) is NOT a blocker. Record it once in TODO.md and move on. Reviewers judge only the delta against main.
2. Workspace-internal signatures: tb-internal-api, tb-raid and the other crates in this Cargo workspace have no external consumers. Changing a pub function signature is allowed when every caller in the workspace is updated in the same change and builds. This is not an API break in the sense of the rules. Do not spend more workflows researching hypothetical external consumers. Same for the typed denylist error.
3. Release order in vertical slices, each one gated, merged, deployed and live-checked before the next starts:
   a) D dashboard English activation plus K central AI guard fix plus remaining main-R5 hygiene. Needs no external credentials.
   b) G schema with new migration identities and runtime membership plus per-guild roles, inactive for foreign guilds until H exists. Shadow parity runs after the DB password rotation (user decision pending); until then build and review it, and stage the merge.
   c) C English bot texts and R raids by language.
   d) H, O, W, A as soon as the user supplies Hub credentials and redirect registration; prepare them fully now so only config is missing.
4. Max two review rounds per slice. After the second round, only correctness, security or data-loss findings block; everything else goes to TODO.md.
5. Fewer, larger workers: one writer per slice with clear file ownership instead of many micro-workflows. Only split when files genuinely conflict.
6. Moli: if one page stalls, check that page at a different viewport or skip that single capture with a note. Do not diagnose Moli itself any further.
7. Production DB stays read-locked for you until the user approves rotation. Do not work around that.

Report at each slice merge and live check only.
