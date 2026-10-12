import { ApiHttpError } from "@/api/core";
import { activeImport, selectionAllowed } from "./presentation";
import type {
  ImportJob,
  ImportPoolApi,
  PoolClip,
  PoolPage,
  PoolPreferences,
} from "./types";

export type FixtureMethod = keyof ImportPoolApi;
export interface ImportPoolFixtureRequest {
  method: FixtureMethod;
  owner: string;
  preferences?: PoolPreferences;
  limit?: number;
  offset?: number;
  id?: number;
  clipId?: string;
  selected?: boolean;
}
export interface ImportPoolFixtureState {
  preferences: PoolPreferences;
  clips: PoolClip[];
  job: ImportJob | null;
}
export interface ImportPoolFixtureOptions {
  owner?: string;
  preferences?: PoolPreferences;
  clips?: PoolClip[];
  job?: ImportJob | null;
}

const DEFAULT_PREFERENCES: PoolPreferences = {
  clip_date_filter: { kind: "all" },
  clip_sort: "oldest_first",
  timezone: "Europe/Berlin",
};

export function createImportPoolFixtureJob(
  overrides: Partial<ImportJob> = {},
): ImportJob {
  return {
    id: 71,
    twitch_user_id: "101",
    clip_date_filter: { kind: "all" },
    timezone: "Europe/Berlin",
    started_at: "1970-01-01T00:00:00Z",
    ended_at: "2026-10-11T12:00:00Z",
    status: "queued",
    raw_fetched: 0,
    eligible_fetched: 0,
    persisted_count: 0,
    completed_windows: 0,
    pending_windows: 1,
    active_window: null,
    incomplete_windows: [],
    attempts: 0,
    total_attempts: 0,
    next_retry_at: null,
    last_error: null,
    created_at: "2026-10-11T12:00:00Z",
    completed_at: null,
    ...overrides,
  };
}

function fixtureClip(
  clipId: string,
  title: string,
  created: string,
  overrides: Partial<PoolClip> = {},
): PoolClip {
  return {
    clip_id: clipId,
    clip_db_id: 100,
    source_created_at: created,
    game_id: "2132205352",
    source_metadata: {
      clip_title: title,
      clip_url: `https://clips.twitch.tv/${encodeURIComponent(clipId)}`,
      duration_seconds: 42,
      thumbnail_url: "",
      streamer_login: "fixture_channel",
    },
    selected_at: null,
    first_selected_at: null,
    processed_at: null,
    published_at: null,
    legacy_reserved: false,
    eligible: true,
    selectable: true,
    ...overrides,
  };
}

function initialClips(): PoolClip[] {
  return [
    fixtureClip(
      "FixtureTieB",
      "Zweiter Clip am selben Zeitpunkt",
      "2025-03-30T12:00:00Z",
      { clip_db_id: 102 },
    ),
    fixtureClip(
      "FixturePublished",
      "Veröffentlichter Clip mit erhaltenem Verlauf",
      "2024-07-05T18:00:00Z",
      {
        clip_db_id: null,
        first_selected_at: "2024-07-06T09:00:00Z",
        processed_at: "2024-07-06T10:00:00Z",
        published_at: "2024-07-07T16:00:00Z",
        selectable: false,
      },
    ),
    fixtureClip(
      "FixtureSelected",
      "Ausgewählter Clip bleibt im Verlauf",
      "2025-06-01T14:00:00Z",
      {
        clip_db_id: 104,
        selected_at: "2026-10-10T09:00:00Z",
        first_selected_at: "2026-10-10T09:00:00Z",
      },
    ),
    fixtureClip(
      "FixtureOldest",
      "Erster Deadlock-Clip im Archiv",
      "2024-01-01T12:00:00Z",
      { clip_db_id: 101 },
    ),
    fixtureClip(
      "FixtureReserved",
      "Bereits für die Verarbeitung reserviert",
      "2026-01-05T14:00:00Z",
      { clip_db_id: 105, legacy_reserved: true, selectable: false },
    ),
    fixtureClip(
      "FixtureTieA",
      "Teamkampf am Zeitwechsel",
      "2025-03-30T12:00:00Z",
      { clip_db_id: 103 },
    ),
  ];
}

function matchingDate(clip: PoolClip, preferences: PoolPreferences): boolean {
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone: preferences.timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(new Date(clip.source_created_at));
  const part = (type: string) =>
    parts.find((value) => value.type === type)?.value ?? "";
  const date = `${part("year")}-${part("month")}-${part("day")}`;
  const filter = preferences.clip_date_filter;
  switch (filter.kind) {
    case "all":
      return true;
    case "year":
      return Number(part("year")) === filter.year;
    case "quarter":
      return (
        Number(part("year")) === filter.year &&
        Math.ceil(Number(part("month")) / 3) === filter.quarter
      );
    case "range":
      return date >= filter.start && date <= filter.end;
    case "before":
      return date < filter.date;
    case "after":
      return date > filter.date;
  }
}

export function createImportPoolFixture(
  options: ImportPoolFixtureOptions = {},
) {
  const initialOwner = options.owner ?? "101";
  const states = new Map<string, ImportPoolFixtureState>([
    [
      initialOwner,
      {
        preferences: structuredClone(
          options.preferences ?? DEFAULT_PREFERENCES,
        ),
        clips: structuredClone(options.clips ?? initialClips()),
        job: structuredClone(options.job ?? null),
      },
    ],
  ]);
  const requests: ImportPoolFixtureRequest[] = [];
  const holds = new Map<FixtureMethod, Promise<void>>();
  const failures = new Map<FixtureMethod, number>();
  const state = (owner: string) => {
    let value = states.get(owner);
    if (!value) {
      value = {
        preferences: structuredClone(DEFAULT_PREFERENCES),
        clips: [],
        job: null,
      };
      states.set(owner, value);
    }
    return value;
  };
  const record = async (
    request: ImportPoolFixtureRequest,
    signal?: AbortSignal,
  ) => {
    requests.push(structuredClone(request));
    const hold = holds.get(request.method);
    holds.delete(request.method);
    if (hold) await hold;
    if (signal?.aborted) throw new DOMException("Aborted", "AbortError");
    const failure = failures.get(request.method);
    failures.delete(request.method);
    if (failure) throw new ApiHttpError("Synthetischer Fehler", failure);
  };
  const poolPage = (owner: string, limit: number, offset: number): PoolPage => {
    const value = state(owner);
    const direction = value.preferences.clip_sort === "newest_first" ? -1 : 1;
    const clips = value.clips
      .map((clip) => {
        const eligible = matchingDate(clip, value.preferences);
        return {
          ...clip,
          eligible,
          selectable:
            eligible &&
            !clip.processed_at &&
            !clip.published_at &&
            !clip.legacy_reserved,
        };
      })
      .sort(
        (a, b) =>
          direction *
            (Date.parse(a.source_created_at) -
              Date.parse(b.source_created_at)) ||
          (a.clip_id < b.clip_id ? -1 : a.clip_id > b.clip_id ? 1 : 0),
      );
    return structuredClone({
      clips: clips.slice(offset, offset + limit),
      total: clips.length,
      limit,
      offset,
      preferences: value.preferences,
    });
  };
  const api: ImportPoolApi = {
    getPreferences: async (owner, signal) => {
      const result = structuredClone(state(owner).preferences);
      await record({ method: "getPreferences", owner }, signal);
      return result;
    },
    savePreferences: async (owner, preferences) => {
      await record({ method: "savePreferences", owner, preferences });
      state(owner).preferences = structuredClone(preferences);
      return structuredClone(preferences);
    },
    getPool: async (owner, limit, offset, signal) => {
      const result = poolPage(owner, limit, offset);
      await record({ method: "getPool", owner, limit, offset }, signal);
      return result;
    },
    getLatestImport: async (owner, signal) => {
      const result = { import: structuredClone(state(owner).job) };
      await record({ method: "getLatestImport", owner }, signal);
      return result;
    },
    getImport: async (owner, id, signal) => {
      const result = structuredClone(state(owner).job);
      await record({ method: "getImport", owner, id }, signal);
      if (!result || result.id !== id)
        throw new ApiHttpError("Nicht gefunden", 404);
      return result;
    },
    startImport: async (owner) => {
      await record({ method: "startImport", owner });
      const value = state(owner);
      if (!activeImport(value.job)) {
        value.job = createImportPoolFixtureJob({
          id: (value.job?.id ?? 70) + 1,
          twitch_user_id: owner,
          clip_date_filter: structuredClone(value.preferences.clip_date_filter),
          timezone: value.preferences.timezone,
        });
      }
      return structuredClone(value.job!);
    },
    resumeImport: async (owner, id) => {
      await record({ method: "resumeImport", owner, id });
      const value = state(owner);
      if (!value.job || value.job.id !== id)
        throw new ApiHttpError("Nicht gefunden", 404);
      if (!activeImport(value.job) && value.job.status !== "completed") {
        value.job = {
          ...value.job,
          status: "queued",
          pending_windows:
            value.job.pending_windows + value.job.incomplete_windows.length,
          incomplete_windows: [],
          attempts: 0,
          next_retry_at: null,
          last_error: null,
          completed_at: null,
        };
      }
      return structuredClone(value.job);
    },
    setSelection: async (owner, clipId, selected) => {
      await record({ method: "setSelection", owner, clipId, selected });
      const clip = state(owner).clips.find((item) => item.clip_id === clipId);
      if (!clip) throw new ApiHttpError("Nicht gefunden", 404);
      if (
        !selectionAllowed(clip) ||
        (selected && !matchingDate(clip, state(owner).preferences))
      )
        throw new ApiHttpError("Gesperrt", 409);
      clip.selected_at = selected
        ? (clip.selected_at ?? "2026-10-11T12:00:00Z")
        : null;
      if (selected) clip.first_selected_at ??= clip.selected_at;
      return { clip_id: clipId, selected };
    },
  };
  return {
    api,
    requests,
    snapshot: (owner = initialOwner) => structuredClone(state(owner)),
    setJob: (job: ImportJob | null, owner = initialOwner) => {
      state(owner).job = structuredClone(job);
    },
    failNext: (method: FixtureMethod, status = 500) => {
      failures.set(method, status);
    },
    holdNext: (method: FixtureMethod) => {
      let release: () => void = () => {};
      holds.set(
        method,
        new Promise<void>((resolve) => {
          release = resolve;
        }),
      );
      return release;
    },
  };
}
