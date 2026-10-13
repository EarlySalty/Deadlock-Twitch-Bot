import type { ChatAnalytics as ChatAnalyticsType, TimeRange } from '@/types/analytics';
import {
  CHAT_AUDIENCE_TOOLTIP,
  normalizeHourlyActivity,
  resolveChatPenetration,
  resolveMessagesPer100ViewerMinutes,
  resolveQualityMethod,
} from '@/utils/engagementKpi';
import { activeLocale, tr } from '../i18n/dictionary';

export interface ChatAnalyticsViewModel {
  totalChatters: number;
  totalTrackedViewers: number;
  firstTimeChatters: number;
  returningChatters: number;
  returningTrackedViewers: number;
  coreLoyalViewers: number;
  silentCoreLoyalViewers: number;
  coreLoyalViewerRate: number;
  loyaltySessionThreshold: number;
  messagesPer100ViewerMinutes: number | null;
  messagesPer100ViewerMinutesPercentile: number | null;
  messagesPer100ViewerMinutesMedian: number | null;
  messagesPer100ViewerMinutesBenchmarkSessions: number;
  messagesGaugeHasBenchmark: boolean;
  chatterReturnRate: number;
  interactionRate: number | null;
  interactionRateReliable: boolean;
  interactionCoverage: number;
  hourlyChartData: Array<{ hour: number; count: number; label: string }>;
  hasHourlySamples: boolean;
  hoursWithData: number;
  peakHour: { hour: number; count: number };
  dataMethod: string;
  noReturnHistory: boolean;
  chattersApiInactive: boolean;
  newViewerShare: number;
  activeChattersShare: number;
  activeChattersDescription: string;
  chatPenetrationGaugeValue: number | null;
  messagesBenchmarkText: string;
  messagesBenchmarkFootnote: string;
  messagesGaugeProgress: number;
  hourlyChartGradientId: string;
  chatAudienceTooltip: string;
}

export function buildChatAnalyticsViewModel(
  data: ChatAnalyticsType,
  days: TimeRange,
  hourlyChartGradientId: string
): ChatAnalyticsViewModel {
  const totalChatters = data.totalChatterSessions ?? data.uniqueChatters ?? 0;
  const totalTrackedViewers = data.totalTrackedViewers ?? totalChatters;
  const firstTimeChatters = data.firstTimeChatters ?? 0;
  const returningChatters = data.returningChatters ?? Math.max(0, totalChatters - firstTimeChatters);
  const returningTrackedViewers = data.returningTrackedViewers ?? returningChatters;
  const coreLoyalViewers = data.coreLoyalViewers ?? 0;
  const silentCoreLoyalViewers = data.silentCoreLoyalViewers ?? 0;
  const coreLoyalViewerRate =
    data.coreLoyalViewerRate ?? (totalTrackedViewers ? (coreLoyalViewers / totalTrackedViewers) * 100 : 0);
  const loyaltySessionThreshold =
    data.loyaltySessionThreshold ?? (days <= 7 ? 2 : days <= 30 ? 3 : days <= 90 ? 8 : 12);
  const messagesPer100ViewerMinutes = resolveMessagesPer100ViewerMinutes(data);
  const messagesPer100ViewerMinutesPercentile = data.messagesPer100ViewerMinutesPercentile ?? null;
  const messagesPer100ViewerMinutesMedian = data.messagesPer100ViewerMinutesMedian ?? null;
  const messagesPer100ViewerMinutesBenchmarkSessions =
    data.messagesPer100ViewerMinutesBenchmarkSessions ?? 0;
  const messagesGaugeHasBenchmark =
    messagesPer100ViewerMinutesPercentile !== null && messagesPer100ViewerMinutesBenchmarkSessions >= 3;
  const chatterReturnRate =
    data.chatterReturnRate ?? (totalChatters ? (returningChatters / totalChatters) * 100 : 0);
  const penetration = resolveChatPenetration(data);
  const interactionRate = penetration.value;
  const interactionRateReliable = penetration.reliable;
  const interactionCoverage = penetration.coverage;
  const hourlyActivity = normalizeHourlyActivity(data.hourlyActivity);
  const hasHourlySamples = hourlyActivity.some((entry) => entry.count > 0);
  const hoursWithData = hourlyActivity.filter((entry) => entry.count > 0).length;
  const hourlyChartData = hourlyActivity.map((entry) => ({
    ...entry,
    label: `${entry.hour}:00`,
  }));
  const peakHour = hourlyActivity.reduce(
    (best, entry) => (entry.count > best.count ? entry : best),
    hourlyActivity[0] ?? { hour: 0, count: 0 }
  );
  const dataMethod = resolveQualityMethod(data.dataQuality?.method, data.totalMessages > 0);
  const noReturnHistory = chatterReturnRate === 0 && firstTimeChatters >= totalChatters && totalChatters > 0;
  const chattersApiInactive = interactionCoverage === 0 && totalTrackedViewers > 0;
  const newViewerShare = totalChatters > 0 ? (firstTimeChatters / totalChatters) * 100 : 0;
  const activeChattersShare = totalTrackedViewers > 0 ? (totalChatters / totalTrackedViewers) * 100 : 0;
  const activeChattersDescription = totalTrackedViewers > 0
    ? tr('{chatters} von {tracked} getrackten Accounts haben im Zeitraum geschrieben.', {
        chatters: totalChatters.toLocaleString(activeLocale()),
        tracked: totalTrackedViewers.toLocaleString(activeLocale()),
      })
    : tr('Anteil der getrackten Chat-Accounts mit mindestens einer Nachricht im Zeitraum.');
  const chatPenetrationGaugeValue =
    chattersApiInactive && data.legacyInteractionActivePerAvgViewer != null
      ? Math.min(100, data.legacyInteractionActivePerAvgViewer)
      : interactionRate;
  const messagesBenchmarkText = (() => {
    if (messagesPer100ViewerMinutes === null) {
      return tr('Keine Viewer-Minuten im Zeitraum');
    }
    if (
      messagesPer100ViewerMinutesPercentile !== null &&
      messagesPer100ViewerMinutesMedian !== null &&
      messagesPer100ViewerMinutesBenchmarkSessions >= 3
    ) {
      const rating =
        messagesPer100ViewerMinutesPercentile >= 75
          ? tr('Uber deinem ublichen Niveau')
          : messagesPer100ViewerMinutesPercentile >= 40
            ? tr('Im typischen Bereich')
            : tr('Unter deinem ublichen Niveau');
      return tr('{rating} · Rohwert {value} Nachrichten pro 100 Viewer-Minuten · besser als {percentile}% deiner {sessions} Streams', {
        rating,
        value: messagesPer100ViewerMinutes.toFixed(1),
        percentile: messagesPer100ViewerMinutesPercentile.toFixed(0),
        sessions: messagesPer100ViewerMinutesBenchmarkSessions,
      });
    }
    return messagesPer100ViewerMinutesBenchmarkSessions > 0
      ? tr('Rohwert {value} Nachrichten pro 100 Viewer-Minuten · Eigenvergleich noch instabil ({sessions} Streams)', {
          value: messagesPer100ViewerMinutes.toFixed(1),
          sessions: messagesPer100ViewerMinutesBenchmarkSessions,
        })
      : tr('Rohwert {value} Nachrichten pro 100 Viewer-Minuten · Noch keine Vergleichsbasis aus fruheren Streams', {
          value: messagesPer100ViewerMinutes.toFixed(1),
        });
  })();
  const messagesBenchmarkFootnote =
    data.viewerMinutes && data.viewerMinutes > 0
      ? tr('Median: {median} · Basis: {minutes} Viewer-Minuten', {
          median: messagesPer100ViewerMinutesMedian?.toFixed(1) ?? '-',
          minutes: data.viewerMinutes.toFixed(0),
        })
      : tr('Keine Viewer-Minuten im Zeitraum');
  const messagesGaugeProgress =
    messagesGaugeHasBenchmark
      ? Math.min(100, Math.max(0, messagesPer100ViewerMinutesPercentile ?? 0))
      : 0;

  return {
    totalChatters,
    totalTrackedViewers,
    firstTimeChatters,
    returningChatters,
    returningTrackedViewers,
    coreLoyalViewers,
    silentCoreLoyalViewers,
    coreLoyalViewerRate,
    loyaltySessionThreshold,
    messagesPer100ViewerMinutes,
    messagesPer100ViewerMinutesPercentile,
    messagesPer100ViewerMinutesMedian,
    messagesPer100ViewerMinutesBenchmarkSessions,
    messagesGaugeHasBenchmark,
    chatterReturnRate,
    interactionRate,
    interactionRateReliable,
    interactionCoverage,
    hourlyChartData,
    hasHourlySamples,
    hoursWithData,
    peakHour,
    dataMethod,
    noReturnHistory,
    chattersApiInactive,
    newViewerShare,
    activeChattersShare,
    activeChattersDescription,
    chatPenetrationGaugeValue,
    messagesBenchmarkText,
    messagesBenchmarkFootnote,
    messagesGaugeProgress,
    hourlyChartGradientId,
    chatAudienceTooltip: tr(CHAT_AUDIENCE_TOOLTIP),
  };
}
