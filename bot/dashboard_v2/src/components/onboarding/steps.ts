export const ONBOARDING_STEPS = [
  { id: 'bookmark', title: 'Dashboard als Lesezeichen speichern', short: 'Dashboard wiederfinden', path: '/twitch/dashboard', anchor: 'onboarding-bookmark' },
  { id: 'discord', title: 'Zuerst Discord verbinden', short: 'Discord verbinden', path: '/twitch/verwaltung#konto', anchor: 'onboarding-discord' },
  { id: 'steam', title: 'Danach Steam verbinden', short: 'Steam verbinden', path: '/twitch/verwaltung#konto', anchor: 'onboarding-steam' },
  { id: 'chat', title: 'Dein Chat, deine Einstellungen', short: 'Chat-Befehle kennenlernen', path: '/twitch/verwaltung#chat', anchor: 'onboarding-chat' },
  { id: 'bot', title: 'Du bestimmst, was der Bot übernimmt', short: 'Bot & Schutz kennenlernen', path: '/twitch/verwaltung#bot', anchor: 'onboarding-bot' },
  { id: 'overlay', title: 'Ein Overlay, das zu dir passt', short: 'Overlay anpassen', path: '/twitch/verwaltung#overlay', anchor: 'onboarding-overlay', optional: true },
  { id: 'advertising', title: 'Werbung bewusst steuern', short: 'Werbemanager entdecken', path: '/twitch/verwaltung#werbung', anchor: 'onboarding-advertising', optional: true },
  { id: 'feedback', title: 'Kritik ist ausdrücklich erwünscht', short: 'Kritik und Wünsche', path: '/twitch/dashboard#feedback', anchor: 'onboarding-feedback' },
] as const;
export type OnboardingStepId = typeof ONBOARDING_STEPS[number]['id'];
export function stepDefinition(id: string) { return ONBOARDING_STEPS.find(step => step.id === id) ?? ONBOARDING_STEPS[0]; }
export function nextStep(id: OnboardingStepId): OnboardingStepId | null {
  return ONBOARDING_STEPS[ONBOARDING_STEPS.findIndex(step => step.id === id) + 1]?.id ?? null;
}
export function isConnectionStep(id: OnboardingStepId) { return id === 'discord' || id === 'steam'; }
export function stepState(id: OnboardingStepId, status: { completed_step_ids: OnboardingStepId[]; discord_status: string; steam_status: string }): 'done' | 'open' | 'error' {
  if (isConnectionStep(id)) {
    const value = id === 'discord' ? status.discord_status : status.steam_status;
    return value === 'connected' ? 'done' : value === 'error' ? 'error' : 'open';
  }
  return status.completed_step_ids.includes(id) ? 'done' : 'open';
}
export function canonicalBookmarkLocation(location: Pick<Location, 'origin' | 'pathname' | 'search' | 'hash'>): boolean {
  return location.origin === 'https://deutsche-deadlock-community.de' && location.pathname === '/twitch/dashboard' && !location.search && !location.hash;
}
