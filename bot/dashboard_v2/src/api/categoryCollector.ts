import { fetchApi } from './core';

/**
 * Globaler Kategoriesammler (Admin).
 *
 * Ein Endpunkt, ein Zeitraum: GET /twitch/api/v2/admin/category-collector?days=…
 * Stream-Sprache (stream_languages) und Chat-Nachrichtensprache
 * (message_languages, chat_heatmap) sind zwei unabhängige Dimensionen und
 * werden im UI getrennt ausgewiesen. Keine Rohchat-Texte in dieser Antwort.
 */

export const CATEGORY_COLLECTOR_ENDPOINT = '/admin/category-collector';

export const COLLECTOR_DAYS = [7, 30, 90] as const;
export type CollectorDays = (typeof COLLECTOR_DAYS)[number];

export type CollectorStatus = 'not_started' | 'running' | 'stale' | 'disabled' | 'error';

export interface CategoryCollectorMeta {
  days: number;
  period_start: string;
  period_end: string;
  timezone: string;
  first_seen_at: string | null;
  last_completed_poll_at: string | null;
  last_message_at: string | null;
  complete_polls: number;
  incomplete_polls: number;
  dropped_messages: number;
  storage_bytes: number;
  retention_days: number;
  collector_status: CollectorStatus;
  measurement_seconds: number;
}

export interface CategoryCollectorStreamLanguage {
  language: string;
  unique_streams: number;
  unique_channels: number;
  broadcast_hours: number;
  viewer_hours: number;
  avg_viewers: number | null;
}

export interface CategoryCollectorMessageLanguage {
  language: string;
  messages: number;
}

export interface CategoryCollectorTrendPoint {
  bucket_at: string;
  avg_streams: number | null;
  avg_viewers: number | null;
  poll_samples: number;
}

export interface CategoryCollectorTopChannel {
  language: string;
  user_id: string;
  login: string;
  display_name: string;
  broadcast_hours: number;
  viewer_hours: number;
  avg_viewers: number | null;
}

export interface CategoryCollectorChatHeatmapCell {
  language: string;
  hour_utc: number;
  messages: number;
}

export interface CategoryCollectorData {
  meta: CategoryCollectorMeta;
  stream_languages: CategoryCollectorStreamLanguage[];
  message_languages: CategoryCollectorMessageLanguage[];
  trend: CategoryCollectorTrendPoint[];
  top_channels: CategoryCollectorTopChannel[];
  chat_heatmap: CategoryCollectorChatHeatmapCell[];
}

/**
 * Begrenzt die Periodenwahl auf die vertraglich vereinbarten Werte.
 * Alles andere (URL-Manipulation, kaputter State) fällt auf 7 zurück.
 */
export function istSammlerTage(value: unknown): value is CollectorDays {
  return COLLECTOR_DAYS.some((days) => days === value);
}

export function parseCollectorDays(raw: unknown): CollectorDays {
  const value = typeof raw === 'string' && /^(7|30|90)$/.test(raw) ? Number(raw) : raw;
  return istSammlerTage(value) ? value : 7;
}

export async function fetchCategoryCollector(days: CollectorDays): Promise<CategoryCollectorData> {
  return fetchApi<CategoryCollectorData>(CATEGORY_COLLECTOR_ENDPOINT, {
    days: parseCollectorDays(days),
  });
}
