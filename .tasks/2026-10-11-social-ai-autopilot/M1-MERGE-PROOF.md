# M1 merge proof

- Candidate:810b5e76e63df33e5fe12d9bbe0ac7fea52cdfff, rebased onto current main0085b717f3285fbf348ab7114fca0336f342173a. Original741ba64a0 is preserved for P1's runner composition.
- Import-pool code/schema are untouched. api-pool-tests were dropped from the pre-gate dependency per the genuine orchestrator instruction; they passed on main0085. Shared access-helper change is visibility only. Owned-lint101 is inherited, not green and not repaired as unrelated work.
- Prior affected build/test results remain in REGISTER.md and M1-GATE-FIX-R1-HANDOVER.md. Fresh expiry/retry and media-runtime checks are pending; no new pass is inferred.
- Configured gate bnab1yoxk returned Sol BLOCK: expired AI allocations break advertised retries. Fresh isolated fixer wdc66u3v8 / wf_5074ec17-c19 started immediately. The genuine P1/P2 decision adds provider-independent MediaRuntime in the SAME round via disjoint worker wel02nyfo / wf_80dcefe4-014, actual Sol start2026-10-12T05:33:39.192Z. Parent combines both checked patches and re-gates directly; P1 owns startup/fallback, no duplicate defaults or provider dependency for static production. The over-about-one-hour follow-up exception remains only if actually measured.
- Unchanged UI screenshot pair: evidence/m1-gate-fix-r1-20261012/newest-unavailable-desktop.png (1440) and newest-unavailable-mobile.png (390). Synthetic warning/layout only, not playback or live authentication.
- Main: not yet published. Live: no. Production deploy is batched after M1 plus P1 B, with P3 if ready.

MERGEPROTOKOLL[MS-1]: 5 Git-Schritte einzeln | Anläufe: 0 | Gate: actual expiry/retry BLOCK; fresh fixer active, no main publication.
