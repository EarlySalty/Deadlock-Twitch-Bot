export type YouTubeLiveSichtbarkeit = 'private' | 'unlisted' | 'public';
export type YouTubeLiveZustand =
  | 'inaktiv'
  | 'vorbereitet'
  | 'sendet'
  | 'live'
  | 'beendet'
  | 'blockiert'
  | 'fehler'
  | 'unklar';

export interface YouTubeLiveEinstellungen {
  titel: string;
  sichtbarkeit: YouTubeLiveSichtbarkeit;
  autoStart: boolean;
  autoStop: boolean;
  freigegebenAm: string | null;
}

export interface YouTubeLiveStatus {
  zustand: YouTubeLiveZustand;
  hinweis: string | null;
  broadcastId: string | null;
  seit: string | null;
  wiederaufnehmbar: boolean;
}

export interface YouTubeLiveEntwurf {
  titel: string;
  sichtbarkeit: YouTubeLiveSichtbarkeit;
  autoStart: boolean;
  autoStop: boolean;
  liveFreigeben: boolean;
}

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

