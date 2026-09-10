import type { QueryClient } from '@tanstack/react-query';
import type { UplinkDestination, UplinkDestinationSaveAck } from './api/uplink';
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

/** PUT bestätigt die Speicherung. Erst ein neuer GET darf den Zielcache ersetzen. */
export async function bestaetigeUplinkSpeichern(
  ack: UplinkDestinationSaveAck,
  client: QueryClient,
  laden: () => Promise<{ destinations: UplinkDestination[] }>,
) {
  if (ack.ok !== true) throw new Error('Speichern wurde nicht bestätigt.');
  const queryKey = ['uplink-destinations'];
  await client.cancelQueries({ queryKey });
  await client.invalidateQueries({ queryKey, refetchType: 'none' });
  return client.fetchQuery({ queryKey, queryFn: laden, staleTime: 0, retry: false });
}
