# RD social workspace redesign

## Goal
Replace the sparse social manager layout with a dense, warm, task-oriented layout while preserving the charcoal, restrained gold, grid background, sidebar identity, and existing account/approval/publication behavior.

## Boundaries
Continue the inherited uncommitted state in `feat/social-rd-redesign-20261014`. No new orchestration threads. No paid AI, publishing, real account setting changes, migration of legacy scheduling, or unrelated backend work. The sole Rust response extension exposes existing nullable Twitch view counts for the clip library.

## Acceptance
- At 1440 px, clear page header and five-source status strip, grouped two/three-column work areas, meaningful layout previews, and dense clip cards.
- At 390 px, one-column work areas, reachable mobile navigation and controls, no document overflow.
- Pipeline distinguishes current work, required action and upcoming work. Unavailable/partial sources must not imply an all-clear.
- Auto-Pilot is the primary schedule editor. Effective legacy approval/cadence controls remain in a closed `Bisherige Pipeline` section with an explicit explanation.
- Library presents thumbnails, title, source date, duration, nullable view count, status, left selection, page bulk actions, and bounded pagination. IDs remain hover/details only.
- Settings use switch/segmented affordances. Section headings and controls share the existing visual vocabulary.
- German text uses real umlauts. Existing behavioral contracts, build, formatter and relevant lint pass.
- Preserve AP and AI package behavior when integrating fresh main. Independent review, local gate and verified main release follow COMMON.md.

## Sequence
1. Resume and inspect the inherited state and legacy scheduling call chain.
2. Complete schedule and import surfaces, then all other tabs and shared headings.
3. Batch desktop/mobile Moli inspection and functional fixture checks; fix the collected defects once.
4. Verify frontend and the nullable Rust response against an owned disposable database.
5. Commit/push, integrate current main, preserve AP/AI behavior, independent review and local gate.
6. Merge/deploy only an allowed verified state, collect live proof, clean up and settle.
