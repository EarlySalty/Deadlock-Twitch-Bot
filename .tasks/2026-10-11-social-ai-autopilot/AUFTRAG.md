# Social Media Auto-Pilot, AI Editor and Dashboard (2026-10-11)

Full spec: `BRIEF.md` (user brief, binding). Existing system map: `INVENTORY-2026-10-07.md` (verify lines against current main before relying on them).
Repo: Deadlock-Twitch-Bot. Base: origin/main `3862b764a`. Main orchestrator: this Claude Opus session (intent thread, see REGISTER.md).

## Goal

Extend the existing Social Media Manager (`tb-social-media`, `tb-dashboard-api/handlers/social_media*.rs`, `bot/dashboard_v2` Social pages, `bot/admin_dashboard`) so that every acceptance criterion in BRIEF.md section 32 can be demonstrated live on https://deutsche-deadlock-community.de/twitch/social-media and https://admin.deutsche-deadlock-community.de/twitch/admin. Extend, do not rebuild. Static pipeline stays the default and must keep working unchanged at every merge.

First milestone (highest priority): manual end-to-end AI processing of one real Deadlock Twitch clip (BRIEF 17, 32 "Manual AI Workflow" 1 to 7).

## Areas and write ownership

| Pkg | Area | Owns (write) | Brief sections |
|---|---|---|---|
| P1 | Auto-Pilot, clip pool, scheduling, publishing, retention, notifications | `tb-social-media` scheduler/queue/approval/upload/retention/clip import modules and new modules for them; new handler file `social_media_autopilot.rs`; migrations prefixed `2026101110*`; new frontend component files under `components/socialmedia/autopilot/` | 3 (toggle storage), 4, 5, 6, 7, 8, 9, 24, 25, 26 (queue side), 28 |
| P2 | AI editor pipeline, feedback, learning, costs, admin AI pages | new module tree `tb-social-media/src/ai_editor/`; section `[social_ai]` of the one main bot TOML (`bot.toml`), editable under admin Betriebseinstellungen; new handler file `social_media_ai.rs`; admin handlers for skill history and budgets; migrations prefixed `2026101120*`; new frontend files under `components/socialmedia/ai/`; new admin_dashboard pages for skill diff/rollback, budgets, usage | 10 to 23, 26 (AI side), 27 |
| P3 | Dashboard redesign and platform status | existing `pages/SocialMedia*.tsx`, shell/navigation, `studio.css`, existing socialmedia components, generalized platform status (replacing TikTok-only check), mounting P1/P2 components | 29, 9 "Generalized Platform Status", 18 layout shell |

Shared files with trivial edits only (module declarations, route registration, worker wiring): `tb-social-media/src/lib.rs`, `tb-dashboard-api/src/lib.rs`, `rust/bin/tb-bot/src/main.rs`, `bot/dashboard_v2/src/api/socialMedia.ts` (append-only new functions; P3 may restructure only after P1/P2 merged). Keep these edits minimal and isolated so merges stay mechanical.

## Shared contracts

C1 Per-user settings (owner P1, lands first as P1 milestone M0, pushed early):
table `social_media_autopilot_settings` keyed by `twitch_user_id`, accessor module `tb_social_media::autopilot_settings` with `load(pool, twitch_user_id) -> AutopilotSettings` returning defaults when no row exists. Fields at least: `enabled` (bool, false), `paused` (bool), `ai_editing_enabled` (false), `publish_mode` (`review`|`full_auto`, review), `lead_time_hours` (24), `timezone` (IANA string), `schedule_slots` (weekday + local time list, one shared schedule), `target_platforms`, `clip_date_filter` (all | year | quarter | range | before | after), `clip_sort` (`oldest_first`|`newest_first`), `multi_output_enabled` (false), `experimental_ai_layout` (false), `publish_language` (`de`|`en`|`auto`), notification preferences. Per-user only; nothing per-user in the TOML config.

C2 Produced videos (owner P1 schema + module `tb_social_media::produced_video`, part of M0):
table `social_produced_videos`: id, twitch_user_id, clip_id, job_id nullable, mode (`static`|`ai`), version_no, parent_version_id, variant_index, file_path nullable, duration_ms, status (`rendering`|`review`|`approved`|`scheduled`|`published`|`expired`|`rejected`|`failed`), needs_review bool, review_reasons jsonb, metadata jsonb, created_at, expires_at. Per-platform approval and publish state stays in the existing approval/queue tables extended by P1 and must reference the produced video id so publishing uploads exactly that file without re-rendering. P2 inserts AI versions only through this module.

C3 Production queue (owner P1): table `social_production_jobs` (sequential, one running job at a time, user-ordered, mode fixed at job start). P1's runner calls static rendering itself and, for mode `ai`, calls P2's entry point `tb_social_media::ai_editor::produce(ctx, job) -> Result<ProducedOutcome, AiEditorError>` which returns the produced video id(s), `needs_review` and reasons. AI failure returns the clip to review; never falls back to static. Until P2 lands, P1 keeps `ai` jobs in a clear "AI editor not available" review state.

C4 Manual AI test (owner P2): runs outside the Auto-Pilot queue, works with Auto-Pilot off or paused, never auto-publishes. "Transfer to schedule" calls a P1 function `produced_video::enqueue_for_publishing(pool, produced_video_id, platforms)` that assigns the next free slot without re-rendering.

C5 AI calls go through the bot's central provider (`tb-llm` / `llm_dispatch.rs`). Provider/model/limits come from section `[social_ai]` of the one main bot TOML (`bot.toml`), never hard-coded. Respect the existing per-user external-AI consent switch. Record tokens, provider cost, latency per call; record FFmpeg runtime separately as internal compute, never as billed cost.

Migration rule: never edit applied migrations; only add new ones with your package prefix.

## Acceptance

BRIEF.md section 32, all blocks, demonstrated live after deploy. UI text German with real umlauts. Static workflow verified to make zero gameplay-analysis model calls.

## Out of scope

BRIEF.md section 31.
