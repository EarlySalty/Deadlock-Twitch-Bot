import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BarChart3,
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
  UPLINK_PLATTFORMEN,
  fetchUplinkCaps,
  fetchUplinkDestinations,
  fetchUplinkMe,
  joinUplinkWaitlist,
} from '@/api/uplink';
import { ZielKarte } from './UplinkZiel';
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
import { obsBitrateEmpfehlung } from '@/uplinkEmpfehlung';
import type { ObsBitrateEmpfehlung } from '@/uplinkEmpfehlung';

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

/**
 * Ein OBS-Feldname oder Knopf, so wie er im Programm steht.
 *
 * Woertlich und optisch abgesetzt: wer die Anleitung neben OBS liest, sucht
 * nach genau dieser Beschriftung. Ein umschriebener Name ("die Serverzeile")
 * kostet an dieser Stelle mehr Zeit als jede Erklaerung spart.
 */
function Feld({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded-md border border-border bg-background/70 px-1.5 py-0.5 font-mono text-[11px] text-white">
      {children}
    </span>
  );
}

/** Eine Station im OBS-Menue. Mehrere hintereinander lesen sich als Pfad. */
function Weg({ children }: { children: React.ReactNode }) {
  return (
    <span className="font-semibold text-white">
      {children}
      <span className="px-1 text-text-secondary">›</span>
    </span>
  );
}

/**
 * Ein nummerierter Schritt mit eigenem Inhalt.
 *
 * Vorher stand die Anleitung als Liste unter den Kopierfeldern. Wer beim
 * Kopieren nach oben scrollte, verlor die Stelle, und die Reihenfolge stimmte
 * nicht mit der in OBS ueberein. Jetzt steht jeder Wert in dem Schritt, in dem
 * er gebraucht wird.
 */
function ObsSchritt({
  nummer,
  titel,
  children,
}: {
  nummer: number;
  titel: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex gap-3">
      <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-primary/40 bg-primary/10 text-xs font-bold text-primary">
        {nummer}
      </span>
      <div className="min-w-0 flex-1 space-y-2">
        <h3 className="text-sm font-bold text-white">{titel}</h3>
        {children}
      </div>
    </div>
  );
}

/**
 * Die Ausgabe-Einstellungen, jede mit dem Grund dahinter.
 *
 * Ohne den Grund stellt niemand etwas um, was schon laeuft. HEVC ist der
 * Punkt, an dem Uplink sich lohnt, und VBR ist genau die Einstellung, die man
 * bei Twitch direkt nicht setzen darf und hier setzen soll.
 *
 * Die Bitrate ist keine feste Zahl mehr, sondern folgt den eingestellten
 * Zielen: siehe `obsBitrateEmpfehlung`.
 */
function obsAusgabe(bitrate: ObsBitrateEmpfehlung) {
  return [
    {
      feld: 'Videoencoder',
      wert: 'HEVC (H.265), Hardware',
      warum: 'NVIDIA NVENC HEVC, AMD HEVC oder Apple VT HEVC. Darum geht es hier: HEVC packt dasselbe Bild in weniger Bits.',
    },
    {
      feld: 'Ratensteuerung',
      wert: 'VBR',
      warum: 'Zu uns darf die Bitrate schwanken. Was zu den Plattformen rausgeht, machen wir selbst konstant.',
    },
    {
      feld: 'Bitrate',
      wert: `${bitrate.kbps} kbps`,
      warum:
        (bitrate.staerkstesZiel === null
          ? 'Startwert, solange kein Ziel eingerichtet ist. Sobald deine Ziele stehen, passt sich die Zahl an. '
          : `Passt zu deinen Zielen: das stärkste steht auf ${bitrate.staerkstesZiel} kbps, dazu etwas Reserve. `) +
        'Maßstab ist dein Upload. Mehr zu senden, als deine Leitung trägt, bringt nichts. Wenn du unsicher bist, miss deinen Upload und bleib rund 20 Prozent darunter. Das ist die Qualität, aus der wir rechnen, nicht die, die rausgeht.',
    },
    {
      feld: 'Keyframe-Intervall',
      wert: '2 s',
      warum: 'Feste 2 Sekunden. Bei „automatisch“ setzen manche Encoder gar keine, und dann startet kein Zuschauer sauber ein.',
    },
  ];
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
  const { data: ziele, isError: zieleFehler } = useQuery({
    queryKey: ['uplink-destinations'],
    queryFn: fetchUplinkDestinations,
    retry: false,
  });
  // Das Relay antwortet auf einen leeren Erfolg auch mal mit `{}`. Ohne die
  // Absicherung wirft `.length` beim Rendern, und die ErrorBoundary ersetzt
  // dann das ganze Dashboard, also auch die SRT-Adresse, die der Streamer
  // gerade braucht.
  const gespeicherteZiele = ziele?.destinations ?? [];
  // Die OBS-Bitrate folgt dem, was der Streamer als Ziele eingestellt hat.
  // Eine feste Zahl in der Anleitung war beides: zu hoch fuer jede normale
  // Leitung und ohne Bezug zu dem, was hier tatsaechlich rausgeht.
  const obsBitrate = obsBitrateEmpfehlung(gespeicherteZiele);
  const waitlist = useMutation({
    mutationFn: joinUplinkWaitlist,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['uplink-me'] }),
  });
  // Die Grenzen kommen vom Server, damit die Oberflaeche sie nicht doppelt
  // pflegt: `relay.platform_caps` ist eine Tabelle in einem anderen Repo.
  // Faellt der Abruf aus, faellt die Karte auf den Ingest-Deckel zurueck.
  const { data: caps } = useQuery({
    queryKey: ['uplink-caps'],
    queryFn: fetchUplinkCaps,
    staleTime: 5 * 60_000,
    retry: false,
  });
  const capsFuer = (platform: string) => caps?.platforms.find((c) => c.platform === platform);

  return (
    <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
      <div className="relative mx-auto max-w-[1800px]">
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

            {/* Zwei Spalten erst ab xl: darunter ist eine Spalte schmaler als
                die Serveradresse, und ein umbrechender SRT-Link ist unlesbar.
                `items-start` verhindert, dass die kuerzere Spalte auf die
                Hoehe der laengeren gestreckt wird und unten leer dasteht. */}
            <div
              className={
                data?.enabled
                  ? 'grid gap-4 md:gap-5 xl:grid-cols-2 xl:items-start'
                  : // Ohne freigeschalteten Zugang gibt es nur die Hilfe. Im
                    // Zweispalter bliebe die linke Haelfte leer und die Hilfe
                    // klebte rechts am Rand.
                    'space-y-4 md:space-y-5'
              }
            >
              {data?.enabled && (
              <div className="space-y-4 md:space-y-5">
                <Rise className="panel-card space-y-4 rounded-2xl p-6">
                  <div>
                    <h2 className="text-lg font-bold text-white">OBS einrichten</h2>
                    <p className="mt-1 text-sm text-text-secondary">
                      Vier Schritte, einmal. Danach startest du wie immer über „Stream starten“.
                    </p>
                  </div>

                  {/* Die Felder heissen wie in OBS, nicht wie bei uns. Wer
                      "SRT-Adresse" liest und in OBS "Server" und
                      "Streamschluessel" vor sich hat, raet sonst, und der Key
                      landet im falschen Feld. Deshalb steht jeder Schritt
                      neben dem Wert, den er braucht, statt in einer Liste
                      darunter, die man beim Kopieren aus dem Blick verliert. */}
                  <ObsSchritt
                    nummer={1}
                    titel="Einstellungen, Stream, Dienst auf „Benutzerdefiniert“"
                  >
                    <p className="text-xs text-text-secondary">
                      In OBS oben rechts <Weg>Einstellungen</Weg> <Weg>Stream</Weg>. Beim Feld{' '}
                      <Feld>Dienst</Feld> von „Twitch“ auf <Feld>Benutzerdefiniert…</Feld> wechseln.
                      Erst danach tauchen die beiden Felder aus Schritt 2 und 3 auf.
                    </p>
                  </ObsSchritt>

                  <ObsSchritt nummer={2} titel="Serveradresse einfügen">
                    {/* srt_hint liefert das Relay immer als String (rs-relay,
                        srt_hint_fuer in src/api/user.rs). Leer ist es genau
                        dann, wenn kein ingest_key existiert. Dieser Block
                        haengt an data.enabled, trotzdem faengt der Guard den
                        Leerfall ab: ein leeres Kopierfeld waere die
                        schlechteste Antwort. */}
                    {data.srt_hint ? (
                      <>
                        <CopyField
                          label="gehört in das OBS-Feld „Server“"
                          value={data.srt_hint}
                          darfAufdecken={data.live_status === 'aus'}
                          grundVerdeckt={
                            data.live_status === 'live'
                              ? 'Du bist gerade live. Solange bleibt die Adresse verdeckt, damit sie nicht im Stream landet. Kopieren geht trotzdem.'
                              : 'Wir wissen gerade nicht sicher, ob du live bist. Solange bleibt die Adresse verdeckt. Kopieren geht trotzdem.'
                          }
                        />
                        {/* Die Adresse traegt den Ingest-Key als streamid. Wer
                            sie abliest, sendet auf denselben Kanal wie der
                            Streamer. Auf einem geteilten Bildschirm passiert
                            genau das, deshalb steht die Warnung direkt am Feld
                            und nicht in der Hilfe. */}
                        <p className="rounded-xl border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
                          <strong className="font-semibold">Zeig diese Adresse nicht im Stream.</strong>{' '}
                          Sie enthält deinen persönlichen Schlüssel. Wer sie sieht, kann auf deinem Kanal
                          senden. Blende das Dashboard aus, bevor du es teilst.
                        </p>
                      </>
                    ) : (
                      <p className="text-sm text-warning">
                        Der Relay hat gerade keine SRT-Adresse geliefert. Lade die Seite neu; bleibt es
                        dabei, meld dich beim Support.
                      </p>
                    )}
                  </ObsSchritt>

                  <ObsSchritt nummer={3} titel="Streamschlüssel leer lassen">
                    <div className="rounded-xl border border-dashed border-border bg-background/40 px-3 py-2 text-xs text-text-secondary">
                      Das OBS-Feld <Feld>Streamschlüssel</Feld> bleibt leer. Falls dort noch dein alter
                      Twitch-Schlüssel steht: markieren und löschen. Dein Schlüssel steckt schon in der
                      Adresse aus Schritt 2.
                    </div>
                  </ObsSchritt>

                  <ObsSchritt nummer={4} titel="Ausgabe einstellen">
                    <p className="text-xs text-text-secondary">
                      <Weg>Einstellungen</Weg> <Weg>Ausgabe</Weg>, Ausgabemodus auf <Feld>Erweitert</Feld>.
                      Diese vier Werte:
                    </p>
                    <dl className="divide-y divide-border/60 overflow-hidden rounded-xl border border-border">
                      {obsAusgabe(obsBitrate).map((zeile) => (
                        <div key={zeile.feld} className="flex items-baseline gap-3 px-3 py-2">
                          <dt className="w-32 shrink-0 text-[11px] font-semibold uppercase tracking-[0.12em] text-text-secondary">
                            {zeile.feld}
                          </dt>
                          <dd className="min-w-0 text-xs text-white">
                            <span className="font-semibold">{zeile.wert}</span>
                            <span className="block text-text-secondary">{zeile.warum}</span>
                          </dd>
                        </div>
                      ))}
                    </dl>
                  </ObsSchritt>

                  <p className="rounded-xl border border-primary/25 bg-primary/10 px-3 py-2 text-xs text-white">
                    Fertig. Ab jetzt drückst du in OBS <Feld>Stream starten</Feld> wie immer. Wir legen
                    die Plattform-Streams an, du musst nirgends sonst etwas starten.
                  </p>
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

              <div className="space-y-4 md:space-y-5">
                {data?.enabled && (
                <Rise className="panel-card space-y-3 rounded-2xl p-6">
                  <div>
                    <h2 className="text-lg font-bold text-white">Wohin wir senden</h2>
                    <p className="mt-1 text-sm text-text-secondary">
                      Für jede Plattform hinterlegst du Adresse, Stream-Schlüssel und die Qualität,
                      die wir dorthin schicken. Die Schlüssel liegen verschlüsselt bei uns.
                    </p>
                  </div>

                  {zieleFehler ? (
                    <p className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
                      Wir können deine gespeicherten Ziele gerade nicht abrufen. Das heißt nicht, dass sie
                      weg sind. Speichere nichts doppelt, lade die Seite in einer Minute neu.
                    </p>
                  ) : null}

                  {/* Bei einem Abrufausfall keine Karten: vier Stueck, die
                      alle "nicht eingerichtet" behaupten und einen Schluessel
                      verlangen, sind das Gegenteil der Warnung darueber. */}
                  <div className={zieleFehler ? 'hidden' : 'space-y-2'}>
                    {UPLINK_PLATTFORMEN.map((plattform) => {
                      const ziel = gespeicherteZiele.find((z) => z.platform === plattform.id);
                      return (
                        <ZielKarte
                          key={plattform.id}
                          platform={plattform.id}
                          label={plattform.label}
                          rtmpVorgabe={plattform.rtmp}
                          ziel={ziel}
                          caps={capsFuer(plattform.id)}
                          // Twitch offen, der Rest zu: fuer fast alle ist
                          // Twitch das einzige Ziel, und vier aufgeklappte
                          // Karten fuellen mehrere Bildschirmhoehen.
                          offenStart={plattform.id === 'twitch' || Boolean(ziel)}
                        />
                      );
                    })}
                  </div>

                  {gespeicherteZiele.length === 0 && !zieleFehler ? (
                    <p className="text-xs text-text-secondary">
                      Noch kein Ziel gespeichert. Ohne Ziel kommt dein Stream bei uns an, geht aber
                      nirgends hin.
                    </p>
                  ) : null}

                  {/* Die haeufigste Rueckfrage, und sie hat eine gute Antwort:
                      was reinkommt und was rausgeht sind zwei verschiedene
                      Dinge. Ohne diesen Absatz stellen Leute ihr OBS auf die
                      Zielbitrate herunter und verschenken Qualitaet. Das
                      Beispiel nennt dieselbe Zahl wie Schritt 4, sonst stehen
                      zwei Empfehlungen auf einer Seite. */}
                  <p className="rounded-xl border border-border bg-background/40 px-3 py-2 text-xs text-text-secondary">
                    <strong className="font-semibold text-white">
                      Was du sendest, ist nicht das, was rausgeht.
                    </strong>{' '}
                    Schick uns HEVC in 1440p mit VBR, so viel Bitrate, wie dein Upload sicher trägt.
                    Wir rechnen daraus für jedes Ziel neu, in H.264 und mit den Werten, die du hier
                    eingestellt hast: also zum Beispiel 1440p HEVC mit {obsBitrate.kbps} kbps rein
                    und H.264 zu Twitch raus. Mehr zu schicken hilft nur, solange deine Leitung es
                    trägt.
                  </p>
                </Rise>
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
      </div>
    </div>
  );
}
