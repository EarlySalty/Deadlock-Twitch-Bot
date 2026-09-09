import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useAuthStatus } from '@/hooks/useAnalytics';
import { fetchOnboardingStatus, saveOnboardingProgress, type OnboardingStatus, type OnboardingUpdate } from '@/api/onboarding';
import { isPreviewModeEnabled } from '@/preview/routes';
import { resolveEffectiveDemoMode, dashboardRuntimeConfig } from '@/runtimeConfig';
import { OnboardingContext } from './onboardingState';
import { ProgressCoordinator } from './progressCoordinator';
import { isConnectionStep, nextStep, stepDefinition, type OnboardingStepId } from './steps';

export function OnboardingProvider({ children }: { children: ReactNode }) {
  const { data: auth } = useAuthStatus();
  const queryClient = useQueryClient();
  const [notice, setNotice] = useState<string | null>(null);
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
  const mutateAsync = mutation.mutateAsync;
  const coordinator = useMemo(() => new ProgressCoordinator(async update => {
    if (!enabled || !id) return false;
    try { await mutateAsync(update); return true; } catch { return false; }
  }), [enabled, id, mutateAsync]);
  useEffect(() => { coordinator.activate(); return () => coordinator.dispose(); }, [coordinator]);
  const save = (update: OnboardingUpdate) => coordinator.save(update);
  const pause = async () => {
    setLocalPause(id);
    setNotice(null);
    const activeStep = query.data?.active_step ?? 'bookmark';
    requestAnimationFrame(() => {
      const target = document.querySelector<HTMLElement>(`[data-tour-id="${stepDefinition(activeStep).anchor}"]`)
        ?? document.querySelector<HTMLElement>('#einrichtung button, [data-onboarding-restart]');
      target?.focus();
    });
    await coordinator.pause();
  };
  const canNavigate = () => {
    if (!document.querySelector('[data-unsaved="true"]')) { setNotice(null); return true; }
    setNotice('Du hast ungespeicherte Änderungen. Speichere deinen Entwurf oder verwirf ihn bewusst, bevor du den Rundgang öffnest.');
    return false;
  };
  const resume = async (update: OnboardingUpdate, target?: string) => {
    if (!canNavigate()) return;
    await coordinator.resume(update, () => {
      // Auch während des Speicherns kann im Formular ein neuer Entwurf entstehen.
      if (!canNavigate()) { void pause(); return; }
      setLocalPause(null);
      if (update.completed) requestAnimationFrame(() => {
        document.querySelector<HTMLElement>('[data-onboarding-complete], [data-tour-id="onboarding-feedback"]')?.focus();
      });
      if (target && window.location.pathname + window.location.search + window.location.hash !== target) window.location.assign(target);
    });
  };
  const open = (step: OnboardingStepId) => resume({ active_step: step, paused: false }, stepDefinition(step).path);
  const advance = (step: OnboardingStepId, confirm: boolean) => {
    const next = nextStep(step);
    return resume({
      ...(confirm && !isConnectionStep(step) ? { complete_step: step } : {}),
      ...(next ? { active_step: next, paused: false } : { completed: true, paused: true }),
    }, next ? stepDefinition(next).path : undefined);
  };
  const status: OnboardingStatus | undefined = query.data && enabled ? { ...query.data, paused: query.data.paused || localPause === id,
    discord_status: query.isError ? 'error' : query.data.discord_status, steam_status: query.isError ? 'error' : query.data.steam_status } : undefined;
  return <OnboardingContext.Provider value={{ enabled, status, loading: query.isFetching, pending: mutation.isPending,
    error: mutation.isError ? 'Dein Fortschritt wurde nicht gespeichert. Bitte versuche es erneut.' : query.isError ? 'Deine Einrichtung konnte gerade nicht geladen werden.' : null,
    refresh: () => { mutation.reset(); void query.refetch(); void queryClient.invalidateQueries({queryKey: ['internal-home']}); }, save, open, pause, advance }}>
    {children}
    {(notice || (localPause === id && mutation.isError)) && <aside role="alert" className="fixed bottom-4 right-4 z-50 max-w-sm rounded-xl border border-warning bg-card p-4 text-sm text-text-primary shadow-xl">
      <p>{notice ?? 'Deine Pause wurde nicht gespeichert. Du kannst hierbleiben und es erneut versuchen.'}</p>
      {!notice && <button type="button" disabled={mutation.isPending} onClick={() => void pause()} className="mt-2 min-h-11 text-primary underline">Pause speichern</button>}
      <button type="button" onClick={() => { setNotice(null); mutation.reset(); }} className="ml-3 min-h-11 underline">Schließen</button>
    </aside>}
  </OnboardingContext.Provider>;
}
