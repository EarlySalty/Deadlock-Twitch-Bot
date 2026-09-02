import { useEffect, useId, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronDown, Loader2, Power } from 'lucide-react';
import kickLogo from '@/assets/platforms/kick.svg';
import tiktokLogo from '@/assets/platforms/tiktok.svg';
import twitchLogo from '@/assets/platforms/twitch.svg';
import youtubeLogo from '@/assets/platforms/youtube.svg';
import {
  PROFIL_WERTE,
  UPLINK_PROFILE,
  profilNameFuer,
  holeUplinkStreamKey,
  saveUplinkDestination,
  trenneUplinkPlattform,
  TRENNEN_HINWEIS,
  VERBINDEN_HINWEIS,
  VERBINDEN_KURZ,
  uplinkConnectUrl,
} from '@/api/uplink';
import type {
  UplinkCaps,
  UplinkDestination,
  UplinkManuellesProfil,
  UplinkPlattform,
  UplinkPlattformVerbindung,
  UplinkProfilAnsicht,
  UplinkProfilName,
} from '@/api/uplink';
import { useUplinkDisclosure } from '@/uplinkDisclosure';

type Modus = 'stufe' | 'manuell';

const PLATTFORM_LOGOS: Record<UplinkPlattform, string> = {
  twitch: twitchLogo,
  youtube: youtubeLogo,
  kick: kickLogo,
  tiktok: tiktokLogo,
};

/** Zahlenfeld im manuellen Modus. Leerer Text ist erlaubt, sonst kann man die
 *  fuehrende Ziffer nicht loeschen, ohne dass eine 0 nachrutscht. */
function ZahlFeld({
  label,
  einheit,
  wert,
  empfehlung,
  onChange,
}: {
  label: string;
  einheit: string;
  /** Was die Plattform empfiehlt. Nur ein Hinweis, keine Grenze. */
  empfehlung: number | null;
  wert: string;
  onChange: (wert: string) => void;
}) {
  const zahl = Number(wert);
  const darueber = empfehlung !== null && Number.isFinite(zahl) && zahl > empfehlung;
  return (
    <label className="block space-y-1">
      <span className="block text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
        {label}
      </span>
      <span className="flex items-center gap-2 rounded-xl border border-border bg-background/70 px-3 py-2">
        <input
          value={wert}
          inputMode="numeric"
          onChange={(e) => onChange(e.target.value.replace(/[^0-9]/g, ''))}
          className="w-full min-w-0 bg-transparent text-sm text-white outline-none"
        />
        <span className="shrink-0 text-xs text-text-secondary">{einheit}</span>
      </span>
      {/* Bewusst dieselbe ruhige Farbe wie der Normalfall: hier ist nichts
          kaputt und nichts abgelehnt, es ist nur ein Wert oberhalb dessen, was
          die Plattform vorschlaegt. Ein rotes Feld waere eine Falschaussage.
          Kurz gehalten: die Spalte ist schmal, und wessen Empfehlung das ist,
          steht in der Ueberschrift ueber den Feldern. Was daraus folgt, steht
          im Satz darunter, nicht viermal nebeneinander. */}
      {empfehlung !== null ? (
        <span className="block text-[11px] text-text-secondary">
          {darueber ? `über der Empfehlung: ${empfehlung}` : `Empfehlung: ${empfehlung}`}
        </span>
      ) : null}
    </label>
  );
}

/** Gleiche Werte? Entscheidet, ob im Kopf "nicht gespeichert" steht. */
function gleicheWerte(a: UplinkProfilAnsicht | undefined, b: UplinkProfilAnsicht | undefined) {
  if (!a || !b) return false;
  return (
    a.width === b.width && a.height === b.height && a.fps === b.fps && a.bitrate_kbps === b.bitrate_kbps
  );
}

/**
 * Verbindungsstand im Kopf der Plattform-Karte.
 *
 * Verbinden holt alles auf einmal: Chat lesen und schreiben, den Stream-Key
 * fuer das Uplink-Ziel und die Rechte fuer Aktivitaet und Kanalpunkte. Der
 * Knopf ist ein Link, keine fetch-Aktion: der Server leitet direkt zur
 * Anmeldeseite der Plattform weiter. Ein Link in der Summary folgt beim Klick
 * dem Ziel, statt die Karte auf- oder zuzuklappen.
 *
 * Trennen sitzt hier und nicht in der Chat-Karte, weil es hier auch etwas
 * ueber das Ziel dieser Plattform entscheidet. Es ist ein Knopf, kein Link:
 * ein Klick, der einen Zugang zurueckgibt, gehoert nicht in eine
 * Browser-Navigation. Der Hinweis darunter sagt, was noch daran haengt.
 */
/**
 * Holt den Stream-Key nach und legt ihn als Uplink-Ziel ab.
 *
 * Zwei Stellen brauchen denselben Klick: der Kopf der Karte, wenn verbunden
 * ist und trotzdem kein Ziel liegt, und der Feldbereich, wenn alles steht und
 * der Streamer seinen Schluessel bei Twitch neu erzeugt hat. Eine Komponente
 * statt zweier Kopien, damit die Rueckmeldung an beiden Orten dieselbe ist.
 */
function StreamKeyKnopf({
  platform,
  csrfToken,
  beschriftung,
  klasse,
}: {
  platform: UplinkPlattform;
  csrfToken: string | null;
  beschriftung: string;
  klasse: string;
}) {
  const queryClient = useQueryClient();
  const holen = useMutation({
    mutationFn: () => holeUplinkStreamKey(platform, csrfToken ?? ''),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
      queryClient.invalidateQueries({ queryKey: ['uplink-destinations'] });
    },
  });
  // Kick und YouTube holen den Schlüssel beim Verbinden, nicht über den
  // Twitch-Weg. Der erneute Anlauf ist deshalb ein erneutes Verbinden.
  if (platform !== 'twitch') {
    return (
      <a href={uplinkConnectUrl(platform)} className={klasse}>
        {beschriftung}
      </a>
    );
  }
  return (
    <>
      <button
        type="button"
        disabled={holen.isPending}
        onClick={(e) => {
          e.preventDefault();
          holen.mutate();
        }}
        className={klasse}
      >
        {holen.isPending ? 'Wird geholt' : beschriftung}
      </button>
      {holen.isError ? (
        <span role="alert" className="text-[11px] text-warning">
          Der Stream-Schlüssel kam gerade nicht durch. Bitte gleich noch einmal versuchen.
        </span>
      ) : null}
      {holen.isSuccess ? (
        <span className="text-[11px] text-success">Stream-Schlüssel ist im Uplink hinterlegt.</span>
      ) : null}
    </>
  );
}

function PlattformVerbindung({
  chat,
  csrfToken,
}: {
  chat: UplinkPlattformVerbindung;
  csrfToken: string | null;
}) {
  const queryClient = useQueryClient();
  const [nachfrage, setNachfrage] = useState(false);
  const trennen = useMutation({
    mutationFn: () => trenneUplinkPlattform(chat.id, csrfToken ?? ''),
    onSuccess: () => {
      setNachfrage(false);
      queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
      queryClient.invalidateQueries({ queryKey: ['uplink-destinations'] });
    },
  });
  // Verbunden, aber im Uplink liegt kein Ziel: dann ist beim automatischen
  // Nachlauf etwas schiefgegangen. Ohne diesen Knopf bliebe nur der
  // Handeintrag, und der Streamer weiss nicht einmal, dass etwas fehlt.
  const keyFehlt = chat.status === 'verbunden' && !chat.streamKeyVorhanden;
  const knopfKlasse =
    'inline-flex items-center rounded-md border border-primary/30 bg-primary/10 px-2 py-0.5 text-[11px] font-semibold text-primary transition-colors hover:border-primary/60';
  const stillKlasse =
    'inline-flex items-center rounded-md border border-border px-2 py-0.5 text-[11px] font-semibold text-text-secondary transition-colors hover:border-warning/60 hover:text-warning disabled:opacity-60';
  const statusKlasse = !chat.aktiv
    ? 'text-text-secondary/80'
    : chat.status === 'verbunden'
      ? 'text-success'
      : 'text-text-secondary';
  return (
    <span data-chat={chat.status} className="mt-1 flex flex-col gap-1 text-xs font-normal">
      <span className="flex flex-wrap items-center gap-2">
        <span className={statusKlasse}>{chat.statusText}</span>
        {chat.aktiv && chat.knopfText ? (
          <a href={uplinkConnectUrl(chat.id)} className={knopfKlasse}>
            {chat.knopfText}
          </a>
        ) : null}
        {keyFehlt ? (
          <StreamKeyKnopf
            platform={chat.id}
            csrfToken={csrfToken}
            beschriftung="Stream-Schlüssel holen"
            klasse={knopfKlasse}
          />
        ) : null}
        {chat.trennenMoeglich ? (
          <button
            type="button"
            onClick={(e) => {
              e.preventDefault();
              setNachfrage(true);
            }}
            className={stillKlasse}
          >
            Trennen
          </button>
        ) : null}
      </span>
      {/* Nur solange nichts verbunden ist: ein Satz, was der Klick bringt.
          Wer verbunden ist, sieht hier nichts ausser den Knoepfen. Das Lange
          steckt in der Hilfe daneben, und der Trennen-Hinweis erscheint erst
          in der Rueckfrage, wo er auch etwas entscheidet. */}
      {chat.status === 'getrennt' ? (
        <span className="flex flex-wrap items-baseline gap-1.5 text-[11px] text-text-secondary">
          <span>{VERBINDEN_KURZ}</span>
          <details className="inline">
            <summary className="cursor-pointer list-none text-primary underline decoration-dotted underline-offset-2">
              Welche Rechte?
            </summary>
            <span className="mt-1 block max-w-prose">{VERBINDEN_HINWEIS}</span>
          </details>
        </span>
      ) : null}
      {nachfrage ? (
        <span className="flex flex-col gap-1 rounded-xl border border-warning/40 bg-warning/5 px-3 py-2">
          <span className="text-[11px] text-warning">{TRENNEN_HINWEIS}</span>
          <span className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              disabled={trennen.isPending}
              onClick={(e) => {
                e.preventDefault();
                trennen.mutate();
              }}
              className={stillKlasse}
            >
              {trennen.isPending ? 'Wird getrennt' : 'Ja, trennen'}
            </button>
            <button
              type="button"
              onClick={(e) => {
                e.preventDefault();
                setNachfrage(false);
              }}
              className={stillKlasse}
            >
              Abbrechen
            </button>
          </span>
        </span>
      ) : null}
      {trennen.isError ? (
        <span role="alert" className="text-[11px] text-warning">
          Das Trennen hat gerade nicht geklappt. Bitte gleich noch einmal versuchen.
        </span>
      ) : null}
    </span>
  );
}

/**
 * Eine Zielkarte je Plattform: Zugangsdaten, Qualitaet, Status.
 *
 * Die Qualitaet laesst sich hier ohne den Stream-Key speichern. Das war der
 * eigentliche Fehler an der alten Fassung: der Speichern-Knopf blieb ohne
 * frisch eingetippten Key tot, und weil der Key nie zurueckkommt, hiess das
 * fuer ein eingerichtetes Ziel: die Stufe liess sich gar nicht mehr aendern.
 */
export function ZielKarte({
  platform,
  label,
  rtmpVorgabe,
  ziel,
  caps,
  chat,
  csrfToken,
  offenStart,
}: {
  platform: UplinkPlattform;
  label: string;
  rtmpVorgabe: string;
  ziel: UplinkDestination | undefined;
  caps: UplinkCaps | undefined;
  /** Stand des Zugangs dieser Plattform: verbunden, neu verbinden, getrennt. */
  chat?: UplinkPlattformVerbindung;
  /** Fuer das Trennen; ohne Token bleibt der Knopf nur eine Anzeige. */
  csrfToken?: string | null;
  offenStart: boolean;
}) {
  const queryClient = useQueryClient();
  const basisId = useId();
  const eingerichtet = Boolean(ziel);
  // Adresse und Schluessel kommen von der Plattform-Verbindung, nicht aus dem
  // Formular. Beide Bedingungen zaehlen: ein Grant ohne hinterlegtes Ziel ist
  // noch kein automatischer Weg, und ein Ziel ohne Grant ist ein Handeintrag,
  // den niemand ueberschreiben darf.
  const automatisch = chat?.status === 'verbunden' && chat.streamKeyVorhanden;
  const [offen, setOffen] = useUplinkDisclosure(`plattform-${platform}`, offenStart);

  const [rtmpUrl, setRtmpUrl] = useState(ziel?.rtmp_url || rtmpVorgabe);
  const [streamKey, setStreamKey] = useState('');
  const [modus, setModus] = useState<Modus>('stufe');
  const [profil, setProfil] = useState<UplinkProfilName>('1080p60');
  const [manuell, setManuell] = useState({
    width: '1920',
    height: '1080',
    fps: '60',
    bitrate_kbps: '6000',
  });
  const [vorbelegt, setVorbelegt] = useState(false);
  const [fehlertext, setFehlertext] = useState('');
  // Nur wahr, solange seit dem letzten Erfolg nichts angefasst wurde. Ein
  // dauerhaftes "Gespeichert" auf dem Knopf sagt nichts mehr ueber den
  // aktuellen Stand und deckt genau die Faelle zu, in denen etwas offen ist.
  const [gespeichert, setGespeichert] = useState(false);
  // Was der Server ueber den gerade laufenden Stream gesagt hat. Ohne diesen
  // Satz stuende hier ein "Gespeichert", waehrend auf der Plattform weiter
  // das alte Bild laeuft, und niemand wuesste, ob das noch kommt.
  const [livetext, setLivetext] = useState('');
  const angefasst = () => {
    setGespeichert(false);
    setFehlertext('');
    setLivetext('');
  };

  const bestellt = ziel?.requested;

  // Einmal aus dem gespeicherten Ziel vorbelegen, nicht bei jedem Refetch:
  // sonst zoege ein Hintergrundabruf die Eingabe zurueck, waehrend jemand
  // gerade tippt. Steht der gespeicherte Wunsch nicht im Stufenkatalog, ist
  // es ein manueller Wert, und die Karte oeffnet direkt im manuellen Modus.
  useEffect(() => {
    if (vorbelegt || !bestellt) return;
    setRtmpUrl(ziel?.rtmp_url || rtmpVorgabe);
    setManuell({
      width: String(bestellt.width),
      height: String(bestellt.height),
      fps: String(bestellt.fps),
      bitrate_kbps: String(bestellt.bitrate_kbps),
    });
    const name = profilNameFuer(bestellt);
    if (name) {
      setProfil(name);
      setModus('stufe');
    } else {
      setModus('manuell');
    }
    setVorbelegt(true);
  }, [bestellt, vorbelegt, ziel?.rtmp_url, rtmpVorgabe]);

  // Sobald der automatische Weg greift, faellt weg, was jemand vorher ins
  // Formular getippt hat. Der Riegel in `speichern` haelt auch ohne das; ein
  // eingetippter Schluessel soll trotzdem nicht im Speicher der Seite liegen
  // bleiben, und nach einem Trennen soll das Feld leer starten.
  useEffect(() => {
    if (!automatisch) return;
    setStreamKey('');
    setRtmpUrl(ziel?.rtmp_url || rtmpVorgabe);
  }, [automatisch, ziel?.rtmp_url, rtmpVorgabe]);

  // Beim Wechsel in den manuellen Modus die Zahlen der gewaehlten Stufe
  // uebernehmen. Ein leeres Formular waere hier der schlechteste Start: es
  // sieht aus, als muesste man von vorn anfangen.
  const nachManuell = () => {
    if (modus === 'manuell') return;
    const [w, h, f, b] = PROFIL_WERTE[profil];
    setManuell({
      width: String(w),
      height: String(h),
      fps: String(f),
      bitrate_kbps: String(b),
    });
    setModus('manuell');
  };

  // Was die Plattform vorschlaegt. Steht als Hinweis am Feld und sonst
  // nirgends: es gibt keine Obergrenze mehr, gegen die hier jemand pruefen
  // koennte. Was eingestellt ist, geht genau so raus.
  const plattformEmpfehlung = {
    width: caps?.recommended_width ?? null,
    height: caps?.recommended_height ?? null,
    fps: caps?.recommended_fps ?? null,
    bitrate_kbps: caps?.recommended_bitrate_kbps ?? null,
  };

  /**
   * Prueft die manuellen Zahlen, bevor sie abgeschickt werden.
   *
   * Nur noch das, was technisch nicht geht: eine fehlende Zahl und ungerade
   * Kantenlaengen, mit denen H.264 nicht umgehen kann. Keine Obergrenze mehr,
   * kein Vergleich gegen die Plattform. Wer 16000 kbps will, bekommt 16000.
   */
  function manuellPruefen(): UplinkManuellesProfil | string {
    const zahlen = {
      width: Number(manuell.width),
      height: Number(manuell.height),
      fps: Number(manuell.fps),
      bitrate_kbps: Number(manuell.bitrate_kbps),
    };
    const felder: [keyof typeof zahlen, string][] = [
      ['width', 'Breite'],
      ['height', 'Höhe'],
      ['fps', 'Bildrate'],
      ['bitrate_kbps', 'Bitrate'],
    ];
    for (const [feld, name] of felder) {
      const wert = zahlen[feld];
      if (!Number.isFinite(wert) || wert <= 0) return `${name} fehlt.`;
    }
    if (zahlen.width % 2 !== 0 || zahlen.height % 2 !== 0) {
      return 'Breite und Höhe müssen gerade Zahlen sein.';
    }
    return zahlen;
  }

  const speichern = useMutation({
    mutationFn: async (enabled?: boolean) => {
      // Im automatischen Weg gibt es kein Formular mehr: Adresse und
      // Schluessel kommen von der Verbindung. Was noch im State liegt, darf
      // nicht mitgehen. Der Fall ist real: wer bei "Verbunden, Schlüssel
      // fehlt" erst einen alten Schluessel eintippt und dann oben "Stream-
      // Schlüssel holen" klickt, haette beim naechsten Speichern der Qualitaet
      // den frisch geholten Schluessel wieder ueberschrieben, waehrend die
      // Karte daneben behauptet, er komme von Twitch. Auffallen wuerde das
      // erst am Sendetag.
      const url = automatisch ? '' : rtmpUrl.trim();
      const key = automatisch ? '' : streamKey.trim();
      if (!automatisch) {
        if (key && !url) {
          throw new Error('Ohne Serveradresse können wir den Schlüssel nicht zuordnen.');
        }
        if (!key && !eingerichtet) {
          throw new Error(`Für ${label} fehlt uns noch der Stream-Schlüssel.`);
        }
      }
      // Adresse und Schluessel gehoeren zusammen: das Relay nimmt sie nur
      // gemeinsam an, weil beide gleich wieder als Argument an ffmpeg gehen
      // und gemeinsam geprueft werden. Eine geaenderte Adresse ohne Schluessel
      // wuerde also stillschweigend unter den Tisch fallen, waehrend der
      // Knopf "Gespeichert" meldet und das Feld die neue Adresse behaelt.
      // Deshalb hier ein Halt mit Grund statt eines verworfenen Feldes.
      if (!automatisch && !key && eingerichtet && url !== (ziel?.rtmp_url ?? '')) {
        throw new Error(
          'Die Serveradresse können wir nur zusammen mit dem Stream-Schlüssel ändern. Trag beides ein.',
        );
      }
      const body: Parameters<typeof saveUplinkDestination>[0] = { platform };
      if (key) {
        body.rtmp_url = url;
        body.stream_key = key;
      }
      if (enabled !== undefined) body.enabled = enabled;
      // Die Qualitaet geht immer mit, auch beim Pausieren. Sonst verliert ein
      // Klick auf "Ziel pausieren" die Stufe, die daneben im Formular steht,
      // wortlos: die Auswahl bliebe stehen, gespeichert waere sie nicht.
      if (modus === 'manuell') {
        const geprueft = manuellPruefen();
        if (typeof geprueft === 'string') throw new Error(geprueft);
        body.manuell = geprueft;
      } else {
        body.profil = profil;
      }
      return saveUplinkDestination(body);
    },
    onSuccess: (antwort) => {
      setStreamKey('');
      setFehlertext('');
      setGespeichert(true);
      setLivetext(antwort.live_quality?.message ?? '');
      queryClient.invalidateQueries({ queryKey: ['uplink-destinations'] });
      queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
    },
    onError: (e) =>
      setFehlertext(
        e instanceof Error && e.message ? e.message : 'Speichern hat nicht geklappt.',
      ),
  });

  const gewaehlteStufe = UPLINK_PROFILE.find((e) => e.name === profil);

  /**
   * Die Werte, die gerade im Formular stehen.
   *
   * `null`, solange ein manuelles Feld leer ist: waehrend jemand eine Zahl
   * loescht und neu tippt, soll im Kopf nicht kurz 0p0 stehen.
   */
  const eingetippt = ((): UplinkProfilAnsicht | null => {
    if (modus === 'stufe') {
      const [width, height, fps, bitrate_kbps] = PROFIL_WERTE[profil];
      return { width, height, fps, bitrate_kbps };
    }
    const zahlen = {
      width: Number(manuell.width),
      height: Number(manuell.height),
      fps: Number(manuell.fps),
      bitrate_kbps: Number(manuell.bitrate_kbps),
    };
    const vollstaendig = Object.values(zahlen).every((n) => Number.isFinite(n) && n > 0);
    return vollstaendig ? zahlen : null;
  })();

  // Der Kopf zeigt mit, was im Formular steht, und faellt auf den gespeicherten
  // Stand zurueck, solange ein Feld leer ist. Vorher stand hier `effective`,
  // also das Ergebnis der Klemmung: im Feld 16000, im Kopf 12000, und keiner
  // der beiden Werte erklaerte den anderen.
  //
  // Vor der Vorbelegung zaehlt nur der gespeicherte Stand: das Formular steht
  // dann noch auf seinem Anfangswert, und ein Bild lang "1080p60, nicht
  // gespeichert" ueber einem 1440p-Ziel waere schlicht falsch.
  const kopfWerte = vorbelegt ? eingetippt ?? bestellt : bestellt;
  const ungespeichert =
    eingerichtet && vorbelegt && !gleicheWerte(eingetippt ?? undefined, bestellt);
  const kartenStatus = !eingerichtet ? 'nicht-eingerichtet' : ziel?.enabled ? 'aktiv' : 'pausiert';
  const statusText = !eingerichtet ? 'nicht eingerichtet' : ziel?.enabled ? 'aktiv' : 'pausiert';
  const rtmpId = `${basisId}-rtmp`;
  const keyId = `${basisId}-key`;
  const profilId = `${basisId}-profil`;
  const fehlerId = `${basisId}-fehler`;
  const kartenKlasse = ziel?.enabled
    ? 'border-success/55 bg-success/10 shadow-[0_18px_46px_rgba(67,181,129,0.18)] ring-1 ring-success/15'
    : eingerichtet
      ? 'border-warning/30 bg-warning/5'
      : 'border-border bg-background/35';

  return (
    <details
      data-platform={platform}
      data-state={kartenStatus}
      open={offen}
      onToggle={(ereignis) => setOffen(ereignis.currentTarget.open)}
      className={`group overflow-hidden rounded-2xl border transition-colors ${kartenKlasse}`}
    >
      <summary className={`flex cursor-pointer list-none flex-wrap items-center justify-between gap-3 transition-colors hover:bg-white/5 [&::-webkit-details-marker]:hidden ${ziel?.enabled ? 'px-5 py-5' : 'px-4 py-3.5'}`}>
        <span className="flex min-w-0 items-center gap-3">
          <span aria-hidden="true" className={`flex shrink-0 items-center justify-center rounded-xl border text-xs font-black tracking-tight ${ziel?.enabled ? 'h-11 w-11 border-success/50 bg-success/15 text-success shadow-[0_8px_24px_rgba(67,181,129,0.18)]' : 'h-10 w-10 border-primary/25 bg-primary/10 text-primary'}`}>
            <img
              src={PLATTFORM_LOGOS[platform]}
              alt=""
              className={`h-5 w-5 ${ziel?.enabled ? 'opacity-100' : 'opacity-80'}`}
            />
          </span>
          <span className="min-w-0">
            <span className="flex flex-wrap items-center gap-2">
              <span className={`${ziel?.enabled ? 'text-base' : 'text-sm'} font-bold text-white`}>{label}</span>
              <span
                className={`rounded-full px-2 py-0.5 text-[11px] font-semibold ${
                  ziel?.enabled
                    ? 'bg-success/15 text-success'
                    : eingerichtet
                      ? 'bg-warning/15 text-warning'
                      : 'bg-white/5 text-text-secondary'
                }`}
              >
                {statusText}
              </span>
            </span>
            <span className="mt-0.5 block text-xs font-normal text-text-secondary">
              {eingerichtet && kopfWerte
                ? `${kopfWerte.height}p${kopfWerte.fps} · ${kopfWerte.bitrate_kbps} kbps`
                : 'Server, Schlüssel und Qualität hinterlegen'}
              {ungespeichert ? <span className="ml-1.5 text-primary">nicht gespeichert</span> : null}
            </span>
            {chat ? <PlattformVerbindung chat={chat} csrfToken={csrfToken ?? null} /> : null}
          </span>
        </span>
        <span className={`flex min-h-9 items-center gap-2 rounded-lg border px-2.5 py-1 text-xs font-semibold transition-colors ${eingerichtet ? 'border-primary/25 bg-primary/10 text-primary' : 'border-primary/35 bg-primary/15 text-primary'}`}>
          {eingerichtet ? 'Einstellungen' : 'Einrichten'}
          <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open:rotate-180" />
        </span>
      </summary>

      <div
        role="group"
        aria-label={`${label}-Einstellungen`}
        className="space-y-4 border-t border-border/60 px-4 py-4"
      >
        {/* Kommen Adresse und Schluessel von der Plattform-Verbindung, sind die
            beiden Felder nur noch eine Fehlerquelle: was dort steht, wird beim
            naechsten Nachlauf ohnehin ueberschrieben, und ein von Hand
            eingetragener alter Schluessel sieht aus wie eine Aenderung, die
            nichts bewirkt. Ohne Verbindung bleibt der Handweg unveraendert. */}
        {automatisch ? (
          <div data-zugang="automatisch" className="flex flex-wrap items-center gap-2">
            <StreamKeyKnopf
              platform={platform}
              csrfToken={csrfToken ?? null}
              beschriftung="Schlüssel erneut holen"
              klasse="inline-flex min-h-11 items-center rounded-xl border border-border px-3 py-2 text-xs font-semibold text-white transition-colors hover:border-primary/50 disabled:opacity-60"
            />
            <span className="text-[11px] text-text-secondary">
              Nötig nur, wenn du deinen Schlüssel bei {label} neu erzeugt hast.
            </span>
          </div>
        ) : (
          <>
            <div className="space-y-1">
              <label htmlFor={rtmpId} className="block text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
                Serveradresse von {label}
              </label>
              <input
                id={rtmpId}
                value={rtmpUrl}
                onChange={(e) => {
                  setRtmpUrl(e.target.value);
                  angefasst();
                }}
                placeholder={rtmpVorgabe || 'rtmp://…'}
                className="min-h-11 w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
              />
              {eingerichtet && (
                <p className="text-[11px] text-text-secondary">
                  Adresse ändern geht nur zusammen mit dem Stream-Schlüssel: wir prüfen beide gemeinsam.
                </p>
              )}
            </div>
            <div className="space-y-1">
              <label htmlFor={keyId} className="block text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
                Stream-Schlüssel von {label}
              </label>
              <input
                id={keyId}
                value={streamKey}
                onChange={(e) => {
                  setStreamKey(e.target.value);
                  angefasst();
                }}
                type="password"
                placeholder={eingerichtet ? 'liegt bei uns, leer lassen' : `Stream-Key von ${label}`}
                className="min-h-11 w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
              />
              <p className="text-[11px] text-text-secondary">
                {eingerichtet
                  ? 'Leer lassen, wenn du nur die Qualität ändern willst. Dein Schlüssel bleibt liegen.'
                  : 'Wird verschlüsselt gespeichert und nie wieder ausgegeben.'}
              </p>
            </div>
          </>
        )}

        <div className="space-y-2 rounded-xl border border-border/60 bg-background/40 p-3">
          <div className="flex items-center justify-between gap-3">
            <span className="text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
              Qualität, die wir an {label} senden
            </span>
            <div role="group" aria-label={`Qualitätsmodus für ${label}`} className="flex shrink-0 overflow-hidden rounded-lg border border-border text-xs font-semibold">
              <button
                type="button"
                aria-pressed={modus === 'stufe'}
                onClick={() => {
                  setModus('stufe');
                  angefasst();
                }}
                className={`min-h-11 px-3 py-1 ${modus === 'stufe' ? 'bg-primary text-[#0D0806]' : 'text-text-secondary hover:text-white'}`}
              >
                Stufe
              </button>
              <button
                type="button"
                aria-pressed={modus === 'manuell'}
                onClick={() => {
                  nachManuell();
                  angefasst();
                }}
                className={`min-h-11 px-3 py-1 ${modus === 'manuell' ? 'bg-primary text-[#0D0806]' : 'text-text-secondary hover:text-white'}`}
              >
                Manuell
              </button>
            </div>
          </div>

          {modus === 'stufe' ? (
            <div className="space-y-1">
              <label htmlFor={profilId} className="sr-only">Qualitätsstufe für {label}</label>
              <select
                id={profilId}
                value={profil}
                onChange={(e) => {
                  setProfil(e.target.value as UplinkProfilName);
                  angefasst();
                }}
                className="min-h-11 w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
              >
                {UPLINK_PROFILE.map((eintrag) => (
                  <option key={eintrag.name} value={eintrag.name}>
                    {eintrag.label}
                  </option>
                ))}
              </select>
              <p className="text-xs text-text-secondary">{gewaehlteStufe?.hinweis}</p>
              {gewaehlteStufe?.warnung && platform === 'twitch' ? (
                <p className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
                  {gewaehlteStufe.warnung}
                </p>
              ) : null}
            </div>
          ) : (
            <div className="space-y-2">
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                <ZahlFeld
                  label="Breite"
                  einheit="px"
                  wert={manuell.width}
                  empfehlung={plattformEmpfehlung.width}
                  onChange={(w) => {
                    setManuell((v) => ({ ...v, width: w }));
                    angefasst();
                  }}
                />
                <ZahlFeld
                  label="Höhe"
                  einheit="px"
                  wert={manuell.height}
                  empfehlung={plattformEmpfehlung.height}
                  onChange={(h) => {
                    setManuell((v) => ({ ...v, height: h }));
                    angefasst();
                  }}
                />
                <ZahlFeld
                  label="Bildrate"
                  einheit="fps"
                  wert={manuell.fps}
                  empfehlung={plattformEmpfehlung.fps}
                  onChange={(f) => {
                    setManuell((v) => ({ ...v, fps: f }));
                    angefasst();
                  }}
                />
                <ZahlFeld
                  label="Bitrate"
                  einheit="kbps"
                  wert={manuell.bitrate_kbps}
                  empfehlung={plattformEmpfehlung.bitrate_kbps}
                  onChange={(b) => {
                    setManuell((v) => ({ ...v, bitrate_kbps: b }));
                    angefasst();
                  }}
                />
              </div>
              {!caps && (
                <p className="text-xs text-text-secondary">
                  Die Empfehlungen von {label} konnten wir gerade nicht abrufen, deshalb stehen
                  keine an den Feldern. Auf das, was wir senden, hat das keinen Einfluss: deine
                  Werte gehen so raus, wie sie hier stehen.
                </p>
              )}
              <p className="text-xs text-text-secondary">
                Freie Werte, wir senden genau das. Die Zahlen von {label} sind eine Empfehlung und
                keine Grenze. Ob {label} mehr annimmt oder drosselt, siehst du im Stream.
                {caps?.force_cbr
                  ? ` ${label} verlangt eine feste Bitrate, wir halten sie konstant.`
                  : ''}
              </p>
            </div>
          )}
        </div>

        {fehlertext && <p id={fehlerId} role="alert" className="text-xs text-warning">{fehlertext}</p>}
        {livetext && <p role="status" className="text-xs text-text-secondary">{livetext}</p>}

        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            disabled={speichern.isPending}
            onClick={() => speichern.mutate(undefined)}
            className="inline-flex min-h-11 items-center gap-2 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
          >
            {speichern.isPending && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {speichern.isPending
              ? `${label} wird gespeichert`
              : gespeichert
                ? 'Gespeichert'
                : `${label} speichern`}
          </button>
          {eingerichtet && (
            <button
              type="button"
              disabled={speichern.isPending}
              onClick={() => speichern.mutate(!ziel?.enabled)}
              className="inline-flex min-h-11 items-center gap-2 rounded-xl border border-border px-4 py-2 text-sm font-semibold text-text-secondary transition-colors hover:text-white disabled:opacity-60"
            >
              <Power className="h-3.5 w-3.5" />
              {ziel?.enabled ? 'Ziel pausieren' : 'Ziel einschalten'}
            </button>
          )}
        </div>

        {eingerichtet && (
          <div role="status" className="flex items-start gap-2 rounded-xl border border-success/30 bg-success/10 px-3 py-2 text-xs text-success">
            <Check className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {/* Bewusst `requested` und nicht `effective`: der Satz beschreibt
                den gespeicherten Stand, und beide Felder sind seit dem Ende der
                Klemmung ohnehin gleich. `effective` bleibt nur fuer aeltere
                Clients im JSON stehen. */}
            <span>
              {automatisch
                ? `Adresse und Stream-Schlüssel kommen von deiner ${label}-Verbindung.`
                : 'Schlüssel liegt verschlüsselt bei uns.'}
              {bestellt ? (
                <>
                  {' '}Wir senden {bestellt.width}x{bestellt.height} mit {bestellt.fps} Bildern und{' '}
                  {bestellt.bitrate_kbps} kbps.
                </>
              ) : null}
              {ungespeichert ? (
                <> Im Formular stehen andere Werte, die noch nicht gespeichert sind.</>
              ) : null}
            </span>
          </div>
        )}
      </div>
    </details>
  );
}
