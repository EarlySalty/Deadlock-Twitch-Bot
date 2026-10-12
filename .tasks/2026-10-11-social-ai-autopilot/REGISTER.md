# Track B integration contract

## Frozen UI contracts

Source commit: `be02ee5974eaa13d015ed9333b0fe93964227626` (initial freeze `2d45c0cfa1273c4725e5f3b26f06dd9d2a74abea`).
Source directory: `/home/nathanael/repos/Deadlock-Twitch-Bot/.claude/worktrees/wf_fac1dca2-678-1/bot/dashboard_v2/src/components/socialmedia/autopilot/schedule-queue/`.

Named export: `ScheduleQueuePanel` from `@/components/socialmedia/autopilot/schedule-queue`.
Props: `ScheduleQueuePanelProps { twitchUserId?: string; api?: ScheduleQueueApi; onChanged?: () => void }`.
Owner changes remount isolated drafts. Undefined owner uses the authenticated partner scope; admins supply the selected numeric Twitch owner ID. The component defaults to the existing `fetchJson` and `withCookieCredentials` helpers exported from `@/api/core`. No shared API export is required. Mount inside the existing Social Studio surface. The component imports its scoped CSS.

Fixture exports: `createScheduleQueueFixture(): ScheduleQueueFixture`, with `{ api, requests, snapshot() }`. The fixture is explicitly synthetic and does not publish, contact users or call AI.

HTTP prefix: `/twitch/social-media/api/autopilot`.

- `GET /schedule?twitch_user_id=<optional-owner>` returns `ScheduleQueueSnapshot`.
- `PUT /schedule` body `{ twitch_user_id?: string } & Partial<ScheduleSettings>` returns `ScheduleQueueSnapshot`. Unknown fields are rejected. Only supplied schedule-owned fields are changed; pool, publication, consent and editor settings are not written.
- `PUT /queue/order` body `{ twitch_user_id?: string; job_ids: number[] }` returns `ScheduleQueueSnapshot`. IDs must be the complete current set of owned queued jobs that have never started.
- `POST /queue/:job_id/next` and `POST /queue/:job_id/cancel` body `{ twitch_user_id?: string }` return `ScheduleQueueSnapshot`. Only owned unstarted queued jobs can be changed. Cancellation prevents automatic reselection.

`ScheduleSettings = { enabled: boolean; paused: boolean; ai_editing_enabled: boolean; lead_time_hours: number; timezone: string; schedule_slots: { weekday: number; local_time: string }[] }`. Weekday is ISO Monday=1 through Sunday=7, local_time is canonical HH:MM, timezone is IANA. Lead time is 0..720 hours, at most 28 unique weekly slots.

`ScheduleQueueSnapshot = { settings: ScheduleSettings; persisted: boolean; next_slot: string | null; jobs: QueueJob[]; review_count: number; production_hold: ProductionHold }`.

`ProductionHold = 'disabled' | 'paused' | 'review_backlog' | 'empty_pool' | 'no_schedule' | 'outside_lead' | null`.

`QueueJob = { id: number; clip_id: number; clip_title: string; source: 'autopilot' | 'manual'; mode: 'static' | 'ai' | null; requested_mode: 'static' | 'ai' | null; status: 'queued' | 'running' | 'review' | 'completed' | 'failed' | 'cancelled'; stage: string; attempts: number; max_attempts: number; scheduled_at: string | null; started_at: string | null; last_error: string | null; review_reasons: string[] }`.

Error responses from the new handlers: `{ error: string; message: string }` with 400 invalid_schedule, 403 owner/access denial, 404 job_not_found, 409 queue_changed/job_started, 500 schedule_unavailable. Existing authentication/access guards retain their existing denial responses. The adapter displays server messages through the existing error helper.

Freeze validation: dashboard `npm run build` completed with Exit 0, TypeScript build and Vite 3109 transformed modules. Final owned-folder TypeScript/ESLint checks completed with Exit 0, and 49 selected frontend tests passed with 0 failures/skips. HTTP route composition and product-page mounting remain pending P1/P3 work; this freeze does not claim their runtime validation. The fixture host exposes `window.setScheduleQueueOwner(owner)` for the pending owner-switch browser check. The owned Moli and Vite processes reached the tool's two-hour limit and were stopped; they were not restarted. Existing 1440/390 screenshots were opened. Cancellation was activated through native DOM click because Moli did not expose geometry for the inserted confirmation button. Brand-font URLs returned HTML in the existing local preview, so the screenshots use fallback fonts.

## Runtime composition input

The linked domain is `tb_social_media::production_jobs::autopilot_schedule`. Public operations are `save_schedule(pool, owner, &SchedulePatch)`, `snapshot(pool, owner)`, `reorder(pool, owner, &[i64])`, `mark_next(pool, owner, id)`, `cancel(pool, owner, id)`, and `plan_owner(pool, owner, now, resolve_bounds)`. The resolver executes while the existing owner lock is held and must use the actual M1 `clip_pool::bounds`; the runner source supplies that adapter. `production_jobs::claim_next(&PgPool)` retains its M0 signature and transaction claim lock. Recovery retains started mode, stage, metadata and bounded attempts. A zero-hour lead uses the next one-minute polling window, not missed slots. Cancellation history excludes the clip from future automatic selection. Reordering preserves global positions and swaps unstarted automatic slot reservations.

`/home/nathanael/repos/Deadlock-Twitch-Bot/.claude/worktrees/wf_fac1dca2-678-1/rust/crates/tb-social-media/src/autopilot_schedule/runner.rs` is concrete production source but deliberately not registered while M1/P2 remain absent from origin/main. It has only been syntax/formatted checked, not typechecked or executed. Do not claim a complete runner before composing it against the actual merged modules.

After M1/P2 adoption, P1 adds to the owned shared social-media crate root:

```rust
#[path = "autopilot_schedule/runner.rs"]
pub mod autopilot_runner;
```

`autopilot_runner::AutopilotWorker::new(pool, WorkerConfig, Arc<dyn ClipDownloader>, VideoProcessor) -> Result<AutopilotWorker, RunnerError>`; `.run(self)` is the worker loop, `.run_once(&self) -> Result<bool, RunnerError>` reports a claimed/handled job, not necessarily a produced video. `WorkerConfig` fields are `package_root: PathBuf`, `source_root: PathBuf`, `output_root: PathBuf`, `media_limits: ai_editor::ResourceLimits`, `max_duration_seconds: i64`, `poll_interval: Duration`, `editor: Option<ai_editor::EditorRuntime>`, `brain_token: Option<String>`. Reuse existing global paths/limits and the existing preview duration, not new provider/model/cost configuration.

The runner uses the actual `production_lease::acquire_production_lease`, `render::render_clip_vertical_with_lease`, `produced_video::insert_with_metadata` and `ai_editor::produce(ProductionContext, ProductionJob)` contracts. There is no lease implementation, static substitute for AI, nested lease acquisition or UploadWorker lease here. Static version paths are `<output_root>/<owner>/production-<job>/version-<version>.mp4`, distinct from mutable preview files. P2 media inspection is local FFprobe only. Rendering time is separate metadata and unknown interrupted time remains null. Until a usable editor runtime is supplied, AI jobs hold as `ai_editor_unavailable`; P2 remains the consent authority. The runner does not publish.

P1 exposes the API child inside its owned `handlers/social_media.rs`:

```rust
#[path = "social_media_autopilot_schedule.rs"]
mod autopilot_schedule;
pub use autopilot_schedule::{
    get_handler as autopilot_schedule_get_handler,
    put_handler as autopilot_schedule_put_handler,
    order_handler as autopilot_queue_order_handler,
    next_handler as autopilot_queue_next_handler,
    cancel_handler as autopilot_queue_cancel_handler,
};
```

Register GET/PUT `/social-media/api/autopilot/schedule`, PUT `/social-media/api/autopilot/queue/order`, POST `/social-media/api/autopilot/queue/{job_id}/next` and POST `/social-media/api/autopilot/queue/{job_id}/cancel` in the existing authenticated router, with the existing external `/twitch` prefix. No second router or authorization implementation is required. The child reuses `require_sm_access` with the requested owner and the existing central registered-target resolver. API handler/tests remain uncompiled until this composition.

P1 additionally owns bot worker spawn and fresh-schema snapshot regeneration. Track C's finished-video reservation reader must adopt the same production-job reservation set; the existing M0 reader omits it. Reuse `production_jobs::autopilot_schedule::taken_on(&mut tx, owner)` under the shared owner lock instead of another planner. Slot triggers reject conflicting jobs, finished videos and uploads, while same-job/video transfer and platform reuse remain allowed. C retention must account for static checkpoint paths of interrupted/failed rendering, and the M2 review-only backpressure policy must be composed with M3's Full Auto policy.

Migration input: `/home/nathanael/repos/Deadlock-Twitch-Bot/.claude/worktrees/wf_fac1dca2-678-1/rust/migrations/20261011102000_social_production_slot_guards.sql`. Not applied to production. P1 remains the independent gate, main, migration, deployment and real-clip/zero-analysis live-proof owner. P3 owns product-page mounting and final browser verification.

Validation inputs for composition: `TB_TEST_REQUIRE_DB=1`, `TB_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:33129/tb_track_b_fac1dca2_20261012`, `cargo-slot test --release --jobs 2 -p tb-social-media autopilot_ -- --include-ignored`. Final normal-parallel run passed 21 tests, 0 failed/ignored, 349 filtered; baseline exact main b0be1e4c passed 10/0/0 with 349 filtered. An earlier 18/3 run was caused by the new tests sharing advisory-lock owner keys with legacy tests. This was fixed in the new fixtures by creating and dropping a separate owned database and schema per test; explicit intra-test races still run concurrently. The red run was not dismissed or suppressed. Final owned-package strict Clippy with `--no-deps` passed; the identical broad command on candidate and exact main both stops at the one existing `tb-raid/src/signup_denylist.rs:71` result_unit_err diagnostic, introduced delta 0. No suppression or foreign source repair. Formatter checks for owned Rust pass. Command/start/end/exit inputs are the adjacent `.log`/`.json` files from `check-command.py`.
