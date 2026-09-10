import { fetchJson, withCookieCredentials } from './core';
import { normalisiereCaps } from '../uplinkEmpfehlung';
import type { UplinkCaps, UplinkCapsRoh } from '../uplinkEmpfehlung';
import type { ZielBetriebsdaten } from '../uplinkBetrieb';
import type { EingangsSession } from '../uplinkBetrieb';
import type { UplinkTwitchOutputMode } from '../uplinkOutputMode';

/**
 * `live_status` kommt nicht vom Relay, sondern aus der Twitch-Beobachtung des
 * Bots (tb-dashboard-api, handlers/uplink.rs).
 *
 * `unbekannt` heisst: der Stand ist zu alt oder es gibt keinen. Die Oberflaeche
 * behandelt das wie `live` und deckt nichts auf. Ein aelterer Server, der das
 * Feld noch nicht kennt, landet ueber `undefined` in derselben sicheren Ecke.
 */
export type UplinkLiveStatus = 'live' | 'aus' | 'unbekannt';

export interface UplinkMe {
  enabled: boolean;
  waitlisted: boolean;
  ingest_key: string;
  public_ingest_url?: string;
  ingest_url?: string;
  service_status?: 'ready' | 'unavailable' | 'input_only';
  capabilities?: { reconnect?: boolean };
  /** Tatsächliche Eingangs-Generation des Uplink-Dienstes, unabhängig von Twitch live. */
  session?: EingangsSession | null;
  live_status?: UplinkLiveStatus;
  /** Wartezeit nur nach einem unerwarteten Internetabriss. */
  reconnect_wait_s: number;
  /** Vom Relay gelieferte Obergrenze, nicht im Frontend duplizieren. */
  reconnect_wait_max_s: number;
  /**
   * Die vier Dock-Adressen, sobald das Relay sie herstellen kann.
   *
   * `null` heisst: es gibt nichts anzuzeigen. Zwei Faelle laufen darin
   * zusammen, und beide enden bei demselben Knopf: es wurde nie eine erzeugt,
   * oder der Zugang stammt aus der Zeit, in der das Relay nur den Fingerabdruck
   * gespeichert hat.
   */
  dock_urls?: DockUrls | null;
  /**
   * Ob es schon Dock-Adressen gibt, auch wenn sie hier nicht mitkommen.
   *
   * Steht neben `dock_urls`, weil beides auseinanderfallen kann. Nur so
   * unterscheidet die Karte "erzeugen" von "neu erzeugen", und nur so erfaehrt
   * der Streamer vorher, dass ein Neuerzeugen seine Eintraege in OBS
   * entwertet.
   */
  dock_url_vorhanden?: boolean;
  /** Je Plattform ein Eintrag; was nicht gespeichert ist, ist getrennt. */
  verbindungen?: UplinkVerbindung[];
}

export type UplinkVerbindungStatus = 'verbunden' | 'neu_verbinden' | 'rechte_ergaenzen' | 'zugang_unbekannt' | 'trennung_offen' | 'getrennt';

export interface UplinkVerbindung {
  platform: string;
  status: UplinkVerbindungStatus;
  /** Ob im Uplink schon ein Ziel fuer diese Plattform liegt. */
  stream_key_vorhanden?: boolean;
  /** Ob diese Plattform auf dieser Instanz verbunden werden kann (Secrets da). */
  verbindbar?: boolean;
}

/**
 * Die vier Fenster hinter einem Zugang. Ein Neuerzeugen macht alle vier alten
 * Adressen ungueltig.
 *
 * Ein leeres Feld ist erlaubt und bedeutet "diese Adresse gibt es nicht":
 * `dockAdressen` laesst die Zeile dann weg, statt eine Kopierzeile ohne Ziel
 * anzubieten.
 */
export interface DockUrls {
  chat: string;
  activity: string;
  stream_info: string;
  points: string;
}

/**
 * Laesst das Relay neue Dock-Adressen ausstellen. Die alten gelten danach nicht
 * mehr.
 *
 * Die Antwort ist nicht mehr die einzige Gelegenheit: `GET /uplink/me` liefert
 * dieselben vier Adressen bei jedem Laden. Sie steht hier trotzdem, damit die
 * Karte direkt nach dem Klick etwas zeigen kann, statt auf den naechsten Abruf
 * zu warten.
 */
export async function rotateUplinkDockToken(): Promise<DockUrls> {
  const antwort = await fetchJson<{ dock_urls?: DockUrls }>(
    '/twitch/api/v2/uplink/dock-token/rotate',
    withCookieCredentials({
      method: 'POST',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: '{}',
    })
  );
  // Kommt hier nichts Anzeigbares zurueck, ist der Zugang trotzdem schon
  // gedreht und die Eintraege in OBS sind tot. Das darf nicht still
  // durchgehen: sonst drueckt der Streamer denselben Knopf noch einmal und
  // entwertet jedes Mal einen weiteren Zugang, ohne je eine Adresse zu sehen.
  if (!antwort.dock_urls?.chat) {
    throw new Error('Das Relay hat keine Dock-Adressen zurueckgegeben.');
  }
  return antwort.dock_urls;
}

/**
 * Startet das Verbinden einer Plattform. Das ist eine Browser-Navigation,
 * kein fetch: der Server leitet direkt zur Anmeldeseite der Plattform weiter.
 *
 * Es ist derselbe Weg, ueber den der Streamer den Bot autorisiert, nur mit
 * mehr Rechten. Deshalb gibt es hier keinen eigenen Pfad mehr: ein zweiter
 * Grant fuer dasselbe Konto hiess zwei Zugaenge, von denen einer irgendwann
 * der falsche war.
 */
export function uplinkConnectUrl(platform: UplinkPlattform): string {
  if (platform === 'twitch') return '/twitch/raid/auth?scope_profile=uplink';
  if (platform === 'kick') return '/twitch/uplink/connect/kick';
  if (platform === 'youtube') return '/twitch/uplink/connect/youtube';
  return '';
}

/**
 * Trennt eine Plattform: Zugang zurueckgenommen, Ziel im Uplink entfernt.
 * Der gemeinsam genutzte Twitch-Grant für Raid und Bot bleibt erhalten.
 */
export function trenneUplinkPlattform(
  platform: UplinkPlattform,
  csrfToken: string
): Promise<{ ok: boolean }> {
  return fetchJson<{ ok: boolean }>(
    `/twitch/api/v2/uplink/connect/${platform}/disconnect`,
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
        'X-CSRF-Token': csrfToken,
      },
      body: '{}',
    })
  );
}

/**
 * Holt den Stream-Key nach und legt ihn als Uplink-Ziel ab. Zwei Aufrufer: die
 * Rueckkehr aus dem Twitch-Dialog und der Knopf "Stream-Key erneut holen" in
 * der Plattform-Karte, der erscheint, solange kein Ziel im Uplink liegt.
 */
export function holeUplinkStreamKey(
  platform: UplinkPlattform,
  csrfToken: string
): Promise<{ ok: boolean }> {
  return fetchJson<{ ok: boolean }>(
    `/twitch/api/v2/uplink/connect/${platform}/streamkey`,
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
        'X-CSRF-Token': csrfToken,
      },
      body: '{}',
    })
  );
}

/** Bis jetzt kann nur Twitch verbunden werden; die anderen folgen. */
export function verbindenAktiv(platform: UplinkPlattform): boolean {
  return platform === 'twitch';
}

export interface UplinkPlattformVerbindung {
  id: UplinkPlattform;
  label: string;
  status: UplinkVerbindungStatus;
  /** Ob der Knopf "Verbinden" etwas tut. */
  aktiv: boolean;
  /** Fertiger Statustext fuer die Oberflaeche. */
  statusText: string;
  /** Beschriftung des Verbinden-Links; null, wenn die Plattform noch nicht verbunden werden kann. */
  knopfText: string | null;
  /** Ob im Uplink schon ein Ziel fuer diese Plattform liegt. */
  streamKeyVorhanden: boolean;
  /** Ob der Trennen-Knopf etwas zu trennen hat. */
  trennenMoeglich: boolean;
}

/**
 * Hinweis unter dem Trennen-Knopf.
 *
 * Uplink wird getrennt; der gemeinsame Twitch-Grant für andere Funktionen bleibt.
 */
export const TRENNEN_HINWEIS =
  'Trennt diese Plattform von Uplink und entfernt ihr Sendeziel. Deine übrigen Bot- und Raidfunktionen bleiben verbunden.';

/**
 * Wofür die Rechte gebraucht werden, in Klartext.
 *
 * Der Twitch-Dialog listet sie einzeln und in seiner eigenen Sprache auf. Wer
 * vorher nicht weiß, wofür sie da sind, klickt entweder blind zu oder gar
 * nicht.
 *
 * Bewusst ohne Zahl: der Dialog zeigt den vollen Satz, und der ist je nach
 * bisherigem Grant unterschiedlich lang. Eine genannte Zahl wäre dort für
 * einen Teil der Streamer immer die falsche. Genannt wird nur, was neu
 * dazukommt; der Rest wird als das benannt, was er ist, ohne zu behaupten,
 * jeder habe ihn schon.
 */
/**
 * Der eine Satz, der neben dem Status steht, solange nichts verbunden ist.
 *
 * Drei Zeilen Erklaertext im Kartenkopf haben die Karte erschlagen: wer schon
 * verbunden ist, liest sie jedes Mal mit und braucht sie nie. Das Lange steht
 * jetzt in der aufklappbaren Hilfe daneben, dieser Satz sagt nur, was der
 * Klick bringt.
 */
export const VERBINDEN_KURZ =
  'Verbindet dein Plattformkonto. Verfügbare Funktionen hängen von den gewährten Rechten ab.';

export const VERBINDEN_HINWEIS =
  'Twitch zeigt dir gleich die Liste der Rechte. Neu dazu kommen: deinen Stream-Key holen, den Chat lesen und darin antworten, Aktivitäten wie Follows sehen und Kanalpunkt-Einlösungen abhaken. Die übrigen Punkte in der Liste gehören zum Bot und zum Dashboard.';

/** Eine Zeile je Plattform fuer den Kopf der Plattform-Karte. */
export function plattformVerbindungen(me: UplinkMe): UplinkPlattformVerbindung[] {
  return UPLINK_PLATTFORMEN.map((p) => {
    const eintrag = me.verbindungen?.find((v) => v.platform === p.id);
    const status = eintrag?.status ?? 'getrennt';
    const aktiv = eintrag?.verbindbar ?? verbindenAktiv(p.id);
    const streamKeyVorhanden = eintrag?.stream_key_vorhanden ?? false;
    let statusText = 'Kontoverbindung nicht verfügbar';
    let knopfText: string | null = null;
    if (status === 'verbunden') {
      statusText = streamKeyVorhanden ? 'Verbunden' : 'Verbunden, Schlüssel fehlt';
    } else if (status === 'neu_verbinden') {
      statusText = 'Zugang erneuern';
    } else if (status === 'rechte_ergaenzen') {
      statusText = 'Für Uplink fehlen noch Rechte';
    } else if (status === 'zugang_unbekannt') {
      statusText = 'Kontozugang konnte gerade nicht geprüft werden';
    } else if (status === 'trennung_offen') {
      statusText = 'Trennung noch nicht bestätigt';
    } else if (!aktiv && (p.id === 'kick' || p.id === 'youtube')) {
      statusText = `Kontoverbindung zu ${p.label} ist hier noch nicht eingerichtet`;
    } else if (aktiv) {
      statusText = 'Nicht verbunden';
    }
    if (aktiv) {
      knopfText = status === 'getrennt' ? `Mit ${p.label} verbinden`
        : status === 'rechte_ergaenzen' ? 'Rechte ergänzen'
        : status === 'zugang_unbekannt' ? null : 'Neu verbinden';
    }
    return {
      id: p.id,
      label: p.label,
      status,
      aktiv,
      statusText,
      knopfText,
      streamKeyVorhanden,
      trennenMoeglich: status !== 'getrennt',
    };
  });
}

/**
 * Die vier eigenen Fenster in Anzeigereihenfolge, mit den Namen, die auch in
 * OBS eingetragen werden. Feste Reihenfolge, damit die Karte nicht bei jedem
 * Laden anders aussieht.
 *
 * Fertige Twitch-Fenster stehen hier bewusst nicht mehr: sie zeigten nur
 * Twitch, brauchten eine eigene Anmeldung im OBS-Browser und standen neben
 * vier Fenstern, die dasselbe fuer alle verbundenen Plattformen tun.
 */
export const EIGENE_DOCKS = [
  { titel: 'Chat', feld: 'chat' },
  { titel: 'Aktivität', feld: 'activity' },
  { titel: 'Stream-Infos', feld: 'stream_info' },
  { titel: 'Kanalpunkte', feld: 'points' },
] as const satisfies ReadonlyArray<{ titel: string; feld: keyof DockUrls }>;

export interface DockAdresse {
  titel: string;
  url: string;
}

/**
 * Unsere vier Dock-Adressen in Anzeigereihenfolge.
 *
 * Eine Quelle, `me`. Frisch erzeugte Adressen kommen ueber den
 * Zwischenspeicher der Abfrage hier an, nicht ueber einen zweiten Parameter:
 * ein eigener Zustand fuer "gerade erzeugt" waere ein zweiter Stand, der
 * haengen bleiben kann, waehrend der Server laengst etwas anderes fuehrt.
 *
 * Eine leere Adresse faellt weg, statt eine Kopierzeile ohne Ziel anzubieten.
 */
export function dockAdressen(me: UplinkMe): DockAdresse[] {
  if (!me.dock_urls) return [];
  const liste: DockAdresse[] = [];
  for (const dock of EIGENE_DOCKS) {
    const url = me.dock_urls[dock.feld]?.trim() ?? '';
    if (url) liste.push({ titel: dock.titel, url });
  }
  return liste;
}

export function fetchUplinkMe(): Promise<UplinkMe> {
  return fetchJson<UplinkMe>('/twitch/api/v2/uplink/me', withCookieCredentials());
}

export function joinUplinkWaitlist(): Promise<{ waitlisted: boolean }> {
  return fetchJson<{ waitlisted: boolean }>(
    '/twitch/api/v2/uplink/waitlist',
    withCookieCredentials({
      method: 'POST',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: '{}',
    })
  );
}

export interface UplinkAdminWaitlistEntry {
  streamer_id: number;
  requested_at: string;
  note?: string | null;
  enabled: boolean;
  /** Twitch-Login, per Helix aus der ID aufgelöst; null, wenn nicht auflösbar. */
  twitch_login?: string | null;
  /** Twitch-Anzeigename; null, wenn nicht auflösbar. */
  display_name?: string | null;
}

export function fetchUplinkAdminWaitlist(): Promise<{ entries: UplinkAdminWaitlistEntry[] }> {
  return fetchJson<{ entries: UplinkAdminWaitlistEntry[] }>(
    '/twitch/api/v2/uplink/admin/waitlist',
    withCookieCredentials(),
  );
}

export function acceptUplinkAdminWaitlistEntry(
  streamerId: number,
  csrfToken: string,
): Promise<{ streamer_id: number; enabled: boolean }> {
  return fetchJson<{ streamer_id: number; enabled: boolean }>(
    '/twitch/api/v2/uplink/admin/users',
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
        'X-CSRF-Token': csrfToken,
      },
      body: JSON.stringify({ streamer_id: streamerId }),
    }),
  );
}

/**
 * Nimmt einen Wartelisteneintrag zurueck, ohne einen Zugang anzulegen.
 *
 * Der Streamer behaelt sein Konto und kann sich neu eintragen; abgelehnt ist
 * nur diese eine Anfrage.
 */
export function rejectUplinkAdminWaitlistEntry(
  streamerId: number,
  csrfToken: string,
): Promise<{ streamer_id: number; rejected: boolean }> {
  return fetchJson<{ streamer_id: number; rejected: boolean }>(
    `/twitch/api/v2/uplink/admin/waitlist/${streamerId}`,
    withCookieCredentials({
      method: 'DELETE',
      headers: {
        Accept: 'application/json',
        'X-CSRF-Token': csrfToken,
      },
    }),
    // Ohne diesen Satz stuende hier "Server-Fehler (HTTP 404)". Der Fall ist
    // harmlos und hat einen klaren Grund: der Eintrag ist schon weg.
    { notFoundMessage: 'Dieser Eintrag steht nicht mehr auf der Warteliste.' },
  );
}

export interface UplinkReconnectWaitSettings {
  reconnect_wait_s: number;
  reconnect_wait_max_s: number;
}

export function saveUplinkReconnectWait(
  reconnectWaitS: number
): Promise<UplinkReconnectWaitSettings> {
  return fetchJson<UplinkReconnectWaitSettings>(
    '/twitch/api/v2/uplink/reconnect-wait',
    withCookieCredentials({
      method: 'PUT',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: JSON.stringify({ reconnect_wait_s: reconnectWaitS }),
    })
  );
}

/** Ein gespeicherter Wert ist noch kein Nachweis einer aktiven Pufferreserve. */
export const UPLINK_RECONNECT_WAIT_TEXT =
  'Gespeicherte Frist für eine Wiederverbindung nach einem Verbindungsabbruch. Ein Verbindungsende allein beweist nicht, dass du den Stream bewusst beendet hast.';

export function reconnectWaitEingabe(wert: number | null | undefined): string {
  return typeof wert === 'number' && Number.isFinite(wert) && wert >= 0 ? String(wert) : '';
}

/** Liest nur ganze, nichtnegative Sekunden; die Obergrenze bleibt beim Relay. */
export function reconnectWaitPayload(wert: string): number | null {
  const getrimmt = wert.trim();
  if (!getrimmt || !/^\d+$/.test(getrimmt)) return null;
  const sekunden = Number(getrimmt);
  return Number.isSafeInteger(sekunden) && sekunden >= 0 ? sekunden : null;
}

/**
 * Ein Videoprofil, wie das Relay es zurueckgibt.
 */
export interface UplinkProfilAnsicht {
  width: number;
  height: number;
  fps: number;
  bitrate_kbps: number;
}

/** Benannte Wunschprofile; keine Aussage über Plattformfreigabe oder aktive Ausgabe. */
export const UPLINK_PROFILE = [
  {
    name: '1440p60',
    label: '2560×1440, 60 fps, 12000 kbit/s',
    hinweis: 'Wunschprofil für einen passenden 2560×1440-Eingang.',
    warnung:
      'Twitch benötigt dafür eine bestätigte Enhanced-Broadcasting-Konfiguration und einen geeigneten Kanalzugang. Gespeichert bedeutet noch nicht freigegeben.',
  },
  {
    name: '1080p60-hoch',
    label: '1080p60, 8000 kbps',
    hinweis: 'Wunschprofil mit höherem Video-Bitratenbudget. Wird für das jeweilige Ziel geprüft.',
    warnung: '',
  },
  {
    name: '1080p60',
    label: '1080p60, 6000 kbps',
    hinweis: 'Wunschprofil mit 6000 kbit/s Video am Ausgang. Dein Upload wird getrennt bewertet.',
    warnung: '',
  },
  {
    name: '720p60',
    label: '720p60, 4500 kbps',
    hinweis: 'Kleineres Ausgabeprofil mit 60 Bildern pro Sekunde.',
    warnung: '',
  },
  {
    name: '480p30',
    label: '480p30, 1500 kbps',
    hinweis: 'Kleineres Ausgabeprofil mit 30 Bildern pro Sekunde.',
    warnung: '',
  },
] as const;

export type UplinkProfilName = (typeof UPLINK_PROFILE)[number]['name'];

/** Die Zahlen hinter jedem Profilnamen, gespiegelt aus `handlers/uplink.rs`. */
export const PROFIL_WERTE: Record<UplinkProfilName, [number, number, number, number]> = {
  '1080p60': [1920, 1080, 60, 6000],
  '1080p60-hoch': [1920, 1080, 60, 8000],
  '1440p60': [2560, 1440, 60, 12000],
  '720p60': [1280, 720, 60, 4500],
  '480p30': [854, 480, 30, 1500],
};

/**
 * Findet den Profilnamen zu einem gespeicherten Ziel.
 *
 * Ohne das startet die Auswahl immer auf dem Standard, und wer nur seinen
 * Stream-Key erneuert, schickt still 1080p60 mit und aendert damit eine
 * Qualitaetsstufe, die er nie angefasst hat. `null` heisst: das gespeicherte
 * Profil steht nicht im Katalog, dann bleibt die Auswahl, wo sie ist.
 */
export function profilNameFuer(werte: UplinkProfilAnsicht | undefined): UplinkProfilName | null {
  if (!werte) return null;
  const treffer = (Object.keys(PROFIL_WERTE) as UplinkProfilName[]).find((name) => {
    const [w, h, f, b] = PROFIL_WERTE[name];
    return werte.width === w && werte.height === h && werte.fps === f && werte.bitrate_kbps === b;
  });
  return treffer ?? null;
}

/**
 * Die vier Plattformen, die das Relay kennt. Reihenfolge ist die der
 * Zielkarten: Twitch zuerst, weil es fuer fast alle das einzige Ziel ist.
 */
export const UPLINK_PLATTFORMEN = [
  { id: 'twitch', label: 'Twitch', rtmp: 'rtmps://ingest.global-contribute.live-video.net:443/app' },
  { id: 'youtube', label: 'YouTube', rtmp: 'rtmps://a.rtmps.youtube.com:443/live2' },
  { id: 'kick', label: 'Kick', rtmp: 'rtmps://fa723fc1b171.global-contribute.live-video.net' },
  { id: 'tiktok', label: 'TikTok', rtmp: '' },
] as const;

export type UplinkPlattform = (typeof UPLINK_PLATTFORMEN)[number]['id'];

/** Freie Werte aus dem manuellen Modus. */
export interface UplinkManuellesProfil {
  width: number;
  height: number;
  fps: number;
  bitrate_kbps: number;
}

/**
 * Was die Aenderung mit einem gerade laufenden Stream gemacht hat.
 *
 * Fehlt, wenn es nichts zu sagen gibt: kein Stream, oder es lief ohnehin
 * schon so. `applied` heisst, dass nur die Bitrate wechselt und niemand
 * etwas merkt; `applied_restart` heisst, dass das Bildformat wechselt und die
 * Zuschauer kurz ein Stocken sehen; `too_busy` heisst, gespeichert ist es,
 * aber der laufende Stream bleibt bis zum naechsten Mal, wie er ist.
 */
export interface UplinkLiveQualitaet {
  status: 'applied' | 'applied_restart' | 'too_busy' | 'next_stream';
  message: string;
}

export type UplinkTwitchAudioMode = 'live' | 'separate_vod';

export const TWITCH_AUDIO_LABEL: Record<UplinkTwitchAudioMode, string> = {
  live: 'Live-Ton',
  separate_vod: 'Separater Twitch-VOD-Ton',
};

/**
 * Ein Ziel speichern. Drei Faelle, alle ueber denselben Aufruf:
 *
 * - Zugangsdaten neu setzen: `rtmp_url` und `stream_key` zusammen.
 * - Nur die Qualitaet aendern: beides weglassen. Genau das ging vorher nicht,
 *   und deshalb sah es aus, als wuerde die Auswahl nicht gespeichert.
 * - An- oder abschalten: `enabled`.
 *
 * `profil` und `manuell` schliessen sich aus; der Server lehnt beides
 * zusammen mit 400 ab, statt sich still fuer eins zu entscheiden.
 */
export interface UplinkDestinationSaveAck {
  ok: boolean;
  connection_generations: Partial<Record<UplinkPlattform, number>>;
  live_quality?: UplinkLiveQualitaet;
}

export function saveUplinkDestination(body: {
  platform: UplinkPlattform;
  rtmp_url?: string;
  stream_key?: string;
  profil?: UplinkProfilName;
  manuell?: UplinkManuellesProfil;
  enabled?: boolean;
  /** Nur für Twitch; weglassen erhält die bisherige Wahl, auch einen Altbestand ohne Wahl. */
  twitch_audio_mode?: UplinkTwitchAudioMode;
  /** Nur für Twitch; weglassen bewahrt die gespeicherte Betriebsart. */
  twitch_output_mode?: UplinkTwitchOutputMode;
}): Promise<UplinkDestinationSaveAck> {
  return fetchJson('/twitch/api/v2/uplink/destinations', withCookieCredentials({
    method: 'PUT',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  }));
}

/**
 * Was eine Plattform an Video empfiehlt. Liegt in `uplinkEmpfehlung.ts`, damit
 * die Umschrift der Serverantwort ohne den Fetch-Unterbau pruefbar bleibt, und
 * steht hier weiter zur Verfuegung, wo die uebrigen Uplink-Typen liegen.
 */
export type { UplinkCaps } from '../uplinkEmpfehlung';

export interface UplinkCapsAntwort {
  platforms: UplinkCaps[];
}

/** Geprüfte Hinweise des Dienstes; fehlende Werte bleiben ausdrücklich unbekannt. */
export async function fetchUplinkCaps(): Promise<UplinkCapsAntwort> {
  const antwort = await fetchJson<{ platforms?: UplinkCapsRoh[] }>(
    '/twitch/api/v2/uplink/caps',
    withCookieCredentials()
  );
  return { platforms: (antwort.platforms ?? []).map(normalisiereCaps) };
}

/**
 * Ein gespeichertes Ziel, wie das Relay es zurueckgibt.
 *
 * Ohne Stream-Key: der liegt verschluesselt in der Datenbank und wird nie
 * wieder ausgeliefert. Fuer die Oberflaeche zaehlt nur, dass es ihn gibt.
 */
export interface UplinkDestination extends ZielBetriebsdaten {
  platform: string;
  rtmp_url: string;
  enabled: boolean;
  requested_output_mode?: UplinkTwitchOutputMode | null;
  active_output_mode?: UplinkTwitchOutputMode | null;
  fallback_reason?: string | null;
  /** Ausdrücklich gespeichert; null bedeutet weiterhin die bisherige Servereinstellung. */
  twitch_audio_mode?: UplinkTwitchAudioMode | null;
  /** Vom Dienst bestätigte Wahl oder bisherige Einstellung für den nächsten Stream. */
  effective_audio_mode?: UplinkTwitchAudioMode | null;
  /** Nur aus dem tatsächlich sendenden Graph, nie aus dem gespeicherten Wunsch. */
  active_audio_mode?: UplinkTwitchAudioMode | null;
  /** Gespeicherter Wunsch. Die tatsächliche Ausgabe steht in active_profile. */
  requested?: UplinkProfilAnsicht;
  /**
   * Frueher das Ergebnis der Klemmung gegen die Plattform-Grenzen, heute immer
   * identisch mit `requested`. Steht nur noch im JSON, damit aeltere Clients
   * nicht brechen. Die Oberflaeche liest ausschliesslich `requested`: sonst
   * zeigt sie wieder einen anderen Wert an, als im Eingabefeld steht.
   */
  effective?: UplinkProfilAnsicht;
}

/** Ein lokaler Entwurf bleibt bei Refetch bestehen; er ändert keine laufende Ausgabe. */
export function twitchAudioFormular(
  ziel: UplinkDestination | undefined,
  entwurf: UplinkTwitchAudioMode | null,
) {
  const bekannt = (wert: unknown): UplinkTwitchAudioMode | null =>
    wert === 'live' || wert === 'separate_vod' ? wert : null;
  const twitch = ziel?.platform === 'twitch' ? ziel : undefined;
  const gespeichert = bekannt(twitch?.twitch_audio_mode);
  return {
    auswahl: entwurf ?? gespeichert,
    gespeichert,
    naechsterStream: bekannt(twitch?.effective_audio_mode),
    aktiv: twitch?.output_state === 'sending' && !twitch.blocked
      ? bekannt(twitch.active_audio_mode) : null,
    geaendert: entwurf !== null && entwurf !== gespeichert,
  };
}

export function fetchUplinkDestinations(): Promise<{ destinations: UplinkDestination[] }> {
  return fetchJson<{ destinations: UplinkDestination[] }>(
    '/twitch/api/v2/uplink/destinations',
    withCookieCredentials()
  );
}
