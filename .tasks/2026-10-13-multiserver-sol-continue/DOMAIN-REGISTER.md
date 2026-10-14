# Multi-server release register

Owner: 329e3031-ae2f-4a4f-b59c-c701973629e6. Task records stay off main.
Binding: STEER-1/2/3 and ABLAUF.md “Durchsatz statt Zeremonie”.
One Sol writer and one critic per slice, then local single-Sol gate; at most four workflows.
Unchanged main findings do not block; workspace-internal signatures may change with all callers updated.
STEER-3: direct production DB access stays prohibited; verified-main deployment through /usr/local/bin/deploy-twitch-release, including service-identity migrations, is authorized.

| Package | Branch or source | SHA | Status | Blocker |
| Main | origin/main | 29011c43b481496741e27a9bf271388a42eb6d4c | Newer LA and social metadata/revision fixes preserved | No overlap with four D activation files |
| A: D English, central K, R5 | fix/slice-a-current-main-sol-20261014 | 2330954563279a96da0e81d2f74ef71c76e807a2 | Pushed; scoped check/clippy/build 0; 49 Rust and 39 Node passes, 0 ignored; remaining semantic review complete | Critic BLOCK: two caller/accounting bugs plus K policy installation only in test SQL; live schema unverified |
| A correction | fix/slice-a-ai-attribution-sol-20261014 | 8ca49e045cb60586eca183b3f91ac7dd5826d59a | Pushed/clean; scoped compile/fmt 0, 32 real guard/store passes and 0 ignored; same critic verified both fixes, no new C/S/D | K schema prerequisite remains; clippy hit unchanged Main findings, lib retry slot-blocked |
| D integration candidate | integration/dashboard-english-activation-sol-20261014 | 767e033e5292068c143968b5af4315b41a02d4e6 | Four-file delta on Main 290; pushed; critic ACCEPT and canonical single-Sol gate ALLOW; three frontend builds 0 | Release b3yw2pryy waits at wrapper global Legacy lock in own reused clone946; no Main push |
| A frozen input | feat/slice-a-english-ai-sol-20261014 | tree 40867b4537949f97981ae8d11413bf2b4d168087 | Preserved read-only; earlier workflow critic incomplete and not resumable | Replaced once by native critic, no specialist cascade |
| D frontend | db0 plus F15 checkpoint | 71e394d1a6e94bc5b43378376bfa4580be88af5b | Salvaged into A input | Broad English coverage not yet merged |
| D small activation | feat/dashboard-english-activation-sol-20261014 | c450ac0e712e2a1ff55363c23906f6d7ef1ce173 | Pushed/clean; build, preview and scoped lint 0; 54 passes/0 ignored, 4 compiled locale cases; 4 DE/EN images viewed | Partial D: unbound texts remain German; exact integration accepted, live proof pending |
| D publishing panel | integration/dashboard-publishing-language-sol-20261014 | d21e72c6a82949ee746758848c42f4a6dc4b80be | Independent critic, local Sol ALLOW, merged and deployed | Authenticated/provider publication not exercised |
| K fixtures | fix/multiserver-sol-k-complete-fixtures-r3-20261014 | 63213dcb3da789cadc8b3da4d1da3cfdc336f2bc | Scoped source accepted, A salvage input | Not full K acceptance |
| R6 and F3 | Stopped draft worktrees | 9a3ad772ecd9f8e69142272c83df45d43f96221d | Inputs preserved and consolidated into A | No separate micro-branch reviews |
| B: G schema | schema48 plus fixture156 | 48cc89162e15b2ebf57a6589555e3ac628c93f09 | Prepared source, not merged | DB rotation, parity 78/62/61/1; foreign guilds inactive until H |
| B: membership | Runtime recovery source | cf7a64d3acf24e8d18eabc2d410273f82e784e18 | Scoped source accepted | Current-main slice and parity pending |
| C: bot English and R | Preserved source drafts | Pending | After A and B | No current-main slice yet |
| H, O, W, A-auth | Preserved source drafts | Pending | After preceding release slices | Hub credentials, redirects, authorized guild and legal approval |
| L | Already on main | 9524ffe1689290004e2d478145afecb8ef9a6163 | Source complete; project In Progress, issue closed | Positive live provider proof absent |
| Installed release | Observed using DB-free wrapper --pruefen | d915c030aa08fa65dd730bad4f4edd8feded36e7 | Later external deploy observed: dashboard PID 2642391, bot 2642229, watch 2643551; all active, NRestarts 0, matching release paths | Recheck before deploy; preserve newer Main and choose only actually affected restart targets |

D21 deployment evidence: /tmp/ttb-d21-live-ui-0rxVio/evidence.json; DE/EN at 1440/390 using then-deployed HTML/JS/CSS and synthetic API data, 0 live API requests or mutations. Desktop images viewed; mobile shows only panel heading below existing shell. Six then-advertised assets byte-matched D21; real anonymous publish GET returned 401.
MERGEPROTOKOLL[MS-1]: 9 Git-Schritte einzeln | Anläufe: 2 | Gate: ALLOW. Canonical origin/main/HEAD review fixed initial branch binding; deploy wrapper verification passed, 9/9 embedded binary revisions matched.
Progress means main merge; report only merges, live proof and concrete blockers. Baseline findings go in final report, no TODO file. Original worktrees stay read-only; no other sessions, model fallback, credential rotation or real-account changes.
