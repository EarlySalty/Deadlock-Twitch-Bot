// Relativ statt ueber den @-Alias: die Node-Tests laufen ohne Vite.
import { ApiHttpError } from '../api/httpError';
import type {
  CategoryCollectorChatHeatmapCell,
  CategoryCollectorTrendPoint,
  CollectorStatus,
} from '../api/categoryCollector';

/**
 * Reine Darstellungslogik für die Seite "Kategorien weltweit".
 * Bewusst ohne React, damit die Datenregeln (null-Schnitte, Datenlücken,
 * Fehler von Leerdaten getrennt) direkt testbar bleiben.
 */

/** Sprache "und" heißt laut Vertrag: nicht sicher erkannt. */
export const UNSICHERE_SPRACHE = 'und';

const SPRACHNAMEN: Record<string, string> = {
  de: 'Deutsch',
  en: 'Englisch',
  fr: 'Französisch',
  es: 'Spanisch',
  pt: 'Portugiesisch',
  it: 'Italienisch',
  ru: 'Russisch',
  pl: 'Polnisch',
  nl: 'Niederländisch',
  tr: 'Türkisch',
  ko: 'Koreanisch',
  ja: 'Japanisch',
  zh: 'Chinesisch',
  da: 'Dänisch',
  fi: 'Finnisch',
  sv: 'Schwedisch',
  no: 'Norwegisch',
  cs: 'Tschechisch',
  el: 'Griechisch',
  hu: 'Ungarisch',
  ro: 'Rumänisch',
  th: 'Thailändisch',
  ar: 'Arabisch',
  hi: 'Hindi',
  id: 'Indonesisch',
  vi: 'Vietnamesisch',
  uk: 'Ukrainisch',
  other: 'Andere Sprache',
};

/** Menschenlesbarer Sprachname mit Code, ohne Flaggen und ohne Region. */
export function sprachLabel(code: string): string {
  const normalized = (code || '').trim().toLowerCase();
  if (!normalized) {
    return 'Unbekannte Sprache';
  }
  if (normalized === UNSICHERE_SPRACHE) {
    return 'Nicht sicher erkannt';
  }
  const name = SPRACHNAMEN[normalized];
  return name ? `${name} (${normalized})` : normalized;
}

export interface SammlerFehlerAnsicht {
  titel: string;
  text: string;
}

/**
 * Fehler sind Fehler: 403/503 und Rest dürfen nie als "keine Daten"
 * durchgehen. Die Seite zeigt Titel und Erklärung statt einer Statistik.
 */
export function klassifiziereSammlerFehler(error: unknown): SammlerFehlerAnsicht {
  const status = error instanceof ApiHttpError ? error.status : null;
  const details = error instanceof Error && error.message ? error.message : null;
  if (status === 401) {
    return {
      titel: 'Nicht angemeldet',
      text: 'Diese Seite braucht eine angemeldete Session. Bitte erneut einloggen.',
    };
  }
  if (status === 403) {
    return {
      titel: 'Kein Zugriff',
      text: 'Der globale Kategoriesammler ist nur für Admins erreichbar.',
    };
  }
  if (status === 503) {
    return {
      titel: 'Sammler-Backend nicht bereit',
      text: 'Die Datentabellen sind auf dem Server noch nicht eingerichtet. Diese Seite zeigt bewusst keine Schein-Nullen an. Bitte später erneut versuchen.',
    };
  }
  return {
    titel: status ? `Abfrage fehlgeschlagen (HTTP ${status})` : 'Abfrage fehlgeschlagen',
    text: details || 'Die Auswertung konnte nicht geladen werden.',
  };
}

export interface SammlerStatusAnsicht {
  label: string;
  hinweis: string | null;
}

/** Textstatus ohne Ampelfarbe: der Status erklärt sich, er bewertet nicht. */
export function sammlerStatusAnsicht(status: CollectorStatus): SammlerStatusAnsicht {
  switch (status) {
    case 'not_started':
      return {
        label: 'Noch nicht gestartet',
        hinweis: 'Der Sammler hat auf diesem System noch keine Daten erhoben.',
      };
    case 'running':
      return { label: 'Läuft', hinweis: null };
    case 'stale':
      return {
        label: 'Stockend',
        hinweis: 'Der Sammler läuft, aber die letzte vollständige Abfrage liegt länger zurück.',
      };
    case 'disabled':
      return { label: 'Deaktiviert', hinweis: 'Der Sammler ist auf diesem System abgeschaltet.' };
    case 'error':
      return {
        label: 'Fehler',
        hinweis: 'Der letzte Sammlerdurchlauf ist fehlgeschlagen. Die Werte können unvollständig sein.',
      };
  }
}

/** Speichergröße in Menschenlesbares, Basis 1024 wie im Betrieb üblich. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return '0 B';
  }
  const stufen = ['B', 'KB', 'MB', 'GB', 'TB'];
  let wert = bytes;
  let stufe = 0;
  while (wert >= 1024 && stufe < stufen.length - 1) {
    wert /= 1024;
    stufe += 1;
  }
  const text = stufe === 0 ? String(Math.round(wert)) : wert.toFixed(1);
  return `${text} ${stufen[stufe]}`;
}

/** Echte Messdauer, nicht Zeitraum mal 24 Stunden. */
export function formatMessdauer(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) {
    return 'Noch keine Messung';
  }
  const tage = Math.floor(seconds / 86400);
  const stunden = Math.floor((seconds % 86400) / 3600);
  const minuten = Math.floor((seconds % 3600) / 60);
  if (tage > 0) {
    return stunden > 0 ? `${tage} Tage ${stunden} Std.` : `${tage} Tage`;
  }
  if (stunden > 0) {
    return minuten > 0 ? `${stunden} Std. ${minuten} Min.` : `${stunden} Std.`;
  }
  return `${minuten} Min.`;
}

export function formatZeitstempel(iso: string | null | undefined): string {
  if (!iso) {
    return 'Bisher keine';
  }
  const datum = new Date(iso);
  if (Number.isNaN(datum.getTime())) {
    return iso;
  }
  return datum.toLocaleString('de-DE', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    timeZone: 'UTC',
  });
}

export interface TrendReihenpunkt {
  /** Tageslabel für die X-Achse, z. B. "18.09.". */
  label: string;
  avg_streams: number | null;
  avg_viewers: number | null;
  poll_samples: number;
  /** true für gefüllte Tage ohne Messung: Lücke, keine Null. */
  luecke: boolean;
}

/**
 * Baut eine lückenlose Tagesreihe zwischen erstem und letztem Messzeitpunkt.
 * Tage ohne Messung bekommen null und brechen die Linie (connectNulls=false),
 * statt still zu interpolieren oder Offline-Nullen zu zeichnen.
 */
export function buildTrendreihe(points: CategoryCollectorTrendPoint[]): TrendReihenpunkt[] {
  const sortiert = [...points]
    .filter((p) => p && typeof p.bucket_at === 'string')
    .sort((a, b) => new Date(a.bucket_at).getTime() - new Date(b.bucket_at).getTime());
  if (sortiert.length === 0) {
    return [];
  }

  const nachTag = new Map<string, CategoryCollectorTrendPoint>();
  for (const punkt of sortiert) {
    const zeit = new Date(punkt.bucket_at);
    const schluessel = isoTag(zeit);
    const bisher = nachTag.get(schluessel);
    // Mehrere Buckets am selben Tag: Beobachtungen zusammenführen, kein Punktverlust.
    nachTag.set(
      schluessel,
      bisher
        ? {
            bucket_at: punkt.bucket_at,
            avg_streams: mittel(bisher.avg_streams, punkt.avg_streams),
            avg_viewers: mittel(bisher.avg_viewers, punkt.avg_viewers),
            poll_samples: bisher.poll_samples + punkt.poll_samples,
          }
        : punkt,
    );
  }

  const reihe: TrendReihenpunkt[] = [];
  const erster = new Date(sortiert[0].bucket_at);
  const letzter = new Date(sortiert[sortiert.length - 1].bucket_at);
  const lauf = new Date(erster.getTime());
  lauf.setUTCHours(0, 0, 0, 0);
  // Hartes Ende, falls ein kaputter Zeitstempel die Schleife aufziehen würde.
  for (let i = 0; i < 366 && lauf.getTime() <= letzter.getTime(); i += 1) {
    const schluessel = isoTag(lauf);
    const punkt = nachTag.get(schluessel);
    if (punkt) {
      reihe.push({
        label: tagLabel(lauf),
        avg_streams: punkt.avg_streams,
        avg_viewers: punkt.avg_viewers,
        poll_samples: punkt.poll_samples,
        luecke: false,
      });
    } else {
      reihe.push({ label: tagLabel(lauf), avg_streams: null, avg_viewers: null, poll_samples: 0, luecke: true });
    }
    lauf.setUTCDate(lauf.getUTCDate() + 1);
  }
  return reihe;
}

function isoTag(zeit: Date): string {
  return zeit.toISOString().slice(0, 10);
}

function mittel(a: number, b: number): number {
  return (a + b) / 2;
}

function tagLabel(zeit: Date): string {
  return zeit.toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit', timeZone: 'UTC' });
}

export interface HeatmapReihe {
  language: string;
  /** 24 Werte, null = keine Nachrichten beobachtet. */
  stunden: (number | null)[];
}

export interface HeatmapAnsicht {
  reihen: HeatmapReihe[];
  max: number;
}

/**
 * Chat-Nachrichten je Sprache und Tageszeit UTC. Zellen ohne Beobachtung
 * bleiben null (neutral), sie werden nicht auf 0 aufgerundet.
 */
export function buildHeatmap(cells: CategoryCollectorChatHeatmapCell[]): HeatmapAnsicht {
  const nachSprache = new Map<string, (number | null)[]>();
  let max = 0;
  for (const zelle of cells) {
    if (!zelle || typeof zelle.language !== 'string') {
      continue;
    }
    const stunde = Math.min(Math.max(Math.trunc(zelle.hour_utc), 0), 23);
    let reihe = nachSprache.get(zelle.language);
    if (!reihe) {
      reihe = Array<number | null>(24).fill(null);
      nachSprache.set(zelle.language, reihe);
    }
    reihe[stunde] = (reihe[stunde] ?? 0) + Math.max(zelle.messages, 0);
  }
  const reihen = [...nachSprache.entries()]
    .map(([language, stunden]) => ({ language, stunden }))
    .sort((a, b) => sprachLabel(a.language).localeCompare(sprachLabel(b.language)));
  for (const reihe of reihen) {
    for (const wert of reihe.stunden) {
      if (wert !== null && wert > max) {
        max = wert;
      }
    }
  }
  return { reihen, max };
}

/** Warme Goldskala für die Heatmap, passend zur Markenpalette. */
export function heatmapFarbe(wert: number | null, max: number): string {
  if (wert === null || !max || wert <= 0) {
    return 'rgba(197, 160, 89, 0.06)';
  }
  const intensitaet = Math.min(wert / max, 1);
  return `rgba(197, 160, 89, ${0.18 + intensitaet * 0.72})`;
}

/** Null-Schnitte sind "-" und nie eine 0. */
export function zahlOderStrich(wert: number | null | undefined, dezimalstellen = 0): string {
  if (wert === null || wert === undefined || !Number.isFinite(wert)) {
    return '-';
  }
  return wert.toLocaleString('de-DE', {
    minimumFractionDigits: dezimalstellen,
    maximumFractionDigits: dezimalstellen,
  });
}

/** Top-Kanäle: Impact ist Zuschauerstunden, nicht Kanal- oder Streamanzahl. */
export function sortiereTopKanaele<T extends { viewer_hours: number }>(kanaele: T[]): T[] {
  return [...kanaele].sort((a, b) => b.viewer_hours - a.viewer_hours);
}
