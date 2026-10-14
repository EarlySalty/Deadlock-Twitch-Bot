import { Fragment, useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
  ChevronDown,
  Copy,
  Eye,
  EyeOff,
  Loader2,
  Lock,
  FileSearch,
  CheckCircle2,
  UserMinus,
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
  rejectUplinkAdminWaitlistEntry,
  rotateUplinkDockToken,
  saveUplinkReconnectWait,
  saveUplinkNative2kHardware,
  holeUplinkStreamKey,
  UPLINK_RECONNECT_WAIT_TEXT,
} from '@/api/uplink';
import type { UplinkAdminWaitlistEntry, UplinkMe } from '@/api/uplink';
import { useLanguage, useT } from '@/context/LanguageContext';
import { useAuthStatus } from '@/hooks/useAnalytics';
import { ZielKarte } from './UplinkZiel';
import { UplinkEingang } from './UplinkEingang';
import { PREVIEW_PRICING_ROUTE } from '@/preview/routes';
import { fetchUplinkHelp, uplinkHelpUrl, UPLINK_HELP_PAGES } from '@/uplinkHelp';
import { obsZugang, zielBetrieb } from '@/uplinkBetrieb';
import { useUplinkDisclosure } from '@/uplinkDisclosure';
import { analysiereObsLog } from '@/uplinkEncoderAnalyse';
import type { UplinkEncoderAnalyse } from '@/uplinkEncoderAnalyse';

/**
 * Inhalt von Schritt 6 der OBS-Anleitung, "Fenster einrichten".
 *
 * Vier Fenster, ein Zugang: Chat mit Antwortfeld, Aktivität, Stream-Infos und
 * Kanalpunkte, jeweils für alle verbundenen Plattformen zugleich. Die Adressen
 * stehen dauerhaft hier, nicht nur in der Sekunde nach dem Erzeugen; wer die
 * Seite neu lädt, findet dieselben vier Zeilen wieder.
 *
 * Vorher eine eigene Karte weit unter der Anleitung. Wer OBS einrichtete,
 * arbeitete die Schritte ab und war fertig, bevor er die Karte ueberhaupt sah.
 * Jetzt ist das Eintragen der Fenster der letzte Schritt derselben Liste.
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
function DockSchrittInhalt({ me }: { me: UplinkMe }) {
  const t = useT();
  const [nachfrage, setNachfrage] = useState(false);
  const queryClient = useQueryClient();
  const erzeugen = useMutation({
    mutationFn: rotateUplinkDockToken,
    onSuccess: (neue) => {
      // Die neuen Adressen gehen direkt in den Zwischenspeicher, aus dem die
      // Karte liest. Damit stehen sie sofort da, ohne zweite Quelle daneben:
      // ein eigener Zustand für "gerade erzeugt" wäre ein zweiter Stand, der
      // hängen bleiben kann, während der Server längst etwas anderes führt.
      // Der Abruf danach holt sich, was sonst noch am Nutzer hängt.
      queryClient.setQueryData(['uplink-me'], (alt?: UplinkMe) =>
        alt ? { ...alt, dock_urls: neue, dock_url_vorhanden: true } : alt
      );
      setNachfrage(false);
      queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
    },
  });
  const adressen = dockAdressen(me);
  // "Vorhanden" und "anzeigbar" fallen auseinander: ein Zugang aus der Zeit
  // vor dem Umbau lässt sich nicht mehr anzeigen. Dann gilt trotzdem die
  // Rückfrage, denn ein Neuerzeugen entwertet auch diese Eintragungen in OBS.
  const vorhanden = Boolean(me.dock_url_vorhanden) || adressen.length > 0;
  const darfAufdecken = me.live_status === 'aus' && !me.session?.active;
  const grundVerdeckt =
    me.live_status === 'live' || me.session?.active
      ? t('Du bist gerade live. Solange bleiben die Adressen verdeckt, damit sie nicht im Stream landen. Kopieren geht trotzdem.')
      : t('Wir wissen gerade nicht sicher, ob du live bist. Solange bleiben die Adressen verdeckt. Kopieren geht trotzdem.');

  return (
    <div className="space-y-3">
      <p className="text-xs text-text-secondary">
        {mitElementen(
          t('In OBS unter {docks}, {browserDocks} Namen und Adresse eintragen. Verbinden geht in der jeweiligen Plattform-Karte.'),
          {
            docks: <strong>{t('Docks')}</strong>,
            browserDocks: <strong>{t('Benutzerdefinierte Browser-Docks')}</strong>,
          },
        )}
      </p>

      <div data-section="eigenes-dock" className="space-y-3">
        {adressen.length > 0 ? (
          <>
            {adressen.map((dock) => (
              <CopyField
                key={dock.titel}
                label={t(dock.titel)}
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
                  <strong className="font-semibold text-white">{t('Privat:')}</strong>{' '}
                  {t('Wer eine dieser Adressen hat, kann in deinem Namen im Chat schreiben. Nicht im Stream zeigen.')}
                </p>
              </div>
            </div>
          </>
        ) : (
          <p className="text-xs text-text-secondary">
            {vorhanden
              ? t('Deine Fenster laufen weiter, ihre Adressen lassen sich hier aber nicht mehr anzeigen. Erzeuge sie einmal neu, dann stehen sie dauerhaft hier.')
              : t('Noch keine Adressen erzeugt.')}
          </p>
        )}

        {nachfrage ? (
          <div className="flex flex-col gap-2 rounded-xl border border-warning/40 bg-warning/5 px-3 py-2">
            <p className="text-xs text-warning">
              {t('Die vier Adressen, die jetzt in OBS stehen, gelten danach nicht mehr. Du musst sie dort alle vier neu eintragen.')}
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
                {erzeugen.isPending ? t('Wird erzeugt') : t('Ja, neu erzeugen')}
              </button>
              <button
                type="button"
                onClick={() => {
                  // Auch die Fehlerzeile geht mit: sie gehoert zu einem
                  // Versuch, den es nach dem Abbrechen nicht mehr gibt.
                  erzeugen.reset();
                  setNachfrage(false);
                }}
                className="inline-flex min-h-11 items-center rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary transition-colors hover:text-white"
              >
                {t('Abbrechen')}
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
            {vorhanden ? t('Neu erzeugen') : t('Adressen erzeugen')}
          </button>
        )}

        {erzeugen.isError ? (
          <p role="alert" className="text-xs text-warning">
            {t('Die Adressen konnten gerade nicht erzeugt werden. Bitte gleich noch einmal versuchen.')}
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

function Code({ children }: { children: React.ReactNode }) {
  return <code className="font-mono text-white">{children}</code>;
}

function mitElementen(text: string, elemente: Record<string, React.ReactNode>): React.ReactNode {
  return text.split(/(\{\w+\})/).map((teil, index) => {
    const name = /^\{(\w+)\}$/.exec(teil)?.[1];
    return <Fragment key={index}>{name !== undefined && name in elemente ? elemente[name] : teil}</Fragment>;
  });
}

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
  const t = useT();
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
                {t('Schritt {nummer} von 6', { nummer })}
              </span>
              <span className="block text-sm font-bold text-white">{t(titel)}</span>
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

function obsAusgabe(t: ReturnType<typeof useT>) {
  return [
    { feld: t('Videoencoder'), wert: t('Hardwareencoder passend zu deinem System'),
      warum: t('Nutze die Analyse direkt darüber. Software-AV1 über AOM/SVT wird für den Live-Uplink nicht automatisch empfohlen.') },
    { feld: t('Ratensteuerung'), wert: t('Aus der Encoder-Analyse übernehmen'),
      warum: t('AMD und NVIDIA benennen und unterstützen unterschiedliche Verfahren. Uplink zeigt nur die zum erkannten Hardwareweg passende Empfehlung.') },
    { feld: t('Auflösung und Bildrate'), wert: t('Dein gewünschtes Quellprofil'),
      warum: t('Die Ausgabeziele werden anhand deines Eingangs geprüft. Gespeicherte 1440p sind noch kein Nachweis einer aktiven 1440p-Ausgabe.') },
    { feld: t('Keyframe-Intervall'), wert: '2 s',
      warum: t('Die endgültigen Anforderungen prüft Uplink je Plattform und Ausgabeprofil.') },
    { feld: 'Audio', wert: t('Live-Mix auf Spur 1, Twitch-VOD auf Spur 2'),
      warum: t('Die VOD-Spur wird im nächsten Schritt ausdrücklich freigeschaltet und getrennt zu Uplink gesendet.') },
  ];
}

function ObsEncoderAnalyse() {
  const t = useT();
  const queryClient = useQueryClient();
  const [analyse, setAnalyse] = useState<UplinkEncoderAnalyse | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [laedt, setLaedt] = useState(false);
  const hardwareSpeichern = useMutation({
    mutationFn: saveUplinkNative2kHardware,
    onSuccess: async () => {
      setFehler(null);
      await queryClient.invalidateQueries({ queryKey: ['uplink-native-2k-hardware'] });
    },
    onError: (error) => setFehler(error instanceof Error ? error.message : '2K-Hardwaredaten konnten nicht gespeichert werden.'),
  });

  async function dateiAnalysieren(datei: File) {
    setLaedt(true);
    setFehler(null);
    try {
      const text = await datei.text();
      const ergebnis = analysiereObsLog(text);
      if (!ergebnis.gpu && !ergebnis.hardware.av1 && !ergebnis.hardware.hevc && !ergebnis.hardware.h264) {
        setAnalyse(null);
        setFehler('In dieser Datei wurden keine OBS-GPU-/Encoderangaben gefunden. Nimm eine aktuelle OBS-Logdatei nach einem normalen OBS-Start.');
        return;
      }
      setAnalyse(ergebnis);
      if (ergebnis.native2kProfile) {
        await hardwareSpeichern.mutateAsync(ergebnis.native2kProfile);
      }
    } catch {
      setAnalyse(null);
      setFehler('Die Logdatei konnte im Browser nicht gelesen werden.');
    } finally {
      setLaedt(false);
    }
  }

  return (
    <div className="space-y-3 rounded-xl border border-primary/30 bg-primary/5 p-3">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2 text-sm font-bold text-white">
            <FileSearch className="h-4 w-4 text-primary" />
            {t('Optimale Encoder-Einstellung analysieren')}
          </div>
          <p className="mt-1 text-xs text-text-secondary">
            {t('Das geht vor dem ersten Stream. OBS schreibt GPU und verfügbare Encoder bereits beim Start in die Logdatei.')}
          </p>
        </div>
        <label className="inline-flex min-h-10 cursor-pointer items-center rounded-xl border border-primary/40 bg-primary/10 px-3 py-2 text-xs font-semibold text-primary hover:bg-primary/15">
          {laedt ? t('Analysiere…') : t('OBS-Log auswählen')}
          <input
            type="file"
            accept=".txt,.log,text/plain"
            className="sr-only"
            disabled={laedt}
            onChange={(ereignis) => {
              const datei = ereignis.currentTarget.files?.[0];
              if (datei) void dateiAnalysieren(datei);
              ereignis.currentTarget.value = '';
            }}
          />
        </label>
      </div>

      <p className="text-[11px] text-text-secondary">
        {mitElementen(
          t('In OBS: {pfad}. Die ausgewählte Datei wird nur lokal in deinem Browser gelesen und nicht zu Uplink hochgeladen.'),
          {
            pfad: (
              <>
                <Weg>{t('Hilfe')}</Weg> <Weg>{t('Logdateien')}</Weg> <Feld>{t('Aktuelle Logdatei anzeigen')}</Feld>
              </>
            ),
          },
        )}
      </p>

      {fehler ? (
        <div role="alert" className="rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
          {t(fehler)}
        </div>
      ) : null}

      {analyse ? (
        <div className="space-y-3 rounded-lg border border-success/35 bg-success/10 p-3">
          <div className="flex items-center gap-2 text-sm font-bold text-white">
            <CheckCircle2 className="h-4 w-4 text-success" />
            {t('Empfehlung für dieses OBS-System')}
          </div>
          <dl className="grid gap-2 text-xs sm:grid-cols-2">
            <div>
              <dt className="text-text-secondary">{t('OBS-Renderadapter')}</dt>
              <dd className="font-semibold text-white">{analyse.gpu ?? t('Nicht erkannt')}</dd>
              {analyse.gpus.length > 1 ? (
                <span className="block text-[11px] font-normal text-text-secondary">
                  {t('{anzahl} Grafikadapter erkannt; die Encoderwahl berücksichtigt Multi-GPU.', { anzahl: analyse.gpus.length })}
                </span>
              ) : null}
            </div>
            <div>
              <dt className="text-text-secondary">Codec</dt>
              <dd className="font-semibold text-white">{analyse.empfehlung.codec}</dd>
            </div>
            <div>
              <dt className="text-text-secondary">{t('Videoencoder')}</dt>
              <dd className="font-semibold text-white">{analyse.empfehlung.encoder}</dd>
            </div>
            <div>
              <dt className="text-text-secondary">{t('Ratensteuerung')}</dt>
              <dd className="font-semibold text-white">{analyse.empfehlung.ratensteuerung}</dd>
            </div>
          </dl>
          <div className="flex flex-wrap gap-1.5 text-[11px]">
            <span className={`rounded-full border px-2 py-1 ${analyse.hardware.av1 ? 'border-success/35 text-success' : 'border-border text-text-secondary'}`}>
              Hardware AV1 {analyse.hardware.av1 ? '✓' : '–'}
            </span>
            <span className={`rounded-full border px-2 py-1 ${analyse.hardware.hevc ? 'border-success/35 text-success' : 'border-border text-text-secondary'}`}>
              Hardware HEVC {analyse.hardware.hevc ? '✓' : '–'}
            </span>
            <span className={`rounded-full border px-2 py-1 ${analyse.hardware.h264 ? 'border-success/35 text-success' : 'border-border text-text-secondary'}`}>
              Hardware H.264 {analyse.hardware.h264 ? '✓' : '–'}
            </span>
          </div>
          <ul className="list-disc space-y-1 pl-4 text-xs text-text-secondary">
            {analyse.empfehlung.hinweise.map((hinweis) => <li key={hinweis}>{t(hinweis)}</li>)}
          </ul>
          {analyse.native2kProfile ? (
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-primary/35 bg-primary/10 px-3 py-2">
              <p className="text-xs text-text-secondary">
                {t('CPU, RAM, Windows, GPU, PCI-ID, VRAM, Treiber sowie HEVC/H.264-Encoder wurden vollständig erkannt. Für Twitch wird nur dieses normalisierte Hardwareprofil gespeichert, nicht deine Logdatei.')}
              </p>
              <button type="button" disabled={hardwareSpeichern.isPending}
                onClick={() => hardwareSpeichern.mutate(analyse.native2kProfile!)}
                className="min-h-10 shrink-0 rounded-lg bg-primary px-3 py-2 text-xs font-semibold text-[#0D0806] disabled:opacity-50">
                {hardwareSpeichern.isPending ? t('Übernehme…') : hardwareSpeichern.isSuccess ? t('2K-Hardware übernommen') : t('Für Native 2K übernehmen')}
              </button>
            </div>
          ) : analyse.native2kFehlendeFelder.length ? (
            <div className="rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
              {t('Für die automatische 2K-Hardwareweitergabe fehlen in dieser OBS-Logdatei: {felder}. Eine neu gestartete aktuelle OBS-Logdatei enthält diese Angaben meist vollständig.', { felder: analyse.native2kFehlendeFelder.map((feld) => t(feld)).join(', ') })}
            </div>
          ) : null}
          {analyse.softwareAv1 && !analyse.hardware.av1 ? (
            <div className="rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
              {t('Software-AV1 erkannt: AOM/SVT wird nicht als Live-Empfehlung verwendet, wenn die GPU AV1 nicht selbst encodieren kann.')}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

/**
 * Benutzerdefinierte RTMP-Dienste blenden OBS' vorhandene Twitch-VOD-Spur
 * standardmäßig aus. Uplink kann die lokale OBS-Einstellung nicht aus der
 * Ferne setzen. Deshalb steht der vollständige einmalige Handgriff direkt in
 * der Einrichtung und nicht nur in einem Hilfelink.
 */
function ObsVodTrackEinrichtung() {
  const t = useT();
  return (
    <>
      <div className="rounded-xl border border-primary/30 bg-primary/10 px-3 py-3 text-xs text-white">
        <strong>{t('Wenn „Twitch-VOD-Spur“ in OBS bereits sichtbar ist:')}</strong>{' '}
        {t('Überspringe die Dateiänderung und stelle unten direkt Live auf Spur 1 und VOD auf Spur 2.')}
      </div>

      <ol className="space-y-3 text-xs text-text-secondary">
        <li className="space-y-1">
          <strong className="block text-white">{t('1. OBS vollständig beenden')}</strong>
          <span>{t('Auch das OBS-Symbol im Infobereich/Tray schließen. Sonst kann OBS die Datei beim Beenden wieder überschreiben.')}</span>
        </li>
        <li className="space-y-2">
          <strong className="block text-white">{t('2. Die Datei user.ini öffnen')}</strong>
          <p>
            {mitElementen(
              t('Unter Windows: {tasten} drücken, {ordner} eingeben und {datei} mit einem Texteditor öffnen. Standardpfade:'),
              {
                tasten: <Feld>Win + R</Feld>,
                ordner: <Code>%APPDATA%\obs-studio</Code>,
                datei: <Code>user.ini</Code>,
              },
            )}
          </p>
          <dl className="grid gap-1 rounded-xl border border-border bg-background/65 px-3 py-2 font-mono text-[11px] text-white">
            <div className="grid gap-1 sm:grid-cols-[5rem_minmax(0,1fr)]">
              <dt className="font-sans font-semibold text-text-secondary">Windows</dt>
              <dd className="break-all">%APPDATA%\obs-studio\user.ini</dd>
            </div>
            <div className="grid gap-1 sm:grid-cols-[5rem_minmax(0,1fr)]">
              <dt className="font-sans font-semibold text-text-secondary">macOS</dt>
              <dd className="break-all">~/Library/Application Support/obs-studio/user.ini</dd>
            </div>
            <div className="grid gap-1 sm:grid-cols-[5rem_minmax(0,1fr)]">
              <dt className="font-sans font-semibold text-text-secondary">Linux</dt>
              <dd className="break-all">~/.config/obs-studio/user.ini</dd>
            </div>
            <div className="grid gap-1 sm:grid-cols-[5rem_minmax(0,1fr)]">
              <dt className="font-sans font-semibold text-text-secondary">Flatpak</dt>
              <dd className="break-all">~/.var/app/com.obsproject.Studio/config/obs-studio/user.ini</dd>
            </div>
          </dl>
          <p>
            {mitElementen(t('Bei Portable-OBS liegt {datei} im verwendeten lokalen OBS-Konfigurationsordner.'), {
              datei: <Code>user.ini</Code>,
            })}
          </p>
        </li>
        <li className="space-y-2">
          <strong className="block text-white">{t('3. Unter dem vorhandenen Abschnitt [General] diese Zeile ergänzen')}</strong>
          <CopyField
            label={t('OBS-Einstellung für benutzerdefinierte VOD-Spur')}
            value="EnableCustomServerVodTrack=true"
            privat={false}
            darfAufdecken
            grundVerdeckt=""
          />
          <p className="text-warning">
            {mitElementen(t('Keinen zweiten {abschnitt}-Block anlegen. Die Zeile gehört in den bereits vorhandenen Abschnitt.'), { abschnitt: <Code>[General]</Code> })}
          </p>
        </li>
        <li className="space-y-1">
          <strong className="block text-white">{t('4. OBS neu starten und die beiden Streaming-Spuren auswählen')}</strong>
          <p>
            {mitElementen(t('{pfad}. Setze {audiospur} auf {eins} und {vodSpur} auf {zwei}.'), {
              pfad: (
                <>
                  <Weg>{t('Einstellungen')}</Weg> <Weg>{t('Ausgabe')}</Weg> <Weg>Stream</Weg>
                </>
              ),
              audiospur: <Feld>{t('Audiospur')}</Feld>,
              eins: <strong className="text-white">1</strong>,
              vodSpur: <Feld>{t('Twitch-VOD-Spur')}</Feld>,
              zwei: <strong className="text-white">2</strong>,
            })}
          </p>
        </li>
        <li className="space-y-1">
          <strong className="block text-white">{t('5. Audioquellen den beiden Mischungen zuordnen')}</strong>
          <p>
            {mitElementen(
              t('In {feld} ist Spur 1 der Livestream. Spur 2 ist das Twitch-VOD. Quellen, die nicht im VOD landen sollen – zum Beispiel Musik – bekommen nur Spur 1.'),
              { feld: <Feld>{t('Erweiterte Audioeigenschaften')}</Feld> },
            )}
          </p>
        </li>
      </ol>

      <div role="note" className="rounded-xl border border-success/30 bg-success/10 px-3 py-2.5 text-xs text-white">
        <strong>{t('Uplink prüft das beim nächsten Stream:')}</strong>{' '}
        {t('Es müssen zwei verschiedene AAC-Spuren ankommen. Fehlt Spur 2, bleibt nur der Twitch-Ausgang angehalten; der Live-Mix wird nicht als VOD-Ersatz verwendet.')}
      </div>
    </>
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
  privat = true,
}: {
  label: string;
  value: string;
  darfAufdecken: boolean;
  grundVerdeckt: string;
  grundAnzeigen?: boolean;
  privat?: boolean;
}) {
  const t = useT();
  const [stand, setStand] = useState<'ruhe' | 'ok' | 'fehler'>('ruhe');
  const [privatOffen, setOffen] = useState(false);
  const offen = !privat || privatOffen;
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

  const knopfText = stand === 'ok' ? t('Kopiert') : stand === 'fehler' ? t('Ging nicht') : t('Kopieren');
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
          // Verdeckt heisst hier nur "nicht auf dem Schirm". Ohne diese Zeile
          // bietet der Passwortmanager fuer jede Adresse das Speichern an.
          autoComplete="off"
          data-1p-ignore
          data-lpignore="true"
          value={value}
          aria-label={offen ? t('{label}: sichtbar', { label }) : t('{label}: verdeckt', { label })}
          className="flex min-h-11 min-w-0 flex-1 items-center truncate rounded-xl border border-border bg-background/70 px-3 py-2 font-mono text-xs text-white"
        />
        <div className="flex gap-2">
          {privat ? <button
            type="button"
            onClick={() => setOffen((vorher) => !vorher)}
            disabled={!darfAufdecken}
            aria-expanded={offen}
            title={darfAufdecken ? undefined : grundVerdeckt}
            className="inline-flex min-h-11 shrink-0 items-center gap-1 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary transition-colors hover:text-white disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:text-text-secondary"
          >
            {offen ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
            {offen ? t('Verdecken') : t('Zeigen')}
          </button> : null}
          <button
            type="button"
            aria-label={t('{label} kopieren', { label })}
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
        {stand === 'ok' ? t('In die Zwischenablage kopiert') : ''}
        {stand === 'fehler' ? t('Kopieren hat nicht geklappt') : ''}
      </p>
      {stand === 'fehler' && (
        <p className="text-xs text-warning">
          {t('Dein Browser hat das automatische Kopieren nicht erlaubt. Das Feld ist markiert; kopier es mit Strg+C.')}
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
  verfuegbar,
  onSaved,
}: {
  wert: number;
  max: number;
  verfuegbar: boolean;
  onSaved: () => void;
}) {
  const t = useT();
  const [entwurf, setEntwurf] = useState<string | null>(null);
  const eingabe = entwurf ?? reconnectWaitEingabe(wert);
  const payload = reconnectWaitPayload(eingabe);
  const speichern = useMutation({
    mutationFn: () => {
      if (!verfuegbar || payload === null) {
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
          {t('Verbindung')}
        </div>
        <h2 className="mt-1 text-base font-bold text-white">{t('Verhalten bei Internetabriss')}</h2>
        <p className="mt-1 text-sm text-text-secondary">{t(UPLINK_RECONNECT_WAIT_TEXT)}</p>
        <p className="mt-2 text-xs text-text-secondary">
          {verfuegbar
            ? t('0 bis {max} Sekunden. Die Änderung gilt für die nächste Session.', { max })
            : t('Die gespeicherte Frist wird derzeit noch nicht angewendet. Die Einstellung wird freigegeben, sobald die Wiederverbindung im Dienst verfügbar ist.')}
        </p>
      </div>
      <div className="space-y-2">
        <div className="flex flex-wrap items-end gap-2">
          <label className="min-w-[11rem] flex-1 space-y-1">
          <span className="text-[11px] font-semibold uppercase tracking-[0.16em] text-text-secondary">
            {t('Wartezeit in Sekunden')}
          </span>
          <input
            type="number"
            disabled={!verfuegbar}
            min={0}
            max={max}
            step={1}
            value={eingabe}
            onChange={(e) => setEntwurf(e.target.value)}
            inputMode="numeric"
            className="min-h-11 w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white outline-none transition-colors focus:border-primary"
            aria-label={t('Wartezeit nach Internetabriss in Sekunden')}
          />
          </label>
          <button
            type="button"
            disabled={!verfuegbar || speichern.isPending || payload === null}
            onClick={() => speichern.mutate()}
            className="min-h-11 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:cursor-not-allowed disabled:opacity-60"
          >
            {speichern.isPending ? t('Speichert …') : speichern.isSuccess ? t('Gespeichert') : t('Speichern')}
          </button>
        </div>
        {speichern.isError && (
          <p role="alert" className="text-xs text-warning">
          {speichern.error instanceof Error
            ? t(speichern.error.message)
            : t('Die Wartezeit ließ sich gerade nicht speichern.')}
          </p>
        )}
      </div>
    </Rise>
  );
}

function wartelistenZeit(roh: string, locale: string): string {
  const datum = new Date(roh);
  if (Number.isNaN(datum.getTime())) return roh;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: 'short',
    timeStyle: 'short',
  }).format(datum);
}

function AdminUplinkWarteliste({ csrfToken }: { csrfToken: string | null }) {
  const { t, locale } = useLanguage();
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
  const ablehnen = useMutation({
    mutationFn: (streamerId: number) => {
      if (!csrfToken) {
        throw new Error('Der Sitzungsschutz fehlt. Lade die Seite neu.');
      }
      return rejectUplinkAdminWaitlistEntry(streamerId, csrfToken);
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
  const beschaeftigt = freischalten.isPending || ablehnen.isPending;

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
              {t('Verwaltung')}
            </div>
            <h2 className="text-base font-bold text-white">{t('Uplink-Warteliste')}</h2>
            <p className="mt-1 text-xs text-text-secondary">
              {t('Freischalten gibt dem Streamer einen Uplink-Zugang und entfernt den Eintrag aus der Liste.')}
            </p>
          </div>
        </div>
        <span className="rounded-full border border-warning/30 bg-warning/10 px-2.5 py-1 text-xs font-semibold text-warning">
          {warteliste.isLoading ? '…' : eintraege.length}
        </span>
      </div>

      {warteliste.isLoading ? (
        <p role="status" className="rounded-xl border border-border bg-background/50 px-3 py-2 text-xs text-text-secondary">
          {t('Warteliste wird geladen …')}
        </p>
      ) : null}

      {warteliste.isError ? (
        <p role="alert" className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
          {t('Die Warteliste ist gerade nicht erreichbar.')}
        </p>
      ) : null}

      {!csrfToken ? (
        <p role="alert" className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
          {t('Der Sitzungsschutz fehlt. Lade die Seite neu, bevor du jemanden freischaltest.')}
        </p>
      ) : null}

      {!warteliste.isLoading && !warteliste.isError && eintraege.length === 0 ? (
        <p className="rounded-xl border border-border bg-background/45 px-3 py-3 text-xs text-text-secondary">
          {t('Gerade wartet niemand auf eine Freischaltung.')}
        </p>
      ) : null}

      {eintraege.length > 0 ? (
        <ul className="max-h-72 space-y-2 overflow-y-auto pr-1" aria-label={t('Uplink-Warteliste')}>
          {eintraege.map((eintrag) => {
            const wirdFreigeschaltet =
              freischalten.isPending && freischalten.variables === eintrag.streamer_id;
            const wirdAbgelehnt =
              ablehnen.isPending && ablehnen.variables === eintrag.streamer_id;
            return (
              <li
                key={eintrag.streamer_id}
                className="flex flex-col gap-3 rounded-xl border border-border bg-background/55 p-3 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="min-w-0">
                  {eintrag.display_name || eintrag.twitch_login ? (
                    <>
                      <p className="truncate text-sm font-semibold text-white">
                        {eintrag.display_name ?? eintrag.twitch_login}
                        {eintrag.twitch_login &&
                        eintrag.display_name &&
                        eintrag.twitch_login.toLowerCase() !== eintrag.display_name.toLowerCase() ? (
                          <span className="ml-1 font-normal text-text-secondary">@{eintrag.twitch_login}</span>
                        ) : null}
                      </p>
                      <p className="mt-0.5 text-[11px] text-text-secondary">
                        {t('Twitch-ID {id} · Anfrage {zeit}', { id: eintrag.streamer_id, zeit: wartelistenZeit(eintrag.requested_at, locale) })}
                      </p>
                    </>
                  ) : (
                    <>
                      <p className="truncate text-sm font-semibold text-white">{t('Twitch-ID {id}', { id: eintrag.streamer_id })}</p>
                      <p className="mt-0.5 text-[11px] text-text-secondary">
                        {t('Anfrage {zeit}', { zeit: wartelistenZeit(eintrag.requested_at, locale) })}
                      </p>
                    </>
                  )}
                  {eintrag.note ? <p className="mt-1 text-xs text-text-secondary">{eintrag.note}</p> : null}
                </div>
                <div className="flex shrink-0 flex-wrap items-center gap-2">
                  <button
                    type="button"
                    disabled={beschaeftigt || !csrfToken}
                    onClick={() => freischalten.mutate(eintrag.streamer_id)}
                    className="inline-flex min-h-10 shrink-0 items-center justify-center gap-2 rounded-xl bg-primary px-3 py-2 text-xs font-semibold text-[#0D0806] disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <UserPlus aria-hidden="true" className="h-3.5 w-3.5" />
                    {wirdFreigeschaltet ? t('Wird freigeschaltet …') : t('Freischalten')}
                  </button>
                  <button
                    type="button"
                    disabled={beschaeftigt || !csrfToken}
                    onClick={() => ablehnen.mutate(eintrag.streamer_id)}
                    className="inline-flex min-h-10 shrink-0 items-center justify-center gap-2 rounded-xl border border-border bg-background/70 px-3 py-2 text-xs font-semibold text-text-secondary hover:text-white disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <UserMinus aria-hidden="true" className="h-3.5 w-3.5" />
                    {wirdAbgelehnt ? t('Wird abgelehnt …') : t('Ablehnen')}
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      ) : null}

      {freischalten.isError ? (
        <p role="alert" className="text-xs text-warning">
          {freischalten.error instanceof Error
            ? t(freischalten.error.message)
            : t('Die Freischaltung hat nicht geklappt.')}
        </p>
      ) : null}

      {ablehnen.isError ? (
        <p role="alert" className="text-xs text-warning">
          {ablehnen.error instanceof Error
            ? t(ablehnen.error.message)
            : t('Das Ablehnen hat nicht geklappt.')}
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
    const verbunden = params.get('verbunden');
    const fehler = params.get('verbinden_fehler');
    const zielOffen = params.get('ziel_offen') === '1';
    const neuVerbinden = params.get('neu_verbinden') === '1';
    if (!verbunden && !fehler) return;
    params.delete('verbunden');
    params.delete('verbinden_fehler');
    params.delete('ziel_offen');
    params.delete('neu_verbinden');
    const rest = params.toString();
    window.history.replaceState(
      null,
      '',
      window.location.pathname + (rest ? `?${rest}` : '')
    );
    if (fehler) {
      setMeldung(
        'Das Verbinden hat gerade nicht geklappt. Bitte versuch es noch einmal.'
      );
      return;
    }
    if (verbunden === 'twitch') {
      setAusstehend(true);
      return;
    }
    queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
    queryClient.invalidateQueries({ queryKey: ['uplink-destinations'] });
    if (neuVerbinden) {
      setMeldung(
        'Fast geschafft. Bitte verbinde dich noch einmal, damit der Zugang dauerhaft hält.'
      );
    } else if (zielOffen) {
      setMeldung(
        "Verbunden, aber der Stream-Key konnte noch nicht übernommen werden. Bitte 'Stream-Key erneut holen' drücken."
      );
    }
  }, [queryClient]);

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
  const t = useT();
  const queryClient = useQueryClient();
  const { data: authStatus, isLoading: authLaedt } = useAuthStatus();
  const [qualitaetOffen, setQualitaetOffen] = useUplinkDisclosure('qualitaet-erklaerung', false);
  const [hilfeOffen, setHilfeOffen] = useUplinkDisclosure('uplink-hilfe', false);
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ['uplink-me'],
    queryFn: fetchUplinkMe,
    retry: false,
    // Der Live-Status steckt in dieser Antwort und entscheidet, ob die Adresse
    // aufgedeckt werden darf. Beendet der Streamer den Stream, waehrend das
    // Dashboard offen liegt, soll das Aufdecken kurz darauf wieder gehen, ohne
    // dass jemand neu laedt. Andersherum genauso: Stream an, Adresse zu.
    refetchInterval: 5_000,
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
    refetchInterval: 5_000,
    refetchOnWindowFocus: true,
  });
  // Das Relay antwortet auf einen leeren Erfolg auch mal mit `{}`. Ohne die
  // Absicherung wirft `.length` beim Rendern, und die ErrorBoundary ersetzt
  // dann das ganze Dashboard, also auch die Einrichtung, die der Streamer
  // gerade braucht.
  const gespeicherteZiele = ziele?.destinations ?? [];
  // Zugangsstand je Plattform, steht im Kopf der Plattform-Karte.
  const chatVerbindungen = data ? plattformVerbindungen(data) : [];
  const streamKeyMeldung = useRueckkehrVomVerbinden(
    queryClient,
    authStatus?.csrfToken ?? authStatus?.csrf_token ?? null,
    !authLaedt
  );
  const obs = data && !isError ? obsZugang(data) : null;
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
    !isError && data?.live_status === 'live'
      ? { text: t('Twitch live'), klasse: 'border-success/35 bg-success/10 text-success' }
      : !isError && data?.live_status === 'aus'
        ? { text: t('Twitch offline'), klasse: 'border-border bg-background/60 text-text-secondary' }
        : { text: isLoading ? t('Streamstatus lädt') : t('Streamstatus unbekannt'), klasse: 'border-warning/30 bg-warning/10 text-warning' };

  return (
    <>
            <Rise className="panel-card rounded-2xl p-5 md:p-6">
              <div className="mb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-primary">
                {t('Eigenes Modul')}
              </div>
              <div className="flex flex-wrap items-end justify-between gap-3">
                <div>
                  <h1 className="display-font text-2xl font-extrabold text-white">Uplink</h1>
                  <p className="mt-2 max-w-2xl text-sm text-text-secondary">
                    {t('Ein Stream zu uns, passend verteilt an deine Plattformen. Start und Stop bleiben in OBS.')}
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
                      {t('Uplink freigeschaltet')}
                    </span>
                  ) : null}
                </div>
              </div>
            </Rise>

            {isLoading && (
              <div className="panel-card flex items-center gap-2 rounded-2xl p-5 text-sm text-text-secondary">
                <Loader2 aria-hidden="true" className="h-4 w-4 animate-spin" />
                {t('Zugang wird geladen')}
              </div>
            )}

            {isError && (
              <div role="alert" className="panel-card rounded-2xl p-5 text-sm text-warning">
                {error instanceof Error ? t(error.message) : t('Uplink ist gerade nicht erreichbar.')}
              </div>
            )}

            {data && !data.enabled && (
              <div className="panel-card relative overflow-hidden rounded-2xl p-6">
                <div className="absolute inset-0 bg-black/20" />
                <div className="relative space-y-3">
                  <div className="inline-flex h-12 w-12 items-center justify-center rounded-full border border-white/10 bg-white/5">
                    <Lock className="h-5 w-5 text-white/40" />
                  </div>
                  <h2 className="text-lg font-bold text-white">{t('Uplink ist ein bezahltes Add-on')}</h2>
                  <p className="max-w-xl text-sm text-text-secondary">
                    {t('Ohne Freischaltung kannst du auf die Warteliste. Danach richtet EarlySalty den Slot ein.')}
                  </p>
                  <div className="flex flex-wrap gap-2">
                    <a
                      href={PREVIEW_PRICING_ROUTE}
                      className="inline-flex min-h-11 items-center rounded-xl border border-border px-4 py-2 text-sm font-semibold text-white no-underline"
                    >
                      {t('Preise ansehen')}
                    </a>
                    <button
                      type="button"
                      disabled={waitlist.isPending || data.waitlisted}
                      onClick={() => waitlist.mutate()}
                      className="min-h-11 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
                    >
                      {data.waitlisted ? t('Stehst auf der Warteliste') : t('Auf die Warteliste')}
                    </button>
                  </div>
                </div>
              </div>
            )}

            {data?.enabled && (
              <UplinkEingang session={data.session} unavailable={isError || data.service_status !== 'ready'} />
            )}

            {data?.enabled && (
              <div className="grid items-start gap-4 md:gap-5 xl:grid-cols-[minmax(0,0.92fr)_minmax(0,1.08fr)]">
                <Rise className="panel-card space-y-4 rounded-2xl p-4 md:p-6">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div>
                      <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">
                        {t('Einmalig')}
                      </div>
                      <h2 className="text-lg font-bold text-white">{t('OBS einrichten')}</h2>
                      <p className="mt-1 text-sm text-text-secondary">
                        {t('Sechs kurze Schritte. Die Twitch-VOD-Spur wird ausdrücklich mit eingerichtet.')}
                      </p>
                    </div>
                    <span className="rounded-full border border-border bg-background/60 px-3 py-1 text-xs font-semibold text-text-secondary">
                      {t('6 Schritte')}
                    </span>
                  </div>

                  <ol aria-label={t('OBS einrichten')} className="space-y-2">
                    <ObsSchritt nummer={1} titel="„Benutzerdefiniert“ wählen">
                      <p className="text-xs text-text-secondary">
                        {mitElementen(t('In OBS {pfad}. Beim Feld {dienst} von „Twitch“ auf {benutzerdefiniert} wechseln.'), {
                          pfad: (
                            <>
                              <Weg>{t('Einstellungen')}</Weg> <Weg>Stream</Weg>
                            </>
                          ),
                          dienst: <Feld>{t('Dienst')}</Feld>,
                          benutzerdefiniert: <Feld>{t('Benutzerdefiniert…')}</Feld>,
                        })}
                      </p>
                    </ObsSchritt>

                    <ObsSchritt nummer={2} titel="Serveradresse einfügen" offenStart>
                      {obs ? (
                        <>
                          <CopyField label={t('Serveradresse für OBS')} value={obs.server}
                            privat={false} darfAufdecken grundVerdeckt="" />
                          <p className="text-xs text-text-secondary">{mitElementen(t('Diese öffentliche Adresse gehört in das OBS-Feld {feld}.'), { feld: <Feld>Server</Feld> })}</p>
                        </>
                      ) : (
                        <p role="alert" className="text-sm text-warning">
                          {t('Die RTMPS-Einrichtung ist gerade nicht vollständig verfügbar. Deine Plattformziele bleiben gespeichert.')}
                        </p>
                      )}
                    </ObsSchritt>

                    <ObsSchritt nummer={3} titel="Privaten Streamschlüssel einfügen" offenStart>
                      {obs ? <CopyField label={t('Privater Streamschlüssel für OBS')} value={obs.key}
                        darfAufdecken={data.live_status === 'aus' && !data.session?.active}
                        grundVerdeckt={t('Solange Uplink Medien empfängt oder dein Plattformstatus live oder unbekannt ist, bleibt der Schlüssel verdeckt. Kopieren geht trotzdem.')} /> : null}
                      <p className="text-xs text-text-secondary">
                        {mitElementen(
                          t('Kopiere deinen Uplink-Schlüssel in {feld}. Serveradresse und Schlüssel sind zwei getrennte Felder.'),
                          { feld: <Feld>{t('Streamschlüssel')}</Feld> },
                        )}
                      </p>
                      <div data-uplink-private-warning role="note"
                        className="rounded-xl border border-warning/45 bg-warning/10 px-3 py-2.5 text-xs text-warning">
                        <strong className="text-white">{t('Privat:')}</strong>{' '}
                        {t('Den Streamschlüssel nicht im Stream zeigen oder weitergeben.')}
                      </div>
                    </ObsSchritt>

                    <ObsSchritt nummer={4} titel="Ausgabe einstellen">
                      <p className="text-xs text-text-secondary">
                        {mitElementen(t('{pfad}, Ausgabemodus auf {modus}.'), {
                          pfad: (
                            <>
                              <Weg>{t('Einstellungen')}</Weg> <Weg>{t('Ausgabe')}</Weg>
                            </>
                          ),
                          modus: <Feld>{t('Erweitert')}</Feld>,
                        })}
                      </p>
                      <ObsEncoderAnalyse />
                      <dl className="divide-y divide-border/60 overflow-hidden rounded-xl border border-border">
                        {obsAusgabe(t).map((zeile, index) => (
                          <div key={index} className="grid gap-1 px-3 py-2 sm:grid-cols-[8rem_minmax(0,1fr)]">
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
                      <p className="mt-3 text-xs text-text-secondary">
                        {t('Die VOD-Spur richtest du direkt im nächsten Schritt ein. Bei „Benutzerdefiniert“ blendet OBS dieses Feld sonst oft aus.')}
                      </p>
                    </ObsSchritt>

                    <ObsSchritt nummer={5} titel="Twitch-VOD-Spur freischalten" offenStart>
                      <ObsVodTrackEinrichtung />
                      <p className="text-xs text-text-secondary">
                        {mitElementen(t('Mehr Hintergrund findest du in der {link}.'), {
                          link: (
                            <a href={`${uplinkHelpUrl('obs.html')}#vod`} className="underline underline-offset-2">
                              {t('OBS-Hilfe zur VOD-Spur')}
                            </a>
                          ),
                        })}
                      </p>
                    </ObsSchritt>

                    <ObsSchritt nummer={6} titel="Fenster einrichten">
                      <DockSchrittInhalt me={data} />
                    </ObsSchritt>
                  </ol>

                  <p className="rounded-xl border border-primary/25 bg-primary/10 px-3 py-2 text-xs text-white">
                    {mitElementen(t('Danach startest du in OBS wie immer über {knopf}. Den Rest übernimmt Uplink.'), {
                      knopf: <Feld>{t('Stream starten')}</Feld>,
                    })}
                  </p>
                </Rise>

                <div data-section="uplink-right-column" className="space-y-4 md:space-y-5">
                  <Rise className="panel-card card-glow space-y-5 rounded-2xl p-4 md:p-6">
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div>
                        <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">
                          {t('Hauptbereich')}
                        </div>
                        <h2 className="text-lg font-bold text-white">{t('Plattformen')}</h2>
                        <p className="mt-1 text-sm text-text-secondary">
                          {t('Status und Qualität stehen im Kartenkopf. Zum Ändern die Karte öffnen.')}
                        </p>
                      </div>
                      <span
                        className={`rounded-full border px-3 py-1 text-xs font-semibold ${zieleLaden || zieleFehler ? 'border-border bg-background/60 text-text-secondary' : 'border-primary/30 bg-primary/10 text-primary'}`}
                      >
                        {zieleLaden
                          ? t('Ziele werden geladen')
                          : zieleFehler
                            ? t('Status unbekannt')
                            : t('{medienfluss} mit Medienfluss · {eingeschaltet} eingeschaltet', {
                              medienfluss: gespeicherteZiele.filter((ziel) => zielBetrieb(ziel).state === 'sending').length,
                              eingeschaltet: gespeicherteZiele.filter((ziel) => ziel.enabled).length,
                            })}
                      </span>
                    </div>

                    {zieleLaden ? (
                      <p
                        role="status"
                        className="rounded-xl border border-border bg-background/50 px-3 py-2 text-xs text-text-secondary"
                      >
                        {t('Plattformziele werden geladen …')}
                      </p>
                    ) : null}

                    {zieleFehler ? (
                      <p
                        role="alert"
                        className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning"
                      >
                        {t('Deine gespeicherten Ziele sind gerade nicht abrufbar. Sie sind nicht weg; bitte nichts doppelt speichern und die Seite später neu laden.')}
                      </p>
                    ) : null}

                    {streamKeyMeldung ? (
                      <p role="alert" className="text-xs text-warning">
                        {t(streamKeyMeldung)}
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
                        {t('Noch kein Ziel gespeichert. Dein Stream kommt bei Uplink an, wird aber noch nicht weitergesendet.')}
                      </p>
                    ) : null}

                    <details
                      open={qualitaetOffen}
                      onToggle={(ereignis) => setQualitaetOffen(ereignis.currentTarget.open)}
                      className="group rounded-xl border border-border bg-background/40"
                    >
                      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-3 py-2.5 text-xs font-semibold text-white [&::-webkit-details-marker]:hidden">
                        {t('Warum Eingangs- und Zielqualität verschieden sind')}
                        <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open:rotate-180" />
                      </summary>
                      <p className="border-t border-border/60 px-3 py-3 text-xs text-text-secondary">
                        {t('Schick uns AV1 oder H.264 mit den Werten aus Schritt 4. In jeder Plattformkarte siehst du getrennt, welche Ausgabe du gespeichert hast und welche gerade läuft.')}
                      </p>
                    </details>
                  </Rise>

                  {authStatus?.adminMode || authStatus?.adminEligible ? (
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
                verfuegbar={data.capabilities?.reconnect === true}
                onSaved={() => queryClient.invalidateQueries({ queryKey: ['uplink-me'] })}
              />
            )}

            <div className="grid gap-4 md:gap-5">
              <details
                data-section="uplink-help"
                open={hilfeOffen}
                onToggle={(ereignis) => setHilfeOffen(ereignis.currentTarget.open)}
                className="panel-card group self-start rounded-2xl"
              >
                <summary className="flex min-h-16 cursor-pointer list-none items-center justify-between gap-4 px-5 py-4 [&::-webkit-details-marker]:hidden">
                  <span>
                    <span className="block text-base font-bold text-white">{t('Uplink-Hilfe')}</span>
                    <span className="mt-0.5 block text-xs text-text-secondary">{t('Einrichtung, Grundlagen und Störungen')}</span>
                  </span>
                  <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary transition-transform group-open:rotate-180" />
                </summary>
                <div className="space-y-3 border-t border-border/60 px-5 py-4">
                  {isHelpError && (
                    <p role="alert" className="text-sm text-warning">{t('Die Uplink-Hilfe ist gerade nicht erreichbar.')}</p>
                  )}
                  {helpPages && helpPages.length < UPLINK_HELP_PAGES.length && (
                    <p role="alert" className="text-sm text-warning">
                      {t('{fehlend} von {gesamt} Kapiteln konnten nicht geladen werden.', {
                        fehlend: UPLINK_HELP_PAGES.length - helpPages.length,
                        gesamt: UPLINK_HELP_PAGES.length,
                      })}
                    </p>
                  )}
                  <div className="space-y-2">
                    {(helpPages ?? (isHelpError ? [] : UPLINK_HELP_PAGES.map((page) => ({ ...page, html: '' })))).map((page) =>
                      page.html ? (
                        <HilfeKapitel
                          key={page.file}
                          datei={page.file}
                          label={t(page.label)}
                          html={page.html}
                        />
                      ) : (
                        <div key={page.file} className="rounded-xl border border-border bg-background/70 p-4 text-sm text-text-secondary">
                          {t('Hilfe wird geladen: {label}', { label: t(page.label) })}
                        </div>
                      ),
                    )}
                  </div>
                  <a className="inline-flex min-h-11 items-center text-sm font-semibold text-primary" href={uplinkHelpUrl('index.html')}>
                    {t('Uplink-Hilfe als eigene Seite öffnen')}
                  </a>
                </div>
              </details>
            </div>
    </>
  );
}
