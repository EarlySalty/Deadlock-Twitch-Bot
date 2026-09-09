import { createContext, useContext, useState, type ReactNode } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useAuthStatus } from '@/hooks/useAnalytics';
import { fetchOnboardingStatus, saveOnboardingProgress, type OnboardingStatus, type OnboardingUpdate } from '@/api/onboarding';
import { isPreviewModeEnabled } from '@/preview/routes';
import { resolveEffectiveDemoMode, dashboardRuntimeConfig } from '@/runtimeConfig';
import { stepDefinition, type OnboardingStepId } from './steps';

type OnboardingContextValue = {
  enabled: boolean;
  status?: OnboardingStatus;
  loading: boolean;
  pending: boolean;
  error: string | null;
  refresh: () => void;
  save: (update: OnboardingUpdate) => Promise<boolean>;
  open: (id: OnboardingStepId) => Promise<void>;
  pause: () => Promise<void>;
};
const OnboardingContext = createContext<OnboardingContextValue | null>(null);
export function OnboardingProvider({ children }: { children: ReactNode }) {
  const { data: auth } = useAuthStatus();
  const queryClient = useQueryClient();
  const [localPause, setLocalPause] = useState<string | null>(null);
  const id = auth?.twitchUserId ?? '';
  const enabled = Boolean(id && auth?.authenticated && !auth.isAdmin && !auth.isLocalhost && !isPreviewModeEnabled()
    && !resolveEffectiveDemoMode({pathname: window.location.pathname, runtimeConfig: dashboardRuntimeConfig}));
  const queryKey = ['streamer-onboarding', id];
  const query = useQuery({ queryKey, queryFn: fetchOnboardingStatus, enabled, staleTime: 30_000, refetchOnWindowFocus: 'always' });
  const mutation = useMutation({
    mutationFn: async (update: OnboardingUpdate) => {
      const owner = id;
      await queryClient.cancelQueries({queryKey});
      const result = await saveOnboardingProgress(update, auth?.csrfToken ?? auth?.csrf_token);
      return {...result, owner};
    },
    onSuccess: ({ progress, owner }) => {
      queryClient.setQueryData<OnboardingStatus>(['streamer-onboarding', owner], previous => previous ? { ...previous, ...progress } : undefined);
    },
  });
  const save = async (update: OnboardingUpdate) => {
    if (!enabled || mutation.isPending) return false;
    try { await mutation.mutateAsync(update); return true; } catch { return false; }
  };
  const pause = async () => {
    // Auch bei Netzausfall lässt sich die Hilfe schließen. Der Speicherfehler bleibt sichtbar.
    setLocalPause(id);
    await save({paused: true});
  };
  const open = async (step: OnboardingStepId) => {
    if (document.querySelector('[data-unsaved="true"]')) {
      return; // Die sichtbare Hilfe erklärt direkt beim Formular, warum Weiter gesperrt ist.
    }
    if (await save({ active_step: step, paused: false })) {
      setLocalPause(null);
      const target = stepDefinition(step).path;
      if (window.location.pathname + window.location.search + window.location.hash !== target) window.location.assign(target);
    }
  };
  const status: OnboardingStatus | undefined = query.data && enabled ? { ...query.data, paused: query.data.paused || localPause === id,
    discord_status: query.isError ? 'error' : query.data.discord_status, steam_status: query.isError ? 'error' : query.data.steam_status } : undefined;
  return <OnboardingContext.Provider value={{ enabled, status, loading: query.isFetching, pending: mutation.isPending,
    error: mutation.isError ? 'Dein Fortschritt wurde nicht gespeichert. Bitte versuche es erneut.' : query.isError ? 'Deine Einrichtung konnte gerade nicht geladen werden.' : null,
    refresh: () => { mutation.reset(); void query.refetch(); void queryClient.invalidateQueries({queryKey: ['internal-home']}); }, save, open, pause }}>
    {children}
  </OnboardingContext.Provider>;
}
export function useOnboarding() {
  const value = useContext(OnboardingContext);
  if (!value) throw new Error('OnboardingProvider fehlt');
  return value;
}
