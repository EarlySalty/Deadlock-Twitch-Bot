import type { QueueStage } from '../../queuePresentation';
import type { LoadSourcesPage } from "../../pipelineApi";

export type ClipDateFilter =
  | { kind: "all" }
  | { kind: "year"; year: number }
  | { kind: "quarter"; year: number; quarter: number }
  | { kind: "range"; start: string; end: string }
  | { kind: "before"; date: string }
  | { kind: "after"; date: string };

export type ClipSort = "oldest_first" | "newest_first";

export interface PoolPreferences {
  clip_date_filter: ClipDateFilter;
  clip_sort: ClipSort;
  timezone: string;
}

export interface PoolClip {
  clip_id: string;
  clip_db_id: number | null;
  source_created_at: string;
  game_id: string;
  source_metadata: unknown;
  view_count?: number | null;
  selected_at: string | null;
  first_selected_at: string | null;
  processed_at: string | null;
  published_at: string | null;
  legacy_reserved: boolean;
  eligible: boolean;
  selectable: boolean;
}

export interface PoolPage {
  clips: PoolClip[];
  total: number;
  limit: number;
  offset: number;
  preferences: PoolPreferences;
}

export interface ImportWindow {
  start: string;
  end: string;
  fetched: number;
  cursor_restarts: number;
  pages: number;
}

export type ImportStatus =
  "queued" | "running" | "retry" | "error" | "incomplete" | "completed";

export interface ImportJob {
  id: number;
  twitch_user_id: string;
  clip_date_filter: ClipDateFilter;
  timezone: string;
  started_at: string;
  ended_at: string;
  status: ImportStatus;
  raw_fetched: number;
  eligible_fetched: number;
  persisted_count: number;
  completed_windows: number;
  pending_windows: number;
  active_window: ImportWindow | null;
  incomplete_windows: ImportWindow[];
  attempts: number;
  total_attempts: number;
  next_retry_at: string | null;
  last_error: string | null;
  created_at: string;
  completed_at: string | null;
}

export interface LatestImport {
  import: ImportJob | null;
}

export interface PoolSelection {
  clip_id: string;
  selected: boolean;
}

export interface ImportPoolApi {
  getPreferences(owner: string, signal?: AbortSignal): Promise<PoolPreferences>;
  savePreferences(
    owner: string,
    preferences: PoolPreferences,
  ): Promise<PoolPreferences>;
  getPool(
    owner: string,
    limit: number,
    offset: number,
    signal?: AbortSignal,
  ): Promise<PoolPage>;
  getLatestImport(owner: string, signal?: AbortSignal): Promise<LatestImport>;
  getImport(
    owner: string,
    id: number,
    signal?: AbortSignal,
  ): Promise<ImportJob>;
  startImport(owner: string): Promise<ImportJob>;
  resumeImport(owner: string, id: number): Promise<ImportJob>;
  setSelection(
    owner: string,
    clipId: string,
    selected: boolean,
  ): Promise<PoolSelection>;
  getSources?: LoadSourcesPage;
}

export interface ImportPoolPanelProps {
  twitchUserId?: string;
  api?: ImportPoolApi;
  onChanged?: () => void;
  reloadKey?: number;
  initialStage?: QueueStage;
}
