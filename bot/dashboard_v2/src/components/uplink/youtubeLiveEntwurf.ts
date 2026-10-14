import type { YouTubeLiveEinstellungen, YouTubeLiveEntwurf } from './UplinkYouTubeLive';

export const TITEL_MAX = 100;

export function entwurfAus(einstellungen: YouTubeLiveEinstellungen | null): YouTubeLiveEntwurf {
  if (!einstellungen) {
    return { titel: '', sichtbarkeit: 'private', autoStart: false, autoStop: false, liveFreigeben: false };
  }
  return {
    titel: einstellungen.titel,
    sichtbarkeit: einstellungen.sichtbarkeit,
    autoStart: einstellungen.autoStart,
    autoStop: einstellungen.autoStop,
    liveFreigeben: einstellungen.freigegebenAm !== null,
  };
}

export function speichernErlaubt(entwurf: YouTubeLiveEntwurf): boolean {
  const laenge = entwurf.titel.trim().length;
  return laenge >= 1 && entwurf.titel.length <= TITEL_MAX;
}

export function naechsterEntwurf(
  entwurf: YouTubeLiveEntwurf,
  einstellungen: YouTubeLiveEinstellungen | null,
  beruehrt: boolean,
): YouTubeLiveEntwurf {
  if (beruehrt) return entwurf;
  return entwurfAus(einstellungen);
}
