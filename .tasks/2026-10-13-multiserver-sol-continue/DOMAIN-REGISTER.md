# Multi-server release register

Owner: T3 c042a18e-f92d-453c-89ad-54138295a8ef, native session 329e3031-ae2f-4a4f-b59c-c701973629e6; coordinator 51e1498f-f4ca-4c6f-8900-6a73abd53c2a.
USER STOP 2026-10-14: hand over all open work; NO new agents, workflows, automatic restarts or queued starts. Await the user's new input.
STEER-1/2/3 remain binding. Direct production DB access prohibited; wrapper migrations permitted. Task records stay off Main.

| Package | Branch or source | SHA | Status | Remaining boundary |
|---|---|---|---|---|
| Main | origin/main | 6cac863acfaa394970ef40b8efc01c441a8ff128 | Remote confirmed at handover | No new Main push during handover |
| G0 | integration/guild-schema-shadow-sol-20261014 | 4658ed2f5a65ec453076312ad9f2f0da8b87a14d | Pushed; exact independent ACCEPT and canonical Sol ALLOW; 11 real PG contracts | NOT Main/live; inactive schema only, no cutover/K/foreign activation; parity awaits approved rotation |
| D core | fix/dashboard-core-stable-keys-sol-r1-20261014 | d8245c65c739ba5b5581adda1a3bdf7be9473cd3 | Pushed; fixes translated React keys; 95 existing plus 4 Moli contracts; fixer gate ALLOW | Independent correction review pending; exclude two .tasks artifacts from future Main delta; NOT Main/live |
| D2 tools | integration/dashboard-tools-english-sol-20261014 | e83718037289aa9f69c9dbaf0fadb30065a3ac9b | Pushed; preserves Main6cac; earlier exact a85 integration independently ACCEPTed | Current integration needs exact carry review/gate; final visual/draft proof incomplete; NOT Main/live |
| D3 components | feat/dashboard-components-english-sol-20261014 | 4da96ee4251447bc3ffbf1336efabbab9432fb4b | Pushed; 27 files, 149968-byte delta; build/lint 0, 60 existing plus 5 formatter tests | No independent review/gate; explicit visual/number-format limitations; NOT Main/live |
| K current WIP | feat/central-ai-guard-current-sol-20261014 | 7772737047639ad96ab72e607e0778ca4a565df5 | Pushed/clean, writer stopped; 13 files including new production policy migration | UNVERIFIED, incomplete callers/editor fixtures; NO build/fmt/clippy/tests/gate; DO NOT MERGE/DEPLOY |
| A correction input | fix/slice-a-ai-attribution-sol-20261014 | 8ca49e045cb60586eca183b3f91ac7dd5826d59a | Read-only salvage input; 32 real guard/store passes, both caller/accounting corrections independently verified | Not Main/live; preserve newer Main social code |
| A aggregate input | fix/slice-a-current-main-sol-20261014 | 2330954563279a96da0e81d2f74ef71c76e807a2 | Read-only draft, original critic BLOCK | Not a release; do not wholesale merge |
| C and R | Preserved source drafts | Pending | No current-Main release | Implementation/integration still open; not an invented external blocker |
| H | #1061 | Pending | Foreign guilds remain inactive | Name-only Hub credentials absent; expected system units absent; authorized test guild access unresolved |
| O | #1062 | Pending | dead-lock.de/streamer observed 308 to primary | Provider callback registration UNVERIFIED, not proven absent; current-Main implementation pending |
| W | #1068 | Pending | Not accepted/live | Legal approval for English privacy copy absent |
| L | Already on Main | 9524ffe1689290004e2d478145afecb8ef9a6163 | Source complete; last board In Progress, issue closed | Positive live provider-accounting proof absent |
| Installed pointer | /opt/deadlock/twitch/current | 3250999ca9e1488c6ed33bf2c9c68956b051a327 | Readlink confirmed 2026-10-14 11:02 CEST; not our handover deployment | Current process SHAs and public assets NOT reverified; do not downgrade |
| Own historic D live proof | D activation release | 50125b0bb67fc2d32b440594a5cd5e95add21c4d | Then Main/Sol ALLOW/wrapper live; 9/9 binary stamps, 6/6 assets, anonymous publishing401 | HISTORICAL, not the current installed pointer; partial D only |

G0 proof: canonical task bz2xnlzsc.output, independent critic a116f55a7255a58b6; remaining current-login denylist raw-import NIT is candidate-specific, not inherited Main.
D core proof: /tmp/ttb-sol-d-core-fix1-gate.log and /tmp/ttb-dashboard-language-identity-Olf1KO/result.json. D2 proof: /tmp/ttb-d2-native-critic-wLk3Oi/. D3 proof: /tmp/ttb-d3-components-evidence-20261014/.
No active native workers after K stop. Do not resume completed critics/writers or create replacements before new user input.
Foreign REGISTER.md and STEER-2/3.md changes untouched. Protected original draft worktrees remain read-only; no cleanup, direct production queries, rotation or real-account mutations.
Full handover, package gaps and evidence: [HANDOVER-PAUSED-20261014-329e3031.md](HANDOVER-PAUSED-20261014-329e3031.md).
Roadmap remains incomplete. Board/PR reconciliation pending; no new Done claim or thread settlement as full roadmap completion.
