import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
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
  UserPlus,
  Users,
} from 'lucide-react';
import { Rise } from '../motion/Rise';
import '../uplinkHelp.css';
import {
  acceptUplinkAdminWaitlistEntry,
  UPLINK_PLATTFORMEN,
  dockAdressen,
  fetchUplinkAdminWaitlist,
  fetchUplinkCaps,
  fetchUplinkDestinations,
  fetchUplinkMe,
  joinUplinkWaitlist,
  reconnectWaitEingabe,
  plattformVerbindungen,
  reconnectWaitPayload,
  rotateUplinkDockToken,
  saveUplinkReconnectWait,
  holeUplinkStreamKey,
  UPLINK_RECONNECT_WAIT_TEXT,
} from '@/api/uplink';
import type { UplinkAdminWaitlistEntry, UplinkMe } from '@/api/uplink';
import { useAuthStatus } from '@/hooks/useAnalytics';
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
import { amdSpitzeKbps, noetigerUploadMbit, obsBitrateEmpfehlung } from '@/uplinkEmpfehlung';
import { useUplinkDisclosure } from '@/uplinkDisclosure';
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
      aria-current={active ? 'page' : undefined}
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
 * Inhalt der Karte "Chat und OBS-Fenster".
 *
 * Vier Fenster, ein Zugang: Chat mit Antwortfeld, Aktivität, Stream-Infos und
 * Kanalpunkte, jeweils für alle verbundenen Plattformen zugleich. Die Adressen
 * stehen dauerhaft hier, nicht nur in der Sekunde nach dem Erzeugen; wer die
 * Seite neu lädt, findet dieselben vier Zeilen wieder.
 *
 * Verdeckt wie die Serveradresse in Schritt 2, mit derselben Komponente: in
 * jeder Adresse steckt der Zugang zum Chat, und wer sie mitliest, kann im Namen
 * des Streamers schreiben. Kopieren geht trotzdem, dafür muss niemand etwas
 * sehen.
 *
 * Die fertigen Twitch-Fenster sind hier weggefallen. Sie zeigten nur Twitch,
 * brauchten eine eigene Anmeldung im OBS-Browser und standen direkt neben vier
 * Fenstern, die dasselbe für alle Plattformen tun.
 */
function DockKarteInhalt({ me }: { me: UplinkMe }) {
  const [nachfrage, setNachfrage] = useState(false);
  const queryClient = useQueryClient();
  const erzeugen = useMutation({
    mutationFn: rotateUplinkDockToken,
    onSuccess: (antwort) => {
      // Die neuen Adressen gehen direkt in den Zwischenspeicher, aus dem die
      // Karte liest. Damit stehen sie sofort da, ohne zweite Quelle daneben:
      // ein eigener Zustand für "gerade erzeugt" wäre ein zweiter Stand, der
      // hängen bleiben kann, während der Server längst etwas anderes führt.
      // Der Abruf danach holt sich, was sonst noch am Nutzer hängt.
      if (antwort.dock_urls?.chat) {
        queryClient.setQueryData(['uplink-me'], (alt?: UplinkMe) =>
          alt ? { ...alt, dock_urls: antwort.dock_urls, dock_url_vorhanden: true } : alt
        );
      }
      setNachfrage(false);
      queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
    },
  });
  const adressen = dockAdressen(me);
  // "Vorhanden" und "anzeigbar" fallen auseinander: ein Zugang aus der Zeit
  // vor dem Umbau lässt sich nicht mehr anzeigen. Dann gilt trotzdem die
  // Rückfrage, denn ein Neuerzeugen entwertet auch diese Eintragungen in OBS.
  const vorhanden = Boolean(me.dock_url_vorhanden) || adressen.length > 0;
  const darfAufdecken = me.live_status === 'aus';
  const grundVerdeckt =
    me.live_status === 'live'
      ? 'Du bist gerade live. Solange bleiben die Adressen verdeckt, damit sie nicht im Stream landen. Kopieren geht trotzdem.'
      : 'Wir wissen gerade nicht sicher, ob du live bist. Solange bleiben die Adressen verdeckt. Kopieren geht trotzdem.';

  return (
    <div className="space-y-4 border-t border-border/60 px-5 py-4">
      <p className="text-sm text-text-secondary">
        In OBS unter <strong>Docks</strong>, <strong>Benutzerdefinierte Browser-Docks</strong>{' '}
        den Namen und die jeweilige Adresse eintragen.
      </p>

      <div data-section="eigenes-dock" className="space-y-3">
        <p className="text-xs text-text-secondary">
          Vier Fenster für alle verbundenen Plattformen zugleich: Chat mit Antwortfeld, Aktivität mit
          Follows, Abos und Bits, Stream-Infos zum Ändern von Titel und Kategorie, und die Kanalpunkte.
          Verbinden geht in der jeweiligen Plattform-Karte oben.
        </p>

        {adressen.length > 0 ? (
          <>
            {adressen.map((dock) => (
              <CopyField
                key={dock.titel}
                label={dock.titel}
                value={dock.url}
                darfAufdecken={darfAufdecken}
                grundVerdeckt={grundVerdeckt}
                grundAnzeigen={false}
              />
            ))}
            {/* Der Grund steht einmal unter allen vier Zeilen. Viermal
                derselbe Satz liest sich wie ein Fehler. */}
            {!darfAufdecken ? <p className="text-xs text-text-secondary">{grundVerdeckt}</p> : null}
            <div
              data-uplink-dock-warning
              role="note"
              className="rounded-xl border border-warning/45 bg-warning/10 px-3 py-2.5 text-xs text-warning shadow-[inset_3px_0_0_var(--color-warning)]"
            >
              <div className="flex items-start gap-2.5">
                <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-lg border border-warning/35 bg-warning/15">
                  <AlertTriangle aria-hidden="true" className="h-3.5 w-3.5" />
                </span>
                <p className="pt-0.5 leading-relaxed">
                  <strong className="font-semibold text-white">Privat:</strong> Wer eine dieser Adressen
                  hat, kann in deinem Namen im Chat schreiben. Nicht im Stream zeigen.
                </p>
              </div>
            </div>
          </>
        ) : (
          <p className="text-xs text-text-secondary">
            {vorhanden
              ? 'Deine Fenster laufen weiter, ihre Adressen lassen sich hier aber nicht mehr anzeigen. Erzeuge sie einmal neu, dann stehen sie dauerhaft hier.'
              : 'Noch keine Adressen erzeugt.'}
          </p>
        )}

        {nachfrage ? (
          <div className="flex flex-col gap-2 rounded-xl border border-warning/40 bg-warning/5 px-3 py-2">
            <p className="text-xs text-warning">
              Die vier Adressen, die jetzt in OBS stehen, gelten danach nicht mehr. Du musst sie dort
              alle vier neu eintragen.
            </p>
            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                disabled={erzeugen.isPending}
                onClick={() => erzeugen.mutate()}
                className="inline-flex min-h-11 items-center gap-2 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-white transition-colors hover:border-primary/50 disabled:opacity-60"
              >
                {erzeugen.isPending ? (
                  <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin" />
                ) : null}
                {erzeugen.isPending ? 'Wird erzeugt' : 'Ja, neu erzeugen'}
              </button>
              <button
                type="button"
                onClick={() => setNachfrage(false)}
                className="inline-flex min-h-11 items-center rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary transition-colors hover:text-white"
              >
                Abbrechen
              </button>
            </div>
          </div>
        ) : (
          <button
            type="button"
            onClick={() => (vorhanden ? setNachfrage(true) : erzeugen.mutate())}
            disabled={erzeugen.isPending}
            className="inline-flex min-h-11 items-center gap-2 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-white transition-colors hover:border-primary/50 disabled:opacity-60"
          >
            {erzeugen.isPending ? (
              <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin" />
            ) : null}
            {vorhanden ? 'Neu erzeugen' : 'Adressen erzeugen'}
          </button>
        )}

        {erzeugen.isError ? (
          <p role="alert" className="text-xs text-warning">
            Die Adressen konnten gerade nicht erzeugt werden. Bitte gleich noch einmal versuchen.
          </p>
        ) : null}
      </div>
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
  offenStart = false,
  children,
}: {
  nummer: number;
  titel: string;
  offenStart?: boolean;
  children: React.ReactNode;
}) {
  const [offen, setOffen] = useUplinkDisclosure(`obs-${nummer}`, offenStart);

  return (
    <li>
      <details
        data-obs-step={nummer}
        open={offen}
        onToggle={(ereignis) => setOffen(ereignis.currentTarget.open)}
        className="group overflow-hidden rounded-xl border border-border/70 bg-background/45 transition-colors open:border-primary/35 open:bg-primary/5"
      >
        <summary className="flex min-h-12 cursor-pointer list-none items-center justify-between gap-3 px-3 py-2.5 text-left [&::-webkit-details-marker]:hidden">
          <span className="flex min-w-0 items-center gap-3">
            <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-primary/40 bg-primary/10 text-xs font-bold text-primary">
              {nummer}
            </span>
            <span>
              <span className="block text-[10px] font-semibold uppercase tracking-[0.16em] text-text-secondary">
                Schritt {nummer} von 4
              </span>
              <span className="block text-sm font-bold text-white">{titel}</span>
            </span>
          </span>
          <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open:rotate-180" />
        </summary>
        <div className="space-y-2 border-t border-border/60 px-4 py-3 pl-13">{children}</div>
      </details>
    </li>
  );
}

function HilfeKapitel({ datei, label, html }: { datei: string; label: string; html: string }) {
  const [offen, setOffen] = useUplinkDisclosure(`hilfe-${datei}`, false);

  return (
    <details
      open={offen}
      onToggle={(ereignis) => setOffen(ereignis.currentTarget.open)}
      className="uplink-help-shell group/chapter overflow-hidden rounded-xl border border-border bg-background/70"
    >
      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-4 py-3 text-sm font-semibold text-white transition-colors hover:bg-white/5 [&::-webkit-details-marker]:hidden">
        <span>{label}</span>
        <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open/chapter:rotate-180" />
      </summary>
      <div dangerouslySetInnerHTML={{ __html: html }} />
    </details>
  );
}

/**
 * Warum diese Bitrate. Ein Satz je Zustand, danach fuer alle derselbe
 * Hinweis auf den Upload.
 *
 * Der Fall `unbekannt` ist der Grund, warum das hier drei Zweige sind und
 * nicht zwei: der Abruf der Ziele laeuft ohne Wiederholung, und faellt er
 * aus, stand hier vorher "solange kein Ziel eingerichtet ist" bei jemandem
 * mit vier eingerichteten Zielen.
 */
function bitrateBegruendung(bitrate: ObsBitrateEmpfehlung): string {
  const anfang =
    bitrate.herkunft === 'unbekannt'
      ? 'Deine Ziele konnten wir gerade nicht laden, deshalb steht hier der Standardwert. Ob er zu dem passt, was du eingerichtet hast, können wir im Moment nicht sagen. '
      : bitrate.herkunft === 'start'
        ? 'Startwert, solange kein Ziel eingerichtet ist. Sobald deine Ziele stehen, passt sich die Zahl an. '
        : bitrate.hoehe !== null && bitrate.hoehe > 1080
          ? `Du schickst 2K weiter, dein höchstes Ziel steht auf ${bitrate.hoehe}p. `
          : `Passt zu deinen Zielen: dein höchstes geht mit ${bitrate.hoehe}p raus. `;
  return (
    anfang +
    `Die Grenze ist dein Upload, und den kennen wir nicht: dafür brauchst du gemessene ${noetigerUploadMbit(bitrate)} Mbit, bei einer AMD-Karte ${noetigerUploadMbit(bitrate, true)} Mbit. Miss ihn, und wenn er darunter liegt, geh eine Stufe runter. Mehr als hier steht brauchst du nicht, weil du HEVC schickst und wir daraus für jede Plattform H.264 rechnen.`
  );
}

/**
 * Was im Bitraten-Feld steht, samt der Spitze, die AMD daraus macht.
 *
 * Die Maximalbitrate ist bei "AMD HW H.264/H.265/AV1" kein Feld: OBS setzt sie
 * dort selbst auf das Anderthalbfache. Wer das nicht weiss, haelt die zweite
 * Zahl fuer eine Grenze und plant seine Leitung ein Drittel zu knapp. Deshalb
 * steht die echte Spitze hier und nicht in einer Fussnote.
 */
function bitrateWert(bitrate: ObsBitrateEmpfehlung): string {
  return `${bitrate.kbps} kbps, Maximum ${bitrate.maxKbps} kbps`;
}

function bitrateAmdHinweis(bitrate: ObsBitrateEmpfehlung): string {
  return `Bei AMD gibt es das Feld „Maximalbitrate“ nicht. Trag dort nur die ${bitrate.kbps} ein, OBS macht daraus von selbst eine Spitze von ${amdSpitzeKbps(bitrate)} kbps. Wenn deine Leitung das nicht trägt, nimm stattdessen CBR: dann ist die eingetragene Zahl auch die Obergrenze.`;
}

/**
 * Die Ausgabe-Einstellungen, jede mit dem Grund dahinter.
 *
 * Ohne den Grund stellt niemand etwas um, was schon laeuft. HEVC ist der
 * Punkt, an dem Uplink sich lohnt, und VBR ist genau die Einstellung, die man
 * bei Twitch direkt nicht setzen darf und hier setzen soll.
 *
 * Die Bitrate ist keine feste Zahl mehr, sondern die Stufe aus der
 * eingebetteten Hilfeseite, die zu den eingestellten Zielen passt: siehe
 * `obsBitrateEmpfehlung`.
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
      wert: bitrateWert(bitrate),
      warum: `${bitrateBegruendung(bitrate)} ${bitrateAmdHinweis(bitrate)}`,
    },
    {
      feld: 'Keyframe-Intervall',
      wert: '2 s',
      warum: 'Feste 2 Sekunden. Bei „automatisch“ setzen manche Encoder gar keine, und dann startet kein Zuschauer sauber ein.',
    },
  ];
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
 *
 * `grundAnzeigen` schaltet nur den Satz unter dem Feld ab, nicht die Sperre und
 * nicht den Hinweis am Knopf. Gedacht fuer die Dock-Karte: dort stehen vier
 * Felder untereinander, und viermal derselbe Satz liest sich wie ein Fehler.
 * Der Grund steht dann einmal darunter.
 */
function CopyField({
  label,
  value,
  darfAufdecken,
  grundVerdeckt,
  grundAnzeigen = true,
}: {
  label: string;
  value: string;
  darfAufdecken: boolean;
  grundVerdeckt: string;
  grundAnzeigen?: boolean;
}) {
  const [stand, setStand] = useState<'ruhe' | 'ok' | 'fehler'>('ruhe');
  const [offen, setOffen] = useState(false);
  const feldRef = useRef<HTMLInputElement>(null);

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
      feldRef.current?.focus();
      feldRef.current?.select();
    }
    window.setTimeout(() => setStand('ruhe'), 2000);
  };

  const knopfText = stand === 'ok' ? 'Kopiert' : stand === 'fehler' ? 'Ging nicht' : 'Kopieren';
  return (
    <div className="space-y-1">
      <div className="text-[11px] font-semibold uppercase tracking-[0.16em] text-text-secondary">
        {label}
      </div>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <input
          ref={feldRef}
          readOnly
          type={offen ? 'text' : 'password'}
          value={value}
          aria-label={`${label}: ${offen ? value : 'verdeckt'}`}
          className="flex min-h-11 min-w-0 flex-1 items-center truncate rounded-xl border border-border bg-background/70 px-3 py-2 font-mono text-xs text-white"
        />
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => setOffen((vorher) => !vorher)}
            disabled={!darfAufdecken}
            aria-expanded={offen}
            title={darfAufdecken ? undefined : grundVerdeckt}
            className="inline-flex min-h-11 shrink-0 items-center gap-1 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary transition-colors hover:text-white disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:text-text-secondary"
          >
            {offen ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
            {offen ? 'Verdecken' : 'Zeigen'}
          </button>
          <button
            type="button"
            aria-label={`${label} kopieren`}
            className="inline-flex min-h-11 shrink-0 items-center gap-1 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary transition-colors hover:border-primary/40 hover:text-white"
            onClick={kopieren}
          >
            <Copy className="h-3.5 w-3.5" />
            {knopfText}
          </button>
        </div>
      </div>
      {/* aria-live, weil die Rueckmeldung sonst nur im Knopftext steht und von
          einem Screenreader nicht angesagt wird. */}
      <p aria-live="polite" className="sr-only">
        {stand === 'ok' ? 'In die Zwischenablage kopiert' : ''}
        {stand === 'fehler' ? 'Kopieren hat nicht geklappt' : ''}
      </p>
      {stand === 'fehler' && (
        <p className="text-xs text-warning">
          Dein Browser hat das automatische Kopieren nicht erlaubt. Das Feld ist markiert; kopier es mit Strg+C.
        </p>
      )}
      {!darfAufdecken && grundAnzeigen && (
        <p className="text-xs text-text-secondary">{grundVerdeckt}</p>
      )}
    </div>
  );
}

function ReconnectWaitKarte({
  wert,
  max,
  onSaved,
}: {
  wert: number;
  max: number;
  onSaved: () => void;
}) {
  const [entwurf, setEntwurf] = useState<string | null>(null);
  const eingabe = entwurf ?? reconnectWaitEingabe(wert);
  const payload = reconnectWaitPayload(eingabe);
  const speichern = useMutation({
    mutationFn: () => {
      if (payload === null) {
        throw new Error('Gib eine ganze Zahl ab 0 Sekunden ein.');
      }
      return saveUplinkReconnectWait(payload);
    },
    onSuccess: (antwort) => {
      setEntwurf(reconnectWaitEingabe(antwort.reconnect_wait_s));
      onSaved();
    },
  });

  return (
    <Rise className="panel-card grid gap-4 rounded-2xl p-5 md:grid-cols-[minmax(0,1fr)_minmax(18rem,0.7fr)] md:items-end">
      <div className="min-w-0">
        <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">
          Verbindung
        </div>
        <h2 className="mt-1 text-base font-bold text-white">Verhalten bei Internetabriss</h2>
        <p className="mt-1 text-sm text-text-secondary">{UPLINK_RECONNECT_WAIT_TEXT}</p>
        <p className="mt-2 text-xs text-text-secondary">
          0 bis {max} Sekunden. Die Änderung gilt für die nächste Session.
        </p>
      </div>
      <div className="space-y-2">
        <div className="flex flex-wrap items-end gap-2">
          <label className="min-w-[11rem] flex-1 space-y-1">
          <span className="text-[11px] font-semibold uppercase tracking-[0.16em] text-text-secondary">
            Wartezeit in Sekunden
          </span>
          <input
            type="number"
            min={0}
            max={max}
            step={1}
            value={eingabe}
            onChange={(e) => setEntwurf(e.target.value)}
            inputMode="numeric"
            className="min-h-11 w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white outline-none transition-colors focus:border-primary"
            aria-label="Wartezeit nach Internetabriss in Sekunden"
          />
          </label>
          <button
            type="button"
            disabled={speichern.isPending || payload === null}
            onClick={() => speichern.mutate()}
            className="min-h-11 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:cursor-not-allowed disabled:opacity-60"
          >
            {speichern.isPending ? 'Speichert …' : speichern.isSuccess ? 'Gespeichert' : 'Speichern'}
          </button>
        </div>
        {speichern.isError && (
          <p role="alert" className="text-xs text-warning">
          {speichern.error instanceof Error
            ? speichern.error.message
            : 'Die Wartezeit ließ sich gerade nicht speichern.'}
          </p>
        )}
      </div>
    </Rise>
  );
}

function wartelistenZeit(roh: string): string {
  const datum = new Date(roh);
  if (Number.isNaN(datum.getTime())) return roh;
  return new Intl.DateTimeFormat('de-DE', {
    dateStyle: 'short',
    timeStyle: 'short',
  }).format(datum);
}

function AdminUplinkWarteliste({ csrfToken }: { csrfToken: string | null }) {
  const queryClient = useQueryClient();
  const warteliste = useQuery({
    queryKey: ['uplink-admin-waitlist'],
    queryFn: fetchUplinkAdminWaitlist,
    retry: false,
    refetchOnWindowFocus: true,
  });
  const freischalten = useMutation({
    mutationFn: (streamerId: number) => {
      if (!csrfToken) {
        throw new Error('Der Sitzungsschutz fehlt. Lade die Seite neu.');
      }
      return acceptUplinkAdminWaitlistEntry(streamerId, csrfToken);
    },
    onSuccess: (_antwort, streamerId) => {
      queryClient.setQueryData<{ entries: UplinkAdminWaitlistEntry[] }>(
        ['uplink-admin-waitlist'],
        (alt) => ({
          entries: (alt?.entries ?? []).filter((eintrag) => eintrag.streamer_id !== streamerId),
        }),
      );
      queryClient.invalidateQueries({ queryKey: ['uplink-admin-waitlist'] });
    },
  });
  const eintraege = warteliste.data?.entries ?? [];

  return (
    <Rise
      data-section="uplink-admin-waitlist"
      className="panel-card card-glow card-glow-warning space-y-4 rounded-2xl border-warning/25 p-4 md:p-5"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-3">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-warning/35 bg-warning/10 text-warning">
            <Users aria-hidden="true" className="h-4 w-4" />
          </span>
          <div className="min-w-0">
            <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-warning">
              Admin-Modus
            </div>
            <h2 className="text-base font-bold text-white">Uplink-Warteliste</h2>
            <p className="mt-1 text-xs text-text-secondary">
              Freischalten gibt dem Streamer einen Uplink-Zugang und entfernt den Eintrag aus der Liste.
            </p>
          </div>
        </div>
        <span className="rounded-full border border-warning/30 bg-warning/10 px-2.5 py-1 text-xs font-semibold text-warning">
          {warteliste.isLoading ? '…' : eintraege.length}
        </span>
      </div>

      {warteliste.isLoading ? (
        <p role="status" className="rounded-xl border border-border bg-background/50 px-3 py-2 text-xs text-text-secondary">
          Warteliste wird geladen …
        </p>
      ) : null}

      {warteliste.isError ? (
        <p role="alert" className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
          Die Warteliste ist gerade nicht erreichbar.
        </p>
      ) : null}

      {!csrfToken ? (
        <p role="alert" className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
          Der Sitzungsschutz fehlt. Lade die Seite neu, bevor du jemanden freischaltest.
        </p>
      ) : null}

      {!warteliste.isLoading && !warteliste.isError && eintraege.length === 0 ? (
        <p className="rounded-xl border border-border bg-background/45 px-3 py-3 text-xs text-text-secondary">
          Gerade wartet niemand auf eine Freischaltung.
        </p>
      ) : null}

      {eintraege.length > 0 ? (
        <ul className="max-h-72 space-y-2 overflow-y-auto pr-1" aria-label="Uplink-Warteliste">
          {eintraege.map((eintrag) => {
            const wirdFreigeschaltet =
              freischalten.isPending && freischalten.variables === eintrag.streamer_id;
            return (
              <li
                key={eintrag.streamer_id}
                className="flex flex-col gap-3 rounded-xl border border-border bg-background/55 p-3 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="min-w-0">
                  <p className="truncate text-sm font-semibold text-white">Twitch-ID {eintrag.streamer_id}</p>
                  <p className="mt-0.5 text-[11px] text-text-secondary">
                    Anfrage {wartelistenZeit(eintrag.requested_at)}
                  </p>
                  {eintrag.note ? <p className="mt-1 text-xs text-text-secondary">{eintrag.note}</p> : null}
                </div>
                <button
                  type="button"
                  disabled={freischalten.isPending || !csrfToken}
                  onClick={() => freischalten.mutate(eintrag.streamer_id)}
                  className="inline-flex min-h-10 shrink-0 items-center justify-center gap-2 rounded-xl bg-primary px-3 py-2 text-xs font-semibold text-[#0D0806] disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <UserPlus aria-hidden="true" className="h-3.5 w-3.5" />
                  {wirdFreigeschaltet ? 'Wird freigeschaltet …' : 'Freischalten'}
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}

      {freischalten.isError ? (
        <p role="alert" className="text-xs text-warning">
          {freischalten.error instanceof Error
            ? freischalten.error.message
            : 'Die Freischaltung hat nicht geklappt.'}
        </p>
      ) : null}
    </Rise>
  );
}

/**
 * Nach der Rueckkehr aus dem Twitch-Dialog (`?verbunden=twitch`) den Stream-Key
 * holen und die Seite auffrischen.
 *
 * Der Server versucht dasselbe schon im Hintergrund, sobald der Callback durch
 * ist, damit es auch bei geschlossenem Tab passiert. Dieser zweite Anlauf ist
 * der sichtbare: er laeuft mit der Session des Streamers, und wenn er
 * scheitert, sieht der Streamer den Grund statt eines leeren Ziels. Beide
 * schreiben denselben Wert, ein doppelter Lauf richtet also nichts an.
 *
 * Zwei getrennte Effekte, und das ist der Kern:
 *
 * 1. Der Merker wird sofort und genau einmal aus der Adresse genommen, damit
 *    ein Neuladen nicht noch einmal losrennt.
 * 2. Der Abruf wartet, bis der Anmeldestand geladen ist, und laeuft dann genau
 *    einmal.
 *
 * Zusammengelegt in einem Effekt mit `csrfToken` in den Abhaengigkeiten hat
 * genau das nicht funktioniert: der erste Durchlauf feuerte ohne Token, der
 * nachladende Token liess den Effekt erneut laufen, das Aufraeumen brach die
 * Rueckmeldung des ersten Laufs ab, und der Merker war aus der Adresse schon
 * verschwunden. Erfolg wie Fehlschlag blieben unsichtbar.
 */
function useRueckkehrVomVerbinden(
  queryClient: ReturnType<typeof useQueryClient>,
  csrfToken: string | null,
  authGeladen: boolean
) {
  const [meldung, setMeldung] = useState<string | null>(null);
  const [ausstehend, setAusstehend] = useState(false);
  const gestartet = useRef(false);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    if (params.get('verbunden') !== 'twitch') return;
    params.delete('verbunden');
    const rest = params.toString();
    window.history.replaceState(
      null,
      '',
      window.location.pathname + (rest ? `?${rest}` : '')
    );
    setAusstehend(true);
  }, []);

  useEffect(() => {
    if (!ausstehend || !authGeladen || gestartet.current) return;
    gestartet.current = true;
    void holeUplinkStreamKey('twitch', csrfToken ?? '')
      .then(() => {
        setAusstehend(false);
        queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
        queryClient.invalidateQueries({ queryKey: ['uplink-destinations'] });
      })
      .catch(() => {
        setAusstehend(false);
        setMeldung(
          'Die Verbindung steht. Nur dein Stream-Key kam noch nicht durch: hol ihn unten in der Twitch-Karte noch einmal oder trag ihn von Hand ein.'
        );
      });
  }, [ausstehend, authGeladen, csrfToken, queryClient]);

  return meldung;
}

export function UplinkPage() {
  const queryClient = useQueryClient();
  const { data: authStatus, isLoading: authLaedt } = useAuthStatus();
  const [qualitaetOffen, setQualitaetOffen] = useUplinkDisclosure('qualitaet-erklaerung', false);
  const [docksOffen, setDocksOffen] = useUplinkDisclosure('obs-docks', false);
  const [hilfeOffen, setHilfeOffen] = useUplinkDisclosure('uplink-hilfe', false);
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
  const { data: ziele, isError: zieleFehler, isLoading: zieleLaden } = useQuery({
    queryKey: ['uplink-destinations'],
    queryFn: fetchUplinkDestinations,
    retry: false,
  });
  // Das Relay antwortet auf einen leeren Erfolg auch mal mit `{}`. Ohne die
  // Absicherung wirft `.length` beim Rendern, und die ErrorBoundary ersetzt
  // dann das ganze Dashboard, also auch die SRT-Adresse, die der Streamer
  // gerade braucht.
  const gespeicherteZiele = ziele?.destinations ?? [];
  // Zugangsstand je Plattform, steht im Kopf der Plattform-Karte.
  const chatVerbindungen = data ? plattformVerbindungen(data) : [];
  const streamKeyMeldung = useRueckkehrVomVerbinden(
    queryClient,
    authStatus?.csrfToken ?? authStatus?.csrf_token ?? null,
    !authLaedt
  );
  // Die OBS-Bitrate folgt dem, was der Streamer als Ziele eingestellt hat.
  // Eine feste Zahl in der Anleitung war beides: zu hoch fuer jede normale
  // Leitung und ohne Bezug zu dem, was hier tatsaechlich rausgeht.
  //
  // `zieleFehler` muss mit: ohne das Flag ist ein fehlgeschlagener Abruf von
  // einem leeren Konto nicht zu unterscheiden, und der Text behauptet dann
  // "kein Ziel eingerichtet" bei jemandem, der Ziele hat.
  const obsBitrate = obsBitrateEmpfehlung(gespeicherteZiele, zieleFehler);
  const waitlist = useMutation({
    mutationFn: joinUplinkWaitlist,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['uplink-me'] }),
  });
  // Die Empfehlungen kommen vom Server, damit die Oberflaeche sie nicht
  // doppelt pflegt: `relay.platform_caps` ist eine Tabelle in einem anderen
  // Repo. Faellt der Abruf aus, bleibt `caps` undefiniert, und die Zielkarte
  // schreibt an die Felder gar keine Empfehlung plus einen Satz, warum. Ein
  // Ersatzwert stuende hier falsch: es sind Empfehlungen und keine Grenzen,
  // und eine erfundene Empfehlung ist schlechter als keine.
  const { data: caps } = useQuery({
    queryKey: ['uplink-caps'],
    queryFn: fetchUplinkCaps,
    staleTime: 5 * 60_000,
    retry: false,
  });
  const capsFuer = (platform: string) => caps?.platforms.find((c) => c.platform === platform);
  const streamStatus =
    data?.live_status === 'live'
      ? { text: 'Stream live', klasse: 'border-success/35 bg-success/10 text-success' }
      : data?.live_status === 'aus'
        ? { text: 'Stream offline', klasse: 'border-border bg-background/60 text-text-secondary' }
        : { text: isLoading ? 'Streamstatus lädt' : 'Streamstatus unbekannt', klasse: 'border-warning/30 bg-warning/10 text-warning' };

  return (
    <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
      <div className="relative mx-auto max-w-[1800px]">
        <div className="grid gap-4 md:gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
          <Rise as="aside" className="panel-card card-glow self-start rounded-2xl p-4 lg:sticky lg:top-4">
            <div className="space-y-4">
              <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                Main
              </div>
              <nav aria-label="Dashboard" className="lg:space-y-1">
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

          <main id="uplink-main" className="space-y-4">
            <Rise className="panel-card rounded-2xl p-5 md:p-6">
              <div className="mb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-primary">
                Eigenes Modul
              </div>
              <div className="flex flex-wrap items-end justify-between gap-3">
                <div>
                  <h1 className="display-font text-2xl font-extrabold text-white">Uplink</h1>
                  <p className="mt-2 max-w-2xl text-sm text-text-secondary">
                    Ein Stream zu uns, passend verteilt an deine Plattformen. Start und Stop bleiben in OBS.
                  </p>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <span
                    role="status"
                    aria-live="polite"
                    className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-semibold ${streamStatus.klasse}`}
                  >
                    <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-current" />
                    {streamStatus.text}
                  </span>
                  {data?.enabled ? (
                    <span className="rounded-full border border-primary/30 bg-primary/10 px-3 py-1 text-xs font-semibold text-primary">
                      Zugang aktiv
                    </span>
                  ) : null}
                </div>
              </div>
            </Rise>

            {isLoading && (
              <div className="panel-card flex items-center gap-2 rounded-2xl p-5 text-sm text-text-secondary">
                <Loader2 aria-hidden="true" className="h-4 w-4 animate-spin" />
                Zugang wird geladen
              </div>
            )}

            {isError && (
              <div role="alert" className="panel-card rounded-2xl p-5 text-sm text-warning">
                {error instanceof Error ? error.message : 'Uplink ist gerade nicht erreichbar.'}
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
                    <a
                      href={PREVIEW_PRICING_ROUTE}
                      className="inline-flex min-h-11 items-center rounded-xl border border-border px-4 py-2 text-sm font-semibold text-white no-underline"
                    >
                      Preise ansehen
                    </a>
                    <button
                      type="button"
                      disabled={waitlist.isPending || data.waitlisted}
                      onClick={() => waitlist.mutate()}
                      className="min-h-11 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
                    >
                      {data.waitlisted ? 'Stehst auf der Warteliste' : 'Auf die Warteliste'}
                    </button>
                  </div>
                </div>
              </div>
            )}

            {data?.enabled && (
              <div className="grid items-start gap-4 md:gap-5 xl:grid-cols-[minmax(0,0.92fr)_minmax(0,1.08fr)]">
                <Rise className="panel-card space-y-4 rounded-2xl p-4 md:p-6">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div>
                      <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">
                        Einmalig
                      </div>
                      <h2 className="text-lg font-bold text-white">OBS einrichten</h2>
                      <p className="mt-1 text-sm text-text-secondary">
                        Vier kurze Schritte. Die Serveradresse ist direkt in Schritt 2.
                      </p>
                    </div>
                    <span className="rounded-full border border-border bg-background/60 px-3 py-1 text-xs font-semibold text-text-secondary">
                      4 Schritte
                    </span>
                  </div>

                  <ol aria-label="OBS einrichten" className="space-y-2">
                    <ObsSchritt nummer={1} titel="„Benutzerdefiniert“ wählen">
                      <p className="text-xs text-text-secondary">
                        In OBS <Weg>Einstellungen</Weg> <Weg>Stream</Weg>. Beim Feld <Feld>Dienst</Feld>{' '}
                        von „Twitch“ auf <Feld>Benutzerdefiniert…</Feld> wechseln.
                      </p>
                    </ObsSchritt>

                    <ObsSchritt nummer={2} titel="Serveradresse einfügen" offenStart>
                      {data.srt_hint ? (
                        <>
                          <CopyField
                            label="Serveradresse für OBS"
                            value={data.srt_hint}
                            darfAufdecken={data.live_status === 'aus'}
                            grundVerdeckt={
                              data.live_status === 'live'
                                ? 'Du bist gerade live. Solange bleibt die Adresse verdeckt, damit sie nicht im Stream landet. Kopieren geht trotzdem.'
                                : 'Wir wissen gerade nicht sicher, ob du live bist. Solange bleibt die Adresse verdeckt. Kopieren geht trotzdem.'
                            }
                          />
                          <div
                            data-uplink-private-warning
                            role="note"
                            className="rounded-xl border border-warning/45 bg-warning/10 px-3 py-2.5 text-xs text-warning shadow-[inset_3px_0_0_var(--color-warning)]"
                          >
                            <div className="flex items-start gap-2.5">
                              <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-lg border border-warning/35 bg-warning/15">
                                <AlertTriangle aria-hidden="true" className="h-3.5 w-3.5" />
                              </span>
                              <p className="pt-0.5 leading-relaxed">
                                <strong className="font-semibold text-white">Privat:</strong> Diese Adresse enthält
                                deinen Schlüssel. Nicht im Stream zeigen.
                              </p>
                            </div>
                          </div>
                        </>
                      ) : (
                        <p role="alert" className="text-sm text-warning">
                          Der Relay hat gerade keine SRT-Adresse geliefert. Lade die Seite neu; bleibt es dabei,
                          meld dich beim Support.
                        </p>
                      )}
                    </ObsSchritt>

                    <ObsSchritt nummer={3} titel="Streamschlüssel leer lassen">
                      <p className="text-xs text-text-secondary">
                        Das OBS-Feld <Feld>Streamschlüssel</Feld> bleibt leer. Einen alten Twitch-Schlüssel dort
                        löschen; dein Schlüssel steckt bereits in der Serveradresse.
                      </p>
                    </ObsSchritt>

                    <ObsSchritt nummer={4} titel="Ausgabe einstellen">
                      <p className="text-xs text-text-secondary">
                        <Weg>Einstellungen</Weg> <Weg>Ausgabe</Weg>, Ausgabemodus auf <Feld>Erweitert</Feld>.
                      </p>
                      <dl className="divide-y divide-border/60 overflow-hidden rounded-xl border border-border">
                        {obsAusgabe(obsBitrate).map((zeile) => (
                          <div key={zeile.feld} className="grid gap-1 px-3 py-2 sm:grid-cols-[8rem_minmax(0,1fr)]">
                            <dt className="text-[11px] font-semibold uppercase tracking-[0.12em] text-text-secondary">
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
                  </ol>

                  <p className="rounded-xl border border-primary/25 bg-primary/10 px-3 py-2 text-xs text-white">
                    Danach startest du in OBS wie immer über <Feld>Stream starten</Feld>. Den Rest übernimmt Uplink.
                  </p>
                </Rise>

                <div data-section="uplink-right-column" className="space-y-4 md:space-y-5">
                  <Rise className="panel-card card-glow space-y-5 rounded-2xl p-4 md:p-6">
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div>
                        <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">
                          Hauptbereich
                        </div>
                        <h2 className="text-lg font-bold text-white">Plattformen</h2>
                        <p className="mt-1 text-sm text-text-secondary">
                          Status und Qualität stehen im Kartenkopf. Zum Ändern die Karte öffnen.
                        </p>
                      </div>
                      <span
                        className={`rounded-full border px-3 py-1 text-xs font-semibold ${zieleLaden || zieleFehler ? 'border-border bg-background/60 text-text-secondary' : 'border-success/30 bg-success/10 text-success'}`}
                      >
                        {zieleLaden
                          ? 'Ziele werden geladen'
                          : zieleFehler
                            ? 'Status unbekannt'
                            : `${gespeicherteZiele.filter((ziel) => ziel.enabled).length} aktiv`}
                      </span>
                    </div>

                    {zieleLaden ? (
                      <p
                        role="status"
                        className="rounded-xl border border-border bg-background/50 px-3 py-2 text-xs text-text-secondary"
                      >
                        Plattformziele werden geladen …
                      </p>
                    ) : null}

                    {zieleFehler ? (
                      <p
                        role="alert"
                        className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning"
                      >
                        Deine gespeicherten Ziele sind gerade nicht abrufbar. Sie sind nicht weg; bitte nichts
                        doppelt speichern und die Seite später neu laden.
                      </p>
                    ) : null}

                    {streamKeyMeldung ? (
                      <p role="alert" className="text-xs text-warning">
                        {streamKeyMeldung}
                      </p>
                    ) : null}

                    <div className={zieleFehler || zieleLaden ? 'hidden' : 'space-y-3'}>
                      {UPLINK_PLATTFORMEN.map((plattform) => {
                        const ziel = gespeicherteZiele.find(
                          (eintrag) => eintrag.platform === plattform.id,
                        );
                        return (
                          <ZielKarte
                            key={plattform.id}
                            platform={plattform.id}
                            label={plattform.label}
                            rtmpVorgabe={plattform.rtmp}
                            ziel={ziel}
                            caps={capsFuer(plattform.id)}
                            chat={chatVerbindungen.find((v) => v.id === plattform.id)}
                            csrfToken={authStatus?.csrfToken ?? authStatus?.csrf_token ?? null}
                            offenStart={false}
                          />
                        );
                      })}
                    </div>

                    {gespeicherteZiele.length === 0 && !zieleFehler && !zieleLaden ? (
                      <p className="text-xs text-text-secondary">
                        Noch kein Ziel gespeichert. Dein Stream kommt bei Uplink an, wird aber noch nicht
                        weitergesendet.
                      </p>
                    ) : null}

                    <details
                      open={qualitaetOffen}
                      onToggle={(ereignis) => setQualitaetOffen(ereignis.currentTarget.open)}
                      className="group rounded-xl border border-border bg-background/40"
                    >
                      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-3 py-2.5 text-xs font-semibold text-white [&::-webkit-details-marker]:hidden">
                        Warum Eingangs- und Zielqualität verschieden sind
                        <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open:rotate-180" />
                      </summary>
                      <p className="border-t border-border/60 px-3 py-3 text-xs text-text-secondary">
                        Schick uns HEVC mit den Werten aus Schritt 4. Uplink rechnet daraus für jedes Ziel H.264
                        mit genau den Werten, die du in der Plattformkarte speicherst.
                      </p>
                    </details>
                  </Rise>

                  {authStatus?.adminMode ? (
                    <AdminUplinkWarteliste
                      csrfToken={authStatus.csrfToken ?? authStatus.csrf_token ?? null}
                    />
                  ) : null}
                </div>
              </div>
            )}

            {data?.enabled && (
              <ReconnectWaitKarte
                wert={data.reconnect_wait_s}
                max={data.reconnect_wait_max_s}
                onSaved={() => queryClient.invalidateQueries({ queryKey: ['uplink-me'] })}
              />
            )}

            <div className={`grid gap-4 md:gap-5 ${data?.enabled ? 'xl:grid-cols-2' : ''}`}>
              {data?.enabled && (
                <details
                  data-section="obs-docks"
                  open={docksOffen}
                  onToggle={(ereignis) => setDocksOffen(ereignis.currentTarget.open)}
                  className="panel-card group self-start rounded-2xl"
                >
                  <summary className="flex min-h-16 cursor-pointer list-none items-center justify-between gap-4 px-5 py-4 [&::-webkit-details-marker]:hidden">
                    <span>
                      <span className="block text-base font-bold text-white">Chat und OBS-Fenster</span>
                      <span className="mt-0.5 block text-xs text-text-secondary">Vier Fenster für alle Plattformen</span>
                    </span>
                    <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open:rotate-180" />
                  </summary>
                  <DockKarteInhalt me={data} />
                </details>
              )}

              <details
                data-section="uplink-help"
                open={hilfeOffen}
                onToggle={(ereignis) => setHilfeOffen(ereignis.currentTarget.open)}
                className="panel-card group self-start rounded-2xl"
              >
                <summary className="flex min-h-16 cursor-pointer list-none items-center justify-between gap-4 px-5 py-4 [&::-webkit-details-marker]:hidden">
                  <span>
                    <span className="block text-base font-bold text-white">Uplink-Hilfe</span>
                    <span className="mt-0.5 block text-xs text-text-secondary">Einrichtung, Grundlagen und Störungen</span>
                  </span>
                  <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open:rotate-180" />
                </summary>
                <div className="space-y-3 border-t border-border/60 px-5 py-4">
                  {isHelpError && (
                    <p role="alert" className="text-sm text-warning">Die Uplink-Hilfe ist gerade nicht erreichbar.</p>
                  )}
                  {helpPages && helpPages.length < UPLINK_HELP_PAGES.length && (
                    <p role="alert" className="text-sm text-warning">
                      {UPLINK_HELP_PAGES.length - helpPages.length} von {UPLINK_HELP_PAGES.length} Kapiteln konnten nicht geladen werden.
                    </p>
                  )}
                  <div className="space-y-2">
                    {(helpPages ?? (isHelpError ? [] : UPLINK_HELP_PAGES.map((page) => ({ ...page, html: '' })))).map((page) =>
                      page.html ? (
                        <HilfeKapitel
                          key={page.file}
                          datei={page.file}
                          label={page.label}
                          html={page.html}
                        />
                      ) : (
                        <div key={page.file} className="rounded-xl border border-border bg-background/70 p-4 text-sm text-text-secondary">
                          Hilfe wird geladen: {page.label}
                        </div>
                      ),
                    )}
                  </div>
                  <a className="inline-flex min-h-11 items-center text-sm font-semibold text-primary" href={uplinkHelpUrl('index.html')}>
                    Uplink-Hilfe als eigene Seite öffnen
                  </a>
                </div>
              </details>
            </div>
          </main>
        </div>
      </div>
    </div>
  );
}
