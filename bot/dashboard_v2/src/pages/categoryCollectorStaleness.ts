export const CATEGORY_HEARTBEAT_MAX_AGE_MS = 120_000;

export interface CollectorStatusDetails {
  discovery_state?: string;
  last_discovery?: string | null;
  disk_paused?: boolean;
  raw_paused?: boolean;
  storage_checked_at?: string | null;
}

export interface CollectorCoverage {
  heartbeat_at?: string | null;
  last_snapshot?: string | null;
  collector_config?: { enabled: boolean; poll_seconds: number } | null;
  status?: CollectorStatusDetails | null;
}

export type CollectorState = 'disabled' | 'heartbeat_stale' | 'disk_paused' | 'snapshots_missing' | 'raw_paused' | 'discovery_pending' | 'active';

export function collectorHeartbeatStale(
  heartbeatAt: string | null | undefined,
  nowMs: number,
  maxAgeMs = CATEGORY_HEARTBEAT_MAX_AGE_MS,
): boolean {
  if (!heartbeatAt || !Number.isFinite(nowMs)) {
    return true;
  }

  const heartbeatMs = new Date(heartbeatAt).getTime();
  if (!Number.isFinite(heartbeatMs)) {
    return true;
  }

  return heartbeatMs > nowMs + CATEGORY_HEARTBEAT_MAX_AGE_MS || nowMs - heartbeatMs > maxAgeMs;
}

export function collectorCoverageState(report: CollectorCoverage, nowMs: number) {
  const pollSeconds = report.collector_config?.poll_seconds;
  const maxAgeMs = pollSeconds != null && Number.isFinite(pollSeconds) && pollSeconds > 0
    ? Math.max(CATEGORY_HEARTBEAT_MAX_AGE_MS, pollSeconds * 2_000)
    : CATEGORY_HEARTBEAT_MAX_AGE_MS;
  const heartbeatStale = collectorHeartbeatStale(report.heartbeat_at, nowMs);
  const snapshotsMissing = collectorHeartbeatStale(report.last_snapshot, nowMs, maxAgeMs);
  const discoveryStale = collectorHeartbeatStale(report.status?.last_discovery, nowMs, maxAgeMs);
  let state: CollectorState;

  if (report.collector_config?.enabled === false) {
    state = 'disabled';
  } else if (heartbeatStale) {
    state = 'heartbeat_stale';
  } else if (report.status?.disk_paused) {
    state = 'disk_paused';
  } else if (snapshotsMissing) {
    state = 'snapshots_missing';
  } else if (report.status?.raw_paused) {
    state = 'raw_paused';
  } else if (discoveryStale || report.status?.discovery_state !== 'ok') {
    state = 'discovery_pending';
  } else {
    state = 'active';
  }

  return {
    state,
    heartbeatStale,
    snapshotsMissing,
    discoveryStale,
    liveChannelsCurrent: report.collector_config?.enabled !== false && !report.status?.disk_paused && !heartbeatStale && !discoveryStale && report.status?.discovery_state === 'ok',
  };
}

export const COLLECTOR_STATE_TEXT: Record<CollectorState, { title: string; detail: string }> = {
  disabled: {
    title: 'Sammlung deaktiviert',
    detail: 'Neue Erfassung ist in den Einstellungen abgeschaltet. Der bisherige Bestand bleibt verfügbar.',
  },
  heartbeat_stale: {
    title: 'Sammler meldet sich nicht aktuell',
    detail: 'Die letzte Statusmeldung fehlt oder ist veraltet. Ob neue Daten erfasst werden, ist derzeit nicht bestätigt.',
  },
  disk_paused: {
    title: 'Neue Erfassung wegen Plattenplatz pausiert',
    detail: 'Der Sammler meldet sich weiterhin. Wegen knappen oder nicht prüfbaren Plattenplatzes pausieren neue Chatzeilen, Kategoriemessungen und Medienmetadaten. Der bisherige Bestand bleibt erhalten.',
  },
  snapshots_missing: {
    title: 'Aktuelle Kategoriemessungen fehlen',
    detail: 'Der Sammler meldet sich, aber eine aktuelle Kategoriemessung fehlt. Die vorhandenen Auswertungen bleiben sichtbar; die Lücke ist keine Null-Aktivität.',
  },
  raw_paused: {
    title: 'Neue Chatzeilen wegen Speicherbudget pausiert',
    detail: 'Das Rohchatbudget ist erreicht. Aktuelle Kategoriemessungen sind vorhanden; neue Chatzeilen werden nicht gespeichert. Der bisherige Bestand bleibt erhalten.',
  },
  discovery_pending: {
    title: 'Kategorieabruf noch nicht aktuell bestätigt',
    detail: 'Der Sammler meldet sich und aktuelle Kategoriemessungen sind vorhanden. Der letzte erfolgreiche Kategorieabruf fehlt, ist veraltet oder der Abruf meldet eine Einschränkung.',
  },
  active: {
    title: 'Sammlung im Twitch-Bot aktiv',
    detail: 'Statusmeldung, Kategoriemessung und erfolgreicher Kategorieabruf sind aktuell.',
  },
};
