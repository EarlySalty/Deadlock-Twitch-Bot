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
      text: 'Das Sammler-Backend ist derzeit nicht verfügbar. Ein Server- oder Datenbankfehler wird nicht als leere Statistik dargestellt.',
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
        hinweis: 'Seit einiger Zeit wurde keine vollständige Abfrage mehr bestätigt.',
      };
    case 'disabled':
      return { label: 'Deaktiviert', hinweis: 'Die Datensammlung ist deaktiviert; die Aufbewahrungsregeln bleiben bestehen.' };
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

/** UTC-Stundenwerte unverändert übernehmen; Lücken bleiben echte nulls. */
export function buildTrendreihe(points: CategoryCollectorTrendPoint[]): TrendReihenpunkt[] {
  const entries = points
    .map((point) => ({ point, time: Date.parse(point?.bucket_at) }))
    .filter(({ time, point }) => Number.isFinite(time) && Number.isFinite(point.poll_samples) && point.poll_samples >= 0)
    .sort((a, b) => a.time - b.time);
  const result: TrendReihenpunkt[] = [];
  const seen = new Set<number>();
  let previous: number | undefined;
  const hour = 3_600_000;
  const label = (time: number) => new Date(time).toLocaleString('de-DE', {
    day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit', timeZone: 'UTC',
  });
  for (const { point, time } of entries) {
    if (seen.has(time)) continue;
    seen.add(time);
    if (previous !== undefined) {
      // A corrupt distant timestamp must not allocate years of empty buckets.
      const missing = Math.min(Math.max(Math.ceil((time - previous) / hour) - 1, 0), 90 * 24);
      for (let index = 1; index <= missing; index += 1) {
        result.push({ label: label(previous + index * hour), avg_streams: null, avg_viewers: null, poll_samples: 0, luecke: true });
      }
    }
    result.push({
      label: label(time),
      avg_streams: Number.isFinite(point.avg_streams) ? point.avg_streams : null,
      avg_viewers: Number.isFinite(point.avg_viewers) ? point.avg_viewers : null,
      poll_samples: point.poll_samples,
      luecke: point.poll_samples === 0 || (point.avg_streams === null && point.avg_viewers === null),
    });
    previous = time;
    if (result.length > 90 * 24 + 2) break;
  }
  return result;
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
    if (!Number.isInteger(zelle.hour_utc) || zelle.hour_utc < 0 || zelle.hour_utc > 23 || !Number.isFinite(zelle.messages) || zelle.messages < 0) continue;
    const stunde = zelle.hour_utc;
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
