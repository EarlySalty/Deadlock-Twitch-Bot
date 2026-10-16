import type { ImportJob, ImportPoolApi, PoolClip, PoolPreferences } from './autopilot/import-pool/types';

export const PIPELINE_DETAIL_CUTOVER = false;

export type SourceEligibility = 'automatic' | 'manual' | 'legacy_non_deadlock_visible';
export type ProductionMode = 'static' | 'ai';

export interface SourceReservation {
  platform: string;
  scheduled_at: string;
}

export interface SourceJob {
  id: number;
  source: 'autopilot' | 'manual';
  requested_mode: ProductionMode | null;
  mode: ProductionMode | null;
  status: string;
  stage: string;
  scheduled_at: string | null;
  created_at: string;
  reservations: SourceReservation[];
}

export interface SourceVersion {
  id: number;
  job_id: number | null;
  mode: ProductionMode;
  version_no: number;
  parent_version_id: number | null;
  variant_index: number;
  status: string;
  needs_review: boolean;
  duration_ms: number | null;
  scheduled_at: string | null;
  created_at: string;
}

export interface SourceDestination {
  queue_id: number;
  platform: string;
  status: string | null;
  produced_video_id: number | null;
  scheduled_at: string | null;
  attempts: number | null;
  last_error: string | null;
  external_id: string | null;
  outcome_reason: string | null;
  completed_at: string | null;
}

export interface LegacyPublication {
  platform: string;
  external_id: string;
}

export interface PipelineSource {
  clip_id: string;
  clip_db_id: number | null;
  source_kind: string;
  eligibility: SourceEligibility;
  pooled: boolean;
  auto_post: boolean;
  source_created_at: string;
  game_id: string | null;
  source_metadata: unknown;
  view_count: number | null;
  selected_at: string | null;
  processed_at: string | null;
  published_at: string | null;
  legacy_reserved: boolean;
  discarded_at: string | null;
  jobs: SourceJob[];
  versions: SourceVersion[];
  destinations: SourceDestination[];
  legacy_publications: LegacyPublication[];
}

export interface EligibilityCounts {
  automatic: number;
  manual: number;
  legacy_non_deadlock_visible: number;
}

export interface PipelinePage {
  sources: PipelineSource[];
  total: number;
  limit: number;
  offset: number;
  eligibility: SourceEligibility | null;
  counts: EligibilityCounts;
  preferences: PoolPreferences;
  latest_import: ImportJob | null;
}

export type LoadSourcesPage = (owner: string, limit: number, offset: number, signal?: AbortSignal) => Promise<PipelinePage>;

export const SOURCE_BATCH = 200;
const MAX_BATCHES = 200;

export async function loadSourceCollection(load: LoadSourcesPage, owner: string, signal?: AbortSignal): Promise<PipelinePage> {
  const sources = new Map<string, PipelineSource>();
  for (let offset = 0, batch = 0; batch < MAX_BATCHES; batch += 1) {
    signal?.throwIfAborted();
    const page = await load(owner, SOURCE_BATCH, offset, signal);
    for (const source of page.sources) sources.set(source.clip_id, source);
    offset += page.sources.length;
    if (page.sources.length < SOURCE_BATCH || offset >= page.total) {
      return { ...page, sources: [...sources.values()], total: Math.max(page.total, sources.size), limit: sources.size, offset: 0 };
    }
  }
  throw new Error('sources_too_large');
}

function poolSource(clip: PoolClip): PipelineSource {
  return {
    clip_id: clip.clip_id,
    clip_db_id: clip.clip_db_id,
    source_kind: 'twitch',
    eligibility: 'automatic',
    pooled: true,
    auto_post: true,
    source_created_at: clip.source_created_at,
    game_id: clip.game_id,
    source_metadata: clip.source_metadata,
    view_count: clip.view_count ?? null,
    selected_at: clip.selected_at,
    processed_at: clip.processed_at,
    published_at: clip.published_at,
    legacy_reserved: clip.legacy_reserved,
    discarded_at: null,
    jobs: [],
    versions: [],
    destinations: [],
    legacy_publications: [],
  };
}

export function poolBackedSources(api: ImportPoolApi): LoadSourcesPage {
  return async (owner, limit, offset, signal) => {
    const pool = await api.getPool(owner, limit, offset, signal);
    return {
      sources: pool.clips.map(poolSource),
      total: pool.total,
      limit: pool.limit,
      offset: pool.offset,
      eligibility: null,
      counts: { automatic: pool.total, manual: 0, legacy_non_deadlock_visible: 0 },
      preferences: pool.preferences,
      latest_import: null,
    };
  };
}
