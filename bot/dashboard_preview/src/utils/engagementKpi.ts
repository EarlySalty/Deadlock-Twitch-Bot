export const CHAT_AUDIENCE_TOOLTIP =
  'Chatters-API zeigt Chat-Audience, nicht alle Video-Viewer.';

export interface EngagementDataQuality {
  method?: string;
  passiveViewerSamples?: number;
  chattersCoverage?: number;
  chattersApiCoverage?: number;
  interactionCoverage?: number;
}

export interface EngagementSource {
  chatPenetrationPct?: number | null;
  chatPenetrationReliable?: boolean;
  interactionRateActivePerViewer?: number | null;
  interactionRateReliable?: boolean;
  activeRatio?: number;
  messagesPer100ViewerMinutes?: number | null;
  viewerMinutes?: number;
  totalMessages?: number;
  dataQuality?: EngagementDataQuality;
}

export function getChattersCoverage(dataQuality?: EngagementDataQuality): number {
  const fromCoverage = dataQuality?.chattersCoverage;
  if (typeof fromCoverage === 'number' && Number.isFinite(fromCoverage)) {
    return Math.max(0, Math.min(1, fromCoverage));
  }
  const fromLegacyCoverage = dataQuality?.chattersApiCoverage;
  if (typeof fromLegacyCoverage === 'number' && Number.isFinite(fromLegacyCoverage)) {
    return Math.max(0, Math.min(1, fromLegacyCoverage));
  }
  const fromInteractionCoverage = dataQuality?.interactionCoverage;
  if (typeof fromInteractionCoverage === 'number' && Number.isFinite(fromInteractionCoverage)) {
    return Math.max(0, Math.min(1, fromInteractionCoverage));
  }
  return 0;
}

function coercePercent(value: number | null | undefined): number | null {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return null;
  }
  return Math.max(0, Math.min(100, value));
}

export function resolveChatPenetration(source: EngagementSource): {
  value: number | null;
  reliable: boolean;
  coverage: number;
} {
  const coverage = getChattersCoverage(source.dataQuality);
  const fallbackFromRatio =
    typeof source.activeRatio === 'number' && Number.isFinite(source.activeRatio)
      ? source.activeRatio * 100
      : null;
  const value = coercePercent(
    source.chatPenetrationPct ?? source.interactionRateActivePerViewer ?? fallbackFromRatio
  );

  const passiveViewerSamples = source.dataQuality?.passiveViewerSamples ?? 0;
  const fallbackReliable = passiveViewerSamples >= 1 && coverage >= 0.2;
  const reliable =
    source.chatPenetrationReliable ??
    source.interactionRateReliable ??
    fallbackReliable;

  return { value, reliable: Boolean(reliable), coverage };
}

export function resolveMessagesPer100ViewerMinutes(source: EngagementSource): number | null {
  if (
    typeof source.messagesPer100ViewerMinutes === 'number' &&
    Number.isFinite(source.messagesPer100ViewerMinutes)
  ) {
    return source.messagesPer100ViewerMinutes;
  }
  const viewerMinutes = source.viewerMinutes ?? 0;
  const totalMessages = source.totalMessages ?? 0;
  if (viewerMinutes > 0 && totalMessages >= 0) {
    return (totalMessages / viewerMinutes) * 100;
  }
  return null;
}

export function resolveQualityMethod(
  method: string | undefined,
  hasMessages: boolean
): 'real_samples' | 'low_coverage' | 'no_data' {
  if (method === 'real_samples' || method === 'low_coverage' || method === 'no_data') {
    return method;
  }
  return hasMessages ? 'low_coverage' : 'no_data';
}

export function normalizeHourlyActivity(
  activity: Array<{ hour: number; count: number }> | undefined
): Array<{ hour: number; count: number }> {
  const safe = Array.isArray(activity) ? activity : [];
  const byHour = new Map<number, number>();
  for (const entry of safe) {
    const hour = Number(entry?.hour);
    const count = Number(entry?.count);
    if (!Number.isFinite(hour) || !Number.isFinite(count)) {
      continue;
    }
    const normalizedHour = Math.max(0, Math.min(23, Math.trunc(hour)));
    byHour.set(normalizedHour, Math.max(0, Math.trunc(count)));
  }
  return Array.from({ length: 24 }, (_, hour) => ({
    hour,
    count: byHour.get(hour) ?? 0,
  }));
}

export function formatPercent(value: number | null, digits = 1): string {
  if (value === null || !Number.isFinite(value)) {
    return '-';
  }
  return `${value.toFixed(digits)}%`;
}
