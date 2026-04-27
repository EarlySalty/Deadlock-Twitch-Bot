import { AlertCircle, Loader2, MessageCircle } from 'lucide-react';

import type { TimeRange } from '@/types/analytics';

import { ChatAnalyticsContent } from './chatAnalyticsContent';
import { useChatAnalyticsPage } from './useChatAnalyticsPage';
import { buildChatAnalyticsViewModel } from './chatAnalyticsViewModel';

interface ChatAnalyticsProps {
  streamer: string;
  days: TimeRange;
}

export function ChatAnalytics({ streamer, days }: ChatAnalyticsProps) {
  const {
    data,
    isLoading,
    coachingData,
    selectedSessionId,
    setSelectedSessionId,
    hypeData,
    contentData,
    socialData,
    hourlyChartGradientId,
    chatSocialGraphEnabled,
  } = useChatAnalyticsPage(streamer, days);

  if (!streamer) {
    return (
      <div className="flex h-64 flex-col items-center justify-center">
        <AlertCircle className="mb-4 h-12 w-12 text-text-secondary" />
        <p className="text-lg text-text-secondary">Wähle einen Streamer aus</p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex h-64 items-center justify-center">
        <Loader2 className="h-8 w-8 animate-spin text-primary" />
      </div>
    );
  }

  if (!data) {
    return (
      <div className="flex h-64 flex-col items-center justify-center">
        <MessageCircle className="mb-4 h-12 w-12 text-text-secondary" />
        <p className="text-lg text-text-secondary">Keine Chat-Daten verfügbar</p>
      </div>
    );
  }

  const model = buildChatAnalyticsViewModel(data, days, hourlyChartGradientId);

  return (
    <ChatAnalyticsContent
      data={data}
      days={days}
      model={model}
      coachingData={coachingData}
      selectedSessionId={selectedSessionId}
      setSelectedSessionId={setSelectedSessionId}
      hypeData={hypeData}
      contentData={contentData}
      socialData={socialData}
      chatSocialGraphEnabled={chatSocialGraphEnabled}
    />
  );
}

export default ChatAnalytics;
