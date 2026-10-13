import { useEffect, useId, useRef, useState } from 'react';
import { Info, Loader2, Radio, ShieldCheck, TriangleAlert } from 'lucide-react';
import { useLanguage, type LanguageContextValue } from '../../context/LanguageContext';

export type YouTubeLiveSichtbarkeit = 'private' | 'unlisted' | 'public';
export type YouTubeLiveZustand =
  | 'inaktiv'
  | 'vorbereitet'
  | 'sendet'
  | 'live'
  | 'beendet'
  | 'blockiert'
  | 'fehler'
  | 'unklar';

export interface YouTubeLiveEinstellungen {
  titel: string;
  sichtbarkeit: YouTubeLiveSichtbarkeit;
  autoStart: boolean;
  autoStop: boolean;
  freigegebenAm: string | null;
}

export interface YouTubeLiveStatus {
  zustand: YouTubeLiveZustand;
  hinweis: string | null;
  broadcastId: string | null;
  seit: string | null;
  wiederaufnehmbar: boolean;
}

export interface YouTubeLiveEntwurf {
  titel: string;
  sichtbarkeit: YouTubeLiveSichtbarkeit;
  autoStart: boolean;
  autoStop: boolean;
  liveFreigeben: boolean;
}

export interface UplinkYouTubeLiveProps {
  verbunden: boolean;
  kanalName: string | null;
  einstellungen: YouTubeLiveEinstellungen | null;
  status: YouTubeLiveStatus | null;
  beschaeftigt: boolean;
  fehlerText?: string | null;
  onSpeichern: (entwurf: YouTubeLiveEntwurf) => void;
  onBeenden: () => void;
}

type Uebersetzer = LanguageContextValue['t'];

function sichtbarkeiten(t: Uebersetzer): ReadonlyArray<{
  wert: YouTubeLiveSichtbarkeit;
  label: string;
  erklaerung: string;
}> {
  return [
    { wert: 'private', label: t('Privat'), erklaerung: t('Nur du siehst den Livestream.') },
    { wert: 'unlisted', label: t('Nicht gelistet'), erklaerung: t('Nur wer den Link hat, sieht den Livestream.') },
    { wert: 'public', label: t('Öffentlich'), erklaerung: t('Für alle sichtbar und in deinem Kanal gelistet.') },
  ];
}

const TITEL_MAX = 100;

export function entwurfAus(einstellungen: YouTubeLiveEinstellungen | null): YouTubeLiveEntwurf {
  if (!einstellungen) {
    return { titel: '', sichtbarkeit: 'private', autoStart: false, autoStop: false, liveFreigeben: false };
  }
  return {
    titel: einstellungen.titel,
    sichtbarkeit: einstellungen.sichtbarkeit,
    autoStart: einstellungen.autoStart,
    autoStop: einstellungen.autoStop,
    liveFreigeben: einstellungen.freigegebenAm !== null,
  };
}

export function speichernErlaubt(entwurf: YouTubeLiveEntwurf): boolean {
  const laenge = entwurf.titel.trim().length;
  return laenge >= 1 && entwurf.titel.length <= TITEL_MAX;
}

export function naechsterEntwurf(
  entwurf: YouTubeLiveEntwurf,
  einstellungen: YouTubeLiveEinstellungen | null,
  beruehrt: boolean,
): YouTubeLiveEntwurf {
  if (beruehrt) return entwurf;
  return entwurfAus(einstellungen);
}

function beendenErlaubt(status: YouTubeLiveStatus | null): boolean {
  if (!status) return false;
  return (
    status.zustand === 'vorbereitet' ||
    status.zustand === 'sendet' ||
    status.zustand === 'live' ||
    status.zustand === 'unklar'
  );
}

function datumText(wert: string, locale: string): string {
  const zeit = new Date(wert);
  if (Number.isNaN(zeit.getTime())) return wert;
  return zeit.toLocaleDateString(locale);
}

function uhrzeitText(wert: string, locale: string): string {
  const zeit = new Date(wert);
  if (Number.isNaN(zeit.getTime())) return wert;
  return zeit.toLocaleTimeString(locale);
}

function zustandText(status: YouTubeLiveStatus, t: Uebersetzer, locale: string): string {
  const hinweis = status.hinweis ?? t('kein Grund genannt');
  switch (status.zustand) {
    case 'inaktiv':
      return t('Kein Livestream vorbereitet');
    case 'vorbereitet':
      return t('Livestream angelegt, wartet auf dein Bild aus OBS');
    case 'sendet':
      return t('Bild kommt bei YouTube an, noch nicht live geschaltet');
    case 'live':
      return status.seit
        ? t('Auf YouTube live seit {uhrzeit}', { uhrzeit: uhrzeitText(status.seit, locale) })
        : t('Auf YouTube live');
    case 'beendet':
      return t('Livestream beendet');
    case 'blockiert':
      return t('Angehalten: {hinweis}', { hinweis });
    case 'fehler':
      return status.wiederaufnehmbar
        ? t('Fehler: {hinweis}. Uplink versucht es beim nächsten Stream erneut.', { hinweis })
        : t('Fehler: {hinweis}. Bitte prüfe deinen YouTube-Kanal und speichere die Einstellungen neu.', { hinweis });
    case 'unklar':
      return t('Stand wird mit YouTube abgeglichen');
    default:
      return t('Stand wird mit YouTube abgeglichen');
  }
}

function zustandKlasse(zustand: YouTubeLiveZustand): string {
  if (zustand === 'live') {
    return 'rounded-xl border border-success/30 bg-success/10 px-3 py-2 text-xs text-success';
  }
  if (zustand === 'blockiert' || zustand === 'fehler') {
    return 'rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning';
  }
  return 'rounded-xl border border-border/60 bg-background/40 px-3 py-2 text-xs text-text-secondary';
}

export function UplinkYouTubeLive({
  verbunden,
  kanalName,
  einstellungen,
  status,
  beschaeftigt,
  fehlerText,
  onSpeichern,
  onBeenden,
}: UplinkYouTubeLiveProps) {
  const { t, locale } = useLanguage();
  const basisId = useId();
  const titelId = `${basisId}-titel`;
  const titelHinweisId = `${basisId}-titel-hinweis`;
  const freigabeId = `${basisId}-freigabe`;
  const autoStartId = `${basisId}-auto-start`;
  const autoStopId = `${basisId}-auto-stop`;
  const sichtHinweisId = `${basisId}-sicht-hinweis`;

  const beruehrt = useRef(false);
  const [entwurf, setEntwurf] = useState<YouTubeLiveEntwurf>(() => entwurfAus(einstellungen));

  useEffect(() => {
    setEntwurf((v) => naechsterEntwurf(v, einstellungen, beruehrt.current));
  }, [einstellungen]);

  const aendern = (teil: Partial<YouTubeLiveEntwurf>) => {
    beruehrt.current = true;
    setEntwurf((v) => ({ ...v, ...teil }));
  };

  const speichern = () => {
    beruehrt.current = false;
    onSpeichern(entwurf);
  };

  if (!verbunden) {
    return (
      <section
        aria-label={t('YouTube Live')}
        className="space-y-2 rounded-2xl border border-border bg-background/70 p-4"
      >
        <h3 className="text-sm font-bold text-white">{t('YouTube Live')}</h3>
        <p className="flex items-start gap-2 text-xs text-text-secondary">
          <Info aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>
            {t('Verbinde zuerst dein YouTube-Konto über den Knopf oben in dieser Karte, dann kannst du hier den Livestream einrichten.')}
          </span>
        </p>
      </section>
    );
  }

  const titelLeer = entwurf.titel.trim().length === 0;
  const titelZuLang = entwurf.titel.length > TITEL_MAX;
  const erlaubt = speichernErlaubt(entwurf);
  const kannBeenden = beendenErlaubt(status);
  const freigegebenAm = einstellungen?.freigegebenAm ?? null;

  let freigabeTon: 'success' | 'warning' | 'neutral';
  let freigabeText: string;
  if (freigegebenAm && entwurf.liveFreigeben) {
    freigabeTon = 'success';
    freigabeText = t('Freigegeben am {datum}.', { datum: datumText(freigegebenAm, locale) });
  } else if (!freigegebenAm && entwurf.liveFreigeben) {
    freigabeTon = 'neutral';
    freigabeText = t('Wird mit dem Speichern freigegeben.');
  } else if (freigegebenAm && !entwurf.liveFreigeben) {
    freigabeTon = 'neutral';
    freigabeText = t('Wird mit dem Speichern zurückgenommen.');
  } else {
    freigabeTon = 'warning';
    freigabeText = t('Noch nicht freigegeben: beim OBS-Start legt Uplink keinen YouTube-Livestream an.');
  }

  return (
    <section
      aria-label={t('YouTube Live')}
      className="space-y-4 rounded-2xl border border-border bg-background/70 p-4"
    >
      <div className="space-y-0.5">
        <h3 className="text-sm font-bold text-white">{t('YouTube Live')}</h3>
        <p className="text-xs text-text-secondary">
          {kanalName
            ? t('Verbunden mit {kanal}. Lege Titel, Sichtbarkeit und den Ablauf für deinen Livestream fest.', { kanal: kanalName })
            : t('Lege Titel, Sichtbarkeit und den Ablauf für deinen Livestream fest.')}
        </p>
      </div>

      <div className="space-y-2 rounded-xl border border-border/60 bg-background/40 p-3">
        <p className="flex items-start gap-2 text-xs text-text-secondary">
          {freigabeTon === 'success' ? (
            <ShieldCheck aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0 text-success" />
          ) : freigabeTon === 'warning' ? (
            <TriangleAlert aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0 text-warning" />
          ) : (
            <Info aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          )}
          <span>{freigabeText}</span>
        </p>
        <label htmlFor={freigabeId} className="flex min-h-11 cursor-pointer items-start gap-3">
          <input
            id={freigabeId}
            type="checkbox"
            checked={entwurf.liveFreigeben}
            onChange={(e) => aendern({ liveFreigeben: e.target.checked })}
            className="mt-0.5 h-4 w-4 shrink-0 accent-primary focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-primary"
          />
          <span className="text-sm text-white">
            {t('Uplink darf bei OBS-Start einen YouTube-Livestream mit diesen Einstellungen anlegen.')}
          </span>
        </label>
      </div>

      <div className="space-y-1">
        <label
          htmlFor={titelId}
          className="block text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary"
        >
          {t('Titel des Livestreams')}
        </label>
        <input
          id={titelId}
          value={entwurf.titel}
          onChange={(e) => aendern({ titel: e.target.value })}
          aria-invalid={!erlaubt}
          aria-describedby={titelHinweisId}
          placeholder={t('Titel für deinen Livestream')}
          className="min-h-11 w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
        />
        <div id={titelHinweisId} className="flex items-center justify-between gap-3 text-[11px]">
          <span className={titelLeer || titelZuLang ? 'text-warning' : 'text-text-secondary'}>
            {titelLeer
              ? t('Bitte gib einen Titel ein, sonst lässt sich nicht speichern.')
              : titelZuLang
                ? t('Der Titel darf höchstens 100 Zeichen haben.')
                : t('Dieser Titel steht später bei deinem YouTube-Livestream.')}
          </span>
          <span className={titelZuLang ? 'text-warning' : 'text-text-secondary'}>
            {entwurf.titel.length}/{TITEL_MAX}
          </span>
        </div>
      </div>

      <fieldset
        aria-describedby={sichtHinweisId}
        className="space-y-3 rounded-xl border border-border/60 bg-background/40 p-3"
      >
        <legend className="px-1 text-xs font-semibold text-white">{t('Sichtbarkeit')}</legend>
        <div className="grid gap-2 sm:grid-cols-3">
          {sichtbarkeiten(t).map((eintrag) => (
            <label
              key={eintrag.wert}
              className={`flex min-h-11 cursor-pointer items-start gap-3 rounded-xl border p-3 ${
                entwurf.sichtbarkeit === eintrag.wert ? 'border-primary/60 bg-primary/10' : 'border-border bg-background/70'
              }`}
            >
              <input
                type="radio"
                name={`${basisId}-sichtbarkeit`}
                value={eintrag.wert}
                checked={entwurf.sichtbarkeit === eintrag.wert}
                onChange={() => aendern({ sichtbarkeit: eintrag.wert })}
                className="mt-0.5 h-4 w-4 shrink-0 accent-primary focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-primary"
              />
              <span className="space-y-1">
                <span className="block text-sm font-semibold text-white">{eintrag.label}</span>
                <span className="block text-xs text-text-secondary">{eintrag.erklaerung}</span>
              </span>
            </label>
          ))}
        </div>
        <p id={sichtHinweisId} className="text-xs text-text-secondary">
          {t('Die Sichtbarkeit gilt für den nächsten Livestream, den Uplink anlegt.')}
        </p>
      </fieldset>

      <div className="space-y-2 rounded-xl border border-border/60 bg-background/40 p-3">
        <label htmlFor={autoStartId} className="flex min-h-11 cursor-pointer items-start gap-3">
          <input
            id={autoStartId}
            type="checkbox"
            checked={entwurf.autoStart}
            onChange={(e) => aendern({ autoStart: e.target.checked })}
            className="mt-0.5 h-4 w-4 shrink-0 accent-primary focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-primary"
          />
          <span className="space-y-1">
            <span className="block text-sm font-semibold text-white">{t('Auto-Start')}</span>
            <span className="block text-xs text-text-secondary">
              {t('YouTube schaltet live, sobald Bild ankommt. Ohne Auto-Start schaltet Uplink erst, wenn dein Stream stabil ankommt.')}
            </span>
          </span>
        </label>
        <label htmlFor={autoStopId} className="flex min-h-11 cursor-pointer items-start gap-3">
          <input
            id={autoStopId}
            type="checkbox"
            checked={entwurf.autoStop}
            onChange={(e) => aendern({ autoStop: e.target.checked })}
            className="mt-0.5 h-4 w-4 shrink-0 accent-primary focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-primary"
          />
          <span className="space-y-1">
            <span className="block text-sm font-semibold text-white">{t('Auto-Stop')}</span>
            <span className="block text-xs text-text-secondary">
              {t('YouTube beendet den Livestream kurz nach dem Ende deines Streams. Ohne Auto-Stop beendest du ihn hier mit "Beenden".')}
            </span>
          </span>
        </label>
      </div>

      {status ? (
        <div className={zustandKlasse(status.zustand)} role="status">
          <span className="flex items-start gap-2">
            {status.zustand === 'live' ? (
              <Radio aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            ) : status.zustand === 'blockiert' || status.zustand === 'fehler' ? (
              <TriangleAlert aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            ) : (
              <Info aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            )}
            <span>{zustandText(status, t, locale)}</span>
          </span>
        </div>
      ) : null}

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          data-knopf="speichern"
          disabled={beschaeftigt || !erlaubt}
          onClick={speichern}
          className="inline-flex min-h-11 items-center gap-2 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
        >
          {beschaeftigt && <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin" />}
          {t('Speichern')}
        </button>
        <button
          type="button"
          data-knopf="beenden"
          disabled={beschaeftigt || !kannBeenden}
          onClick={() => onBeenden()}
          className="inline-flex min-h-11 items-center gap-2 rounded-xl border border-border px-4 py-2 text-sm font-semibold text-text-secondary transition-colors hover:text-white disabled:opacity-60"
        >
          {t('Beenden')}
        </button>
      </div>

      {fehlerText ? (
        <p role="alert" className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
          {fehlerText}
        </p>
      ) : null}

      {status?.broadcastId ? (
        <details className="text-xs text-text-secondary">
          <summary className="cursor-pointer">{t('Technische Details')}</summary>
          <p className="mt-2">{t('Broadcast-Kennung: {id}', { id: status.broadcastId })}</p>
        </details>
      ) : null}
    </section>
  );
}

export default UplinkYouTubeLive;
