import { OnboardingWizard } from '@/components/onboarding/OnboardingWizard';

interface OnboardingPageProps {
  onNavigateOverview?: () => void;
}

export function OnboardingPage({ onNavigateOverview }: OnboardingPageProps) {
  return <OnboardingWizard onNavigateOverview={onNavigateOverview} />;
}
