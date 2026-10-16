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

## Carry-over notes for later packages (2026-10-16)

- Merged to main, not deployed: U1, U2, U3 088f24e82, U5 7f1b0678c, U6 5715d4584, U4 35fb3e9bb, U13 00bc11995, U14 cf6c9938c, U7 39b2a20f6.
- U12/U17: veto deadline is written in a separate step after the video (U7); a publisher acting on the deadline must not run before it exists.
- U17: switched channels with review off or veto are still held by the legacy review limit in claim_next.
- U17: held jobs (U5/U6) keep their pool clip and scheduled slot until Process or Skip.
- U6: provider errors go straight to a review hold without retry; held job switched to Upload keeps metadata.pipeline_mode=upload_text.
- U16: ScheduleQueuePanel still renders the old queue card and modeSlot below the U13 editor; remove once U15 review exists.
- U15: KI, Layout and Texte entries in the clip "..." menu are disabled until U15 enables them; "Erstmals ausgewählt" note dropped (no first-selected date in U4 projection).
- U4 nits: source query cost for large owners, raw DB error text to client, broken settings answers 400, counts.automatic includes non-pooled Deadlock clips.

### From U8b (f05c5c9e4, 2026-10-16)
- U10/U17: `apply_tiktok_choice` selects TikTok rows without `produced_video_id IS NULL`; guard it when touching TikTok dispatch.
- U17: legacy queue call can add a second active row next to a pending produced row for manual_upload clips; prod count of produced manual_upload videos was 0 on 2026-10-16, recheck before cutover.
- Rollback: `TextDefaults` denies unknown fields and now saves `title`; binaries older than f05c5c9e4 fail to load such policies.
- U16: no UI field for the `title` text default; frontend `schedule-queue/api.ts` TextDefaults lacks `title`.
- Title-only defaults skip the legacy default template (empty description); untested.

### From U9 (4f9935911, 2026-10-16)
- U10: `valid_tiktok_choice` does not check choices against live creator info; validate `options.validate(&info, duration)` before init or inside `bind_tiktok_on`.
- U10: executor still writes only `tiktok_effect_boundary_pending`; write the specific reason from `TikTokBindingIssue::reason()`.
- U10: dispatcher must send `tiktok_options.caption` from the binding, never the queue row text (`clip_title` fallback differs).
- U11/U15: snapshot `bound` state ignores credential id, music consent and option validity; derive from `TikTokSubject::issue`.
- U15: `PublishReviewPanel.tsx` retry-button exclusion list lacks the new `tiktok_*` hold reasons.
- Cross-origin approval test proves CSRF 403 only, not that the route is mounted.
- Dashboard test noise on main f05c5c9e4: `social_media_pipeline` sources fails with "column produced_video_id already exists"; `autopilot_schedule_authenticated_composition` and `auth_login callback_mit_kontext_cookie_erfolgreich` fail on main too.

### From U10 (3fc208214, 2026-10-16)
- U11: dispatch is off until `with_tiktok_dispatch(true)` in main.rs; before enabling, reconcile `tiktok_processing`, `tiktok_transfer_uncertain`, `tiktok_init_uncertain` to done/failed, otherwise every post triggers the "outcome unclear" Discord alert.
- U11: `tiktok_effect_boundary_pending` is not in `REAPPROVAL_REASONS`; held rows would be stuck.
- U11: pre-effect validation maps file problems (empty file, hash mismatch) to `tiktok_options_invalid`; split file reasons from option reasons.
- U11: music consent not re-checked inside the effect-start transaction; file hashed twice per claim; non-TikTok start update carries a dummy `$8::text IS NULL`.
- U11/U12: `recover_stale` overwrites TikTok reason with `interrupted`; 7-day cleanup deletes files of UNCERTAIN rows; `upload_worker::resolve_tiktok_uploader` should reuse `direct_post_uploader`.
- U17 (decision per "one system"): a clip must never post to TikTok from both a legacy queue row and an autopilot row; the canonical writer wins, legacy writes are rejected or delegated. U11 must not enable dispatch for channels without `social_pipeline_cutover`.

### From U15 (6e011dba4, 2026-10-16)
- Integrator before deploy: U15 set frontend `PIPELINE_DETAIL_CUTOVER` on; verify it only affects channels with `social_pipeline_cutover` or that the new detail is safe for legacy channels.
- Backend gap: publish snapshot knows only review and full-auto, no veto review mode (U12/U17).
- Verify the snapshot actually sends `tiktok_consent` (U11).
- `publishReviewLanguage.browser` subtest 3 stops on a fixture id mismatch (2203 vs 101); the schedule call is not exercised by any test (U18).
- U16: SocialMedia.tsx still mounts old video lists, the AI editor's separate clip picker and `TikTokPostDialog`; ScheduleQueuePanel still shows the old queue card; pass `owner` to `PipelineClips` everywhere.

### From U11 (b8ce1f2a5, 2026-10-16)
- TikTok dispatch is wired on in main.rs but only dispatches for owners with `social_pipeline_cutover` state `unified` (checked in the effect-start transaction); others stay held with re-approvable `tiktok_effect_boundary_pending`. Clip-level fence stops legacy + autopilot double transfer.
- U16 (U15 is merged, so the frontend follow-up moves here): handle `autopilot_processing` and reasons `tiktok_file_invalid`, `tiktok_transfer_active`, `tiktok_already_published`, `tiktok_init_unconfirmed`, `tiktok_inbox_only`, `tiktok_rejected`, status unconfirmed; update the retry exclusion list in PublishReviewPanel.tsx.
- U12: uncertain rows on other platforms keep files indefinitely (no cleanup); file still hashed twice per claim; `upload_worker::resolve_tiktok_uploader` duplicates `direct_post_uploader`; `Capabilities.tiktok_effect_boundary` static.
- notify.rs alerts on held rows only for account and file reasons.
- U17: no owner has a `social_pipeline_cutover` row yet.

### From U12 (0c5567451, 2026-10-16)
- Snapshot now has `settings.review_mode` (review / full_auto / veto_until_deadline); U16 shows it.
- U12b (fix package, parallel to U16): sync review_policy and publish_mode (Q1: review off must auto-publish; today `off` + publish_mode review never publishes); impossible deadline values (month 13) must not break the whole publish tick; promote a veto video whose deadline passes within the same tick instead of silently moving it via schedule_slotless; Instagram "published but no media id" becomes uncertain, not retry.
- Later: warn before grace-period file release of non-TikTok uncertain uploads; `Capabilities.tiktok_effect_boundary` static.

### From U12b (9169c2ca3, 2026-10-16)
- Shared `EFFECTIVE_REVIEW` rule: for channels with `pipeline_mode`, `review_policy` decides (off auto-publishes even with publish_mode review); legacy channels still follow publish_mode. Bad veto deadlines hold only that video (15 min, logged). A veto deadline passing within a tick promotes inline with shared `PROMOTABLE` guards. Instagram missing media id is `PublishUnconfirmed`, not retried.
- U17: UI may still show publish_mode for unified channels; switching back to review revokes auto approvals that may be promoted again; production backlog limit (`production_jobs.rs:221`) still checks `publish_mode = 'full_auto'`; legacy channel with review off and no pipeline_mode still follows publish_mode.
- U17/U18: broken-deadline hold has no user-visible reason; Instagram media_publish 5xx still retried (possible duplicate); old upload worker ends `PublishUnconfirmed` as failed, not uncertain; promotion query loads all veto videos then caps at 20.

### From U16 (4595cfe87, 2026-10-16)
- Studio has five tabs: Heute, Clips, Planen, VOD-Archiv, Einstellungen. Planen = PipelinePlanControls + read-only effective review_mode; Clips = ImportPoolPanel → PipelineClips(owner) + ClipReviewDetail, "..." menu kept, `stage=` survives reload; old view aliases redirect.
- U17 (blocking): legacy-pipeline channels lose the old approval/schedule UI, so U16 must deploy together with or after the cutover.
- U17/U18: failed clips show no `last_error`; `studioActions` retry rows still point to Planen; `source.jobs` lacks `review_reasons`; queue reorder and "next" actions dropped; re-clicking the active stage does not reset PipelineClips filters; EN labels for new review modes/hold reasons unchecked; preview `libraryImport` ignores `initialStage`; VOD "Details & Aktionen" overflows at 390px (pre-existing).
- `tests/socialStudio.browser.test.mjs` not run (launches Chromium); screenshots in /tmp/u16-final/.

### From U17 (423a4257f, 2026-10-16)
- Write barrier: DB trigger rejects new/reopened legacy queue rows and legacy approvals for owners `unified` or `paused`, and a second open row per clip+platform across pipelines; autopilot discards a destination with `legacy_publication_open` while a legacy row is open.
- Operator CLI: `tb-bot --config <abs> --social-cutover --dry-run | --apply | --pause <owner> | --resume <owner> | --resolve <queue_id> --published|--failed`. Cutover pause keeps its own state and restores the streamer's own pause.
- Deploy order: deploy binary (migration safe before cutover) → `--dry-run` → `--apply` → restart bot/worker → check owner bd04cb state and its 58 legacy rows draining. Prod before: 58 live legacy rows (yt pending 25, tt pending 28, tt awaiting approval 5), approvals 151 awaiting / 37 approved over 21 owners, 0 cutover rows.
- Conflict rule: autopilot settings win over legacy; bd04cb would keep "review required" instead of legacy full_auto (user decision pending).
- U18 open: held jobs keep clip+slot (carry-over 2) not addressed; `hold()` autopilot_schedule/mod.rs:759 reads publish_mode; veto-until-deadline owners no longer count against review limit (confirm intent); no German frontend texts for `pipeline_unified`/`pipeline_busy`; owners with no targets become unified with autopilot off; legacy `uncertain` rows invisible in dashboard and unalerted; `--resolve --published` does not set `*_uploaded_at` or clip published state; double-publish race between 30-min stale reclaim and `mark_upload_uncertain`; retry can hold an upload task ~63 s; restoring an autopilot revision with an open legacy row returns raw 23505; barrier takes no lock when owner unresolvable; `--apply` rebuilds the full plan per owner; two doc comments misplaced in upload_worker.rs and clip_queue.rs.

## Finish run (2026-10-16 evening)
- 20:30 U17b worker died; fresh Opus 5.5 (medium) worker resumed in /home/nathanael/.worktrees/tb-social-u17b from the 19 dirty files. Scope includes the user decision: every legacy channel cuts over as "Nur hochladen" keeping its legacy approval setting (full_auto -> review off, approval -> required, veto -> veto until deadline); explicit new Planen choices win; bd04cb = upload-only + review off.
- Next: U18 six-cell harness (PLAN 7), deploy origin/main, cutover --dry-run table to user, --apply, restart, live check of 58 legacy rows, real upload matrix only after explicit user go.

### From U17b (180c02ddf, 2026-10-16)
- Double publish: stale reclaim sends rows with `effect_started_at` to `uncertain` (`publish_unconfirmed`) instead of `pending`; YouTube/Instagram claim the effect atomically before the platform call; requeue of a claimed row returns `pipeline_busy`; failed/other writes never overwrite `uncertain`/`completed`.
- Legacy `uncertain` rows: shown in the dashboard queue with a reason and alerted once per owner per day (`legacy_uncertain:<day>`, kind publication_failed).
- `--resolve <id> --published` sets `*_uploaded_at` and refreshes the clip's published state.
- `hold()` review limit and production backlog limit both use `EFFECTIVE_REVIEW`; veto-until-deadline owners do not count against the review limit (unchanged intent, now explicit).
- Held jobs: slots are released (reservation trigger plus `release_unfinished_slots_on` on review finish and unsuitable flag). The pool clip intentionally stays bound to the held job until Process or Skip and is visible in the review stage.
- Revision restore with an open legacy row: parent stays parked, API answers 409 `pipeline_busy`. Migration `20261016200000_social_pipeline_barrier_clip_lock.sql`: barrier always takes a per-clip advisory lock (`social_clip_publication:<clip>`) and raises a WARNING when the owner is unresolvable instead of skipping silently.
- Texts: `pipeline_unified`, `pipeline_busy` and the new reasons in DE (labels.ts) and EN (dictionary.ts, en/social.ts).
- Cutover: every legacy channel becomes `upload`; legacy approval maps full_auto -> off, manual -> required, veto_window -> veto_until_deadline; a saved Planen choice (policy_revision > 0) wins. `--dry-run` prints one line per channel before the JSON report:
  `owner=<twitch_user_id> login=<login|-> mode=<pipeline_mode> review=<review_policy> source=<legacy-mapped|explicit-new|unknown> open_legacy_rows=<n>`
- Integrator before `--apply`: bd04cb's dry-run line must read `mode=upload review=off source=legacy-mapped`. If it shows `source=explicit-new`, the channel has a saved Planen choice and the user must decide (not checkable without prod DB access).
- Test harness defect (also on origin/main): `upload_worker::tests` race in parallel because `social_media_video_production` is a database-wide advisory lock shared across test schemas; all 16 pass with `--test-threads=1`.
- Still U18: Instagram 5xx retry, the 63 s retry hold, `--apply` plan rebuild cost per owner, misplaced doc comments in upload_worker.rs/clip_queue.rs; owners with no targets become unified with autopilot off.
