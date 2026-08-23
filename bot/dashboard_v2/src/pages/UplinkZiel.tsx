import { useEffect, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronDown, Loader2, Power, TriangleAlert } from 'lucide-react';
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
  UplinkProfilName,
} from '@/api/uplink';

/**
 * Fallback-Grenzen, falls der Caps-Aufruf nicht durchkommt.
 *
 * Dieselben Zahlen wie der Ingest-Deckel in rs-relay. Sie sind absichtlich
 * grosszuegig: das Klemmen macht das Relay, hier geht es nur darum, einen
 * Tippfehler mit einer Null zu viel abzufangen, bevor er abgeschickt wird.
 */
const INGEST_FALLBACK: UplinkCaps = {
  platform: 'ingest',
  max_width: 2560,
  max_height: 1440,
  max_fps: 60,
  max_bitrate_kbps: 30000,
  force_cbr: false,
};

/** Die kleinere der beiden Grenzen. `null` heisst "klemmt hier nicht". */
function engereGrenze(a: number | null, b: number | null): number | null {
  if (a === null) return b;
  if (b === null) return a;
  return Math.min(a, b);
}

type Modus = 'stufe' | 'manuell';

/** Zahlenfeld im manuellen Modus. Leerer Text ist erlaubt, sonst kann man die
 *  fuehrende Ziffer nicht loeschen, ohne dass eine 0 nachrutscht. */
function ZahlFeld({
  label,
  einheit,
  wert,
  max,
  onChange,
}: {
  label: string;
  einheit: string;
  wert: string;
  max: number | null;
  onChange: (wert: string) => void;
}) {
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
      {max !== null && (
        <span className="block text-[11px] text-text-secondary">max. {max}</span>
      )}
    </label>
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
  ingest,
  offenStart,
}: {
  platform: UplinkPlattform;
  label: string;
  rtmpVorgabe: string;
  ziel: UplinkDestination | undefined;
  caps: UplinkCaps | undefined;
  ingest: UplinkCaps | undefined;
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

  const grenze = {
    width: engereGrenze(caps?.max_width ?? null, ingest?.max_width ?? INGEST_FALLBACK.max_width),
    height: engereGrenze(caps?.max_height ?? null, ingest?.max_height ?? INGEST_FALLBACK.max_height),
    fps: engereGrenze(caps?.max_fps ?? null, ingest?.max_fps ?? INGEST_FALLBACK.max_fps),
    bitrate_kbps: engereGrenze(
      caps?.max_bitrate_kbps ?? null,
      ingest?.max_bitrate_kbps ?? INGEST_FALLBACK.max_bitrate_kbps,
    ),
  };

  /** Prueft die manuellen Zahlen, bevor sie abgeschickt werden. Der Server
   *  prueft noch einmal; hier geht es um die Antwort ohne Rundreise. */
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
      const max = grenze[feld];
      if (max !== null && wert > max) return `${name} darf höchstens ${max} sein.`;
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
      const body: Parameters<typeof saveUplinkDestination>[0] = { platform };
      if (key) {
        body.rtmp_url = url;
        body.stream_key = key;
      }
      if (enabled !== undefined) {
        body.enabled = enabled;
      } else if (modus === 'manuell') {
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
      queryClient.invalidateQueries({ queryKey: ['uplink-destinations'] });
      queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
    },
    onError: (e) =>
      setFehlertext(
        e instanceof Error && e.message ? e.message : 'Speichern hat nicht geklappt.',
      ),
  });

  const gewaehlteStufe = UPLINK_PROFILE.find((e) => e.name === profil);
  // Nur Hoehe und Bildrate vergleichen: die Bitrate klemmt das Relay auch im
  // Normalfall, daraus eine Warnung zu bauen hiesse, jedem eine zu zeigen.
  const geklemmt =
    ziel?.requested &&
    ziel.effective &&
    (ziel.requested.height !== ziel.effective.height || ziel.requested.fps !== ziel.effective.fps);

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
          {ziel?.effective && (
            <span className="text-xs font-normal text-text-secondary">
              {ziel.effective.height}p{ziel.effective.fps} · {ziel.effective.bitrate_kbps} kbps
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
            onChange={(e) => setRtmpUrl(e.target.value)}
            placeholder={rtmpVorgabe || 'rtmp://…'}
            className="w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
          />
        </div>
        <div className="space-y-1">
          <label className="block text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
            Stream-Schlüssel von {label}
          </label>
          <input
            value={streamKey}
            onChange={(e) => setStreamKey(e.target.value)}
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
                onClick={() => setModus('stufe')}
                className={`px-3 py-1 ${modus === 'stufe' ? 'bg-primary text-[#0D0806]' : 'text-text-secondary hover:text-white'}`}
              >
                Stufe
              </button>
              <button
                type="button"
                onClick={nachManuell}
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
                onChange={(e) => setProfil(e.target.value as UplinkProfilName)}
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
                  max={grenze.width}
                  onChange={(w) => setManuell((v) => ({ ...v, width: w }))}
                />
                <ZahlFeld
                  label="Höhe"
                  einheit="px"
                  wert={manuell.height}
                  max={grenze.height}
                  onChange={(h) => setManuell((v) => ({ ...v, height: h }))}
                />
                <ZahlFeld
                  label="Bildrate"
                  einheit="fps"
                  wert={manuell.fps}
                  max={grenze.fps}
                  onChange={(f) => setManuell((v) => ({ ...v, fps: f }))}
                />
                <ZahlFeld
                  label="Bitrate"
                  einheit="kbps"
                  wert={manuell.bitrate_kbps}
                  max={grenze.bitrate_kbps}
                  onChange={(b) => setManuell((v) => ({ ...v, bitrate_kbps: b }))}
                />
              </div>
              <p className="text-xs text-text-secondary">
                Freie Werte. Was über den Grenzen von {label} liegt, rechnen wir beim Senden
                herunter, statt den Stream zu verweigern.
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
            {speichern.isSuccess && !speichern.isPending ? 'Gespeichert' : `${label} speichern`}
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
            <span>
              Schlüssel liegt verschlüsselt bei uns.
              {ziel?.effective ? (
                <>
                  {' '}Wir senden {ziel.effective.width}x{ziel.effective.height} mit{' '}
                  {ziel.effective.fps} Bildern und {ziel.effective.bitrate_kbps} kbps.
                </>
              ) : null}
            </span>
          </div>
        )}

        {/* Das Relay klemmt gegen `relay.platform_caps`, eine Tabelle in einem
            anderen Repo. Sie kann sich per Migration bewegen, ohne dass hier
            jemand etwas tut. Wer eine Stufe waehlt und eine andere bekommt,
            soll das hier lesen und nicht erst im Stream sehen. */}
        {geklemmt && (
          <div className="flex items-start gap-2 rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
            <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {/* Den Grund nicht raten: geklemmt wird gegen die Grenze der
                Plattform UND gegen die aller anderen eingeschalteten Ziele,
                weil alle denselben Encode bekommen. "{label} nimmt nicht
                mehr an" waere in der zweiten Haelfte der Faelle falsch. */}
            <span>
              Du hast {ziel?.requested?.height}p{ziel?.requested?.fps} bestellt, raus gehen aber{' '}
              {ziel?.effective?.height}p{ziel?.effective?.fps}. Entweder nimmt {label} gerade nicht
              mehr an, oder ein anderes eingeschaltetes Ziel kann weniger: alle Ziele bekommen
              denselben Encode.
            </span>
          </div>
        )}
      </div>
    </details>
  );
}
