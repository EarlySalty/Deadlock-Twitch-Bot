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
import { shouldFetchChatHypeTimeline } from './chatAnalyticsQueries';

const CHAT_SOCIAL_GRAPH_ENABLED = false;

export function useChatAnalyticsPage(streamer: string, days: TimeRange) {
  const { data, isLoading } = useQuery<ChatAnalyticsType>({
    queryKey: ['chatAnalytics', streamer, days],
    queryFn: () => fetchChatAnalytics(streamer, days),
    enabled: !!streamer,
  });

  const { data: coachingData } = useCoaching(streamer, days);
  const [selectedSessionId, setSelectedSessionId] = useState<number | undefined>(undefined);
  const hypeStreamer = shouldFetchChatHypeTimeline(streamer, selectedSessionId)
    ? streamer
    : null;
  const { data: hypeData } = useChatHypeTimeline(hypeStreamer, selectedSessionId);
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
