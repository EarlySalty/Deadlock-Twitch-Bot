# Social-Media-Studio: one pipeline (2026-10-15)

## Goal (user, 2026-10-15)
One system, not two or three. Chronological, clearly structured, with and without AI, with and without human review.
Today two systems run side by side in the same studio:
- A, legacy clip pipeline: `twitch_clips_social_media` (+ approval, category, `social_media_platform_schedule` per platform with "times, comma separated", `social_media_streamer_settings` mode full_auto, TikTok per-post options, metadata per clip/platform). Owns tab Freigabe, Kategorien, Zeitplan, TikTok dialog. Only system that ever uploaded (YouTube 8, TikTok 0, Instagram 0).
- B, Auto-Pilot: `social_media_autopilot_settings` (slots, target platforms, publish_mode), `social_clip_pool`, `social_production_jobs`, `social_produced_videos`, AI editor (`social_ai_*`). Owns tab Planen, Termine, Clip-Vorrat, KI-Videoschnitt, Veröffentlichungsmodus. Live since 7e15101b2 but never produced (0 slots, 0 targets).

## Target model (decided by user)
One pipeline, one clip source (the pool), one schedule, one mode per channel:
1. Nur hochladen: clip as is, own title, by schedule.
2. Hochladen + KI-Texte: title, description, hashtags generated; shared across platforms or separate per platform (user toggle).
3. KI-Videoschnitt: AI edits, review before publishing.
Orthogonal: human review on/off (Freigabe step exists only when review is on).
Schedule: several times per weekday (e.g. Mo 08:00 and 22:00), either shared for all platforms or per platform. No timezone field in Planen (lives in Verwaltung).
Metadata: user can choose "same for all platforms" or "per platform"; reusable defaults (description/hashtags) for all platforms, not only TikTok.
TikTok per-post consent stays legally required but must be part of the flow (set once in the review step, defaults prefilled), never a silent hold.
Keep the clip menu ("..." button) with its actions KI bearbeiten, Layout, Titel/Beschreibung/Hashtags, Vorschau, Contest, Original. The user likes it.
Global AI switch `external_llm_consent` needs an admin UI (set to true in prod on 2026-10-15 by the orchestrator).

## Acceptance
- Exactly one schedule, one clip list, one mode selector visible; no duplicate concepts (Termin vs Zeitplan, Pool vs Freigabe list).
- Each of the 3 modes x review on/off can be tested end-to-end with a throwaway clip, one real upload per connected platform, verified on the platform.
- Existing data of A (approved clips, schedules, metadata, TikTok options) migrated or explicitly retired, no loss.

## Packages
| Pkg | Branch | Owner | Status |
|---|---|---|---|
| U0 plan | feat/social-unify-20261015 | sol-worker | done 9c89249d0 |
| K1 admin AI switch + metadata shared/per-platform toggle | feat/social-ai-consent-meta-20261015 | sol-worker | running |

## User decisions on PLAN.md questions (2026-10-15)
- Q1: A. Automatic AI-edited output may publish without review when review is off; quality/legal holds stay; manually requested revisions keep review.
- Q2: A. Upload-only and AI-text modes keep the original footage, only required technical conversion; layout changes only via the explicit "..." menu action.
- Q3: Both. The system converts legacy cadence into a weekday/time pattern automatically, and the user can always edit days and times manually in the one planning editor. Committed posts stay.
- Q4: A. Automatic intake Deadlock-only, manual uploads allowed; non-Deadlock legacy rows visible but not auto-posted.
- Q5: User setting per channel: "same clip on all selected platforms (next free slot each)" or "each platform draws its own clips". Default: same clip on all platforms.
- Q6: Veto window stays as a review policy: the clip posts automatically at the deadline unless the user stops it before (user responsibility). Review policy therefore has three values: off, required, veto until deadline. Legacy veto channels map to veto until deadline.
