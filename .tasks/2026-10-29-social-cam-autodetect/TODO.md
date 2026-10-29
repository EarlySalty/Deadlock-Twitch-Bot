# Social AI: automatic cam detection per clip

Branch `feat/social-cam-autodetect`, worktree `/home/nathanael/.worktrees/tb-cam-autodetect`.

## Goal

Before planning, the AI editor sends three stills of each clip to the central editor provider (stage `scene_layout`). With a visible facecam the stored layout gets the detected cam crop and a matching band (stacked) or tile (pip). Without a cam only the game scene is shown. The per-channel option `ai_auto_cam` defaults to on and has a toggle "Cam automatisch erkennen" in the AI options panel.

## Done before deploy

- clippy for tb-social-media and tb-dashboard-api is clean.
- The 14 scene/media unit tests pass.
- Frontend: tsc, eslint, the ai-editor tests (15) and the i18n, studio and contract tests (73 of 74) pass. The one failure is a pre-existing contract test about the `autopilot/tiktok-publishing` route, which this diff does not touch.
- Moli DOM check on the preview build: the toggle renders and is checked by default. The pixel screenshot hangs in Moli's layout engine once the AI path step opens.

## Function proof after deploy

1. Migration applied: `social_media_autopilot_settings.ai_auto_cam` exists with default true.
2. Options API: GET `/twitch/social-media/api/ai/options` returns `auto_cam`. In the UI the toggle is under Social Media, Planen, Schritt 2, KI path, in the options panel.
3. On the next AI job, `llm_usage` has a row with stage `scene_layout`. The job snapshot `publication.auto_cam` carries cam/game/applied, and with applied=true `layout.cam_crop` follows the detected cam.
4. Journal of tb-bot: no `cam detection failed` warnings in series.
