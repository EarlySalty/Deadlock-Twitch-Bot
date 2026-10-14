# Register (max 30 lines)

Main 637da79b1 (+D6, W3, not yet deployed). Deployed 0a695264d (D2, D-core, O, D3a, W1, D3b, D4, D5, W2; G0 reverted). LIVE 0a695264d (deployed 16:3x, --pruefen ok). Caddy dead-lock.de splice applied (backup Caddyfile.before-dead-lock-en-20261014); /twitch English verified. Reporting: directly to the user.

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
| H1 hub fix | round 4 fixer | ttb-opus-h1-20261014 | feat/opus-h1-discord-hub-20261014 | fae3a3599 | BLOCK round 3 (retries, role race, empty 200, ratelimit reservations); round 4 running |
| D6 pages | done | - | deleted | aa9f3dbfd | merged 0a10934d6 |
| O2 website on dead-lock.de | a9de02eb730b9e607 | ttb-opus-o2-20261014 | feat/opus-o2-dead-lock-website-20261014 | 5ac2da8ac | gate running; integrator applies Caddy |
| D7 pages | a8308e50820c0717e | ttb-opus-d7-20261014 | feat/opus-d7-pages-english-20261014 | 50eff72ea | ALLOW but conflicts with D6; worker merging origin/main, re-gate |
| W2 sub-sites | done | - | deleted | 80c078327 | merged 0a695264d (faq, vergleich, onboarding, clips) |
| W3 sub-sites | done | - | deleted | 607a00eb7 | merged 637da79b1 |

Main test fix bacbd6838. Local object store had missing tree 31c7ae2f; repaired via fetch --refetch origin main.
Queue: demo snapshot English insight texts, 390px live overflow check, A, C (after G), K and G re-land (after rotation).
L: parked, blocked by user decision.
