import type { CommunityData, CommunityLobby, CommunityMode, CommunityProfile, SharedWindow } from '../types/community';
import { tr } from '../i18n/dictionary';

export const COMMUNITY_GUILD = '1289721245281292288';
export const STREAMER_VC = '1326984426906714236';
export const COMMUNITY_INVITE = 'https://discord.com/channels/1289721245281292288/1326984426906714236';
export const WEEKDAYS = ['Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa', 'So'];
export const RANK_NAMES = ['Initiate', 'Seeker', 'Alchemist', 'Arcanist', 'Ritualist', 'Emissary', 'Archon', 'Oracle', 'Phantom', 'Ascendant', 'Eternus'];
export function voiceLink(channelId: string): string | null {
  return /^[1-9][0-9]{0,19}$/.test(channelId) ? `https://discord.com/channels/${COMMUNITY_GUILD}/${channelId}` : null;
}
export function twitchLink(login: string): string | null {
  return /^[a-zA-Z0-9_]{1,25}$/.test(login) ? `https://www.twitch.tv/${login}` : null;
}
export function modeLabel(mode: CommunityMode | null): string {
  return ({ normal: 'Standard', street_brawl: 'Street Brawl', sandbox: 'Sandbox', custom: 'Custom / Scrim' } as const)[mode as CommunityMode] ?? tr('Modus unbekannt');
}
export function modeSource(profile: CommunityProfile): string {
  switch (profile.mode_source) {
    case 'steam_history': return tr('Steam-Spielhistorie · {count} Spiele', { count: profile.mode_samples });
    case 'current_title': return tr('Hinweis aus aktuellem Streamtitel');
    case 'historical_title': return tr('Hinweis aus letztem passenden Streamtitel');
    default: return tr('Noch kein belastbarer Modushinweis');
  }
}
export function rankLabel(profile: CommunityProfile): string {
  return profile.rank_name ? `${profile.rank_name}${profile.subrank ? ` ${profile.subrank}` : ''}` : tr('Rang unbekannt');
}
export function rankAverageLabel(average: number | null): string {
  if (average === null || !Number.isFinite(average) || average < 1 || average > 11.6) return tr('Rang unbekannt');
  return tr('etwa {rank}', { rank: RANK_NAMES[Math.min(10, Math.max(0, Math.round(average - 0.3) - 1))] });
}
export function clockMinute(value: number): string {
  return `${Math.floor(value / 60).toString().padStart(2, '0')}:${(value % 60).toString().padStart(2, '0')}`;
}
export function windowLabel(window: SharedWindow): string {
  const day = WEEKDAYS[window.weekday - 1];
  return `${day ? tr(day) : '?'} ${clockMinute(window.start_minute)}–${clockMinute(window.end_minute)}`;
}
export function directoryFresh(discord: CommunityData['discord'], nowMs: number): boolean {
  if (discord.status !== 'ok' || discord.captured_at === null) return false;
  const age = nowMs / 1000 - discord.captured_at;
  return Number.isFinite(age) && age >= -5 && age <= 60;
}
export function canOpenLobby(lobby: CommunityLobby, fresh: boolean): boolean {
  return fresh && lobby.joinable && lobby.member_count > 0 && lobby.slots_free !== 0 && voiceLink(lobby.channel_id) !== null;
}
export function lobbyFit(lobby: CommunityLobby, profile: CommunityProfile): { points: number; conflict: boolean; explanation: string } {
  if (lobby.is_streamer_vc) return { points: 0, conflict: false, explanation: tr('Treffpunkt für gemeinsame Streams') };
  const modeConflict = !!lobby.mode && !!profile.mode && lobby.mode !== profile.mode;
  const rankConflict = profile.rank_tier !== null && lobby.rank_average !== null && Math.abs(profile.rank_tier - lobby.rank_average) > 2;
  if (modeConflict) return { points: -10, conflict: true, explanation: tr('Anderer Modus als dein letzter Hinweis') };
  if (rankConflict) return { points: -5, conflict: true, explanation: tr('Größerer Rangunterschied · vorher absprechen') };
  const modeKnown = !!profile.mode && !!lobby.mode;
  const rankKnown = profile.rank_tier !== null && lobby.rank_average !== null;
  return {
    points: (modeKnown ? 2 : 0) + (rankKnown ? 2 : 0), conflict: false,
    explanation: modeKnown && rankKnown ? tr('Modus und ungefährer Rang passen') : modeKnown ? tr('Passender Modushinweis') : rankKnown ? tr('Ähnlicher Rang laut Discord-Rollen') : tr('Modus oder Rang noch nicht vergleichbar'),
  };
}
export function directoryMessage(status: CommunityData['discord']['status'], fresh: boolean): string | null {
  if (status === 'link_required') return tr('Verbinde dein Discord-Konto, damit wir dir nur Sprachkanäle zeigen, auf die du zugreifen darfst.');
  if (status === 'membership_unconfirmed') return tr('Deine Discord-Mitgliedschaft konnte nicht bestätigt werden. Prüfe deine Verbindung und den Serverbeitritt.');
  if (status === 'unavailable') return tr('Die Discord-Liveansicht ist gerade nicht verfügbar. Das bedeutet nicht, dass niemand spielt.');
  if (status === 'stale' || !fresh) return tr('Die Lobby-Anzeige ist nicht mehr aktuell. Zum Öffnen bitte neu laden.');
  return null;
}
