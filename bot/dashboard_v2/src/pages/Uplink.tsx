import { useState } from 'react';
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
  FileText,
} from 'lucide-react';
import { Rise } from '../motion/Rise';
import '../uplinkHelp.css';
import {
  fetchUplinkMe,
  joinUplinkWaitlist,
  saveUplinkTwitchDestination,
} from '@/api/uplink';
import {
  PREVIEW_CHANGELOG_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_UPLINK_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
  analyticsTabHref,
} from '@/preview/routes';
import { fetchUplinkHelp, uplinkHelpUrl, UPLINK_HELP_PAGES } from '@/uplinkHelp';

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

/**
 * Kopierfeld: der Knopf kopiert, und das Feld selbst ist ebenfalls ein
 * Klickziel. Beides laeuft ueber denselben Weg, damit die Rueckmeldung nicht
 * an einer der beiden Stellen fehlt.
 *
 * `navigator.clipboard` scheitert still, wenn die Seite ohne HTTPS laeuft oder
 * der Browser die Erlaubnis verweigert. Ohne den Fehlerzweig sagt das Feld
 * dann "Kopiert", waehrend die Zwischenablage leer bleibt, und der Streamer
 * sucht den Fehler spaeter in OBS.
 */
function CopyField({ label, value }: { label: string; value: string }) {
  const [stand, setStand] = useState<'ruhe' | 'ok' | 'fehler'>('ruhe');
  if (!value) return null;

  const kopieren = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setStand('ok');
    } catch {
      setStand('fehler');
    }
    window.setTimeout(() => setStand('ruhe'), 2000);
  };

  const knopfText = stand === 'ok' ? 'Kopiert' : stand === 'fehler' ? 'Ging nicht' : 'Kopieren';

  return (
    <div className="space-y-1">
      <div className="text-[11px] font-semibold uppercase tracking-[0.16em] text-text-secondary">
        {label}
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={kopieren}
          title="Klicken zum Kopieren"
          className="min-w-0 flex-1 cursor-pointer truncate rounded-xl border border-border bg-background/70 px-3 py-2 text-left font-mono text-xs text-white transition-colors hover:border-primary/40 hover:bg-background"
        >
          {value}
        </button>
        <button
          type="button"
          className="inline-flex shrink-0 items-center gap-1 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary hover:text-white"
          onClick={kopieren}
        >
          <Copy className="h-3.5 w-3.5" />
          {knopfText}
        </button>
      </div>
      {/* Ein aria-live-Bereich, weil die Rueckmeldung sonst nur im Knopftext
          steht und von einem Screenreader nicht angesagt wird. */}
      <p aria-live="polite" className="sr-only">
        {stand === 'ok' ? 'In die Zwischenablage kopiert' : ''}
        {stand === 'fehler' ? 'Kopieren hat nicht geklappt' : ''}
      </p>
      {stand === 'fehler' && (
        <p className="text-xs text-warning">
          Dein Browser hat das Kopieren nicht erlaubt. Markier die Adresse und kopier sie von Hand.
        </p>
      )}
    </div>
  );
}

export function UplinkPage() {
  const queryClient = useQueryClient();
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ['uplink-me'],
    queryFn: fetchUplinkMe,
    retry: false,
  });
  const { data: helpPages, isError: isHelpError } = useQuery({
    queryKey: ['uplink-help'],
    queryFn: fetchUplinkHelp,
    staleTime: Infinity,
  });
  const waitlist = useMutation({
    mutationFn: joinUplinkWaitlist,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['uplink-me'] }),
  });
  const [rtmpUrl, setRtmpUrl] = useState('rtmp://live.twitch.tv/app');
  const [streamKey, setStreamKey] = useState('');
  const saveDest = useMutation({
    mutationFn: () =>
      saveUplinkTwitchDestination({ rtmp_url: rtmpUrl, stream_key: streamKey }),
    onSuccess: () => {
      setStreamKey('');
      queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
    },
  });

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

            {isLoading && (
              <div className="panel-card flex items-center gap-2 rounded-2xl p-6 text-text-secondary">
                <Loader2 className="h-4 w-4 animate-spin" />
                Status wird geladen
              </div>
            )}

            {isError && (
              <div className="panel-card rounded-2xl p-6 text-sm text-warning">
                {error instanceof Error
                  ? error.message
                  : 'Uplink ist gerade nicht erreichbar.'}
              </div>
            )}

            {data && !data.enabled && (
              <div className="panel-card relative overflow-hidden rounded-2xl p-6">
                <div className="absolute inset-0 bg-black/20" />
                <div className="relative space-y-3">
                  <div className="inline-flex h-12 w-12 items-center justify-center rounded-full border border-white/10 bg-white/5">
                    <Lock className="h-5 w-5 text-white/40" />
                  </div>
                  <h2 className="text-lg font-bold text-white">Uplink ist ein bezahltes Add-on</h2>
                  <p className="max-w-xl text-sm text-text-secondary">
                    Ohne Freischaltung kannst du auf die Warteliste. Danach richtet EarlySalty den Slot ein.
                  </p>
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
                      className="rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
                    >
                      {data.waitlisted ? 'Stehst auf der Warteliste' : 'Auf die Warteliste'}
                    </button>
                  </div>
                </div>
              </div>
            )}

            {data?.enabled && (
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
                  {/* Die Adresse traegt den Ingest-Key als streamid. Wer sie
                      abliest, sendet auf denselben Kanal wie der Streamer. Auf
                      einem geteilten Bildschirm passiert genau das, deshalb
                      steht die Warnung direkt am Feld und nicht in der Hilfe. */}
                  <p className="rounded-xl border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
                    <strong className="font-semibold">Zeig diese Adresse nicht im Stream.</strong>{' '}
                    Sie enthält deinen persönlichen Schlüssel. Wer sie sieht, kann auf deinem Kanal senden.
                    Blende das Dashboard aus, bevor du es teilst.
                  </p>
                  <p className="text-xs text-text-secondary">
                    In OBS: Dienst Benutzerdefiniert, SRT-Adresse aus dem Dashboard. Hardware-HEVC, VBR, Keyframe 2 s. Danach Stream starten.
                  </p>
                </Rise>

                <Rise className="panel-card space-y-3 rounded-2xl p-6">
                  <h2 className="text-lg font-bold text-white">Twitch-Ziel</h2>
                  <p className="text-sm text-text-secondary">
                    Stream-Schlüssel von Twitch, nicht der Schlüssel für Uplink. Er wird verschlüsselt gespeichert.
                  </p>
                  <input
                    value={rtmpUrl}
                    onChange={(e) => setRtmpUrl(e.target.value)}
                    className="w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
                    placeholder="rtmp://live.twitch.tv/app"
                  />
                  <input
                    value={streamKey}
                    onChange={(e) => setStreamKey(e.target.value)}
                    type="password"
                    className="w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
                    placeholder="Twitch Stream-Key"
                  />
                  <button
                    type="button"
                    disabled={saveDest.isPending || !streamKey}
                    onClick={() => saveDest.mutate()}
                    className="rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
                  >
                    {saveDest.isSuccess ? 'Gespeichert' : 'Twitch-Ziel speichern'}
                  </button>
                </Rise>
              </div>
            )}

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
