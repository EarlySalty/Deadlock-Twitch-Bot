import { useEffect, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronDown, Loader2, Power } from 'lucide-react';
import {
  PROFIL_WERTE,
  UPLINK_PROFILE,
  profilNameFuer,
  saveUplinkDestination,
} from '@/api/uplink';
import type {
  UplinkCaps,
  UplinkDestination,
  UplinkManuellesProfil,
  UplinkPlattform,
  UplinkProfilAnsicht,
  UplinkProfilName,
} from '@/api/uplink';

type Modus = 'stufe' | 'manuell';

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
  offenStart,
}: {
  platform: UplinkPlattform;
  label: string;
  rtmpVorgabe: string;
  ziel: UplinkDestination | undefined;
  caps: UplinkCaps | undefined;
  offenStart: boolean;
}) {
  const queryClient = useQueryClient();
  const eingerichtet = Boolean(ziel);

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
  const angefasst = () => {
    setGespeichert(false);
    setFehlertext('');
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
      const url = rtmpUrl.trim();
      const key = streamKey.trim();
      if (key && !url) throw new Error('Ohne Serveradresse können wir den Schlüssel nicht zuordnen.');
      if (!key && !eingerichtet) {
        throw new Error(`Für ${label} fehlt uns noch der Stream-Schlüssel.`);
      }
      // Adresse und Schluessel gehoeren zusammen: das Relay nimmt sie nur
      // gemeinsam an, weil beide gleich wieder als Argument an ffmpeg gehen
      // und gemeinsam geprueft werden. Eine geaenderte Adresse ohne Schluessel
      // wuerde also stillschweigend unter den Tisch fallen, waehrend der
      // Knopf "Gespeichert" meldet und das Feld die neue Adresse behaelt.
      // Deshalb hier ein Halt mit Grund statt eines verworfenen Feldes.
      if (!key && eingerichtet && url !== (ziel?.rtmp_url ?? '')) {
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
    onSuccess: () => {
      setStreamKey('');
      setFehlertext('');
      setGespeichert(true);
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

  return (
    <details
      open={offenStart}
      className="group overflow-hidden rounded-2xl border border-border bg-background/40"
    >
      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-4 py-3 text-sm font-semibold text-white transition-colors hover:bg-white/5 [&::-webkit-details-marker]:hidden">
        <span className="flex items-center gap-2">
          {label}
          {eingerichtet ? (
            <span
              className={`rounded-full px-2 py-0.5 text-[11px] font-semibold ${
                ziel?.enabled
                  ? 'bg-success/15 text-success'
                  : 'bg-white/10 text-text-secondary'
              }`}
            >
              {ziel?.enabled ? 'aktiv' : 'aus'}
            </span>
          ) : (
            <span className="rounded-full bg-white/5 px-2 py-0.5 text-[11px] font-semibold text-text-secondary">
              nicht eingerichtet
            </span>
          )}
        </span>
        <span className="flex items-center gap-3">
          {eingerichtet && kopfWerte && (
            <span className="text-xs font-normal text-text-secondary">
              {kopfWerte.height}p{kopfWerte.fps} · {kopfWerte.bitrate_kbps} kbps
              {ungespeichert ? (
                <span className="ml-1.5 text-primary">nicht gespeichert</span>
              ) : null}
            </span>
          )}
          <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open:rotate-180" />
        </span>
      </summary>

      <div className="space-y-3 border-t border-border/60 px-4 py-4">
        <div className="space-y-1">
          <label className="block text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
            Serveradresse von {label}
          </label>
          <input
            value={rtmpUrl}
            onChange={(e) => {
              setRtmpUrl(e.target.value);
              angefasst();
            }}
            placeholder={rtmpVorgabe || 'rtmp://…'}
            className="w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
          />
          {eingerichtet && (
            <p className="text-[11px] text-text-secondary">
              Adresse ändern geht nur zusammen mit dem Stream-Schlüssel: wir prüfen beide gemeinsam.
            </p>
          )}
        </div>
        <div className="space-y-1">
          <label className="block text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
            Stream-Schlüssel von {label}
          </label>
          <input
            value={streamKey}
            onChange={(e) => {
              setStreamKey(e.target.value);
              angefasst();
            }}
            type="password"
            placeholder={eingerichtet ? 'liegt bei uns, leer lassen' : `Stream-Key von ${label}`}
            className="w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
          />
          <p className="text-[11px] text-text-secondary">
            {eingerichtet
              ? 'Leer lassen, wenn du nur die Qualität ändern willst. Dein Schlüssel bleibt liegen.'
              : 'Wird verschlüsselt gespeichert und nie wieder ausgegeben.'}
          </p>
        </div>

        <div className="space-y-2 rounded-xl border border-border/60 bg-background/40 p-3">
          <div className="flex items-center justify-between gap-3">
            <span className="text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
              Qualität, die wir an {label} senden
            </span>
            <div className="flex shrink-0 overflow-hidden rounded-lg border border-border text-xs font-semibold">
              <button
                type="button"
                onClick={() => {
                  setModus('stufe');
                  angefasst();
                }}
                className={`px-3 py-1 ${modus === 'stufe' ? 'bg-primary text-[#0D0806]' : 'text-text-secondary hover:text-white'}`}
              >
                Stufe
              </button>
              <button
                type="button"
                onClick={() => {
                  nachManuell();
                  angefasst();
                }}
                className={`px-3 py-1 ${modus === 'manuell' ? 'bg-primary text-[#0D0806]' : 'text-text-secondary hover:text-white'}`}
              >
                Manuell
              </button>
            </div>
          </div>

          {modus === 'stufe' ? (
            <div className="space-y-1">
              <select
                value={profil}
                onChange={(e) => {
                  setProfil(e.target.value as UplinkProfilName);
                  angefasst();
                }}
                className="w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
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

        {fehlertext && <p className="text-xs text-warning">{fehlertext}</p>}

        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            disabled={speichern.isPending}
            onClick={() => speichern.mutate(undefined)}
            className="inline-flex items-center gap-2 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
          >
            {speichern.isPending && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {gespeichert && !speichern.isPending ? 'Gespeichert' : `${label} speichern`}
          </button>
          {eingerichtet && (
            <button
              type="button"
              disabled={speichern.isPending}
              onClick={() => speichern.mutate(!ziel?.enabled)}
              className="inline-flex items-center gap-2 rounded-xl border border-border px-4 py-2 text-sm font-semibold text-text-secondary transition-colors hover:text-white disabled:opacity-60"
            >
              <Power className="h-3.5 w-3.5" />
              {ziel?.enabled ? 'Ziel pausieren' : 'Ziel einschalten'}
            </button>
          )}
        </div>

        {eingerichtet && (
          <div className="flex items-start gap-2 rounded-xl border border-success/30 bg-success/10 px-3 py-2 text-xs text-success">
            <Check className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {/* Bewusst `requested` und nicht `effective`: der Satz beschreibt
                den gespeicherten Stand, und beide Felder sind seit dem Ende der
                Klemmung ohnehin gleich. `effective` bleibt nur fuer aeltere
                Clients im JSON stehen. */}
            <span>
              Schlüssel liegt verschlüsselt bei uns.
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
