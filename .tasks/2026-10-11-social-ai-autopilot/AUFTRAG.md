# P2: AI editor, feedback, learning and costs

## Contract

Goal: demonstrate BRIEF section 32 Manual AI Workflow and Learning Workflow on the live Social Studio and admin dashboard. M1, one manual real-clip production with original-language subtitles, structured decisions, side-by-side review and measured costs, is first priority.

Binding specifications: `/home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/COMMON.md`, `AUFTRAG.md` contracts C1 to C5 and `BRIEF.md` sections 10 to 23, 26, 27 and 32. Main orchestrator: `eddfce29-0b2a-4f47-931f-8b5387672f94`.

Workspace: `/home/nathanael/.worktrees/tb-social-ai`, branch `feat/social-ai-editor-20261011`, initial clean HEAD `3862b764a56327435860eece92337e5968809c89`. Rebase onto P1 M0 before integrating C1 settings and C2 produced videos. AI calls use only the bot central provider and require per-user external-AI consent. Manual production never automatically publishes. Preserve the static workflow and all existing foreign work.

## Ownership

P2 owns `rust/crates/tb-social-media/src/ai_editor/`, global AI YAML configuration, new `rust/crates/tb-dashboard-api/src/handlers/social_media_ai.rs`, AI admin handlers, migrations `2026101120*`, new `bot/dashboard_v2/src/components/socialmedia/ai/` files and AI admin pages. Shared registrations and API additions remain minimal and isolated. P3 owns mounting and existing Social Studio pages. Native implementation workers use isolated worktrees with disjoint owned paths; no extra T3 threads or orchestration levels.

## Milestones

1. M1: investigate verified provider, optional read-only Brain, HyperFrames, existing STT and subtitle/render capabilities. Build persistent step-level manual production, versioned stage skills, validated editing decisions, safe FFmpeg rendering through the existing layout path, local original-language STT, quality checks, call costs and separate compute timings, authenticated API and review components. Do not invent provider capabilities or paid credentials.
2. M2: full-sequence feedback, cheap direct revisions and confirmation of expensive revisions, preserved version comparison, approval and C4 transfer without re-render, C3 queue entry point, conservative per-user multi-output and experimental-layout flags.
3. M3: personal preferences from accepted/rejected feedback sequences; reliable anonymized global evidence; validated, versioned non-code skill updates with diff and rollback; architecture/code proposals remain suggestions. No Brain writes or application self-modification.
4. M4: existing admin dashboard skill history/rollback, critical proposals, user usage/costs and configurable global/user budget controls prepared for existing plans without new billing.

## Approved M1 decisions

On 2026-10-11 the main orchestrator approved P2 ownership of a native Anthropic vision/structured-output connector inside `tb-llm`, scoped to the Social AI editor. Use the existing Infisical `ANTHROPIC_API_KEY`; provider/model Claude Opus 5.5 and technical limits come only from global YAML. Preserve all existing Fireworks routes. Enforce a persisted hard $2 total budget for initial live tests before calls, including in-flight/ambiguous attempts and retries. Token-derived dollar amounts are estimates unless actually reconciled with provider billing; unknown usage is not zero.

P2 owns a new per-user external-AI consent table/API/German UI, default false, checked together with the existing global gate before each external call. It is not C1 and does not block P1 M0. Enable live-test consent only for the user's own channel.

The deliberately masked STT server and recovery units remain masked. Do not touch `deadlock-patchn` on 8791. Start the existing Rust STT server transiently per AI job, on a YAML-configured free loopback port such as observed-free 8795, with low CPU priority, and stop only that child at job completion. No permanent unit. On 2026-10-11 the orchestrator approved packaging this existing Rust binary in the verified release through the deploy wrapper. Required model files may be installed once, root-owned and read-only, outside `/home`, such as `/opt/deadlock/stt-models`; retain only the configured Whisper model and required VAD file. Point the STT configuration there. Do not bind a `/home` path into production. Preserve `ProtectHome=yes`. Any unit/drop-in changes belong in repository ops files and the normal deploy path. Use `sudo -n` only and report failure as a blocker. Brain retrieval remains optional with relevance checked on the real clip; HyperFrames is omitted.

Visual proof uses `dashboard_v2` preview mode, added fixtures and a dedicated Moli process on a free loopback port without `--block-private-networks`. Playwright may attach only to that Moli process. No new login mechanism, demo login, real-site header injection or reads of `~/.claude/.sichtpruefung-sitzungen/`. Live functional proof uses the documented `X-Internal-Token` admin API plus redacted read-only DB/service checks. Authenticated live UI proof is deferred to one user-assisted check requested by the main orchestrator at the end; report required pages explicitly.

All static, AI and manual production share the same global session advisory lock, `hashtextextended('social_media_video_production', 0)`. Acquire before claiming a job and hold for the entire job. The orchestrator approved this contract on 2026-10-11 and forwarded it to P1. Whoever merges first places `acquire_production_lease` in a neutral shared module; the other package adopts that module on rebase. The P2 store-local placement is provisional, not a second independent lock.

After P1 M0 is on main, P2 may add an additive `produced_video::insert_with_metadata` helper that atomically stores `manual_ai_job_id` in metadata at version allocation. This was approved by the orchestrator on 2026-10-11. Preserve existing `insert` and all ownership/version guards. `ai_editor` still never writes C2 SQL directly. Notify P1 of this addition in the merge report so P1 can rebase cleanly.

## M1 application/UI contract

This is the new P2 contract, not an assertion of existing API support. Endpoint prefix: `/twitch/social-media/api/ai`. Scope every request with `twitch_user_id` and the existing Social authorization/ownership guards. Internal/admin aliases are added only under the documented existing internal-auth router.

- `GET /consent?twitch_user_id=...` and `PUT /consent` with `{twitch_user_id, external_ai_consent}` return `{external_ai_consent, global_allowed}`. Revocation does not discard completed output, but subsequent transmissions fail closed.
- `GET /jobs?twitch_user_id=...` returns `{items: AiJobView[]}` with a bounded recent-job list.
- `POST /jobs` with `{twitch_user_id, clip_db_id}` creates a manual job and returns `{job: AiJobView}`. No implicit consent or auto-publishing.
- `GET /jobs/{id}?twitch_user_id=...` returns `AiJobView`.
- `POST /jobs/{id}/retry` with `{twitch_user_id}` retries the failed stage only, bounded by server policy, and returns `{job: AiJobView}`.
- Job source and produced-version file URLs are supplied by the backend after ownership checks; clients must never construct URLs from stored filesystem paths.

`AiJobView`: `id` (number), `clip_db_id` (number), `status` (`queued|preparing|transcribing|analyzing|planning|rendering|validating|review|failed`), `stage` (string), `created_at`, `updated_at`, `completed_at` (nullable ISO strings), `provider`/`model` (nullable strings), `original_duration_ms`/`output_duration_ms`/`processing_ms`/`rendering_ms` (nullable numbers), `input_tokens`/`output_tokens` (nullable numbers), `estimated_provider_cost_usd`/`measured_provider_cost_usd` (nullable numbers), `cost_basis` (`usage_priced_estimate|provider_billed|unknown`), `highlights` (`{start_ms,end_ms,description}[]`), `decisions` (`{start_ms,end_ms,description}[]`), `uncertainty` (`string[]`), `review_reasons` (`string[]`), `original_url` (nullable string), `versions` (`AiVideoVersion[]`), `failure_code`/`failure_message` (nullable strings), `can_retry` (bool).

`AiVideoVersion`: `id`, `version_no`, `created_at`, `status`, `duration_ms` (nullable), `file_url` (nullable), `parent_version_id` (nullable). Manual jobs have their own IDs; C2's production-job reference remains null for manual jobs unless its actual schema provides a distinct compatible association. Link manual job IDs in produced metadata instead of confusing them with C3 jobs.

Frontend export: `AiEditorPanel({ twitchUserId: string, streamer: string, clipDbId?: number })`, from `components/socialmedia/ai/AiEditorPanel.tsx`; P3 reserves and mounts it in the existing Social Studio shell when the file lands. On 2026-10-11 the main orchestrator additionally authorized P2 to add a minimal one-line mount in the current shell for M1 if P3's area has not landed yet; P3 relocates it later. It uses the existing clip pool API for selection. Backend runtime unavailability is a visible error, never simulated success. M1 revisions/approval/scheduling are not enabled until M2 endpoints exist.

## Acceptance and delivery

Each milestone requires formatter, matching release build, clippy, existing relevant tests, frontend build/tests where changed, independent intent review, local merge gate ALLOW, merge and deploy of verified origin/main through the existing wrapper, and live verification. No paid connector or new key without an orchestrator decision. Do not reactivate legacy enrichment or automatic publishing.

M1 proof: real job ID, configured model, token usage and measured provider cost distinguished from pricing-derived estimates, stage/total durations, playable output and Moli screenshots of the side-by-side review. M3/M4 proof: real skill diff and rollback shown in existing admin UI. Keep task records out of main. Report only milestone completion, decision or blocker to the main orchestrator, at most five lines.
