import { tr } from '../../i18n/dictionary';

export const ONBOARDING_STEPS = [
  { id: 'bookmark', get title() { return tr('Dashboard als Lesezeichen speichern'); }, get short() { return tr('Dashboard als Lesezeichen speichern'); }, path: '/twitch/dashboard', anchor: 'onboarding-bookmark' },
  { id: 'discord', get title() { return tr('Zuerst Discord verbinden'); }, get short() { return tr('Discord verbinden'); }, path: '/twitch/verwaltung#konto', anchor: 'onboarding-discord' },
  { id: 'steam', get title() { return tr('Danach Steam verbinden'); }, get short() { return tr('Steam verbinden'); }, path: '/twitch/verwaltung#konto', anchor: 'onboarding-steam' },
  { id: 'chat', get title() { return tr('Dein Chat, deine Einstellungen'); }, get short() { return tr('Chat-Befehle kennenlernen'); }, path: '/twitch/verwaltung#chat', anchor: 'onboarding-chat' },
  { id: 'bot', get title() { return tr('Du bestimmst, was der Bot übernimmt'); }, get short() { return tr('Bot & Schutz kennenlernen'); }, path: '/twitch/verwaltung#bot', anchor: 'onboarding-bot' },
  { id: 'overlay', get title() { return tr('Ein Overlay, das zu dir passt'); }, get short() { return tr('Overlay anpassen'); }, path: '/twitch/verwaltung#overlay', anchor: 'onboarding-overlay', optional: true },
  { id: 'advertising', get title() { return tr('Werbung bewusst steuern'); }, get short() { return tr('Werbemanager entdecken'); }, path: '/twitch/verwaltung#werbung', anchor: 'onboarding-advertising', optional: true },
  { id: 'feedback', get title() { return tr('Kritik ist ausdrücklich erwünscht'); }, get short() { return tr('Kritik und Wünsche'); }, path: '/twitch/dashboard#feedback', anchor: 'onboarding-feedback' },
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
