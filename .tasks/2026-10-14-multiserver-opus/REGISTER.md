# Register (max 30 lines)

Round 2 (Opus r2, 2026-10-15): Main = LIVE 408557c6a (D11-D13, SOC-EN, SEC-NPM, H1). Deployed via ops/twitch-release-build + deploy-twitch-release, --pruefen exit 0, #1060 closed. Report to main orch f52add9e-54d7-4bc7-9351-2bb88580787b. Round 1 thread 44b13b55 stopped, do not resume.

| Pkg | Agent | Worktree | Branch | HEAD | Status |
|---|---|---|---|---|---|
| D2-D10, D-core, D3a/b | done | - | deleted | - | merged, live 1a46231ee |
| O, O2, W1-W3 | done | - | deleted | - | merged, live; #1062, #1068 closed |
| G0 schema | done | - | kept feat/opus-g0-shadow-schema-20261014 | b3c7b5dc8 | merged 3fb21fee4, reverted 47f3e26ab |
| D12 + D13 | done | - | deleted | - | merged 3d96fe282, gate ALLOW |
| SEC-NPM source-map-js | done | - | deleted | - | merged f31a1a57b |
| D11 Verwaltung | done | - | deleted | - | merged 83389fa3f, gate ALLOW (nit: EN overlay previews vs DE OBS output) |
| SOC-EN + approval legend | done | - | deleted | - | merged e4849ebd6, gate ALLOW |
| H1 hub (per-bucket serialization) | done | - | deleted | df58666d6 | merged 408557c6a, gate ALLOW (sol), 73 tests; new crate, not deployed as service yet |
| SEC rustls | done | - | - | - | main clean; feat/tiktok-dialog-presets deleted (patch-identical 091794583) |
| TEST | done | - | - | - | npm test green (480+20, 0 fail) |
| 390px check | done | - | - | - | no overflow DE/EN on demo tabs, /twitch/, /twitch/demo, /twitch/social-media, dead-lock.de |

| DB rotation (user ok 15.10, Opus) | coder opus | ttb-opus-dbrot-20261015 | ops/opus-db-rotation-20261015 | - | rotated 10:31, backup+restore proven; consumers+decrypt verified, PREV+backup deleted on user ok |
| H2 hub rollout (user ok) | sol | ttb-opus-h2-20261015 | feat/opus-h2-hub-rollout-20261015 | a0cacfcd0 | LIVE 909d8f362, unit enabled, auth 200, #1061 closed |
| G1 guilds | opus (sol ended) | ttb-opus-g1-20261015 | feat/opus-g1-guilds-20261015 | - | gate + Rust verify running |
| K1 AI switch | opus (sol ended) | ttb-opus-k1-20261015 | feat/opus-k1-ai-switch-20261015 | - | Rust verify, commit, gate running |
| C1 English texts #1067 | sol | ttb-opus-c1-20261015 | feat/opus-c1-english-texts-20261015 | - | running, base G1; WIP pushed, no new agent (user 15.10) |
| R1 raids by language #1065 | opus (sol ended) | ttb-opus-r1-20261015 | feat/opus-r1-raids-lang-20261015 | - | running, base G1; WIP pushed, no new agent (user 15.10) |

Blocked:
- L #1069: Fireworks HTTP 412 (account suspended), billing = user/money decision.
- C #1067, A #1064, R #1065 depend on G. Sol subscription ends 2026-10-15 evening.
- Canonical checkout /home/nathanael/repos/Deadlock-Twitch-Bot is on a stale HEAD with rustls 0.23.45; push-gate scans from there fail. Push from worktrees on main.
Sol subscription ended 15.10 afternoon. User 15.10: let running agents finish, start no new agents. C1 WIP 8bf317361 pushed, waits for restart.
