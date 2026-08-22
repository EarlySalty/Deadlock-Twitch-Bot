import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BarChart3,
  Check,
  ChevronDown,
  Copy,
  Eye,
  EyeOff,
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
  UPLINK_PROFILE,
  fetchUplinkDestinations,
  fetchUplinkMe,
  joinUplinkWaitlist,
  saveUplinkTwitchDestination,
} from '@/api/uplink';
import type { UplinkProfilName } from '@/api/uplink';
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
 * Kopierfeld mit verdecktem Wert.
 *
 * Kopieren geht immer, auch verdeckt: dafuer muss niemand den Wert sehen.
 * Aufdecken haengt an `darfAufdecken`. Sendet der Streamer gerade, bleibt der
 * Wert verdeckt, denn ein geteilter Bildschirm zeigt ihn sonst der ganzen
 * Zuschauerschaft.
 *
 * Der Effekt verdeckt auch wieder: wer aufdeckt und dann den Stream startet,
 * haette den Wert sonst offen auf dem Schirm, genau im gefaehrlichsten Moment.
 *
 * `navigator.clipboard` scheitert still ohne HTTPS oder ohne Erlaubnis. Ohne
 * den Fehlerzweig meldete das Feld "Kopiert", waehrend die Zwischenablage leer
 * bliebe, und der Streamer suchte den Fehler spaeter in OBS.
 */
/**
 * Die Twitch-Fenster, die OBS bei Dienst „Benutzerdefiniert“ ausblendet.
 *
 * Es sind dieselben Adressen, die OBS in seine eigenen Docks laedt: siehe
 * `frontend/oauth/TwitchAuth.cpp` im OBS-Quellcode. Die eingebauten Docks sind
 * selbst nur Browser-Fenster, sie werden nur automatisch angelegt, sobald ein
 * Twitch-Konto verbunden ist. Inhaltlich ist ein eigenes Dock dasselbe Fenster.
 *
 * Drei der vier Adressen kommen ohne Kanalnamen aus: Twitch leitet einen
 * angemeldeten Nutzer auf seinen eigenen Kanal weiter. Das ist robuster als
 * der Namensweg, weil es auch nach einer Namensaenderung noch stimmt. Nur der
 * Chat braucht den Kanal in der Adresse.
 */
const OBS_DOCKS = [
  {
    titel: 'Chat',
    pfad: (k: string) => (k ? `https://www.twitch.tv/popout/${k}/chat?darkpopout` : ''),
  },
  {
    titel: 'Aktivitätsfeed',
    pfad: () => 'https://dashboard.twitch.tv/popout/stream-manager/activity-feed',
  },
  {
    titel: 'Stream-Informationen',
    pfad: () => 'https://dashboard.twitch.tv/popout/stream-manager/edit-stream-info',
  },
  {
    titel: 'Kanalpunkte',
    pfad: () => 'https://dashboard.twitch.tv/popout/stream-manager/community-points',
  },
] as const;

function DockZeile({ titel, url }: { titel: string; url: string }) {
  const [kopiert, setKopiert] = useState(false);

  async function kopieren() {
    try {
      await navigator.clipboard.writeText(url);
      setKopiert(true);
      window.setTimeout(() => setKopiert(false), 1600);
    } catch {
      // Ohne Zwischenablage bleibt die Adresse lesbar im Feld stehen, dann
      // markiert man sie von Hand. Eine Fehlermeldung hilft hier nicht.
      setKopiert(false);
    }
  }

  return (
    <div className="flex items-center gap-2">
      <button
        type="button"
        onClick={kopieren}
        title={url}
        className="flex min-w-0 flex-1 items-center justify-between gap-2 rounded-xl border border-border bg-background/70 px-3 py-2 text-left text-xs text-white hover:border-primary/50"
      >
        <span className="shrink-0 font-semibold">{titel}</span>
        <span className="truncate font-mono text-[11px] text-text-secondary">{url}</span>
      </button>
      <span
        aria-live="polite"
        className="w-16 shrink-0 text-[11px] text-success"
      >
        {kopiert ? 'Kopiert' : ''}
      </span>
    </div>
  );
}

function CopyField({
  label,
  value,
  darfAufdecken,
  grundVerdeckt,
}: {
  label: string;
  value: string;
  darfAufdecken: boolean;
  grundVerdeckt: string;
}) {
  const [stand, setStand] = useState<'ruhe' | 'ok' | 'fehler'>('ruhe');
  const [offen, setOffen] = useState(false);

  // Sobald das Aufdecken nicht mehr erlaubt ist, faellt ein offener Wert zu.
  useEffect(() => {
    if (!darfAufdecken) setOffen(false);
  }, [darfAufdecken]);

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
  const anzeige = offen ? value : '•'.repeat(Math.min(value.length, 48));

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
          {anzeige}
        </button>
        <button
          type="button"
          onClick={() => setOffen((vorher) => !vorher)}
          disabled={!darfAufdecken}
          title={darfAufdecken ? undefined : grundVerdeckt}
          className="inline-flex shrink-0 items-center gap-1 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary transition-colors hover:text-white disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:text-text-secondary"
        >
          {offen ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
          {offen ? 'Verdecken' : 'Zeigen'}
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
      {/* aria-live, weil die Rueckmeldung sonst nur im Knopftext steht und von
          einem Screenreader nicht angesagt wird. */}
      <p aria-live="polite" className="sr-only">
        {stand === 'ok' ? 'In die Zwischenablage kopiert' : ''}
        {stand === 'fehler' ? 'Kopieren hat nicht geklappt' : ''}
      </p>
      {stand === 'fehler' && (
        <p className="text-xs text-warning">
          Dein Browser hat das Kopieren nicht erlaubt. Markier die Adresse und kopier sie von Hand.
        </p>
      )}
      {!darfAufdecken && <p className="text-xs text-text-secondary">{grundVerdeckt}</p>}
    </div>
  );
}

export function UplinkPage() {
  const queryClient = useQueryClient();
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ['uplink-me'],
    queryFn: fetchUplinkMe,
    retry: false,
    // Der Live-Status steckt in dieser Antwort und entscheidet, ob die Adresse
    // aufgedeckt werden darf. Beendet der Streamer den Stream, waehrend das
    // Dashboard offen liegt, soll das Aufdecken kurz darauf wieder gehen, ohne
    // dass jemand neu laedt. Andersherum genauso: Stream an, Adresse zu.
    refetchInterval: 15_000,
    refetchOnWindowFocus: true,
  });
  const { data: helpPages, isError: isHelpError } = useQuery({
    queryKey: ['uplink-help'],
    queryFn: fetchUplinkHelp,
    staleTime: Infinity,
  });
  // Die gespeicherten Ziele. Ohne sie sieht ein hinterlegtes Ziel aus wie ein
  // leeres Formular, weil der Stream-Key nie zurueckkommt.
  const { data: ziele } = useQuery({
    queryKey: ['uplink-destinations'],
    queryFn: fetchUplinkDestinations,
    retry: false,
  });
  const waitlist = useMutation({
    mutationFn: joinUplinkWaitlist,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['uplink-me'] }),
  });
  const [rtmpUrl, setRtmpUrl] = useState('rtmp://live.twitch.tv/app');
  const [streamKey, setStreamKey] = useState('');
  const [profil, setProfil] = useState<UplinkProfilName>('1080p60');
  const saveDest = useMutation({
    mutationFn: () =>
      saveUplinkTwitchDestination({ rtmp_url: rtmpUrl, stream_key: streamKey, profil }),
    onSuccess: () => {
      setStreamKey('');
      queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
      // Ohne das bliebe die Zielliste auf dem Stand von vor dem Speichern, und
      // die Rueckmeldung fehlte genau in dem Moment, in dem sie zaehlt.
      queryClient.invalidateQueries({ queryKey: ['uplink-destinations'] });
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
                  {/* Die Felder heissen wie in OBS, nicht wie bei uns. Wer
                      "SRT-Adresse" liest und in OBS "Server" und
                      "Streamschluessel" vor sich hat, raet sonst, und der Key
                      landet im falschen Feld. Bei SRT gibt es keinen zweiten
                      Wert: die streamid steckt in der Serveradresse, das
                      Schluesselfeld bleibt leer. Genau das steht deshalb als
                      eigenes Feld da, statt als Nebensatz. */}
                  {data.srt_hint ? (
                    <CopyField
                      label="OBS-Feld „Server“"
                      value={data.srt_hint}
                      darfAufdecken={data.live_status === 'aus'}
                      grundVerdeckt={
                        data.live_status === 'live'
                          ? 'Du bist gerade live. Solange bleibt die Adresse verdeckt, damit sie nicht im Stream landet. Kopieren geht trotzdem.'
                          : 'Wir wissen gerade nicht sicher, ob du live bist. Solange bleibt die Adresse verdeckt. Kopieren geht trotzdem.'
                      }
                    />
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
                  <div className="space-y-1">
                    <div className="text-[11px] font-semibold uppercase tracking-[0.16em] text-text-secondary">
                      OBS-Feld „Streamschlüssel“
                    </div>
                    <div className="rounded-xl border border-dashed border-border bg-background/40 px-3 py-2 text-xs text-text-secondary">
                      Leer lassen. Dein Schlüssel steckt schon in der Serveradresse oben.
                    </div>
                  </div>

                  <ol className="list-decimal space-y-1 pl-5 text-xs text-text-secondary marker:text-primary/70">
                    <li>In OBS: Einstellungen, Stream, Dienst auf „Benutzerdefiniert“.</li>
                    <li>Serveradresse oben kopieren und in „Server“ einfügen.</li>
                    <li>„Streamschlüssel“ leer lassen.</li>
                    <li>Ausgabe: Hardware-HEVC, VBR, Keyframe 2 s.</li>
                    <li>Stream starten. Den Rest machen wir.</li>
                  </ol>
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
                  <div className="space-y-1">
                    <label
                      htmlFor="uplink-profil"
                      className="block text-[11px] font-semibold uppercase tracking-[0.16em] text-text-secondary"
                    >
                      Qualität, die wir an Twitch senden
                    </label>
                    <select
                      id="uplink-profil"
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
                    <p className="text-xs text-text-secondary">
                      {UPLINK_PROFILE.find((e) => e.name === profil)?.hinweis}
                    </p>
                    <p className="text-xs text-text-secondary">
                      Du darfst uns 1440p schicken, wir rechnen daraus diese Stufe. Twitch selbst nimmt über
                      diesen Weg kein 1440p an.
                    </p>
                  </div>

                  <button
                    type="button"
                    disabled={saveDest.isPending || !streamKey}
                    onClick={() => saveDest.mutate()}
                    className="rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
                  >
                    {saveDest.isSuccess ? 'Gespeichert' : 'Twitch-Ziel speichern'}
                  </button>

                  {/* Der Stream-Key kommt nie zurueck, das Feld bleibt also
                      leer, auch wenn ein Ziel gespeichert ist. Ohne diese Zeile
                      sieht ein fertig eingerichtetes Konto aus wie ein leeres
                      Formular, und der Streamer speichert ein zweites Mal. */}
                  {ziele && ziele.destinations.length > 0 ? (
                    <div className="space-y-1">
                      {ziele.destinations.map((ziel) => (
                        <div
                          key={ziel.platform}
                          className="flex items-center gap-2 rounded-xl border border-success/30 bg-success/10 px-3 py-2 text-xs text-success"
                        >
                          <Check className="h-3.5 w-3.5 shrink-0" />
                          <span>
                            <strong className="font-semibold capitalize">{ziel.platform}</strong> ist
                            gespeichert{ziel.enabled ? '' : ' (aus)'}. Schlüssel liegt verschlüsselt bei uns.
                            {ziel.effective ? (
                              <>
                                {' '}Wir senden {ziel.effective.height}p{ziel.effective.fps} mit{' '}
                                {ziel.effective.bitrate_kbps} kbps.
                              </>
                            ) : null}
                          </span>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="text-xs text-text-secondary">
                      Noch kein Ziel gespeichert. Ohne Ziel kommt dein Stream bei uns an, geht aber nirgends hin.
                    </p>
                  )}
                </Rise>

                <Rise className="panel-card space-y-3 rounded-2xl p-6">
                  <h2 className="text-lg font-bold text-white">Chat und OBS-Fenster</h2>
                  <p className="text-sm text-text-secondary">
                    Bei Dienst „Benutzerdefiniert“ blendet OBS die Twitch-Fenster aus. Dein Chat läuft
                    normal weiter, nur die Fenster fehlen. Es sind dieselben Seiten, die OBS auch in
                    seine eigenen Fenster lädt, du legst sie einmal selbst an. In OBS unter{' '}
                    <strong>Docks</strong>, <strong>Benutzerdefinierte Browser-Docks</strong>: Name
                    eintragen, Adresse hier kopieren, einfügen.
                  </p>
                  <div className="space-y-1.5">
                    {OBS_DOCKS.map((dock) => {
                      const url = dock.pfad(data.twitch_login ?? '');
                      if (!url) return null;
                      return <DockZeile key={dock.titel} titel={dock.titel} url={url} />;
                    })}
                  </div>
                  {data.twitch_login ? null : (
                    <p className="text-xs text-text-secondary">
                      Für den Chat brauchen wir deinen Kanalnamen, den kennen wir gerade nicht. Melde
                      dich neu an, dann steht auch diese Adresse hier.
                    </p>
                  )}
                  <p className="text-xs text-text-secondary">
                    Im Dock musst du bei Twitch angemeldet sein, danach bleibt die Anmeldung stehen.
                    Einmal einrichten, Fenster anordnen, unter <strong>Docks</strong> das Layout
                    speichern. Das übersteht jeden OBS-Neustart.
                  </p>
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
              {/* Jedes Kapitel klappt einzeln auf und startet zu. Aufgeklappt
                  fuellte die Hilfe mehrere Bildschirmhoehen und schob alles
                  darueber aus dem Blick; wer sie braucht, sucht ohnehin ein
                  bestimmtes Kapitel.

                  `details` statt eigenem Zustand: das Auf und Zu, die
                  Tastaturbedienung und die Ansage fuer Screenreader bringt der
                  Browser mit, und die Seitensuche des Browsers findet auch
                  zugeklappten Text. */}
              <div className="space-y-2">
                {/* Bei einem Fehler keine Platzhalter mehr: drei Kacheln
                    "Hilfe wird geladen" neben der Fehlerzeile behaupten einen
                    Fortschritt, der nicht mehr kommt. */}
                {(helpPages ?? (isHelpError ? [] : UPLINK_HELP_PAGES.map((page) => ({ ...page, html: '' })))).map((page) =>
                  page.html ? (
                    <details
                      key={page.file}
                      className="uplink-help-shell group overflow-hidden rounded-xl border border-border bg-background/70"
                    >
                      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-4 py-3 text-sm font-semibold text-white transition-colors hover:bg-white/5 [&::-webkit-details-marker]:hidden">
                        <span>{page.label}</span>
                        <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open:rotate-180" />
                      </summary>
                      <div dangerouslySetInnerHTML={{ __html: page.html }} />
                    </details>
                  ) : (
                    <div
                      key={page.file}
                      className="rounded-xl border border-border bg-background/70 p-4 text-sm text-text-secondary"
                    >
                      Hilfe wird geladen: {page.label}
                    </div>
                  ),
                )}
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
