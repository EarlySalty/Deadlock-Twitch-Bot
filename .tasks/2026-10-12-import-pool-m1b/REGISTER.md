# Track A M1b integration register

## Frozen UI contracts

Source checkpoint: `1f90898098c5cab7aa80b485f617d885c3deb61c`.
Verified application source: `0edc2ef013d9e0584e5b713e596b27161cd42daf`. The frozen exports, props, API and fixture signatures are unchanged. This follow-up cancels obsolete preferences/import reads before writes, prefers a newly fetched terminal latest job over an old detail cache, adds visible processing/publication timestamps, and corrects scoped responsive field layout and the primary button background. The synthetic built host consumes the real P3 guard from source `45c2695ad4a6741859f27ba10e0edf99fbe49a1f`; its source blob is `baf85183bfd34ffc6163545f9a9546200d1b11de` and matches that commit. This is fixture integration, not product mounting.
Application commits to integrate are the two source SHAs above. Keep this register out of main. Machine-readable proofs and reproduction inputs are in `/tmp/import-pool-m1b-wf82/`; final outcomes are returned through StructuredOutput.
Base and fetched origin/main: `d5c30b730acc3ad7e598915913c93c501f036a59`.
Worktree: `/home/nathanael/repos/Deadlock-Twitch-Bot/.claude/worktrees/wf_82ba4a29-939-1`.
Branch: `worktree-wf_82ba4a29-939-1`.
Checkpoint checks: dashboard `tsc -b` and owned ESLint exited 0. Browser proof is not part of this initial freeze.

Import named `ImportPoolPanel` and `importPoolApi` from `@/components/socialmedia/autopilot/import-pool`. The TypeScript public contract is frozen:

```ts
interface ImportPoolPanelProps {
  twitchUserId?: string;
  api?: ImportPoolApi;
  onChanged?: () => void;
}
```

A numeric Twitch owner string is required for any request. Missing/non-numeric owner renders a German channel-selection prompt and makes no requests. The component requires the existing QueryClientProvider and the existing Social Studio styles/tokens. It imports its own scoped CSS. P3 owns mounting; no production route or page was changed.

`api` defaults to the existing cookie-based `importPoolApi`, using `api/core.fetchJson` and `withCookieCredentials`. Optional injection is for synthetic preview/tests, not a new authentication path. `onChanged` is captured when a mutation starts and fires once after a successful preferences/selection/start/resume operation, including successful completion after unmount. Failed operations do not fire it. Owner-captured QueryClient cache updates and existing clips invalidation also survive unmount; component-local state updates do not. The keyed owner subtree resets drafts, pagination, errors and active job state on owner changes.

Types live in the owned `types.ts`. `ImportPoolApi` methods:

```ts
getPreferences(owner: string, signal?: AbortSignal): Promise<PoolPreferences>;
savePreferences(owner: string, preferences: PoolPreferences): Promise<PoolPreferences>;
getPool(owner: string, limit: number, offset: number, signal?: AbortSignal): Promise<PoolPage>;
getLatestImport(owner: string, signal?: AbortSignal): Promise<LatestImport>;
getImport(owner: string, id: number, signal?: AbortSignal): Promise<ImportJob>;
startImport(owner: string): Promise<ImportJob>;
resumeImport(owner: string, id: number): Promise<ImportJob>;
setSelection(owner: string, clipId: string, selected: boolean): Promise<PoolSelection>;
```

All browser paths have prefix `/twitch/social-media/api/autopilot` and `?twitch_user_id=<selected numeric owner>`. Verified merged handlers mount at `/social-media/api/autopilot`.

| Request | Body | Response |
| --- | --- | --- |
| GET `/preferences` | none | PoolPreferences |
| PUT `/preferences` | exactly clip_date_filter, clip_sort, timezone | PoolPreferences |
| GET `/pool` | none; limit/offset query | PoolPage |
| GET `/imports` | none | {import: ImportJob or null} |
| POST `/imports` | none | 202 ImportJob |
| GET `/imports/{id}` | none | ImportJob |
| POST `/imports/{id}/resume` | none | 202 ImportJob |
| PUT `/pool/{encoded clip_id}/selection` | {selected: boolean} | {clip_id: string, selected: boolean} |

PoolPreferences has `{clip_date_filter, clip_sort, timezone}`. Tagged filters are `{kind:'all'}`, `{kind:'year',year:number}`, `{kind:'quarter',year:number,quarter:number}`, `{kind:'range',start:string,end:string}`, `{kind:'before',date:string}`, `{kind:'after',date:string}`. Dates use YYYY-MM-DD. Sort is oldest_first/newest_first, timezone IANA. No persisted/default flag exists, so the saved timezone is retained. Browser timezone is adopted only by an explicit button action. Inclusive range end, exclusive before date and after-date-following-day are stated in the UI. Chrono date bounds and IANA membership remain authoritative server checks; no invented historical year limits.

PoolPage has clips/total/limit/offset/preferences. PoolClip has clip_id/nullable clip_db_id/source_created_at/game_id/source_metadata/nullable selected_at/first_selected_at/processed_at/published_at/legacy_reserved/eligible/selectable. source_metadata is JSON (unknown in TypeScript), accessed defensively for clip_title, clip_url and duration_seconds. Stable identity is clip_id. Server chronology, including ascending clip_id ties in both sort directions, is preserved. History is displayed even outside current eligibility. Retained rows with null clip_db_id do not promise a playable source or rehydration. Server conflicts remain authoritative for deselection of reserved rows.

ImportJob contains id/twitch_user_id/clip_date_filter/timezone/started_at/ended_at/status/raw_fetched/eligible_fetched/persisted_count/completed_windows/pending_windows/active_window/incomplete_windows/attempts/total_attempts/next_retry_at/last_error/created_at/completed_at. Nullable fields and exact shapes are in types.ts. ImportWindow contains start/end/fetched/cursor_restarts/pages. Status is queued/running/retry/error/incomplete/completed. RFC3339 timestamps are displayed in the job's frozen timezone. Active job detail polling is bounded to five seconds, stops on terminal state or query error, and does not overwrite filter drafts. Persisted-count/status changes invalidate the owner pool. Raw/eligible retrievals are not presented as unique durable clips; incomplete archives are not successful completion.

Handler errors are `{error,message}`: 400 invalid_pool_input, 404 not_found, 409 pool_state_conflict, 500 pool_unavailable; auth can return 401/403, extraction 400/415/422. The existing fetchJson/ApiHttpError path is reused, displaying German recovery messages without raw error internals. Mutations are disabled while initial preferences/import state are unknown or unauthorized, duplicate actions are disabled while pending, and selection waits for a matching fresh pool/preferences snapshot.

Query namespace: `['social-media','autopilot','import-pool',owner]`, suffixes preferences/pool/latest-import/import. Pool keys also include saved preferences and offset, import keys include id. Successful owned mutations invalidate `['social-media','clips']`; P3 can refresh additional parent-owned data via onChanged.

Markers are on the owner root: `data-unsaved='true'` for an effective dirty draft or pending mutation; `data-saving='true'` for any pending mutation. Both are omitted when false. Failed saves retain the draft. Successful saves clear it. beforeunload is prevented while dirty/pending. Use the existing shell departure guard; props are not extended for navigation.

Fixtures are separate from the production barrel: named `createImportPoolFixture` and `createImportPoolFixtureJob` from `.../import-pool/fixtures`. Fixture constructor options are owner/preferences/clips/job (all optional), default owner 101. It returns `{api,requests,snapshot(owner?),setJob(job,owner?),failNext(method,status?),holdNext(method)}`; holdNext returns a release function. Requests contain method/owner and actual payload identifiers/fields. Other owners begin with isolated empty pools. FixtureMethod is keyof ImportPoolApi. ImportPoolFixtureRequest, ImportPoolFixtureState and ImportPoolFixtureOptions are exported types. Fixtures must only be imported by an owned synthetic host, never normal production composition.

The register is an integration input for P1/P3 and remains outside application commits. Final checks and browser measurements will be returned directly through StructuredOutput with absolute evidence paths.
