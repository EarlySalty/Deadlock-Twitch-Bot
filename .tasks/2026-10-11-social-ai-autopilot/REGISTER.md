# Track B integration contract

## Frozen UI contracts

Source commit: `2d45c0cfa1273c4725e5f3b26f06dd9d2a74abea`.
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

Freeze validation: dashboard `npm run build` completed with Exit 0, TypeScript build and Vite 3109 transformed modules. Owned-folder ESLint completed with Exit 0. HTTP route composition and product-page mounting remain pending P1/P3 work; this freeze does not claim their runtime validation.
