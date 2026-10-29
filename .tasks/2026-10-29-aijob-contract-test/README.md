# Package B: AiJobView contract fixtures and non-empty rejected impact

Briefing: `.tasks/2026-10-29-aijob-contract-test/BRIEFING-B.md` (main checkout).

## Changes

- `61ab88bcb` `rejected_proposal()` writes `REJECTED_IMPACT` (bounded by `max_bytes`) instead of an empty impact. Contract test `ai_editor::store::contract_tests` serialises one `AiJobView` per dashboard status into `bot/dashboard_v2/src/components/socialmedia/ai/fixtures/*.json` (check by default, `AIJOB_FIXTURES_WRITE=1` regenerates). `aijob-fixtures.test.ts` decodes every fixture with `decodeAiJob`; wired via `npm run test:ai-editor` into `npm test` (also runs the previously unwired `revision-api.test.ts`).
- `67b2e5978` gate nits: impact text no longer repeats the summary, failed fixture uses `provider_unavailable`.

## Tests

- `cargo-slot test -p tb-social-media` on ionos with a throwaway DB: 595 passed, 5 failed, 0 ignored. Baseline origin/main: 593 passed, the same 5 failed (missing twitch_analytics DB x2, wall-clock schedule edge, missing yt-dlp, tiktok queue time).
- `ai_editor::` filter after the nit fix: 106 passed, 0 failed.
- clippy `-p tb-social-media --all-targets -D warnings` exit 0, `cargo fmt --check` exit 0.
- Dashboard: `npm run build` exit 0; `npm test` main suite 511/516 passed, baseline identical 511/516; `test:ai-editor` 15/15.
- Negative proof: removing `impact` from `failed.json` failed the Rust drift check and the TS decode test; restored, both green.

## Gate

`gate-review --base origin/main`: round 1 ALLOW (claude-opus-5-5, 5 NITs, two fixed), round 2 ALLOW (claude-opus-5-5, NITs only). Pushed `5e2768873..67b2e5978` to main.

## Deploy

Deploy queue ticket 8, run 7, outcome deployed, live 2026-10-29 09:35:27 +01:00.

- `deploy-twitch-release --pruefen` exit 0, current = 67b2e59782089f1f925fe2259ae2125c900a1b5b.
- PIDs: bot 3260342 -> 49709, dashboard 3261356 -> 51212, coaching-watch 3261195 -> 51121; exe not deleted.
- `journalctl -p err` since 09:35 empty for bot and dashboard.
- Anchor "Du kannst es später noch einmal versuchen" in the new tb-bot: 1 hit, in the old 5e2768873 tb-bot: 0.

## Live data

Read-only query on `social_ai_jobs`: jobs 127, 129, 130, 131, 132 are failed revisions with a proposal whose impact is empty (legacy rows written before this change); 128 is in review. `decodeAiJob` accepts the empty legacy impact (`revision-api.test.ts`). The route `/twitch/social-media/api/ai/jobs` answers 401 without a session. An authenticated look at the KI-Editor job list needs a user session (`sichtpruefung/anmelden.mjs`, user logs in), which an agent may not create.
