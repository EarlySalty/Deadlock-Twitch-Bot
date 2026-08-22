import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BarChart3,
  Copy,
  Film,
  Home,
  Loader2,
  Lock,
  Radio,
  Settings,
  MonitorPlay,
  Sparkles,
  FileText,
} from 'lucide-react';
import { Rise } from '../motion/Rise';
import '../uplinkHelp.css';
import { fetchUplinkHelp, uplinkHelpUrl, UPLINK_HELP_PAGES } from '@/uplinkHelp';
import { useAuthStatus } from '@/hooks/useAnalytics';
import {
  connectUplinkTwitch,
  fetchUplinkAdminOverview,
  fetchUplinkAdminWaitlist,
  fetchUplinkDestinations,
  fetchUplinkMe,
  fetchUplinkMetrics,
  fetchUplinkSchedule,
  joinUplinkWaitlist,
  killUplinkSession,
  saveUplinkAdminSettings,
  saveUplinkDestination,
  saveUplinkSchedule,
  type UplinkDestination,
  type UplinkScheduleEntry,
} from '@/api/uplink';
import {
  UPLINK_KILL_LAEUFT_NOCH,
  UPLINK_LAST_LABEL,
  UPLINK_PLATFORMS,
  UPLINK_REAUTH_HREF,
  UPLINK_SCHEDULE_TEXT,
  UPLINK_TWITCH_LOGIN_HINT,
  UPLINK_TWITCH_SCOPE_HINT,
  UPLINK_WAITLIST_FEHLER,
  UPLINK_WAITLIST_TEXT,
  UPLINK_ME_REFETCH_INTERVAL_MS,
  aktiveSessionId,
  canSaveDestination,
  clampedFields,
  egressJeZiel,
  formatDauer,
  formularAusEinstellungen,
  killErfolgreich,
  lastProzent,
  speedLage,
  scheduleSavePlan,
  toEingabeZeit,
  twitchFehlertext,
  uplinkAdminBloeckeSichtbar,
  uplinkAnsicht,
  uplinkMetricsQueryKey,
  uplinkStreamerBloeckeSichtbar,
  wartelistenAnzeige,
  zielVerbindungsLabel,
  zielRumpf,
  zahlOderUndefined,
} from './uplinkModel';
import {
  PREVIEW_CHANGELOG_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_UPLINK_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
  analyticsTabHref,
} from '@/preview/routes';

const FELD_KLASSE =
  'w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white';
const KNOPF_KLASSE =
  'rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60';
const LABEL_KLASSE =
  'text-[11px] font-semibold uppercase tracking-[0.16em] text-text-secondary';

function fehlertext(error: unknown, ersatz: string): string {
  return error instanceof Error && error.message ? error.message : ersatz;
}

function SidebarLink({
  href,
  label,
  icon: Icon,
  active,
}: {
  href: string;
  label: string;
  icon: typeof Home;
  active?: boolean;
}) {
  const activeClasses =
    'border border-primary/25 bg-primary/10 text-primary lg:rounded-l-none lg:border-y-0 lg:border-r-0 lg:border-t-0 lg:border-l-2 lg:border-primary lg:pl-2.5';
  const inactiveClasses =
    'border border-transparent text-text-secondary hover:bg-white/5 hover:text-white';
  return (
    <a
      href={href}
      className={`flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-semibold no-underline transition-colors whitespace-nowrap ${
        active ? activeClasses : inactiveClasses
      }`}
    >
      <Icon className="h-4 w-4 shrink-0" />
      <span>{label}</span>
    </a>
  );
}

function CopyField({ label, value }: { label: string; value: string }) {
  const [ok, setOk] = useState(false);
  if (!value) return null;
  return (
    <div className="space-y-1">
      <div className={LABEL_KLASSE}>{label}</div>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded-xl border border-border bg-background/70 px-3 py-2 text-xs text-white">
          {value}
        </code>
        <button
          type="button"
          className="inline-flex items-center gap-1 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary hover:text-white"
          onClick={async () => {
            await navigator.clipboard.writeText(value);
            setOk(true);
            window.setTimeout(() => setOk(false), 1500);
          }}
        >
          <Copy className="h-3.5 w-3.5" />
          {ok ? 'Kopiert' : 'Kopieren'}
        </button>
      </div>
    </div>
  );
}

interface ZielEingabe {
  rtmpUrl: string;
  streamKey: string;
  width: string;
  height: string;
  fps: string;
  bitrate: string;
}

function leereEingabe(defaultRtmpUrl: string): ZielEingabe {
  return {
    rtmpUrl: defaultRtmpUrl,
    streamKey: '',
    width: '',
    height: '',
    fps: '',
    bitrate: '',
  };
}

function PlattformKarte({
  platform,
  gespeichert,
  status,
  onSaved,
}: {
  platform: (typeof UPLINK_PLATFORMS)[number];
  gespeichert?: UplinkDestination;
  status: 'loading' | 'error' | 'ready';
  onSaved: () => void;
}) {
  const [eingabe, setEingabe] = useState<ZielEingabe>(() =>
    leereEingabe(platform.defaultRtmpUrl)
  );
  const [twitchHinweis, setTwitchHinweis] = useState<string | null>(null);
  // Adresse wurde vom Nutzer angefasst, nicht nur automatisch vorbefuellt. Ohne
  // diese Unterscheidung erlaubt canSaveDestination ein Speichern, das zielRumpf
  // dann still verwirft, weil Adresse ohne Schluessel dort ausgelassen wird.
  const [urlBeruehrt, setUrlBeruehrt] = useState(false);

  useEffect(() => {
    if (gespeichert?.rtmp_url) {
      setEingabe((alt) => (alt.rtmpUrl ? alt : { ...alt, rtmpUrl: gespeichert.rtmp_url }));
    }
  }, [gespeichert?.rtmp_url]);

  const speichern = useMutation({
    mutationFn: () => saveUplinkDestination(zielRumpf({ platform: platform.id, ...eingabe })),
    onSuccess: () => {
      setEingabe((alt) => ({ ...alt, streamKey: '' }));
      setUrlBeruehrt(false);
      onSaved();
    },
  });

  const twitchHolen = useMutation({
    mutationFn: connectUplinkTwitch,
    onSuccess: () => {
      setTwitchHinweis(null);
      onSaved();
    },
    onError: (e) => setTwitchHinweis(twitchFehlertext(e)),
  });

  const profilBeruehrt = Boolean(
    eingabe.width.trim() ||
      eingabe.height.trim() ||
      eingabe.fps.trim() ||
      eingabe.bitrate.trim()
  );
  const speicherbar = canSaveDestination({
    rtmpUrl: eingabe.rtmpUrl,
    streamKey: eingabe.streamKey,
    urlTouched: urlBeruehrt,
    profileTouched: profilBeruehrt,
    verbunden: Boolean(gespeichert),
  });
  const geklemmt = clampedFields(gespeichert?.requested, gespeichert?.effective);
  const verbindungsText = zielVerbindungsLabel(status, gespeichert?.enabled);
  const verbindungsKlasse =
    status === 'error'
      ? 'border-warning/30 bg-warning/10 text-warning'
      : status === 'loading'
        ? 'border-border text-text-secondary'
        : gespeichert?.enabled
          ? 'border-success/30 bg-success/10 text-success'
          : 'border-border text-text-secondary';

  return (
    <Rise className="panel-card space-y-3 rounded-2xl p-5">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-base font-bold text-white">{platform.label}</h3>
        <span
          className={`rounded-full border px-2.5 py-0.5 text-[11px] font-semibold ${verbindungsKlasse}`}
        >
          {verbindungsText}
        </span>
      </div>
      <p className="text-xs text-text-secondary">{platform.hint}</p>

      {platform.id === 'twitch' && (
        <div className="space-y-2">
          <button
            type="button"
            disabled={twitchHolen.isPending}
            onClick={() => twitchHolen.mutate()}
            className={KNOPF_KLASSE}
          >
            {twitchHolen.isPending ? 'Wird geholt' : 'Twitch automatisch verbinden'}
          </button>
          {twitchHinweis && (
            <p className="text-xs text-warning">
              {twitchHinweis}
              {twitchHinweis === UPLINK_TWITCH_SCOPE_HINT && (
                <>
                  {' '}
                  <a href={UPLINK_REAUTH_HREF} className="font-semibold underline">
                    Twitch neu verbinden
                  </a>
                </>
              )}
            </p>
          )}
        </div>
      )}

      <div className="space-y-2">
        <input
          value={eingabe.rtmpUrl}
          onChange={(e) => {
            setUrlBeruehrt(true);
            setEingabe({ ...eingabe, rtmpUrl: e.target.value });
          }}
          className={FELD_KLASSE}
          placeholder="Server-Adresse"
          aria-label={`Server-Adresse ${platform.label}`}
        />
        <input
          value={eingabe.streamKey}
          onChange={(e) => setEingabe({ ...eingabe, streamKey: e.target.value })}
          type="password"
          className={FELD_KLASSE}
          placeholder="Stream-Schlüssel"
          aria-label={`Stream-Schlüssel ${platform.label}`}
        />
      </div>

      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        {(
          [
            ['width', 'Breite'],
            ['height', 'Höhe'],
            ['fps', 'Bilder/s'],
            ['bitrate', 'kbit/s'],
          ] as const
        ).map(([feld, label]) => (
          <label key={feld} className="space-y-1">
            <span className={LABEL_KLASSE}>{label}</span>
            <input
              value={eingabe[feld]}
              onChange={(e) => setEingabe({ ...eingabe, [feld]: e.target.value })}
              inputMode="numeric"
              className={FELD_KLASSE}
              placeholder={
                gespeichert?.effective
                  ? String(
                      (feld === 'bitrate'
                        ? gespeichert.effective.bitrate_kbps
                        : gespeichert.effective[feld as 'width' | 'height' | 'fps']) ?? ''
                    )
                  : ''
              }
            />
          </label>
        ))}
      </div>

      {gespeichert?.effective && (
        <p className="text-xs text-text-secondary">
          Wird gesendet mit {gespeichert.effective.width} mal {gespeichert.effective.height},{' '}
          {gespeichert.effective.fps} Bildern pro Sekunde und{' '}
          {gespeichert.effective.bitrate_kbps} kbit/s.
        </p>
      )}
      {geklemmt.length > 0 && (
        <p className="text-xs text-warning">
          {platform.label} nimmt nicht mehr als{' '}
          {geklemmt
            .map((f) => `${f.label} ${f.effective}`)
            .join(', ')}
          . Wir senden deshalb den kleineren Wert.
        </p>
      )}

      <button
        type="button"
        disabled={speichern.isPending || !speicherbar}
        onClick={() => speichern.mutate()}
        className={KNOPF_KLASSE}
      >
        {speichern.isSuccess ? 'Gespeichert' : 'Speichern'}
      </button>
      {speichern.isError && (
        <p className="text-xs text-warning">
          {fehlertext(speichern.error, 'Das ließ sich gerade nicht speichern.')}
        </p>
      )}
    </Rise>
  );
}

function ZeitplanKarte() {
  const queryClient = useQueryClient();
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ['uplink-schedule'],
    queryFn: fetchUplinkSchedule,
    retry: false,
  });
  const [entwurf, setEntwurf] = useState<Array<{ von: string; bis: string }> | null>(null);

  const zeilen =
    entwurf ??
    (data?.entries ?? []).map((e) => ({
      von: toEingabeZeit(e.starts_at),
      bis: toEingabeZeit(e.ends_at),
    }));
  const speicherplan = scheduleSavePlan(zeilen, {
    loaded: data !== undefined,
    failed: isError,
  });

  const speichern = useMutation({
    mutationFn: () => {
      if (!speicherplan.entries) {
        throw new Error(speicherplan.error ?? 'Der Zeitplan kann nicht gespeichert werden.');
      }
      const eintraege: UplinkScheduleEntry[] = speicherplan.entries;
      return saveUplinkSchedule(eintraege);
    },
    onSuccess: () => {
      setEntwurf(null);
      queryClient.invalidateQueries({ queryKey: ['uplink-schedule'] });
    },
  });

  const setzeZeile = (index: number, feld: 'von' | 'bis', wert: string) => {
    const kopie = zeilen.map((z) => ({ ...z }));
    kopie[index][feld] = wert;
    setEntwurf(kopie);
  };

  return (
    <Rise className="panel-card space-y-3 rounded-2xl p-6">
      <h2 className="text-lg font-bold text-white">Wann du senden willst</h2>
      <p className="text-sm text-text-secondary">{UPLINK_SCHEDULE_TEXT}</p>
      <div className="space-y-2">
        {isLoading && <p className="text-xs text-text-secondary">Zeitplan wird geladen.</p>}
        {isError && (
          <p className="text-xs text-warning">
            {fehlertext(error, 'Der Zeitplan konnte nicht geladen werden.')}
          </p>
        )}
        {!isLoading && !isError && speicherplan.error && (
          <p className="text-xs text-warning">{speicherplan.error}</p>
        )}
        {zeilen.map((zeile, index) => (
          <div key={index} className="flex flex-wrap items-center gap-2">
            <input
              type="datetime-local"
              value={zeile.von}
              onChange={(e) => setzeZeile(index, 'von', e.target.value)}
              className={`${FELD_KLASSE} max-w-[15rem]`}
              aria-label="Beginn"
            />
            <input
              type="datetime-local"
              value={zeile.bis}
              onChange={(e) => setzeZeile(index, 'bis', e.target.value)}
              className={`${FELD_KLASSE} max-w-[15rem]`}
              aria-label="Ende"
            />
            <button
              type="button"
              onClick={() => setEntwurf(zeilen.filter((_, i) => i !== index))}
              className="rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary hover:text-white"
            >
              Entfernen
            </button>
          </div>
        ))}
        {!isLoading && !isError && zeilen.length === 0 && (
          <p className="text-xs text-text-secondary">Noch nichts geplant.</p>
        )}
      </div>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          disabled={isLoading || isError}
          onClick={() => setEntwurf([...zeilen, { von: '', bis: '' }])}
          className="rounded-xl border border-border px-4 py-2 text-sm font-semibold text-white"
        >
          Zeit hinzufügen
        </button>
        <button
          type="button"
          disabled={speichern.isPending || speicherplan.entries === null}
          onClick={() => speichern.mutate()}
          className={KNOPF_KLASSE}
        >
          {speichern.isSuccess && !entwurf ? 'Gespeichert' : 'Zeitplan speichern'}
        </button>
      </div>
      {speichern.isError && (
        <p className="text-xs text-warning">
          {fehlertext(speichern.error, 'Der Zeitplan ließ sich gerade nicht speichern.')}
        </p>
      )}
    </Rise>
  );
}

function StatusKarte({ sessionId }: { sessionId: number | null | undefined }) {
  const { data, isError } = useQuery({
    queryKey: uplinkMetricsQueryKey(sessionId),
    queryFn: () => fetchUplinkMetrics(sessionId as number),
    enabled: typeof sessionId === 'number' && sessionId > 0,
    refetchInterval: 15000,
    retry: false,
  });

  const letzte = data?.samples?.[data.samples.length - 1];
  const proZiel = Object.entries(data?.gb_by_target ?? {});
  const lage = speedLage(letzte?.encoder_speed);
  const auslastung = lastProzent(letzte?.cpu_pct);
  const ausgang = egressJeZiel(letzte?.egress_kbps_by_target);

  return (
    <Rise className="panel-card space-y-3 rounded-2xl p-6">
      <h2 className="text-lg font-bold text-white">Dein laufender Stream</h2>
      {!data && !isError && (
        <p className="text-sm text-text-secondary">
          Sobald du sendest, stehen hier Laufzeit, Datenmenge je Plattform und die
          Qualität deiner Zuleitung.
        </p>
      )}
      {isError && (
        <p className="text-sm text-text-secondary">
          Zu diesem Stream gibt es gerade keine Zahlen.
        </p>
      )}
      {data && (
        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <div className={LABEL_KLASSE}>Läuft seit</div>
            <div className="text-sm text-white">
              {formatDauer(data.started_at, data.ended_at) || 'gerade gestartet'}
            </div>
          </div>
          <div>
            <div className={LABEL_KLASSE}>Zuleitung</div>
            <div className="text-sm text-white">
              {letzte?.ingest_kbps ? `${letzte.ingest_kbps} kbit/s` : 'noch keine Messung'}
              {letzte?.dropped_pkts ? `, ${letzte.dropped_pkts} verlorene Pakete` : ''}
            </div>
          </div>
          {auslastung && (
            <div>
              <div className={LABEL_KLASSE}>{UPLINK_LAST_LABEL}</div>
              <div className="text-sm text-white">{auslastung}</div>
            </div>
          )}
          {ausgang.length > 0 && (
            <div>
              <div className={LABEL_KLASSE}>Ausgehend je Plattform</div>
              <div className="text-sm text-white">
                {ausgang.map((z) => `${z.ziel}: ${z.kbps} kbit/s`).join(' · ')}
              </div>
            </div>
          )}
          {lage && (
            <div className="sm:col-span-2">
              <div className={LABEL_KLASSE}>Wie es gerade läuft</div>
              <div className="text-sm text-white">{lage}</div>
            </div>
          )}
          {proZiel.length > 0 && (
            <div className="sm:col-span-2">
              <div className={LABEL_KLASSE}>Übertragen</div>
              <div className="text-sm text-white">
                {proZiel.map(([ziel, gb]) => `${ziel}: ${Number(gb).toFixed(2)} GB`).join(' · ')}
              </div>
            </div>
          )}
        </div>
      )}
    </Rise>
  );
}

function VerwaltungsKarte() {
  const queryClient = useQueryClient();
  const uebersicht = useQuery({
    queryKey: ['uplink-admin-overview'],
    queryFn: fetchUplinkAdminOverview,
    retry: false,
    refetchInterval: 20000,
  });
  const warteliste = useQuery({
    queryKey: ['uplink-admin-waitlist'],
    queryFn: fetchUplinkAdminWaitlist,
    retry: false,
  });

  const [plaetze, setPlaetze] = useState('');
  const [lastgrenze, setLastgrenze] = useState('');
  const [bestaetigteSession, setBestaetigteSession] = useState<number | null>(null);

  const einstellungen = useMutation({
    mutationFn: () =>
      saveUplinkAdminSettings({
        max_points: zahlOderUndefined(plaetze, true),
        load_reject_threshold: lastgrenze.trim() ? Number(lastgrenze) : undefined,
      }),
    onSuccess: (antwort) => {
      const formular = formularAusEinstellungen(antwort);
      setPlaetze(formular.plaetze);
      setLastgrenze(formular.lastgrenze);
      queryClient.invalidateQueries({ queryKey: ['uplink-admin-overview'] });
    },
  });

  const beenden = useMutation({
    mutationFn: (sessionId: number) => killUplinkSession(sessionId),
    onSuccess: (antwort) => {
      if (killErfolgreich(antwort)) {
        setBestaetigteSession(null);
      }
      queryClient.invalidateQueries({ queryKey: ['uplink-admin-overview'] });
    },
  });

  // Bei einem fehlgeschlagenen neuen Abruf keine veraltete Session als aktuell anzeigen.
  const daten = uebersicht.isError ? undefined : uebersicht.data;
  const wartelistenStatus = wartelistenAnzeige({
    isLoading: warteliste.isLoading,
    isError: warteliste.isError,
    hasData: warteliste.data !== undefined,
    entryCount: warteliste.data?.entries.length ?? 0,
  });

  return (
    <Rise className="panel-card space-y-4 rounded-2xl p-6">
      <div>
        <div className="mb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-primary">
          Verwaltung
        </div>
        <h2 className="text-lg font-bold text-white">Auslastung und Grenzen</h2>
      </div>

      {uebersicht.isError && (
        <p className="text-sm text-warning">
          {fehlertext(uebersicht.error, 'Die Übersicht ist gerade nicht erreichbar.')}
        </p>
      )}
      {uebersicht.isLoading && (
        <p className="text-sm text-text-secondary">Laufende Streams werden geladen.</p>
      )}

      {daten && (
        <div className="grid gap-3 sm:grid-cols-3">
          <div>
            <div className={LABEL_KLASSE}>Plätze gesamt</div>
            <div className="text-lg font-bold text-white">{daten.max_points}</div>
          </div>
          <div>
            <div className={LABEL_KLASSE}>Plätze belegt</div>
            <div className="text-lg font-bold text-white">{daten.used_points}</div>
          </div>
          <div>
            <div className={LABEL_KLASSE}>Serverlast</div>
            <div className="text-lg font-bold text-white">{(daten.loadavg ?? 0).toFixed(2)}</div>
          </div>
        </div>
      )}

      <div className="grid gap-2 sm:grid-cols-2">
        <label className="space-y-1">
          <span className={LABEL_KLASSE}>Plätze gesamt</span>
          <input
            value={plaetze}
            onChange={(e) => setPlaetze(e.target.value)}
            inputMode="numeric"
            className={FELD_KLASSE}
            placeholder={daten ? String(daten.max_points) : ''}
          />
        </label>
        <label className="space-y-1">
          <span className={LABEL_KLASSE}>Lastgrenze für neue Streams</span>
          <input
            value={lastgrenze}
            onChange={(e) => setLastgrenze(e.target.value)}
            inputMode="decimal"
            className={FELD_KLASSE}
            placeholder="zum Beispiel 6.0"
          />
        </label>
      </div>
      <button
        type="button"
        disabled={einstellungen.isPending || (!plaetze.trim() && !lastgrenze.trim())}
        onClick={() => einstellungen.mutate()}
        className={KNOPF_KLASSE}
      >
        {einstellungen.isSuccess ? 'Gespeichert' : 'Grenzen speichern'}
      </button>
      {einstellungen.isError && (
        <p className="text-xs text-warning">
          {fehlertext(einstellungen.error, 'Die Grenzen ließen sich nicht speichern.')}
        </p>
      )}

      <div className="space-y-2 border-t border-border pt-4">
        <h3 className="text-base font-bold text-white">Laufenden Stream beenden</h3>
        {daten && !Array.isArray(daten.active_sessions) && (
          <p className="text-xs text-warning">
            Laufende Streams sind gerade nicht verfügbar.
          </p>
        )}
        {daten && Array.isArray(daten.active_sessions) && daten.active_sessions.length === 0 && (
          <p className="text-xs text-text-secondary">Gerade läuft kein Stream.</p>
        )}
        <ul className="space-y-2">
          {daten?.active_sessions?.map((eintrag) => (
            <li
              key={eintrag.session_id}
              className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-border p-3"
            >
              <span className="text-sm text-white">Stream {eintrag.session_id}</span>
              <div className="flex flex-wrap items-center gap-2">
                <label className="flex items-center gap-2 text-xs text-text-secondary">
                  <input
                    type="checkbox"
                    checked={bestaetigteSession === eintrag.session_id}
                    onChange={(e) =>
                      setBestaetigteSession(e.target.checked ? eintrag.session_id : null)
                    }
                  />
                  Ja, wirklich beenden
                </label>
                <button
                  type="button"
                  disabled={
                    beenden.isPending || bestaetigteSession !== eintrag.session_id
                  }
                  onClick={() => beenden.mutate(eintrag.session_id)}
                  className="rounded-xl border border-warning/40 bg-warning/10 px-4 py-2 text-sm font-semibold text-warning disabled:opacity-60"
                >
                  Beenden
                </button>
              </div>
              {beenden.isSuccess && beenden.data?.session_id === eintrag.session_id && (
                <p className="basis-full text-xs text-warning">
                  {killErfolgreich(beenden.data)
                    ? 'Der Stream wurde beendet.'
                    : UPLINK_KILL_LAEUFT_NOCH}
                </p>
              )}
            </li>
          ))}
        </ul>
        {beenden.isError && (
          <p className="text-xs text-warning">
            {fehlertext(beenden.error, UPLINK_KILL_LAEUFT_NOCH)}
          </p>
        )}
      </div>

      <div className="space-y-2 border-t border-border pt-4">
        <h3 className="text-base font-bold text-white">Warteliste</h3>
        {wartelistenStatus === 'loading' && (
          <p className="text-xs text-text-secondary">Warteliste wird geladen.</p>
        )}
        {wartelistenStatus === 'error' && (
          <p className="text-xs text-warning">Die Warteliste ist gerade nicht erreichbar.</p>
        )}
        {wartelistenStatus === 'empty' && (
          <p className="text-xs text-text-secondary">Niemand wartet gerade.</p>
        )}
        {wartelistenStatus === 'entries' && (
          <ul className="space-y-1 text-sm text-white">
            {warteliste.data?.entries.map((eintrag) => (
              <li key={eintrag.streamer_id} className="flex items-center justify-between gap-3">
                <span>{eintrag.streamer_id}</span>
                <span className="text-xs text-text-secondary">
                  {eintrag.enabled ? 'freigeschaltet' : eintrag.requested_at}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Rise>
  );
}

export function UplinkPage() {
  const queryClient = useQueryClient();
  const { data: authStatus } = useAuthStatus();
  const planName = authStatus?.plan?.planName ?? 'Free';
  const ansicht = uplinkAnsicht({
    isAdmin: authStatus?.isAdmin,
    twitchLogin: authStatus?.twitchLogin,
  });
  const streamerBloeckeSichtbar = uplinkStreamerBloeckeSichtbar(ansicht);
  const adminBloeckeSichtbar = uplinkAdminBloeckeSichtbar(ansicht);
  const streamerAbfrageAktiv =
    authStatus?.authenticated === true && streamerBloeckeSichtbar;
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ['uplink-me'],
    queryFn: fetchUplinkMe,
    retry: false,
    refetchInterval: UPLINK_ME_REFETCH_INTERVAL_MS,
    enabled: streamerAbfrageAktiv,
  });
  const waitlist = useMutation({
    mutationFn: joinUplinkWaitlist,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['uplink-me'] }),
  });
  const { data: helpPages, isError: isHelpError } = useQuery({
    queryKey: ['uplink-help'],
    queryFn: fetchUplinkHelp,
    staleTime: Infinity,
  });
  const ziele = useQuery({
    queryKey: ['uplink-destinations'],
    queryFn: fetchUplinkDestinations,
    retry: false,
    enabled: streamerBloeckeSichtbar && Boolean(data?.enabled),
  });
  const zielStatus = ziele.isLoading ? 'loading' : ziele.isError ? 'error' : 'ready';

  const zieleNachPlattform = useMemo(() => {
    const karte = new Map<string, UplinkDestination>();
    for (const ziel of ziele.data?.destinations ?? []) {
      karte.set(ziel.platform, ziel);
    }
    return karte;
  }, [ziele.data]);

  const zieleNeuLaden = () => {
    queryClient.invalidateQueries({ queryKey: ['uplink-destinations'] });
    queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
  };

  return (
    <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
      <div className="relative mx-auto max-w-[1440px]">
        <div className="grid gap-4 md:gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
          <Rise as="aside" className="panel-card card-glow self-start rounded-2xl p-4 lg:sticky lg:top-4">
            <div className="space-y-4">
              <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                Main
              </div>
              <nav className="lg:space-y-1">
                <SidebarLink href={PREVIEW_HOME_ROUTE} icon={Home} label="Home" />
                <SidebarLink href={analyticsTabHref('overview')} icon={BarChart3} label="Analyse" />
                <SidebarLink href="/social-media-admin" icon={Film} label="Social Media Dashboard" />
                <SidebarLink href={PREVIEW_UPLINK_ROUTE} icon={Radio} label="Uplink" active />
              </nav>
              <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                Tools
              </div>
              <div className="lg:space-y-1">
                <SidebarLink href={PREVIEW_VERWALTUNG_ROUTE} icon={Settings} label="Verwaltung" />
                <SidebarLink href={PREVIEW_OVERLAY_ROUTE} icon={MonitorPlay} label="Stream-Overlay" />
                <SidebarLink href={PREVIEW_PRICING_ROUTE} icon={Sparkles} label={`Plan: ${planName}`} />
                <SidebarLink href={PREVIEW_CHANGELOG_ROUTE} icon={FileText} label="Changelog" />
              </div>
            </div>
          </Rise>

          <div className="space-y-4">
            <Rise className="panel-card rounded-2xl p-6">
              <div className="mb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-primary">
                Eigenes Modul
              </div>
              <h1 className="display-font text-2xl font-extrabold text-white">Uplink</h1>
              <p className="mt-2 max-w-2xl text-sm text-text-secondary">
                Du sendest zu uns, wir legen die Plattform-Streams an. Start und Stop machst du in OBS.
              </p>
            </Rise>

            {authStatus && streamerBloeckeSichtbar && isLoading && (
              <div className="panel-card flex items-center gap-2 rounded-2xl p-6 text-text-secondary">
                <Loader2 className="h-4 w-4 animate-spin" />
                Status wird geladen
              </div>
            )}

            {streamerBloeckeSichtbar && isError && (
              <div className="panel-card rounded-2xl p-6 text-sm text-warning">
                {fehlertext(error, 'Uplink ist gerade nicht erreichbar.')}
              </div>
            )}

            {ansicht === 'admin-ohne-twitch' && (
              <Rise className="panel-card space-y-2 rounded-2xl p-6">
                <h2 className="text-lg font-bold text-white">Persönliche Ansicht</h2>
                <p className="text-sm text-text-secondary">{UPLINK_TWITCH_LOGIN_HINT}</p>
              </Rise>
            )}

            {streamerBloeckeSichtbar && data && !data.enabled && (
              <div className="panel-card relative overflow-hidden rounded-2xl p-6">
                <div className="absolute inset-0 bg-black/20" />
                <div className="relative space-y-3">
                  <div className="inline-flex h-12 w-12 items-center justify-center rounded-full border border-white/10 bg-white/5">
                    <Lock className="h-5 w-5 text-white/40" />
                  </div>
                  <h2 className="text-lg font-bold text-white">Auf mehreren Plattformen gleichzeitig senden</h2>
                  <p className="max-w-xl text-sm text-text-secondary">{UPLINK_WAITLIST_TEXT}</p>
                  <div className="flex flex-wrap gap-2">
                    {/* Was es kostet, steht vor dem Klick auf die Warteliste,
                        nicht erst in der Rechnung. */}
                    <a
                      href={PREVIEW_PRICING_ROUTE}
                      className="rounded-xl border border-border px-4 py-2 text-sm font-semibold text-white no-underline"
                    >
                      Preise ansehen
                    </a>
                    <button
                      type="button"
                      disabled={waitlist.isPending || data.waitlisted}
                      onClick={() => waitlist.mutate()}
                      className={KNOPF_KLASSE}
                    >
                      {data.waitlisted ? 'Stehst auf der Warteliste' : 'Auf die Warteliste'}
                    </button>
                  </div>
                  {waitlist.isError && (
                    <p className="text-xs text-warning">{UPLINK_WAITLIST_FEHLER}</p>
                  )}
                </div>
              </div>
            )}

            {streamerBloeckeSichtbar && data?.enabled && (
              <div className="space-y-4">
                <Rise className="panel-card space-y-4 rounded-2xl p-6">
                  <h2 className="text-lg font-bold text-white">OBS einrichten</h2>
                  {/* srt_hint liefert das Relay immer als String (rs-relay,
                      srt_hint_fuer in src/api/user.rs). Leer ist es genau
                      dann, wenn kein ingest_key existiert, also fuer einen
                      nicht freigeschalteten Zugang. Dieser Block haengt an
                      data.enabled, trotzdem faengt der Guard den Leerfall ab:
                      ein leeres Kopierfeld waere die schlechteste Antwort. */}
                  {data.srt_hint ? (
                    <CopyField label="SRT-Adresse" value={data.srt_hint} />
                  ) : (
                    <p className="text-sm text-warning">
                      Der Relay hat gerade keine SRT-Adresse geliefert. Lade die Seite neu; bleibt es dabei, meld dich beim Support.
                    </p>
                  )}
                  <p className="text-xs text-text-secondary">
                    In OBS: Dienst Benutzerdefiniert, Adresse einsetzen, Hardware-HEVC, VBR,
                    Keyframe alle 2 Sekunden. Danach Stream starten.
                  </p>
                </Rise>

                <div className="grid gap-4 lg:grid-cols-2">
                  {UPLINK_PLATFORMS.map((platform) => (
                    <PlattformKarte
                      key={platform.id}
                      platform={platform}
                      gespeichert={zieleNachPlattform.get(platform.id)}
                      status={zielStatus}
                      onSaved={zieleNeuLaden}
                    />
                  ))}
                </div>

                <ZeitplanKarte />
                <StatusKarte sessionId={aktiveSessionId(data)} />

                <Rise className="panel-card space-y-4 rounded-2xl p-6">
                  <div>
                    <h2 className="text-lg font-bold text-white">Uplink-Hilfe</h2>
                    <p className="mt-1 text-sm text-text-secondary">
                      Die Streamer-Hilfe erklärt Uplink, die OBS-Einrichtung und häufige Störungen.
                    </p>
                  </div>
                  {isHelpError && (
                    <p className="text-sm text-warning">Die Uplink-Hilfe ist gerade nicht erreichbar.</p>
                  )}
                  <div className="space-y-4">
                    {(helpPages ?? UPLINK_HELP_PAGES.map((page) => ({ ...page, html: '' }))).map((page) => (
                      <div
                        key={page.file}
                        className="uplink-help-shell overflow-hidden rounded-xl border border-border bg-background/70"
                      >
                        {page.html ? (
                          <div dangerouslySetInnerHTML={{ __html: page.html }} />
                        ) : (
                          <div className="p-4 text-sm text-text-secondary">Hilfe wird geladen: {page.label}</div>
                        )}
                      </div>
                    ))}
                  </div>
                  <a className="text-sm font-semibold text-primary" href={uplinkHelpUrl('index.html')}>
                    Uplink-Hilfe als eigene Seite öffnen
                  </a>
                </Rise>
              </div>
            )}

            {adminBloeckeSichtbar && <VerwaltungsKarte />}
            <Rise className="panel-card space-y-4 rounded-2xl p-6">
              <div>
                <h2 className="text-lg font-bold text-white">Uplink-Hilfe</h2>
                <p className="mt-1 text-sm text-text-secondary">
                  Die Streamer-Hilfe erklärt Uplink, die OBS-Einrichtung und häufige Störungen.
                </p>
              </div>
              {isHelpError && (
                <p className="text-sm text-warning">Die Uplink-Hilfe ist gerade nicht erreichbar.</p>
              )}
              {/* Teilausfall sichtbar machen: sonst haelt der Streamer zwei
                  Kacheln fuer die vollstaendige Hilfe. */}
              {helpPages && helpPages.length < UPLINK_HELP_PAGES.length && (
                <p className="text-sm text-warning">
                  {UPLINK_HELP_PAGES.length - helpPages.length} von {UPLINK_HELP_PAGES.length} Kapiteln konnten nicht geladen werden.
                </p>
              )}
              <div className="space-y-4">
                {/* Bei einem Fehler keine Platzhalter mehr: drei Kacheln
                    "Hilfe wird geladen" neben der Fehlerzeile behaupten einen
                    Fortschritt, der nicht mehr kommt. */}
                {(helpPages ?? (isHelpError ? [] : UPLINK_HELP_PAGES.map((page) => ({ ...page, html: '' })))).map((page) => (
                  <div key={page.file} className="uplink-help-shell overflow-hidden rounded-xl border border-border bg-background/70">
                    {page.html ? (
                      <div dangerouslySetInnerHTML={{ __html: page.html }} />
                    ) : (
                      <div className="p-4 text-sm text-text-secondary">Hilfe wird geladen: {page.label}</div>
                    )}
                  </div>
                ))}
              </div>
              <a className="text-sm font-semibold text-primary" href={uplinkHelpUrl('index.html')}>
                Uplink-Hilfe als eigene Seite öffnen
              </a>
            </Rise>
          </div>
        </div>
      </div>
    </div>
  );
}
