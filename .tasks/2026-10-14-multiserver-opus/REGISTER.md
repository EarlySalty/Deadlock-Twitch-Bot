# Register (max 30 lines)

Main = LIVE 5f1dc5bd2 (D2, D-core, O, O2, D3a, D3b, D4-D9, W1-W3; G0 reverted), --pruefen ok, demo EN insights verified. Caddy dead-lock.de website splice applied (backup Caddyfile.before-dead-lock-website-20261014). #1062, #1068 closed. Reporting: directly to the user.

| Pkg | Agent | Worktree | Branch | HEAD | Status |
|---|---|---|---|---|---|
| D2 tools | done | - | deleted | f5355daab | merged 1f9ba9520 |
| D-core | done | - | deleted | 180f7aa78 | merged 36b59fdf6 |
| O | done | - | deleted | 40c7b8805 | merged f61f6e0cf; live, Caddy splice applied |
| D3a cards | done | - | deleted | 9a0cd62da | merged d58b58b60 |
| G0 schema | done | - | kept feat/opus-g0-shadow-schema-20261014 | b3c7b5dc8 | merged 3fb21fee4, reverted 47f3e26ab: prod parity unverifiable before rotation |
| W1 landing | done | - | deleted | 6e060d9ec | merged 84da6947d |
| D3b charts | done | - | deleted | 213cebc63 | merged 8d9124a38 |
| D4 pages | done | - | deleted | 475d59525 | merged 27376adcc |
| D5 Community | done | - | deleted | 8f0306806 | merged 8677271c1 |
| H1 hub fix | blocked | ttb-opus-h1-20261014 | feat/opus-h1-discord-hub-20261014 | f97e73bfd (local) | BLOCKER after 5 rounds: ratelimit.rs:195 stale-response misclassification; proposal per-bucket serialization, awaiting user |
| D6 pages | done | - | deleted | aa9f3dbfd | merged 0a10934d6 |
| O2 website on dead-lock.de | done | - | deleted | 5221e8b3b | merged, live 0a2ce1e11, Caddy applied |
| D7 pages | done | - | deleted | f9c0f1303 | merged 1775f131c (in 0a2ce1e11) |
| W2 sub-sites | done | - | deleted | 80c078327 | merged 0a695264d (faq, vergleich, onboarding, clips) |
| W3 sub-sites | done | - | deleted | 607a00eb7 | merged 637da79b1 |
| D8 AIAnalysis, CategoryCollector | done | - | deleted | b28192cb7 | merged 5f1dc5bd2 |
| D9 demo insights | done | - | deleted | 667a3cef7 | merged e98e607dc |
| D10 Experimental, chat analytics | done | - | deleted | 18b9e034f | merged 1a46231ee, release building |
| D11-D13 remaining EN strings | sol x3 | ttb-opus-d1{1,2,3}-20261014 | feat/opus-d1{1,2,3}-pages-english-20261014 | - | building (verwaltung 75, uplink/social 46, trial/onboarding 53) |

Main test fix bacbd6838. Local object store had missing tree 31c7ae2f; repaired via fetch --refetch origin main.
Queue: 390px live overflow check, A, C (after G), K and G re-land (after rotation).
L: parked, blocked by user decision.
