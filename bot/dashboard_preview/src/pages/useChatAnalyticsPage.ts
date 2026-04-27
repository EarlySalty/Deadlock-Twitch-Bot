import { useId, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { fetchChatAnalytics } from '@/api/analytics';
import {
  useCoaching,
  useChatHypeTimeline,
  useChatContentAnalysis,
  useChatSocialGraph,
} from '@/hooks/useAnalytics';
import type { ChatAnalytics as ChatAnalyticsType, TimeRange } from '@/types/analytics';

const CHAT_SOCIAL_GRAPH_ENABLED = false;

export function useChatAnalyticsPage(streamer: string, days: TimeRange) {
  const { data, isLoading } = useQuery<ChatAnalyticsType>({
    queryKey: ['chatAnalytics', streamer, days],
    queryFn: () => fetchChatAnalytics(streamer, days),
    enabled: !!streamer,
  });

  const { data: coachingData } = useCoaching(streamer, days);
  const [selectedSessionId, setSelectedSessionId] = useState<number | undefined>(undefined);
  const { data: hypeData } = useChatHypeTimeline(streamer, selectedSessionId);
  const { data: contentData } = useChatContentAnalysis(streamer, days);
  const socialGraphStreamer = CHAT_SOCIAL_GRAPH_ENABLED ? streamer : null;
  const { data: socialData } = useChatSocialGraph(socialGraphStreamer, days);
  const hourlyChartGradientId = `hourly-chat-${useId().replace(/:/g, '')}`;

  return {
    data,
    isLoading,
    coachingData,
    selectedSessionId,
    setSelectedSessionId,
    hypeData,
    contentData,
    socialData,
    hourlyChartGradientId,
    chatSocialGraphEnabled: CHAT_SOCIAL_GRAPH_ENABLED,
  };
}
