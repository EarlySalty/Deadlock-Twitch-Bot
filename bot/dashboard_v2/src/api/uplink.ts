import { fetchJson, withCookieCredentials } from './core';

const BASE = '/twitch/api/v2/uplink';

function jsonRequest(method: string, body: unknown): RequestInit {
  return withCookieCredentials({
    method,
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify(body ?? {}),
  });
}

/** Die laufende Session, so wie der Uplink sie unter `session` mitschickt. */
export interface UplinkSession {
  id: number;
  started_at: string;
  ingest_protocol: string;
  ingest_codec?: string | null;
}

export interface UplinkMe {
  enabled: boolean;
  waitlisted: boolean;
  ingest_key: string;
  /** Push-Adresse auf diesem Rechner. Nach aussen leer, dann gilt SRT. */
  rtmp_url: string;
  /** Vollstaendige SRT-Adresse fuer OBS. */
  srt_hint: string;
  /**
   * Freigabe fuer alle. Fehlt das Feld, gilt es als aus: der Uplink taucht
   * dann nur im Admin-Modus in der Hauptnavigation auf.
   */
  public_visible?: boolean;
  /**
   * Laufende Session, sobald der Server eine kennt. Der Uplink liefert das
   * Objekt unter `session`, eine Nummer auf oberster Ebene gibt es nicht.
   */
  session?: UplinkSession | null;
  /** Wie stark der Server die Ausgabe gerade heruntergefahren hat. */
  degraded_level?: number | null;
  /** Wartezeit nur nach einem unerwarteten Internetabriss. */
  reconnect_wait_s: number;
  /** Vom Relay gelieferte Obergrenze, nicht im Frontend duplizieren. */
  reconnect_wait_max_s: number;
}

export function fetchUplinkMe(): Promise<UplinkMe> {
  return fetchJson<UplinkMe>(`${BASE}/me`, withCookieCredentials());
}

export function joinUplinkWaitlist(): Promise<{ waitlisted: boolean }> {
  return fetchJson<{ waitlisted: boolean }>(`${BASE}/waitlist`, jsonRequest('POST', {}));
}

export interface UplinkReconnectWaitSettings {
  reconnect_wait_s: number;
  reconnect_wait_max_s: number;
}

export function saveUplinkReconnectWait(
  reconnectWaitS: number
): Promise<UplinkReconnectWaitSettings> {
  return fetchJson<UplinkReconnectWaitSettings>(
    `${BASE}/reconnect-wait`,
    jsonRequest('PUT', { reconnect_wait_s: reconnectWaitS })
  );
}

export interface UplinkProfileView {
  width: number;
  height: number;
  fps: number;
  bitrate_kbps: number;
}

export interface UplinkDestination {
  platform: string;
  rtmp_url: string;
  enabled: boolean;
  requested: UplinkProfileView;
  effective: UplinkProfileView;
}

export interface UplinkDestinations {
  destinations: UplinkDestination[];
}

export function fetchUplinkDestinations(): Promise<UplinkDestinations> {
  return fetchJson<UplinkDestinations>(`${BASE}/destinations`, withCookieCredentials());
}

export interface SaveDestinationBody {
  platform: string;
  rtmp_url?: string;
  stream_key?: string;
  enabled?: boolean;
  width?: number;
  height?: number;
  fps?: number;
  bitrate_kbps?: number;
}

export function saveUplinkDestination(body: SaveDestinationBody): Promise<UplinkDestinations> {
  return fetchJson<UplinkDestinations>(`${BASE}/destinations`, jsonRequest('PUT', body));
}

/**
 * Laesst den Server den Twitch-Schluessel holen und eintragen.
 *
 * Der Schluessel kommt nie im Browser an: das Dashboard holt ihn serverseitig
 * per Helix und reicht ihn direkt an den Uplink weiter.
 */
export function connectUplinkTwitch(): Promise<UplinkDestinations> {
  return fetchJson<UplinkDestinations>(
    `${BASE}/destinations/twitch-auto`,
    jsonRequest('POST', {})
  );
}

export interface UplinkScheduleEntry {
  id?: number;
  starts_at: string;
  ends_at: string;
}

export interface UplinkSchedule {
  entries: UplinkScheduleEntry[];
}

export function fetchUplinkSchedule(): Promise<UplinkSchedule> {
  return fetchJson<UplinkSchedule>(`${BASE}/schedule`, withCookieCredentials());
}

export function saveUplinkSchedule(entries: UplinkScheduleEntry[]): Promise<UplinkSchedule> {
  return fetchJson<UplinkSchedule>(
    `${BASE}/schedule`,
    jsonRequest('PUT', {
      entries: entries.map((e) => ({ starts_at: e.starts_at, ends_at: e.ends_at })),
    })
  );
}

export interface UplinkMetricSample {
  ts: string;
  ingest_kbps?: number | null;
  dropped_pkts?: number | null;
  /**
   * Rechenlast dieses Streams in Prozent. Kommt aus `session_metrics` und
   * meint nur den eigenen Stream, nicht die Last der ganzen Maschine.
   */
  cpu_pct?: number | null;
  /** Unter 1 kommt der Server beim Senden nicht hinterher. */
  encoder_speed?: number | null;
  /** Ausgehende Datenrate je Plattform in kbit/s. */
  egress_kbps_by_target?: Record<string, number | null> | null;
  degraded_level?: number | null;
}

export interface UplinkMetrics {
  session_id: number;
  started_at: string;
  ended_at: string | null;
  ingest_protocol: string;
  ingest_codec?: string | null;
  end_reason?: string | null;
  gb_by_target: Record<string, number>;
  sample_count: number;
  samples: UplinkMetricSample[];
}

export function fetchUplinkMetrics(session: number): Promise<UplinkMetrics> {
  return fetchJson<UplinkMetrics>(
    `${BASE}/metrics?session=${encodeURIComponent(String(session))}`,
    withCookieCredentials()
  );
}

export interface UplinkAdminOverview {
  loadavg: number;
  max_points: number;
  used_points: number;
  active_sessions: UplinkAdminSession[];
}

export interface UplinkAdminSession {
  session_id: number;
  streamer_id?: number;
  started_at?: string;
  ingest_protocol?: string;
  ingest_codec?: string | null;
}

export function fetchUplinkAdminOverview(): Promise<UplinkAdminOverview> {
  return fetchJson<UplinkAdminOverview>(`${BASE}/admin/overview`, withCookieCredentials());
}

export interface UplinkAdminWaitlistEntry {
  streamer_id: number;
  requested_at: string;
  note: string | null;
  enabled: boolean;
}

export function fetchUplinkAdminWaitlist(): Promise<{ entries: UplinkAdminWaitlistEntry[] }> {
  return fetchJson<{ entries: UplinkAdminWaitlistEntry[] }>(
    `${BASE}/admin/waitlist`,
    withCookieCredentials()
  );
}

export interface UplinkAdminSettings {
  max_points: number;
  load_reject_threshold: number;
}

export function saveUplinkAdminSettings(body: {
  max_points?: number;
  load_reject_threshold?: number;
}): Promise<UplinkAdminSettings> {
  return fetchJson<UplinkAdminSettings>(`${BASE}/admin/settings`, jsonRequest('PUT', body));
}

export interface UplinkKillResult {
  session_id: number;
  /** Der Server hat den Stream als beendet vermerkt. */
  ended: boolean;
  end_reason?: string | null;
  /** Erst hier steht, ob der Stream wirklich steht. */
  stopped: boolean;
}

/** Beendet eine laufende Session. Ohne `confirm=true` lehnt der Server ab. */
export function killUplinkSession(sessionId: number): Promise<UplinkKillResult> {
  return fetchJson<UplinkKillResult>(
    `${BASE}/admin/sessions/${encodeURIComponent(String(sessionId))}/kill?confirm=true`,
    jsonRequest('POST', {})
  );
}
