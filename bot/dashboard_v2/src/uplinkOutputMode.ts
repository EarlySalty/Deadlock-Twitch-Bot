import type { UplinkDestination } from './api/uplink';
import { profilText } from './uplinkBetrieb';

export type UplinkTwitchOutputMode = 'single' | 'enhanced';

export const TWITCH_OUTPUT_LABEL: Record<UplinkTwitchOutputMode, string> = {
  single: 'Einzelstream',
  enhanced: 'Enhanced Broadcasting',
};

const bekannt = (mode: unknown): UplinkTwitchOutputMode | null =>
  mode === 'single' || mode === 'enhanced' ? mode : null;

/** Ein Entwurf überlebt Refetches; laufende Ausgabe wird nie aus dem Wunsch abgeleitet. */
export function twitchOutputFormular(ziel: UplinkDestination | undefined, entwurf: UplinkTwitchOutputMode | null) {
  const twitch = ziel?.platform === 'twitch' ? ziel : undefined;
  const gespeichert = bekannt(twitch?.requested_output_mode);
  const sendet = twitch?.output_state === 'sending' && !twitch.blocked;
  const aktiv = sendet ? bekannt(twitch.active_output_mode) : null;
  const profile = sendet && Array.isArray(twitch.active_profiles) ? twitch.active_profiles : [];
  return {
    auswahl: entwurf ?? gespeichert,
    gespeichert,
    aktiv,
    geaendert: entwurf !== null && entwurf !== gespeichert,
    stufen: profile.map(profilText).filter((text): text is string => text !== null),
    fallback: typeof twitch?.fallback_reason === 'string' ? twitch.fallback_reason.trim() || null : null,
  };
}

/** Unberührte Wahl und andere Plattformen bleiben bei Teiländerungen unangetastet. */
export function twitchOutputPayload(platform: string, entwurf: UplinkTwitchOutputMode | null) {
  return platform === 'twitch' && entwurf !== null ? { twitch_output_mode: entwurf } : {};
}
