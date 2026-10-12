# M1 migration-first deployment proof

## Actual production boundary

Runtime:/opt/deadlock/twitch/releases/536b53c0552d5f4eee5509ae2f38258cca0e8105. This is the originally accepted/frozen origin/main M1 snapshot, deliberately deployed independently of P1 B. Later MediaRuntime mainf6272c5eedb3823b0d3daa76f21a5d1318954519 is available to P1 but NOT claimed live. Minimal deployment control plane is separately gated/remote-verified main759e7bc78f2e39dfa97c158ad8efea7f7e96bf35. No runtime source/artifact mixing, relabeling or second production build.

## Build and execution

- Sole completed builder a49e8302e1cb51343/task b2jcrhplh used the supported CARGO_SLOT_DEPLOY=1 cargo-slot build --release --jobs 2 --locked route. Actual optimized repository Thin-LTO release, exit0 after32m17; normal slot/shared/repository release locks. Nine regular/non-symlink ELFs each carry exact full536, no dirty stamp. All three frontend npm ci/builds passed with indexes/referenced assets. No cache/target copies or source switching.
- Parent inspected installed control-plane targets, current release and hardened migrator, then installed the gated main deploy wrapper and accepted installer under /run/lock/deploy-twitch-release.lock. The installed migrator was byte-identical to tracked source; existing source paths use current, so running it before candidate installation alone would not apply M1.
- Literal migration-FIRST: moved the existing frozen clone into a root-owned/read-only /opt/deadlock/twitch/candidates/536... tree, without copying/rebuilding. Existing /usr/local/libexec/cargo-sqlx ran from that candidate via transient collected deadlock-twitch-m1-migrate-536b53c.service under the shared deploy flock. Same tracked PostgreSQL/local-socket hardening, ProtectHome=yes, ProtectSystem=strict, AF_UNIX, empty capabilities, and explicit RemoveIPC=no. Existing candidate runtime-role script applied in ExecStartPost. Task bba1tmta3 exited0/result success in2.814s; current release/services were not switched by this predeployment step.
- Read-only postmigration aggregates: M1 versions20261011200001/002 successful2/2; consent_rows0; twitchanalysis consent SELECT true, write false; C1 AI-enabled rows0. One initial read-only probe referenced a nonexistent consent column and failed; corrected row-count/effective-rights probe succeeded with no data/consent mutation.
- Installed wrapper task b9rmhe75e deployed exact536 from the candidate, installed/confirmed accepted root-owned STT models outside /home, reapplied migrations/grants, installed/reloaded role-specific peers and passed actual ACL/HBA/peer acceptance/rejection checks. Only twitch-dashboard, twitch-bot and coaching-watch restarted. Exit0. Existing /dev/null passfile warnings are retained, not suppressed; peer/no-password access succeeded.
- Actual deploy-twitch-release --pruefen: current536; bot PID1707161, dashboard1707275, Twitch audit1708206. All executable paths point to exact536, none deleted, ActiveState=active and NRestarts0. No unrelated Caddy, streams, browsers, ai-coach or releases disturbed.

## No-cost functional acceptance

Fresh sole read-only verifier wtem5nsqf / wf_febca48c-5b8 / af06eb2652d22cdfc started genuinely at2026-10-12T08:51:27.962Z/model gpt-6.1-sol. Assigned actual connector/consent/readiness and Moli-only real panel visibility or honest authentication blocker. Result/URL/UI location pending; runtime deployment is proved but full functional/visual acceptance is NOT yet claimed.

No consent write, manual AI job, paid external-AI call, STT inference or publication has occurred in P2. The latest genuine correction forbids the first paid manual own-channel test until new explicit approval. Shared application cap remains unchanged at2000000 micro-USD.
