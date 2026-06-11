import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { AdminConfigScope, LegalPageSlug, MarketShareScope, StreamerView } from '@/api/types';
import {
  addStreamer,
  archiveStreamer,
  blockStreamer,
  clearManualPlanOverride,
  createChangelogEntry,
  fetchMarketShare,
  fetchScopeStatus,
  fetchAnnouncements,
  fetchAffiliateDetail,
  fetchAffiliateGutschriften,
  fetchAffiliatesList,
  fetchAffiliateStats,
  fetchAllGutschriften,
  fetchAuditLog,
  fetchAdminStreamerDetail,
  fetchAdminStreamers,
  fetchConfigOverview,
  fetchDashboardOverview,
  fetchDatabaseStats,
  fetchErrorLogs,
  fetchEventSubStatus,
  fetchLegalPage,
  fetchRoadmap,
  generateGutschriften,
  saveManualPlanOverride,
  saveAnnouncements,
  saveLegalPage,
  saveRoadmap,
  sendPartnerChatAction,
  fetchSubscriptions,
  fetchSystemHealth,
  fetchEngagementSettings,
  toggleEngagement,
  removeStreamer,
  reloadBot,
  toggleStreamerDiscordFlag,
  toggleAffiliateActive,
  updateStreamerDiscordProfile,
  updateChatConfig,
  updatePromoConfig,
  updateRaidConfig,
  verifyStreamer,
} from '@/api/client';

function invalidateStreamerQueries(queryClient: ReturnType<typeof useQueryClient>, login?: string) {
  void queryClient.invalidateQueries({ queryKey: ['admin-streamers'] });
  void queryClient.invalidateQueries({ queryKey: ['admin-dashboard-overview'] });
  void queryClient.invalidateQueries({ queryKey: ['admin-scope-status'] });
  if (login) {
    void queryClient.invalidateQueries({ queryKey: ['admin-streamer-detail', login] });
  } else {
    void queryClient.invalidateQueries({ queryKey: ['admin-streamer-detail'] });
  }
}

export function useDashboardOverview() {
  return useQuery({
    queryKey: ['admin-dashboard-overview'],
    queryFn: fetchDashboardOverview,
    staleTime: 60_000,
  });
}

export function useStreamers(view: StreamerView = 'active') {
  return useQuery({
    queryKey: ['admin-streamers', view],
    queryFn: () => fetchAdminStreamers(view),
    staleTime: 30_000,
  });
}

export function useStreamerDetail(login: string | undefined) {
  return useQuery({
    queryKey: ['admin-streamer-detail', login],
    queryFn: () => fetchAdminStreamerDetail(login!),
    enabled: Boolean(login),
    staleTime: 30_000,
  });
}

export function useSystemHealth() {
  return useQuery({
    queryKey: ['admin-system-health'],
    queryFn: fetchSystemHealth,
    staleTime: 20_000,
    refetchInterval: 30_000,
  });
}

export function useScopeStatus() {
  return useQuery({
    queryKey: ['admin-scope-status'],
    queryFn: fetchScopeStatus,
    staleTime: 30_000,
    refetchInterval: 60_000,
  });
}

export function useEventSubStatus() {
  return useQuery({
    queryKey: ['admin-eventsub-status'],
    queryFn: fetchEventSubStatus,
    staleTime: 20_000,
    refetchInterval: 45_000,
  });
}

export function useDatabaseStats() {
  return useQuery({
    queryKey: ['admin-database-stats'],
    queryFn: fetchDatabaseStats,
    staleTime: 120_000,
  });
}

export function useErrorLogs(page: number, pageSize: number) {
  return useQuery({
    queryKey: ['admin-error-logs', page, pageSize],
    queryFn: () => fetchErrorLogs(page, pageSize),
    staleTime: 15_000,
  });
}

export function useAuditLog(params: { since?: string; limit?: number; source?: string } = {}) {
  return useQuery({
    queryKey: ['admin-audit-log', params.since ?? '', params.limit ?? 100, params.source ?? ''],
    queryFn: () => fetchAuditLog(params),
    staleTime: 30_000,
  });
}

export function useConfigOverview(scope?: AdminConfigScope) {
  return useQuery({
    queryKey: ['admin-config-overview', scope ?? 'default'],
    queryFn: () => fetchConfigOverview(scope),
    staleTime: 30_000,
  });
}

export function useAnnouncements() {
  return useQuery({
    queryKey: ['admin-announcements'],
    queryFn: fetchAnnouncements,
    staleTime: 30_000,
  });
}

export function useSaveAnnouncements() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: string) => saveAnnouncements(body),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin-announcements'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-config-overview'] });
    },
  });
}

export function useRoadmap() {
  return useQuery({
    queryKey: ['admin-roadmap'],
    queryFn: fetchRoadmap,
    staleTime: 30_000,
  });
}

export function useSaveRoadmap() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: string) => saveRoadmap(body),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin-roadmap'] });
    },
  });
}

export function useLegalPage(slug: LegalPageSlug) {
  return useQuery({
    queryKey: ['admin-legal-page', slug],
    queryFn: () => fetchLegalPage(slug),
    staleTime: 30_000,
  });
}

export function useSaveLegalPage(slug: LegalPageSlug) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (payload: { title?: string; body: string }) => saveLegalPage(slug, payload),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin-legal-page', slug] });
    },
  });
}

export function useCreateChangelogEntry() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: createChangelogEntry,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin-dashboard-overview'] });
    },
  });
}

export function useSubscriptions() {
  return useQuery({
    queryKey: ['admin-subscriptions'],
    queryFn: fetchSubscriptions,
    staleTime: 120_000,
  });
}

export function useAffiliatesList() {
  return useQuery({
    queryKey: ['admin-affiliates'],
    queryFn: fetchAffiliatesList,
    staleTime: 45_000,
  });
}

export function useAffiliateStats() {
  return useQuery({
    queryKey: ['admin-affiliate-stats'],
    queryFn: fetchAffiliateStats,
    staleTime: 30_000,
  });
}

export function useAffiliateDetail(login: string | undefined) {
  return useQuery({
    queryKey: ['admin-affiliate-detail', login],
    queryFn: () => fetchAffiliateDetail(login!),
    enabled: Boolean(login),
    staleTime: 30_000,
  });
}

export function useAllGutschriften() {
  return useQuery({
    queryKey: ['admin-gutschriften'],
    queryFn: fetchAllGutschriften,
    staleTime: 60_000,
  });
}

export function useAffiliateGutschriften(login: string | undefined) {
  return useQuery({
    queryKey: ['admin-affiliate-gutschriften', login],
    queryFn: () => fetchAffiliateGutschriften(login!),
    enabled: Boolean(login),
    staleTime: 60_000,
  });
}

export function useAddStreamer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: addStreamer,
    onSuccess: () => {
      invalidateStreamerQueries(queryClient);
    },
  });
}

export function useRemoveStreamer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: removeStreamer,
    onSuccess: (_result, login) => {
      invalidateStreamerQueries(queryClient, login);
    },
  });
}

export function useVerifyStreamer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ login, mode }: { login: string; mode?: 'permanent' | 'temp' | 'failed' | 'clear' }) =>
      verifyStreamer(login, mode),
    onSuccess: (_result, variables) => {
      invalidateStreamerQueries(queryClient, variables.login);
    },
  });
}

export function useArchiveStreamer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      login,
      mode,
    }: {
      login: string;
      mode?: 'archive' | 'unarchive' | 'toggle';
    }) => archiveStreamer(login, mode),
    onSuccess: (_result, variables) => {
      invalidateStreamerQueries(queryClient, variables.login);
    },
  });
}

export function useBlockStreamer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      login,
      mode,
    }: {
      login: string;
      mode?: 'block' | 'unblock' | 'toggle_block';
    }) => blockStreamer(login, mode),
    onSuccess: (_result, variables) => {
      invalidateStreamerQueries(queryClient, variables.login);
    },
  });
}

export function useUpdateStreamerDiscordProfile() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: updateStreamerDiscordProfile,
    onSuccess: (_result, variables) => {
      invalidateStreamerQueries(queryClient, variables.login);
    },
  });
}

export function useToggleStreamerDiscordFlag() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ login, mode }: { login: string; mode: 'mark' | 'unmark' }) =>
      toggleStreamerDiscordFlag(login, mode),
    onSuccess: (_result, variables) => {
      invalidateStreamerQueries(queryClient, variables.login);
    },
  });
}

export function useManualPlanOverride() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: saveManualPlanOverride,
    onSuccess: (_result, variables) => {
      invalidateStreamerQueries(queryClient, variables.login);
      void queryClient.invalidateQueries({ queryKey: ['admin-subscriptions'] });
    },
  });
}

export function useClearManualPlanOverride() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: clearManualPlanOverride,
    onSuccess: (_result, login) => {
      invalidateStreamerQueries(queryClient, login);
      void queryClient.invalidateQueries({ queryKey: ['admin-subscriptions'] });
    },
  });
}

export function usePartnerChatAction() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: sendPartnerChatAction,
    onSuccess: (_result, variables) => {
      invalidateStreamerQueries(queryClient, variables.login);
    },
  });
}

export function useReloadBot() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: reloadBot,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin-config-overview'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-dashboard-overview'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-system-health'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-eventsub-status'] });
    },
  });
}

export function useToggleAffiliateActive() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (login: string) => toggleAffiliateActive(login),
    onSuccess: (_result, login) => {
      void queryClient.invalidateQueries({ queryKey: ['admin-affiliates'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-affiliate-stats'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-affiliate-detail', login] });
    },
  });
}

export function useGenerateGutschriften() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: generateGutschriften,
    onSuccess: (_result, variables) => {
      void queryClient.invalidateQueries({ queryKey: ['admin-affiliates'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-affiliate-stats'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-gutschriften'] });
      if (variables?.affiliateLogin) {
        void queryClient.invalidateQueries({ queryKey: ['admin-affiliate-detail', variables.affiliateLogin] });
        void queryClient.invalidateQueries({
          queryKey: ['admin-affiliate-gutschriften', variables.affiliateLogin],
        });
        return;
      }
      void queryClient.invalidateQueries({ queryKey: ['admin-affiliate-detail'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-affiliate-gutschriften'] });
    },
  });
}

export function usePromoConfigMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: updatePromoConfig,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin-config-overview'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-dashboard-overview'] });
    },
  });
}

export function useRaidConfigMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: updateRaidConfig,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin-config-overview'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-streamer-detail'] });
    },
  });
}

export function useChatConfigMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: updateChatConfig,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin-config-overview'] });
      void queryClient.invalidateQueries({ queryKey: ['admin-streamer-detail'] });
    },
  });
}

export function useEngagementSettings(login: string | undefined) {
  return useQuery({
    queryKey: ['admin-engagement-settings', login],
    queryFn: () => fetchEngagementSettings(login!),
    enabled: Boolean(login),
    staleTime: 30_000,
  });
}

export function useEngagementToggle() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ login, enabled }: { login: string; enabled: boolean }) =>
      toggleEngagement(login, enabled),
    onSuccess: (_result, variables) => {
      void queryClient.invalidateQueries({
        queryKey: ['admin-engagement-settings', variables.login],
      });
    },
  });
}

export function useMarketShare(days: number, scope: MarketShareScope) {
  return useQuery({
    queryKey: ['admin-market-share', days, scope],
    queryFn: () => fetchMarketShare(days, scope),
    staleTime: 60_000,
  });
}
