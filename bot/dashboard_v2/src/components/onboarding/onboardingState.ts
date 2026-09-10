import { createContext, useContext } from 'react';
import type { OnboardingStatus, OnboardingUpdate } from '../../api/onboarding';
import type { OnboardingStepId } from './steps';

export type OnboardingContextValue = {
  enabled: boolean;
  status?: OnboardingStatus;
  loading: boolean;
  pending: boolean;
  error: string | null;
  refresh: () => void;
  save: (update: OnboardingUpdate) => Promise<boolean>;
  open: (id: OnboardingStepId) => Promise<void>;
  pause: () => Promise<void>;
  advance: (step: OnboardingStepId, confirm: boolean) => Promise<void>;
};
export const OnboardingContext = createContext<OnboardingContextValue | null>(null);
export function useOnboarding() {
  const value = useContext(OnboardingContext);
  if (!value) throw new Error('OnboardingProvider fehlt');
  return value;
}
