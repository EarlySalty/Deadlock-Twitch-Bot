export interface LayoutBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type LayoutMode = 'pip' | 'stacked';

/**
 * Effective layout payload as returned/accepted by the backend.
 * Backend serializes the layout as a flat JSON object: version + source + boxes
 * plus cam_enabled and mode on the same level.
 */
export interface LayoutPayload {
  version: 1;
  source: { width: number; height: number };
  game_crop: LayoutBox;
  cam_crop: LayoutBox;
  cam_position: LayoutBox;
  cam_enabled: boolean;
  mode: LayoutMode;
}

export interface StreamerLayoutResponse {
  streamer_login: string;
  layout: LayoutPayload;
  cam_enabled: boolean;
  mode: LayoutMode;
  is_default?: boolean;
  updated_at: string | null;
  updated_by: string | null;
}

export type ClipSourceKind = 'twitch' | 'manual_upload';

export type ClipStatus =
  | 'pending'
  | 'enriched'
  | 'awaiting_approval'
  | 'approved'
  | 'editing'
  | 'skipped'
  | 'publishing'
  | 'published_partial'
  | 'published_all'
  | 'discarded'
  | 'failed';

export interface ClipPlatformStatus {
  tiktok: boolean;
  youtube: boolean;
  instagram: boolean;
}

export interface SocialClip {
  clip_db_id: number;
  clip_id: string;
  clip_url: string | null;
  title: string;
  thumbnail_url: string | null;
  streamer_login: string;
  created_at: string;
  duration_seconds: number;
  view_count: number;
  game_name: string | null;
  status: ClipStatus;
  source_kind: ClipSourceKind;
  upload_local_path: string | null;
  retention_until: string | null;
  discarded_at: string | null;
  platform_status: ClipPlatformStatus;
  layout_override: LayoutPayload | null;
  effective_layout: LayoutPayload;
  enrichment_status?: EnrichmentStatus | null;
  enrichment_summary?: { top_hashtags?: string[]; provider?: string | null } | null;
  approval?: ClipApprovalRecord | null;
}

export type EnrichmentStatus =
  | 'pending'
  | 'transcribing'
  | 'correcting'
  | 'llm'
  | 'done'
  | 'failed'
  | 'skipped_no_key';

export type SocialPlatform = 'youtube' | 'tiktok' | 'instagram';
export type AnalyticsBucket = '24h' | '7d' | '30d';
export type SocialMediaReportKind = 'streamer' | 'cross' | 'admin';
export type ApprovalState = 'awaiting_approval' | 'approved' | 'skipped' | 'editing';

export interface ClipApprovalRecord {
  clip_db_id: number;
  state: ApprovalState;
  approved_platforms: SocialPlatform[];
  approver_user_id: string | null;
  decided_at: string | null;
  dm_message_id: string | null;
  dm_channel_id: string | null;
  last_sent_at: string | null;
}

export interface AutoApproveSettings {
  youtube: boolean;
  tiktok: boolean;
  instagram: boolean;
}

export interface ClipEnrichment {
  clip_db_id: number;
  transcript_raw: string | null;
  transcript_corrected: string | null;
  transcript_segments: Array<{ start: number; end: number; text: string }> | null;
  transcript_lang?: string | null;
  detected_terms: string[];
  title_youtube: string | null;
  title_tiktok: string | null;
  title_instagram: string | null;
  description_youtube: string | null;
  description_tiktok: string | null;
  description_instagram: string | null;
  hashtags_youtube: string[];
  hashtags_tiktok: string[];
  hashtags_instagram: string[];
  llm_provider: string | null;
  llm_model: string | null;
  cost_usd_estimate: number | null;
  status: EnrichmentStatus;
  error_message: string | null;
  started_at: string | null;
  completed_at: string | null;
  edited_by: string | null;
  updated_at: string | null;
}

export interface VocabEntry {
  term: string;
  canonical: string;
  category: 'hero' | 'item' | 'ability' | 'slang';
  source: 'deadlock_api' | 'manual';
  aliases: string[];
  weight: number;
  updated_at: string;
}

export interface VocabListResponse {
  items: VocabEntry[];
  total: number;
  page: number;
  page_size: number;
}

export interface ClipListResponse {
  items: SocialClip[];
  total: number;
  page: number;
  page_size: number;
}

export interface UploadResponse {
  clip_db_id: number;
  clip_id: string;
  retention_until: string;
}

export interface ClipAnalytics {
  clip_db_id: number;
  platform: SocialPlatform;
  bucket: AnalyticsBucket;
  views: number;
  likes: number;
  comments: number;
  shares: number;
  watch_time_seconds: number | null;
  ctr_percent: number | null;
  engagement_rate: number | null;
  provider: string | null;
  synced_at: string | null;
  next_pull_at: string | null;
}

export interface ClipAnalyticsResponse {
  clip_db_id: number;
  items: ClipAnalytics[];
}

export interface SocialMediaReport {
  id: number;
  kind: SocialMediaReportKind;
  streamer_login: string | null;
  period_start: string;
  period_end: string;
  content_md: string;
  model: string | null;
  created_at: string | null;
}

export interface SocialMediaReportListResponse {
  items: SocialMediaReport[];
}

export const DEFAULT_SOURCE_WIDTH = 1920;
export const DEFAULT_SOURCE_HEIGHT = 1080;

export const DEFAULT_LAYOUT: LayoutPayload = {
  version: 1,
  source: { width: DEFAULT_SOURCE_WIDTH, height: DEFAULT_SOURCE_HEIGHT },
  game_crop: { x: 420, y: 0, w: 1080, h: 1080 },
  cam_crop: { x: 1500, y: 50, w: 380, h: 380 },
  cam_position: { x: 0, y: 0, w: 1080, h: 540 },
  cam_enabled: true,
  mode: 'pip',
};
