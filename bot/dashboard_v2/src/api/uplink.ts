import { fetchJson, withCookieCredentials } from './core';
import { normalisiereCaps } from '../uplinkEmpfehlung';
import type { UplinkCaps, UplinkCapsRoh } from '../uplinkEmpfehlung';

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
  rtmp_url: string;
  srt_hint: string;
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

export type UplinkVerbindungStatus = 'verbunden' | 'neu_verbinden' | 'getrennt';

export interface UplinkVerbindung {
  platform: string;
  status: UplinkVerbindungStatus;
  /** Ob im Uplink schon ein Ziel fuer diese Plattform liegt. */
  stream_key_vorhanden?: boolean;
}

/**
 * Die vier Fenster hinter einem Zugang. Ein Neuerzeugen macht alle vier alten
 * Adressen ungueltig.
 *
 * Optional, weil aeltere Server nur `dock_url` liefern. Dann gilt die eine
 * Adresse als Chat-Fenster und die drei anderen fehlen einfach, statt dass die
 * Karte auf einen Platzhalter zeigt.
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
export function rotateUplinkDockToken(): Promise<{ dock_url: string; dock_urls?: DockUrls }> {
  return fetchJson<{ dock_url: string; dock_urls?: DockUrls }>(
    '/twitch/api/v2/uplink/dock-token/rotate',
    withCookieCredentials({
      method: 'POST',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: '{}',
    })
  );
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
  if (platform !== 'twitch') return '';
  return '/twitch/raid/auth?scope_profile=uplink';
}

/**
 * Trennt eine Plattform: Zugang zurueckgenommen, Ziel im Uplink entfernt.
 * Das stoppt auch den Raid-Bot, weil beides an demselben Zugang haengt.
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
 * Trennen nimmt den ganzen Zugang zurueck, nicht nur den Uplink-Teil. Wer das
 * nicht dazuschreibt, schaltet Leuten unbemerkt ihre Auto-Raids ab.
 */
export const TRENNEN_HINWEIS =
  'Trennen nimmt den Zugang ganz zurück. Damit hören auch die automatischen Raids auf, bis du dich neu verbindest.';

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
  'Holt Stream-Schlüssel, Chat, Aktivitäten, Stream-Infos und Kanalpunkte in einem Schritt.';

export const VERBINDEN_HINWEIS =
  'Twitch zeigt dir gleich die Liste der Rechte. Neu dazu kommen: deinen Stream-Key holen, den Chat lesen und darin antworten, Aktivitäten wie Follows sehen und Kanalpunkt-Einlösungen abhaken. Die übrigen Punkte in der Liste gehören zum Bot und zum Dashboard.';

/** Eine Zeile je Plattform fuer den Kopf der Plattform-Karte. */
export function plattformVerbindungen(me: UplinkMe): UplinkPlattformVerbindung[] {
  return UPLINK_PLATTFORMEN.map((p) => {
    const eintrag = me.verbindungen?.find((v) => v.platform === p.id);
    const status = eintrag?.status ?? 'getrennt';
    const aktiv = verbindenAktiv(p.id);
    // Verbinden holt jetzt alles auf einmal: Chat lesen und schreiben, den
    // Stream-Key und die Rechte fuer Aktivitaet und Kanalpunkte. Deshalb steht
    // in den Texten nicht mehr nur "Chat".
    const streamKeyVorhanden = eintrag?.stream_key_vorhanden ?? false;
    // Drei Stufen, nicht zwei. "Verbunden" heisst: der Grant traegt alle
    // noetigen Rechte UND der Stream-Schluessel liegt im Uplink. Fehlt der
    // Schluessel, ist der Zugang zwar da, aber es geht noch kein Bild raus,
    // und ein blankes "Verbunden" waere genau die Falschaussage, die den
    // Streamer am Sendetag suchen laesst.
    let statusText = 'Folgt später';
    let knopfText: string | null = null;
    if (aktiv) {
      if (status === 'verbunden') {
        statusText = streamKeyVorhanden ? 'Verbunden' : 'Verbunden, Schlüssel fehlt';
      } else if (status === 'neu_verbinden') {
        // Nicht wortgleich mit dem Knopf daneben: der Status sagt den Zustand,
        // der Knopf die Handlung. Zweimal "Neu verbinden" nebeneinander liest
        // sich wie zwei Knoepfe.
        statusText = 'Zugang abgelaufen';
      } else {
        statusText = 'Nicht verbunden';
      }
      knopfText =
        status === 'getrennt' ? `Mit ${p.label} verbinden` : 'Neu verbinden';
    }
    return {
      id: p.id,
      label: p.label,
      status,
      aktiv,
      statusText,
      knopfText,
      streamKeyVorhanden,
      trennenMoeglich: aktiv && status !== 'getrennt',
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
 * `frisch` kommt aus dem Erzeugen und geht vor: das Relay liefert die neuen
 * Adressen sofort in der Antwort, waehrend `me` erst mit dem naechsten Abruf
 * nachzieht. Ohne diesen Vorrang stuende der Streamer direkt nach dem Klick
 * kurz vor einer leeren Karte, obwohl seine alten Adressen schon nicht mehr
 * gelten.
 *
 * Eine leere Adresse faellt weg, statt eine Kopierzeile ohne Ziel anzubieten.
 */
export function dockAdressen(me: UplinkMe, frisch?: DockUrls | null): DockAdresse[] {
  const quelle = frisch ?? me.dock_urls;
  if (!quelle) return [];
  const liste: DockAdresse[] = [];
  for (const dock of EIGENE_DOCKS) {
    const url = quelle[dock.feld]?.trim() ?? '';
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

/** Der Wert gilt nur fuer einen unerwarteten Abriss, nicht fuer OBS-Stop. */
export const UPLINK_RECONNECT_WAIT_TEXT =
  'Diese Zeit gilt nur nach einem unerwarteten Internetabriss. Wenn du den Stream in OBS beendest, räumt Uplink sofort auf.';

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

/**
 * Die Profilnamen muessen zum Katalog in `handlers/uplink.rs` passen. Ein Name,
 * den der Server nicht kennt, gibt 400 statt still auf den Standard zu fallen.
 *
 * `warnung` ist gesetzt, wo die Stufe zwar waehlbar, aber nicht unbedenklich
 * ist. Getrennt vom `hinweis`, damit die Oberflaeche sie anders faerben kann:
 * ein Nachteil, den man ueberliest, ist keiner.
 *
 * Die 1440p-Warnung steht auf den Angaben aus dem Twitch-Hilfeartikel "2k
 * Streaming auf Twitch": 2K braucht dort Enhanced Broadcasting, das gibt es nur
 * fuer Partner und Affiliates, und es geht an Twitchs eigenen Ingest. Ueber
 * Uplink laeuft klassisches RTMP, damit ist 1440p bei Twitch offiziell nicht
 * unterstuetzt und es gibt keine Qualitaetsstufen fuer die Zuschauer.
 *
 * Die Bitrate aus demselben Artikel steht bewusst NICHT in der Warnung. Sie
 * gilt fuer jemanden, der 2K direkt an Twitch schickt. Ueber Uplink geht
 * `PROFIL_WERTE['1440p60']` an Twitch, also 12000 kbps, und was der Streamer
 * zu uns hochlaedt, ist davon wieder unabhaengig. Eine Zahl, die auf keinen
 * dieser drei Wege passt, gehoert nicht in eine Warnung.
 */
export const UPLINK_PROFILE = [
  {
    name: '1440p60',
    label: '1440p60 (2K), 12000 kbps',
    hinweis: 'Deine volle 2K-Auflösung, ohne Verkleinerung. Schick uns dafür auch 1440p aus OBS.',
    warnung:
      'Twitch nimmt 2K offiziell nur über Enhanced Broadcasting an, und das läuft über Twitchs eigenen Ingest, nicht über uns. Für deine Zuschauer heißt das keine Qualitätsstufen: wer eine schwache Leitung hat, puffert, statt auf 720p zu wechseln. Probier es einen Abend aus und schick uns dafür auch 1440p aus OBS.',
  },
  {
    name: '1080p60-hoch',
    label: '1080p60, 8000 kbps',
    hinweis: 'Twitch-Maximum für 1080p. Nur mit Partner- oder Affiliate-Status sinnvoll.',
    warnung: '',
  },
  {
    name: '1080p60',
    label: '1080p60, 6000 kbps',
    hinweis: 'Standard. Passt auf jede Leitung, die Twitch akzeptiert.',
    warnung: '',
  },
  {
    name: '720p60',
    label: '720p60, 4500 kbps',
    hinweis: 'Weniger Serverlast und weniger Upload, immer noch 60 Bilder.',
    warnung: '',
  },
  {
    name: '480p30',
    label: '480p30, 1500 kbps',
    hinweis: 'Notfallstufe bei schlechter Leitung.',
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
  { id: 'twitch', label: 'Twitch', rtmp: 'rtmp://live.twitch.tv/app' },
  { id: 'youtube', label: 'YouTube', rtmp: 'rtmp://a.rtmp.youtube.com/live2' },
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
  status: 'applied' | 'applied_restart' | 'too_busy';
  message: string;
}

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
export function saveUplinkDestination(body: {
  platform: UplinkPlattform;
  rtmp_url?: string;
  stream_key?: string;
  profil?: UplinkProfilName;
  manuell?: UplinkManuellesProfil;
  enabled?: boolean;
}): Promise<{ destinations: UplinkDestination[]; live_quality?: UplinkLiveQualitaet }> {
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

/**
 * Der Empfehlungskatalog. Kommt vom Server, damit die Oberflaeche ihn nicht
 * doppelt pflegt: `relay.platform_caps` ist eine Tabelle in einem anderen
 * Repo und kann sich per Migration bewegen, ohne dass hier jemand etwas tut.
 */
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
export interface UplinkDestination {
  platform: string;
  rtmp_url: string;
  enabled: boolean;
  /** Was der Streamer eingestellt hat, und damit auch das, was rausgeht. */
  requested?: UplinkProfilAnsicht;
  /**
   * Frueher das Ergebnis der Klemmung gegen die Plattform-Grenzen, heute immer
   * identisch mit `requested`. Steht nur noch im JSON, damit aeltere Clients
   * nicht brechen. Die Oberflaeche liest ausschliesslich `requested`: sonst
   * zeigt sie wieder einen anderen Wert an, als im Eingabefeld steht.
   */
  effective?: UplinkProfilAnsicht;
}

export function fetchUplinkDestinations(): Promise<{ destinations: UplinkDestination[] }> {
  return fetchJson<{ destinations: UplinkDestination[] }>(
    '/twitch/api/v2/uplink/destinations',
    withCookieCredentials()
  );
}
