export interface LaufendesProfil {
  width: number;
  height: number;
  fps: number;
  bitrate_kbps: number;
  codec?: string;
  /** Herkunft aus dem laufenden Graph; Bitrate ist dessen Ziel, kein Messwert. */
  profile_origin?: 'running_graph';
}

export interface ZielBetriebsdaten {
  enabled: boolean;
  blocked?: boolean;
  error?: string | null;
  output_state?: 'unknown' | 'starting' | 'sending' | 'failed' | 'finished';
  reason?: string | null;
  active_profile?: LaufendesProfil | null;
  publication_confirmed?: boolean;
}

export function zielBetrieb(ziel: ZielBetriebsdaten | undefined, zugang?: string) {
  const result = { state: 'unknown', label: 'Nicht eingerichtet', tone: 'neutral',
    reason: null as string | null, activeProfile: null as LaufendesProfil | null };
  if (zugang === 'trennung_offen') {
    return { ...result, state: 'disconnect_pending', label: 'Trennung noch nicht bestätigt', tone: 'warning',
      reason: 'Der Dienst hat das Entfernen des Sendeziels noch nicht bestätigt. Trennen erneut ausführen.' };
  }
  if (!ziel) return result;
  if (ziel.blocked || ziel.output_state === 'failed') {
    return { ...result, state: 'failed', label: 'Ausgabe angehalten', tone: 'warning',
      reason: ziel.reason || ziel.error || 'Der Dienst konnte dieses Ziel nicht bedienen.' };
  }
  if (ziel.output_state === 'sending') {
    const erneuern = zugang === 'neu_verbinden' || zugang === 'rechte_ergaenzen';
    return { ...result, state: 'sending',
      label: ziel.publication_confirmed ? 'Plattform bestätigt live' : 'Medien werden gesendet',
      tone: erneuern ? 'warning' : ziel.publication_confirmed ? 'success' : 'neutral',
      reason: erneuern ? 'Zugang für Chat und weitere Plattformfunktionen erneuern.' : ziel.reason ?? null,
      activeProfile: profilText(ziel.active_profile) ? ziel.active_profile ?? null : null };
  }
  if (ziel.output_state === 'starting') {
    return { ...result, state: 'starting', label: 'Verbindung wird aufgebaut', reason: ziel.reason ?? null };
  }
  if (zugang === 'neu_verbinden') {
    return { ...result, label: 'Zugang erneuern', tone: 'warning',
      reason: 'Für Chat oder Kontofunktionen fehlen Rechte oder ein gültiger Zugang.' };
  }
  if (zugang === 'rechte_ergaenzen') {
    return { ...result, label: 'Rechte ergänzen', tone: 'warning',
      reason: 'Der vorhandene Kontozugang enthält noch nicht alle gewünschten Uplink-Rechte.' };
  }
  if (ziel.output_state === 'finished') return { ...result, state: 'finished', label: 'Ausgabe beendet' };
  return { ...result, label: ziel.enabled ? 'Für nächsten Stream eingeschaltet' : 'Für nächsten Stream ausgeschaltet',
    reason: ziel.reason ?? null };
}

export function profilText(profil: LaufendesProfil | null | undefined): string | null {
  if (!profil || ![profil.width, profil.height, profil.fps, profil.bitrate_kbps]
    .every((value) => Number.isFinite(value) && value > 0)) return null;
  return `${profil.width}×${profil.height} · ${Number(profil.fps.toFixed(3))} fps${profil.codec ? ` · ${profil.codec.toUpperCase()}` : ''} · ${profil.bitrate_kbps} kbit/s${profil.profile_origin === 'running_graph' ? ' Zielbitrate' : ''}`;
}

export interface EingangsBeobachtung {
  codec: string;
  width: number;
  height: number;
  fps_numerator: number;
  fps_denominator: number;
  audio: { wire_track: number; codec: string; sample_rate: number; channels: number }[];
  sampled_duration_ms: number;
}

export interface EingangsSession {
  active: boolean;
  state: string;
  received_events: number;
  received_bytes: number;
  error?: string | null;
  ingest_end_reason?: string | null;
  source_observation?: EingangsBeobachtung | null;
  outputs?: { encode_groups: number; video_decoders: number } | null;
}

export function eingangStatus(session: EingangsSession | null | undefined, unavailable: boolean) {
  if (unavailable) return { label: 'Eingangsstatus unbekannt', observation: null };
  if (!session) return { label: 'Noch kein Stream empfangen', observation: null };
  const label = !session.active ? 'Eingang beendet'
    : session.received_events > 0 ? 'Stream wird empfangen'
    : 'Verbindung aufgebaut, Medien werden erwartet';
  return { label, observation: session.source_observation ?? null };
}

export function obsZugang(me: {
  service_status?: string;
  public_ingest_url?: string;
  ingest_url?: string;
  ingest_key?: string;
}) {
  if (me.service_status !== 'ready') return null;
  const server = me.public_ingest_url ?? me.ingest_url;
  const key = me.ingest_key;
  if (!server || !key || !key.trim() || /[\u0000-\u0020\u007f]/.test(key)) return null;
  try {
    const url = new URL(server);
    if (url.protocol !== 'rtmps:' || !url.hostname || url.username || url.password
      || url.search || url.hash || !['/live', '/live/'].includes(url.pathname)) return null;
    return { server, key };
  } catch {
    return null;
  }
}
