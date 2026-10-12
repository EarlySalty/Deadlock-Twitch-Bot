# Priority 1: remove the Social AI YAML advisory

Fresh sole implementation worker. Parent78e43a4b-d7c4-4910-adb3-26be78165791 is the integrator; main orchestrator eddfce29-0b2a-4f47-931f-8b5387672f94 owns other packages. No extra agents/workflows/T3 threads, user questions, leaf reviews or deploy. worker_gross resolved sol. Inherit the native model, never Sonnet. Do not resume any stopped/completed actor.

## Goal and acceptance

User's highest-priority request: main759e7bc78f2e39dfa97c158ad8efea7f7e96bf35 fails `cargo deny check advisories` exit1, RUSTSEC-2024-0449 via unsafe-libyaml0.2.11/serde_yaml0.9.34. Remove the cause, no deny exception. Replace serde_yaml with the existing locked TOML dependency. Convert ops/social-ai/editor.yaml and pricing-v1.yaml to editor.toml and pricing-v1.toml. Update tb-llm/social_editor/config.rs, tb-social-media/ai_editor/runtime_config.rs, affected tests, release packaging/deploy paths and source docs naming those files. Unknown keys must still be rejected with actual deny_unknown_fields regression proof. Preserve every configuration value, model/rate/timeout/resource/root/consent/cap default and contract, including the fixed shared initial2000000 micro-USD cap. This is a format/dependency repair, not a product change.

Acceptance: `cargo deny check advisories bans sources` exit0 with Rust1.97.1 tools first in PATH; relevant affected crate and packaging/deploy tests, matching formatter/build/owned lint; exact normally published clean feature SHA. Parent runs the canonical gate on its actual sending branch, then publishes main immediately after ALLOW and reports the SHA. No application deployment requested here; P1 still owns next production release.

## Owned starting point and paths

Only write /home/nathanael/.worktrees/tb-social-ai-toml-advisory-20261012, branch fix/social-ai-toml-advisory-20261012, clean creation759e7bc78f2e39dfa97c158ad8efea7f7e96bf35. Fetch/read actual main at each check boundary; preserve source freeze under Cargo and adopt later main safely. Do not mutate the canonical checkout or other trees.

Own minimal Cargo manifests/lock changes removing this dependency, the two renamed nonsecret config files, both loaders, exact affected fixtures/tests, exact release installer/deploy-verifier packaging paths and source documentation references. Source `.tasks` must NOT enter your commit. Do not edit applied migrations or unrelated production code. Check all tracked consumers, including test fixture serialization and legacy release packaging behavior, rather than replacing only literal top-level paths. Do not relax strict serde validation or grant/token boundaries. No fallback YAML parser, deny ignore, model/pricing changes or new hash/recovery manifest.

M2 w013duqfb was explicitly stopped for this priority shift; preserve its existing warm branch/worktree read-only. M3 learning and M4 usage/limits remain separate active workers in their own trees. Their existing work is not yours. Shared deploy packaging/verifier changes necessary for this advisory now belong exclusively to you; do not touch their runtime-role/policy/domain implementation. M3 must use TOML for any new learning configuration and not reintroduce serde_yaml. No direct peer messages/resumption.

## References and verification

Follow the existing global rules and mandatory /home/nathanael/Documents/claude-config/orchestrierung/ABLAUF.md. Graphify/code-suche first, then targeted source reading. Parent already queried the graph and found serde_yaml in tb-llm/Cargo.toml:33 and tb-social-media/Cargo.toml:12; install-twitch-release.sh:161-162 and test_deploy_twitch_pruefen.py:719,733,744 name the old packaged paths. Those are starting facts, not complete inventory. Query roles through the helper, not a reconstructed model table.

Invoke rolle-test-waechter before tests/green proof and relevant existing build/deploy skills. Verify actual installed Rust1.97.1 toolchain path without printing environments. Use cargo-slot --release --jobs2 --locked, shared sccache/central artifacts/default ionos route. NO CARGO_SLOT_REMOTE=0, competing production build, target/cache copies, profile overrides, lock bypass or source edits under an active Cargo command. Report effective verify/release profile and remote/local route faithfully. This repair has critical compiler priority over M2/M3/M4; only use normal locks/wrappers.

Read /home/nathanael/Documents/claude-config/wissen/ionos-builds.md8994822 for every routing problem. PostgreSQL16/initdb is installed on ionos and today's local markers were cleared. Self-repair listed cases using the documented sudo path, same failing cargo-remote --try command and same workspace proof before clearing only the own documented marker. Keep no-main-cluster/disabled PostgreSQL and foreign jobs/services. DB-free checks have no DB env; use only filtered affected DB tests and the exact confirmed throwaway recipe if needed. No full DB suite each fix round or production DSNs.

Relevant existing tests must pass. Add only concrete format/strictness/packaged-contract regressions, not broad new suites. No repeat of unrelated pool/frontend suites. No frontend behavior change or browser test needed unless you actually modify UI. Main lint failures outside owned paths stay explicit, never suppressed or repaired off-scope. A blocker needs exact tool, command, exit code and check/task ID. Return actual commands/results/counts/evidence paths and unknown/skipped cases, not predicted green proof.

## Safety and handover

Rust production/Postgres, no new code comments/production unwrap/unsafe. English internal docs/commits, German product text, no em dashes. No paid/provider/STT user-data calls, production DB/consent/config/grant/service/model writes, migrations, deployment or publication. Runtime536 remains untouched. Never read/output secrets/token/session files/unit Environment/raw DB rows/process arguments/environments. pid/comm only; no ps args, /proc cmdline or pgrep -f. Do not read/copy the excluded reviewer log. No browser tokens or persistent profiles. Preserve streams/Caddy/personal browsers/ai-coach/foreign resources. sudo -n only if necessary; never ask for passwords.

Stage only your verified owned source, normal hooks, commit and push the feature branch promptly. No force push, hook bypass, `.tasks`, main merge or main push. End commit message exactly with Co-Authored-By: Claude Code <noreply@anthropic.com>. Return exact clean published SHA/branch, changed paths, actual dependency scan/crate/strict-keys/packaging checks and precise remaining failures. Do not stop at a plan.

BRIEFING[WB-1]: Pflichtteile5/5 | Timer20 min | Worktree:/home/nathanael/.worktrees/tb-social-ai-toml-advisory-20261012
