import { ApiHttpError } from "@/api/core";
import type {
  ClipDateFilter,
  ClipSort,
  ImportJob,
  PoolClip,
  PoolPreferences,
} from "./types";

export interface PreferencesDraft {
  kind: ClipDateFilter["kind"];
  year: string;
  quarter: string;
  start: string;
  end: string;
  date: string;
  clipSort: ClipSort;
  timezone: string;
}

export function preferencesDraft(
  preferences: PoolPreferences,
  defaultYear = new Date().getFullYear(),
): PreferencesDraft {
  const filter = preferences.clip_date_filter;
  return {
    kind: filter.kind,
    year: String("year" in filter ? filter.year : defaultYear),
    quarter: String("quarter" in filter ? filter.quarter : 1),
    start: "start" in filter ? filter.start : `${defaultYear}-01-01`,
    end: "end" in filter ? filter.end : `${defaultYear}-12-31`,
    date: "date" in filter ? filter.date : `${defaultYear}-01-01`,
    clipSort: preferences.clip_sort,
    timezone: preferences.timezone,
  };
}

function validDate(value: string): boolean {
  const match = /^([+-]?\d{4,6})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return false;
  const [, year, month, day] = match.map(Number);
  const date = new Date(0);
  date.setUTCFullYear(year, month - 1, day);
  return (
    date.getUTCFullYear() === year &&
    date.getUTCMonth() === month - 1 &&
    date.getUTCDate() === day
  );
}

export function parsePreferencesDraft(draft: PreferencesDraft): {
  preferences: PoolPreferences | null;
  error: string | null;
} {
  const fail = (error: string) => ({ preferences: null, error });
  const timezone = draft.timezone.trim();
  try {
    if (!timezone || /^[+-]/.test(timezone))
      return fail("Bitte eine gültige IANA-Zeitzone angeben.");
    new Intl.DateTimeFormat("de-DE", { timeZone: timezone });
  } catch {
    return fail("Bitte eine gültige IANA-Zeitzone angeben.");
  }
  let filter: ClipDateFilter;
  switch (draft.kind) {
    case "all":
      filter = { kind: "all" };
      break;
    case "year":
    case "quarter": {
      const year = Number(draft.year);
      if (
        !/^[+-]?\d+$/.test(draft.year) ||
        !Number.isInteger(year) ||
        year < -2147483648 ||
        year > 2147483647
      ) {
        return fail("Bitte ein ganzzahliges Jahr angeben.");
      }
      if (draft.kind === "year") filter = { kind: "year", year };
      else {
        const quarter = Number(draft.quarter);
        if (!Number.isInteger(quarter) || quarter < 1 || quarter > 4)
          return fail("Bitte ein Quartal von 1 bis 4 auswählen.");
        filter = { kind: "quarter", year, quarter };
      }
      break;
    }
    case "range": {
      if (!validDate(draft.start) || !validDate(draft.end))
        return fail("Bitte Beginn und Ende als gültiges Datum angeben.");
      if (new Date(draft.start).getTime() > new Date(draft.end).getTime())
        return fail("Der Beginn darf nicht nach dem Ende liegen.");
      filter = { kind: "range", start: draft.start, end: draft.end };
      break;
    }
    case "before":
    case "after":
      if (!validDate(draft.date))
        return fail("Bitte ein gültiges Datum angeben.");
      filter = { kind: draft.kind, date: draft.date };
  }
  return {
    preferences: {
      clip_date_filter: filter,
      clip_sort: draft.clipSort,
      timezone,
    },
    error: null,
  };
}

export function samePreferences(
  a: PoolPreferences,
  b: PoolPreferences,
): boolean {
  return (
    a.clip_sort === b.clip_sort &&
    a.timezone === b.timezone &&
    JSON.stringify(a.clip_date_filter) === JSON.stringify(b.clip_date_filter)
  );
}

export function filterDescription(filter: ClipDateFilter): string {
  switch (filter.kind) {
    case "all":
      return "Alle verfügbaren historischen Clips";
    case "year":
      return `Gesamtes Jahr ${filter.year}`;
    case "quarter":
      return `Quartal ${filter.quarter} im Jahr ${filter.year}`;
    case "range":
      return `${filter.start} bis ${filter.end}, einschließlich beider Tage`;
    case "before":
      return `Vor dem ${filter.date}, ohne diesen Tag`;
    case "after":
      return `Nach dem ${filter.date}, ab dem folgenden Tag`;
  }
}

export function activeImport(job: ImportJob | null | undefined): boolean {
  return Boolean(job && ["queued", "running", "retry"].includes(job.status));
}

export function importStatusLabel(job: ImportJob): string {
  switch (job.status) {
    case "queued":
      return "Import wartet";
    case "running":
      return "Import läuft";
    case "retry":
      return "Import wartet auf erneuten Versuch";
    case "error":
      return "Import unterbrochen";
    case "incomplete":
      return "Archiv unvollständig erfasst";
    case "completed":
      return job.incomplete_windows.length
        ? "Archiv unvollständig erfasst"
        : "Import abgeschlossen";
  }
}

export function poolError(error: unknown): string {
  if (error instanceof ApiHttpError) {
    if (error.status === 401)
      return "Die Anmeldung ist abgelaufen. Bitte erneut anmelden.";
    if (error.status === 403) return "Für diesen Kanal fehlt die Berechtigung.";
    if (error.status === 409)
      return "Der gespeicherte Stand hat sich geändert oder ist gesperrt. Bitte erneut laden.";
    if ([400, 415, 422].includes(error.status))
      return "Die Eingaben wurden nicht übernommen. Bitte Zeitraum und Zeitzone prüfen.";
    if (error.status === 404)
      return "Der Import oder Clip wurde nicht gefunden. Bitte erneut laden.";
  }
  return "Der Clip-Pool konnte nicht geladen oder geändert werden. Bitte erneut versuchen.";
}

export function unauthorized(error: unknown): boolean {
  return error instanceof ApiHttpError && [401, 403].includes(error.status);
}

export function selectionAllowed(clip: PoolClip): boolean {
  if (clip.processed_at || clip.published_at || clip.legacy_reserved)
    return false;
  return clip.selected_at !== null || clip.selectable;
}

export function clipStatus(clip: PoolClip): string {
  if (clip.published_at) return "Veröffentlicht";
  if (clip.processed_at) return "Verarbeitet";
  if (clip.legacy_reserved) return "Für bestehende Verarbeitung reserviert";
  if (clip.selected_at) return "Ausgewählt";
  if (!clip.eligible) return "Außerhalb des gespeicherten Zeitraums";
  if (!clip.selectable) return "Für Verarbeitung reserviert";
  return "Auswählbar";
}

export function sourceMetadata(clip: PoolClip): {
  title: string;
  url: string | null;
  duration: number | null;
} {
  const metadata =
    clip.source_metadata && typeof clip.source_metadata === "object"
      ? (clip.source_metadata as Record<string, unknown>)
      : {};
  let url: string | null = null;
  if (typeof metadata.clip_url === "string") {
    try {
      const parsed = new URL(metadata.clip_url);
      if (parsed.protocol === "https:") url = parsed.href;
    } catch {
      url = null;
    }
  }
  return {
    title:
      typeof metadata.clip_title === "string" && metadata.clip_title
        ? metadata.clip_title
        : clip.clip_id,
    url,
    duration:
      typeof metadata.duration_seconds === "number" &&
      Number.isFinite(metadata.duration_seconds) &&
      metadata.duration_seconds >= 0
        ? metadata.duration_seconds
        : null,
  };
}

export function clipThumbnail(clip: PoolClip): string | null {
  const metadata =
    clip.source_metadata && typeof clip.source_metadata === "object"
      ? (clip.source_metadata as Record<string, unknown>)
      : {};
  if (typeof metadata.thumbnail_url !== "string") return null;
  if (import.meta.env?.MODE === 'preview' && metadata.thumbnail_url.startsWith('data:image/svg+xml,'))
    return metadata.thumbnail_url;
  try {
    const parsed = new URL(metadata.thumbnail_url);
    return parsed.protocol === "https:" ? parsed.href : null;
  } catch {
    return null;
  }
}

export function clipStatusTone(clip: PoolClip): string {
  if (clip.published_at) return "success";
  if (clip.processed_at) return "info";
  if (clip.legacy_reserved) return "muted";
  if (clip.selected_at) return "gold";
  if (!clip.eligible || !clip.selectable) return "muted";
  return "neutral";
}

export function clipDuration(seconds: number): string {
  const whole = Math.round(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

export function localTimestamp(value: string, timezone: string): string {
  try {
    return new Intl.DateTimeFormat("de-DE", {
      dateStyle: "medium",
      timeStyle: "short",
      timeZone: timezone,
    }).format(new Date(value));
  } catch {
    return value;
  }
}
