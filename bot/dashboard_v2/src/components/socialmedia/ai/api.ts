import { fetchSocialAiJson } from '@/api/socialMedia';

const AI_PREFIX = '/twitch/social-media/api/ai';

export type AiJobStatus = 'queued' | 'preparing' | 'transcribing' | 'analyzing' | 'planning' | 'rendering' | 'validating' | 'review' | 'failed' | 'awaiting_confirmation' | 'rejected';
export type AiCostBasis = 'usage_priced_estimate' | 'provider_billed' | 'subscription_included' | 'unknown';

export type AiProviderAccess = 'ready' | 'key_missing' | 'workspace_required' | 'workspace_invalid' | 'key_rejected' | 'unreachable';

const PROVIDER_ACCESS: readonly AiProviderAccess[] = ['ready', 'key_missing', 'workspace_required', 'workspace_invalid', 'key_rejected', 'unreachable'];

export interface AiConsent {
  external_ai_consent: boolean;
  global_allowed: boolean;
  provider_access?: AiProviderAccess | null;
}

export type AiPublishLanguage = 'de' | 'en' | 'auto';

export type AiFrameFormat = 'layout' | 'fill' | 'full';

export interface AiEditorOptions {
  multi_output_enabled: boolean;
  experimental_ai_layout: boolean;
  publish_language: AiPublishLanguage;
  frame_format: AiFrameFormat;
}

export interface AiVideoVersion {
  id: number;
  version_no: number;
  created_at: string;
  status: string;
  duration_ms: number | null;
  file_url: string | null;
  parent_version_id: number | null;
  job_id: number;
  variant_index: number;
  original_url: string | null;
  decisions: AiTimeRange[];
  approved_platforms: string[];
  rating: AiVersionRating | null;
  layout_mode: string | null;
  streamer_layout_configured: boolean;
}

export type AiVersionRating = 'accepted' | 'rejected';

export interface AiRevisionProposal {
  summary: string;
  impact: string;
  reanalyze: boolean;
  requires_confirmation: boolean;
  estimated_provider_cost_usd: number | null;
}

export interface AiTimeRange {
  start_ms: number;
  end_ms: number;
  description: string;
}

export interface AiJobView {
  id: number;
  clip_db_id: number;
  status: AiJobStatus;
  stage: string;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
  provider: string | null;
  model: string | null;
  original_duration_ms: number | null;
  output_duration_ms: number | null;
  processing_ms: number | null;
  provider_latency_ms: number | null;
  rendering_ms: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  estimated_provider_cost_usd: number | null;
  measured_provider_cost_usd: number | null;
  cost_basis: AiCostBasis;
  highlights: AiTimeRange[];
  decisions: AiTimeRange[];
  uncertainty: string[];
  review_reasons: string[];
  original_url: string | null;
  versions: AiVideoVersion[];
  failure_code: string | null;
  failure_message: string | null;
  can_retry: boolean;
  root_job_id: number;
  parent_version_id: number | null;
  selected_version_id: number | null;
  revision_proposal: AiRevisionProposal | null;
}

function invalidResponse(): never {
  throw new Error('Die Antwort des KI-Editors ist unvollständig. Bitte lade die Ansicht erneut.');
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) invalidResponse();
  return value as Record<string, unknown>;
}

function text(value: unknown): string {
  if (typeof value !== 'string' || !value.trim()) invalidResponse();
  return value;
}

function optionalText(value: unknown): string | null {
  if (value === null) return null;
  return text(value);
}

function date(value: unknown): string {
  const result = text(value);
  if (!Number.isFinite(Date.parse(result))) invalidResponse();
  return result;
}

function optionalDate(value: unknown): string | null {
  return value === null ? null : date(value);
}

function number(value: unknown, positive = false): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || (positive ? value < 1 : value < 0)) invalidResponse();
  return value;
}

function optionalNumber(value: unknown): number | null {
  if (value === null) return null;
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) invalidResponse();
  return value;
}

function optionalMilliseconds(value: unknown): number | null {
  return value === null || value === undefined ? null : number(value);
}

function flag(value: unknown): boolean {
  if (typeof value !== 'boolean') invalidResponse();
  return value;
}

function list<T>(value: unknown, decode: (item: unknown) => T): T[] {
  if (!Array.isArray(value)) invalidResponse();
  return value.map(decode);
}

function mediaUrl(value: unknown): string | null {
  if (value === null) return null;
  const raw = text(value);
  if (!raw.startsWith('/twitch/social-media/api/') || raw.startsWith('//') || raw.includes('\\') || Array.from(raw).some((character) => character.charCodeAt(0) < 32)) invalidResponse();
  const parsed = new URL(raw, 'https://preview.invalid');
  if (parsed.origin !== 'https://preview.invalid' || !parsed.pathname.startsWith('/twitch/social-media/api/')) invalidResponse();
  return raw;
}

function decodeTimeRange(value: unknown): AiTimeRange {
  const item = record(value);
  const start_ms = optionalNumber(item.start_ms);
  const end_ms = optionalNumber(item.end_ms);
  if (start_ms === null || end_ms === null || end_ms <= start_ms) invalidResponse();
  return { start_ms, end_ms, description: text(item.description) };
}

function decodeVersion(value: unknown): AiVideoVersion {
  const item = record(value);
  return {
    id: number(item.id, true),
    version_no: number(item.version_no, true),
    created_at: date(item.created_at),
    status: text(item.status),
    duration_ms: optionalNumber(item.duration_ms),
    file_url: mediaUrl(item.file_url),
    parent_version_id: item.parent_version_id === null ? null : number(item.parent_version_id, true),
    job_id: number(item.job_id, true),
    variant_index: number(item.variant_index),
    original_url: mediaUrl(item.original_url),
    decisions: list(item.decisions, decodeTimeRange),
    approved_platforms: decodePlatforms(item.approved_platforms),
    rating: item.rating === 'accepted' || item.rating === 'rejected' ? item.rating : null,
    layout_mode: item.layout_mode === undefined || item.layout_mode === null ? null : text(item.layout_mode),
    streamer_layout_configured: item.streamer_layout_configured === true,
  };
}

function decodePlatforms(value: unknown): string[] {
  const platforms = list(value,text);
  if (platforms.some((platform) => !['youtube','tiktok','instagram'].includes(platform)) || new Set(platforms).size !== platforms.length) invalidResponse();
  return platforms;
}

function decodeProposal(value: unknown): AiRevisionProposal | null {
  if (value === null) return null;
  const proposal = record(value);
  return {summary:text(proposal.summary),impact:typeof proposal.impact === 'string' ? proposal.impact : invalidResponse(),reanalyze:flag(proposal.reanalyze),requires_confirmation:flag(proposal.requires_confirmation),estimated_provider_cost_usd:optionalNumber(proposal.estimated_provider_cost_usd)};
}

export function decodeAiConsent(value: unknown): AiConsent {
  const item = record(value);
  const access = PROVIDER_ACCESS.find((value) => value === item.provider_access) ?? null;
  return { external_ai_consent: flag(item.external_ai_consent), global_allowed: flag(item.global_allowed), provider_access: access };
}

export function decodeAiJob(value: unknown): AiJobView {
  const item = record(value);
  const status = text(item.status);
  if (!['queued', 'preparing', 'transcribing', 'analyzing', 'planning', 'rendering', 'validating', 'review', 'failed', 'awaiting_confirmation', 'rejected'].includes(status)) invalidResponse();
  const cost_basis = text(item.cost_basis);
  if (!['usage_priced_estimate', 'provider_billed', 'subscription_included', 'unknown'].includes(cost_basis)) invalidResponse();
  return {
    id: number(item.id, true),
    clip_db_id: number(item.clip_db_id, true),
    status: status as AiJobStatus,
    stage: text(item.stage),
    created_at: date(item.created_at),
    updated_at: date(item.updated_at),
    completed_at: optionalDate(item.completed_at),
    provider: optionalText(item.provider),
    model: optionalText(item.model),
    original_duration_ms: optionalNumber(item.original_duration_ms),
    output_duration_ms: optionalNumber(item.output_duration_ms),
    processing_ms: optionalNumber(item.processing_ms),
    provider_latency_ms: optionalMilliseconds(item.provider_latency_ms),
    rendering_ms: optionalNumber(item.rendering_ms),
    input_tokens: optionalNumber(item.input_tokens),
    output_tokens: optionalNumber(item.output_tokens),
    estimated_provider_cost_usd: optionalNumber(item.estimated_provider_cost_usd),
    measured_provider_cost_usd: optionalNumber(item.measured_provider_cost_usd),
    cost_basis: cost_basis as AiCostBasis,
    highlights: list(item.highlights, decodeTimeRange),
    decisions: list(item.decisions, decodeTimeRange),
    uncertainty: list(item.uncertainty, text),
    review_reasons: list(item.review_reasons, text),
    original_url: mediaUrl(item.original_url),
    versions: list(item.versions, decodeVersion),
    failure_code: optionalText(item.failure_code),
    failure_message: optionalText(item.failure_message),
    can_retry: flag(item.can_retry),
    root_job_id: number(item.root_job_id,true),
    parent_version_id: item.parent_version_id === null ? null : number(item.parent_version_id,true),
    selected_version_id: item.selected_version_id === null ? null : number(item.selected_version_id,true),
    revision_proposal: decodeProposal(item.revision_proposal),
  };
}

function channelId(twitchUserId: string): string {
  const id = twitchUserId.trim();
  if (!id) throw new Error('Bitte wähle zuerst einen Kanal.');
  return id;
}

function jobId(id: number): number {
  if (!Number.isSafeInteger(id) || id < 1) throw new Error('Der Auftrag konnte nicht zugeordnet werden.');
  return id;
}

export async function fetchAiConsent(twitchUserId: string, signal?: AbortSignal): Promise<AiConsent> {
  const query = new URLSearchParams({ twitch_user_id: channelId(twitchUserId) });
  return decodeAiConsent(await fetchSocialAiJson(`${AI_PREFIX}/consent?${query}`, { signal, cache: 'no-store' }));
}

export async function saveAiConsent(twitchUserId: string, externalAiConsent: boolean): Promise<AiConsent> {
  return decodeAiConsent(await fetchSocialAiJson(`${AI_PREFIX}/consent`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ twitch_user_id: channelId(twitchUserId), external_ai_consent: externalAiConsent }),
  }));
}

export function decodeAiOptions(value: unknown): AiEditorOptions {
  const item = record(value);
  const language = item.publish_language === 'en' || item.publish_language === 'auto' ? item.publish_language : 'de';
  const frame = item.frame_format === 'fill' || item.frame_format === 'full' ? item.frame_format : 'layout';
  return { multi_output_enabled: flag(item.multi_output_enabled), experimental_ai_layout: flag(item.experimental_ai_layout), publish_language: language, frame_format: frame };
}

export async function fetchAiOptions(twitchUserId: string, signal?: AbortSignal): Promise<AiEditorOptions> {
  const query = new URLSearchParams({ twitch_user_id: channelId(twitchUserId) });
  return decodeAiOptions(await fetchSocialAiJson(`${AI_PREFIX}/options?${query}`, { signal, cache: 'no-store' }));
}

export async function saveAiOptions(twitchUserId: string, options: AiEditorOptions): Promise<AiEditorOptions> {
  return decodeAiOptions(await fetchSocialAiJson(`${AI_PREFIX}/options`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ twitch_user_id: channelId(twitchUserId), ...options }),
  }));
}

export async function fetchAiJobs(twitchUserId: string, signal?: AbortSignal): Promise<AiJobView[]> {
  const query = new URLSearchParams({ twitch_user_id: channelId(twitchUserId) });
  const response = record(await fetchSocialAiJson(`${AI_PREFIX}/jobs?${query}`, { signal, cache: 'no-store' }));
  return list(response.items, decodeAiJob);
}

export async function startAiJob(twitchUserId: string, clipDbId: number): Promise<AiJobView> {
  const response = record(await fetchSocialAiJson(`${AI_PREFIX}/jobs`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ twitch_user_id: channelId(twitchUserId), clip_db_id: jobId(clipDbId) }),
  }));
  return decodeAiJob(response.job);
}

export async function fetchAiJob(twitchUserId: string, id: number, signal?: AbortSignal): Promise<AiJobView> {
  const query = new URLSearchParams({ twitch_user_id: channelId(twitchUserId) });
  const result = decodeAiJob(await fetchSocialAiJson(`${AI_PREFIX}/jobs/${jobId(id)}?${query}`, { signal, cache: 'no-store' }));
  if (result.id !== id) invalidResponse();
  return result;
}

export async function retryAiJob(twitchUserId: string, id: number): Promise<AiJobView> {
  const response = record(await fetchSocialAiJson(`${AI_PREFIX}/jobs/${jobId(id)}/retry`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ twitch_user_id: channelId(twitchUserId) }),
  }));
  const result = decodeAiJob(response.job);
  if (result.id !== id) invalidResponse();
  return result;
}

async function versionAction(twitchUserId: string, id: number, version: number, action: string, body: Record<string, unknown> = {}): Promise<AiJobView> {
  const response = record(await fetchSocialAiJson(`${AI_PREFIX}/jobs/${jobId(id)}/versions/${jobId(version)}/${action}`, {
    method:'POST', headers:{'Content-Type':'application/json'},body:JSON.stringify({...body,twitch_user_id:channelId(twitchUserId)}),
  }));
  const result = decodeAiJob(response.job);
  if (action === 'revision' ? result.parent_version_id !== version || result.id === id : result.id !== id) invalidResponse();
  return result;
}

export function reviseAiVersion(channel: string, id: number, version: number, feedback: string): Promise<AiJobView> {
  return versionAction(channel,id,version,'revision',{feedback});
}

export function selectAiVersion(channel: string, id: number, version: number): Promise<AiJobView> {
  return versionAction(channel,id,version,'select');
}

export function decideAiVersion(channel: string, id: number, version: number, accept: boolean): Promise<AiJobView> {
  return versionAction(channel,id,version,'decision',{accept});
}

export function approveAiVersion(channel: string, id: number, version: number, platforms: string[]): Promise<AiJobView> {
  return versionAction(channel,id,version,'approve',{platforms});
}

export async function decideAiProposal(channel: string, id: number, accept: boolean): Promise<AiJobView> {
  const response = record(await fetchSocialAiJson(`${AI_PREFIX}/jobs/${jobId(id)}/proposal`, {
    method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({twitch_user_id:channelId(channel),accept}),
  }));
  const result = decodeAiJob(response.job);
  if (result.id !== id) invalidResponse();
  return result;
}

export async function scheduleAiVersion(channel: string, id: number, version: number, platforms: string[]): Promise<{job: AiJobView; scheduledAt: string}> {
  const response = record(await fetchSocialAiJson(`${AI_PREFIX}/jobs/${jobId(id)}/versions/${jobId(version)}/schedule`, {
    method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({twitch_user_id:channelId(channel),platforms}),
  }));
  const job = decodeAiJob(response.job);
  const reservation = record(response.reservation);
  if (job.id !== id || number(reservation.produced_video_id,true) !== version) invalidResponse();
  const queues = list(reservation.queue_ids,(entry) => {
    if (!Array.isArray(entry) || entry.length !== 2) invalidResponse();
    return {platform:text(entry[0]),id:number(entry[1],true)};
  });
  if (queues.length !== platforms.length || new Set(queues.map((queue) => queue.platform)).size !== platforms.length || queues.some((queue) => !platforms.includes(queue.platform))) invalidResponse();
  return {job,scheduledAt:date(reservation.scheduled_at)};
}

export type AiCopyPlatform = 'youtube' | 'tiktok' | 'instagram';

export interface AiPlatformCopy {
  title: string;
  description: string;
  hashtags: string[];
}

export interface AiVersionMetadata {
  video_id: number;
  editable: boolean;
  targets: AiCopyPlatform[];
  platforms: Record<AiCopyPlatform, AiPlatformCopy>;
}

export const AI_COPY_PLATFORMS: AiCopyPlatform[] = ['youtube', 'tiktok', 'instagram'];

function plainText(value: unknown): string {
  if (typeof value !== 'string') invalidResponse();
  return value;
}

function copyPlatform(value: unknown): AiCopyPlatform {
  const platform = AI_COPY_PLATFORMS.find((entry) => entry === value);
  if (!platform) invalidResponse();
  return platform;
}

function decodeTargets(value: unknown): AiCopyPlatform[] {
  const targets = list(value, copyPlatform);
  if (targets.length === 0 || new Set(targets).size !== targets.length) invalidResponse();
  return AI_COPY_PLATFORMS.filter((platform) => targets.includes(platform));
}

function decodePlatformCopy(value: unknown): AiPlatformCopy {
  const item = record(value);
  return { title: plainText(item.title), description: plainText(item.description), hashtags: list(item.hashtags, plainText) };
}

export function decodeAiVersionMetadata(value: unknown): AiVersionMetadata {
  const item = record(value);
  const platforms = record(item.platforms);
  return {
    video_id: number(item.video_id, true),
    editable: flag(item.editable),
    targets: decodeTargets(item.targets),
    platforms: { youtube: decodePlatformCopy(platforms.youtube), tiktok: decodePlatformCopy(platforms.tiktok), instagram: decodePlatformCopy(platforms.instagram) },
  };
}

export async function fetchAiVersionMetadata(channel: string, videoId: number, signal?: AbortSignal): Promise<AiVersionMetadata> {
  const query = new URLSearchParams({ twitch_user_id: channelId(channel) });
  const result = decodeAiVersionMetadata(await fetchSocialAiJson(`${AI_PREFIX}/videos/${jobId(videoId)}/metadata?${query}`, { signal, cache: 'no-store' }));
  if (result.video_id !== videoId) invalidResponse();
  return result;
}

export async function saveAiVersionMetadata(channel: string, videoId: number, platforms: Record<AiCopyPlatform, AiPlatformCopy>): Promise<AiVersionMetadata> {
  const result = decodeAiVersionMetadata(await fetchSocialAiJson(`${AI_PREFIX}/videos/${jobId(videoId)}/metadata`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ twitch_user_id: channelId(channel), platforms }),
  }));
  if (result.video_id !== videoId) invalidResponse();
  return result;
}

export async function openAiVideoEditor(channel: string, videoId: number): Promise<AiJobView> {
  const response = record(await fetchSocialAiJson(`${AI_PREFIX}/videos/${jobId(videoId)}/editor`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ twitch_user_id: channelId(channel) }),
  }));
  return decodeAiJob(response.job);
}

export async function fetchAiPublishingContext(channel: string, id: number): Promise<boolean> {
  const query = new URLSearchParams({ twitch_user_id: channelId(channel) });
  const item = record(await fetchSocialAiJson(`${AI_PREFIX}/jobs/${jobId(id)}?${query}`, { cache: 'no-store' }));
  return item.publishing_in_autopilot === true;
}
