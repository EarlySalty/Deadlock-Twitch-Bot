import type {
  CasterBuiltinEventLogo,
  CasterCountdown,
  CasterEvent,
  CasterEventLogo,
  CasterScene,
  CasterSceneTeam,
} from '@/api/client';

export const PUBLIC_ORIGIN = 'https://deutsche-deadlock-community.de';
export const OVERLAY_BASE = `${PUBLIC_ORIGIN}/twitch/caster-overlay`;
export const BRAND_URL = '/twitch/caster-overlay/builtin/dachlock.png';

export const builtinEventLogos: Array<{ id: CasterBuiltinEventLogo; label: string; url: string }> = [
  { id: 'wochenende', label: 'Wochenende Scrim Match', url: '/twitch/caster-overlay/builtin/wochenende.webp' },
  { id: 'community', label: 'Deutsche Deadlock Community', url: '/twitch/caster-overlay/builtin/community.webp' },
  { id: 'fun_turnier', label: 'Fun Turnier', url: '/twitch/caster-overlay/builtin/fun-turnier.webp' },
];

export const overlayPages = [
  { id: 'caster', label: 'Caster-Szene', url: OVERLAY_BASE },
  { id: 'scoreboard', label: 'Scoreboard', url: `${OVERLAY_BASE}/scoreboard` },
  { id: 'versus', label: 'Versus-Bild', url: `${OVERLAY_BASE}/versus` },
  { id: 'countdown', label: 'Countdown', url: `${OVERLAY_BASE}/countdown` },
] as const;

export type OverlayPageId = (typeof overlayPages)[number]['id'];

export function publicUrl(path: string | null) {
  return path ? `${PUBLIC_ORIGIN}${path}` : null;
}

export function assetUrl(id: string) {
  return `/twitch/caster-overlay/assets/${encodeURIComponent(id)}`;
}

export function eventLogoUrl(logo: CasterEventLogo): string | null {
  if (logo.kind === 'none') return null;
  if (logo.kind === 'asset') return assetUrl(logo.assetId);
  return builtinEventLogos.find((item) => item.id === logo.id)?.url ?? null;
}

export function normalizedEvent(event: CasterEvent | undefined): CasterEvent {
  return {
    logo: event?.logo ?? { kind: 'builtin', id: 'wochenende' },
    showBrand: event?.showBrand ?? true,
  };
}

export function normalizedCountdown(countdown: CasterCountdown | undefined): CasterCountdown {
  return { title: countdown?.title ?? 'STREAM STARTET IN', target: countdown?.target ?? null };
}

function publicTeam(team: CasterSceneTeam | null | undefined) {
  if (!team) return null;
  return {
    name: team.name,
    short: team.short ?? null,
    logoUrl: team.logoAssetId ? assetUrl(team.logoAssetId) : null,
    players: team.players,
    coach: team.coach?.name || null,
  };
}

export function previewPayload(scene: CasterScene) {
  const teamA = publicTeam(scene.matchContext.teamA);
  const teamB = publicTeam(scene.matchContext.teamB);
  return {
    type: 'ddc-caster-preview',
    layout: scene.layout,
    slots: scene.slots.map((id) => scene.roster.find((person) => person.id === id) ?? null),
    matchContext: scene.matchContext,
    teams: scene.sidesSwapped ? { left: teamB, right: teamA } : { left: teamA, right: teamB },
    sidesSwapped: scene.sidesSwapped,
    event: { logoUrl: eventLogoUrl(scene.event.logo), showBrand: scene.event.showBrand, brandUrl: BRAND_URL },
    countdown: { title: scene.countdown.title, target: scene.countdown.target ?? null },
  };
}

export function toLocalInput(value: string | null | undefined) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60000);
  return local.toISOString().slice(0, 16);
}

export function fromLocalInput(value: string) {
  if (!value) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date.toISOString();
}
