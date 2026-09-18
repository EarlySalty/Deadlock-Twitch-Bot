import { useMemo, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  Globe,
  Info,
  MessageSquare,
  Radio,
  RefreshCw,
  ShieldAlert,
} from 'lucide-react';
// Relativ statt ueber den @-Alias: die Node-Tests laden diese Seite ohne Vite.
// useAuthStatus aus hooks/useAnalytics scheidet aus demselben Grund aus; der
// Query-Key 'auth-status' bleibt identisch, der Cache wird also geteilt.
import {
  COLLECTOR_DAYS,
  fetchCategoryCollector,
  parseCollectorDays,
  type CategoryCollectorData,
  type CollectorDays,
} from '../api/categoryCollector';
import { fetchAuthStatus } from '../api/auth';
import { useT } from '../context/LanguageContext';
import {
  buildHeatmap,
  buildTrendreihe,
  heatmapFarbe,
  klassifiziereSammlerFehler,
  formatBytes,
  formatMessdauer,
  formatZeitstempel,
  sammlerStatusAnsicht,
  sortiereTopKanaele,
  sprachLabel,
  zahlOderStrich,
} from './kategorieWeltweitViewModel';
import { TrendChart } from './kategorieWeltweitTrendChart';

const STALE_TIME_MS = 60 * 1000;

/**
 * Globale Kategorien-Seite ("Kategorien weltweit", globaler Kategoriesammler).
 *
 * Bewusst eine eigene Route und kein Analyse-Tab: Hier geht es nicht um einen
 * Streamer, sondern um die Gesamterhebung über alle Kanäle. Auswertung
 * ausschließlich nach Sprache, nie nach Land, Region oder Flaggen.
 * Stream-Sprache und Chat-Nachrichtensprache sind getrennte Dimensionen.
 */
export function KategorieWeltweitPage() {
  const t = useT();
  const {
    data: authStatus,
    isLoading: loadingAuth,
  } = useQuery({
    queryKey: ['auth-status'],
    queryFn: fetchAuthStatus,
    staleTime: 60 * 1000,
    retry: false,
  });
  const isAdminView = Boolean(authStatus?.isAdmin || authStatus?.isLocalhost);

  let inhalt: ReactNode;
  if (loadingAuth) {
    inhalt = (
      <div className="panel-card rounded-2xl p-8 text-center text-text-secondary">
        {t('Zugriff wird geprüft…')}
      </div>
    );
  } else if (!authStatus) {
    // Auth-Abfrage selbst fehlgeschlagen (Backend nicht erreichbar): Fehler
    // zeigen, nicht fälschlich "Nicht angemeldet" behaupten.
    inhalt = (
      <KeinZugriffKarte
        titel={t('Zugriffsprüfung fehlgeschlagen')}
        text={t(
          'Der Login-Status konnte nicht geladen werden. Bitte die Seite neu laden oder später erneut versuchen.',
        )}
      />
    );
  } else if (!authStatus.authenticated) {
    inhalt = <KeinZugriffKarte titel={t('Nicht angemeldet')} text={t('Diese Seite braucht eine angemeldete Session. Bitte erneut einloggen.')} />;
  } else if (!isAdminView) {
    inhalt = (
      <KeinZugriffKarte
        titel={t('Kein Zugriff')}
        text={t('Der globale Kategoriesammler ist nur für Admins erreichbar.')}
      />
    );
  } else {
    inhalt = <SammlerInhalt />;
  }

  return (
    <>
      <div className="panel-card rounded-2xl p-5 md:p-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="mb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-primary">
              {t('Admin')}
            </div>
            <h1 className="display-font flex items-center gap-2 text-2xl font-extrabold text-white">
              <Globe className="h-6 w-6 text-primary" aria-hidden />
              {t('Kategorien weltweit')}
            </h1>
            <p className="mt-2 max-w-2xl text-sm leading-5 text-text-secondary">
              {t(
                'Globaler Kategoriesammler über alle beobachteten Kanäle. Auswertung nach Sprache, nicht nach Land oder Region. „und" bedeutet: Sprache nicht sicher erkannt.',
              )}
            </p>
          </div>
        </div>
      </div>
      {inhalt}
    </>
  );
}

function KeinZugriffKarte({ titel, text }: { titel: string; text: string }) {
  return (
    <div className="panel-card rounded-2xl p-8 text-center">
      <ShieldAlert className="mx-auto mb-4 h-12 w-12 text-warning" aria-hidden />
      <h2 className="mb-2 text-xl font-bold text-white">{titel}</h2>
      <p className="text-text-secondary">{text}</p>
    </div>
  );
}

function SammlerInhalt() {
  const t = useT();
  const [days, setDays] = useState<CollectorDays>(7);

  const { data, error, isLoading, isFetching, isPlaceholderData, refetch } = useQuery({
    queryKey: ['category-collector', days],
    queryFn: () => fetchCategoryCollector(days),
    staleTime: STALE_TIME_MS,
    placeholderData: (previous) => previous,
  });

  return (
    <>
      <div className="panel-card rounded-2xl px-5 py-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div
            className="flex flex-wrap gap-2"
            role="group"
            aria-label={t('Zeitraum wählen')}
          >
            {COLLECTOR_DAYS.map((option) => (
              <button
                key={option}
                type="button"
                aria-pressed={days === option}
                onClick={() => setDays(parseCollectorDays(option))}
                className={`rounded-xl border px-3 py-1.5 text-sm font-semibold transition-colors ${
                  days === option
                    ? 'border-primary bg-primary/10 text-primary'
                    : 'border-border bg-background/60 text-text-secondary hover:border-border-hover hover:text-white'
                }`}
              >
                {t('{tage} Tage', { tage: option })}
              </button>
            ))}
          </div>
          <div className="flex items-center gap-3">
            {(isFetching || isPlaceholderData) && (
              <span className="text-xs text-text-secondary" data-testid="sammler-laedt">
                {t('Zeitraum wird geladen…')}
              </span>
            )}
            <button
              type="button"
              onClick={() => void refetch()}
              disabled={isFetching}
              className="flex items-center gap-2 rounded-xl border border-border bg-background/60 px-3 py-1.5 text-sm font-medium text-text-secondary transition-colors hover:border-border-hover hover:text-white disabled:cursor-not-allowed disabled:opacity-60"
            >
              <RefreshCw className={`h-4 w-4 ${isFetching ? 'animate-spin' : ''}`} aria-hidden />
              {t('Aktualisieren')}
            </button>
          </div>
        </div>
      </div>

      {error ? (
        <SammlerFehlerKarte error={error} />
      ) : isLoading ? (
        <div className="panel-card rounded-2xl p-8 text-center text-text-secondary">
          {t('Auswertung wird geladen…')}
        </div>
      ) : data ? (
        <div className={isPlaceholderData ? 'space-y-4 opacity-60' : 'space-y-4'} data-days={days}>
          <SammlerStatusUndMeta data={data} />
          <SammlerSektionen data={data} />
        </div>
      ) : null}
    </>
  );
}

export function SammlerFehlerKarte({ error }: { error: unknown }) {
  const t = useT();
  const ansicht = klassifiziereSammlerFehler(error);
  return (
    <div
      className="panel-card rounded-2xl border-warning/30 p-8 text-center"
      data-testid="sammler-fehler"
      role="alert"
    >
      <AlertTriangle className="mx-auto mb-4 h-12 w-12 text-warning" aria-hidden />
      <h2 className="mb-2 text-xl font-bold text-white">{ansicht.titel}</h2>
      <p className="mx-auto max-w-xl text-text-secondary">{ansicht.text}</p>
      <p className="mt-3 text-xs text-text-secondary">
        {t('Fehler werden hier als Fehler gezeigt und nicht als leere Statistik.')}
      </p>
    </div>
  );
}

function SammlerStatusUndMeta({ data }: { data: CategoryCollectorData }) {
  const t = useT();
  const meta = data.meta;
  const status = sammlerStatusAnsicht(meta.collector_status);
  return (
    <>
      <div
        className="rounded-2xl border border-white/10 bg-white/[0.04] px-5 py-4"
        data-testid="sammler-status"
      >
        <div className="flex items-start gap-3">
          <Info className="mt-0.5 h-5 w-5 shrink-0 text-primary" aria-hidden />
          <div>
            <p className="font-medium text-white">
              {t('Sammler-Status')}: {status.label}
            </p>
            {status.hinweis && <p className="mt-1 text-sm text-text-secondary">{status.hinweis}</p>}
          </div>
        </div>
      </div>

      <div className="panel-card rounded-2xl p-5" data-testid="sammler-meta">
        <h2 className="mb-4 text-lg font-bold text-white">{t('Erfassung und Datenstand')}</h2>
        <dl className="grid grid-cols-1 gap-x-6 gap-y-3 text-sm sm:grid-cols-2 xl:grid-cols-3">
          <MetaEintrag label={t('Zeitraum')} value={`${formatZeitstempel(meta.period_start)} bis ${formatZeitstempel(meta.period_end)} (UTC)`} />
          <MetaEintrag label={t('Messdauer')} value={formatMessdauer(meta.measurement_seconds)} />
          <MetaEintrag label={t('Letzte vollständige Abfrage')} value={formatZeitstempel(meta.last_completed_poll_at)} />
          <MetaEintrag label={t('Letzte gesicherte Nachricht')} value={formatZeitstempel(meta.last_message_at)} />
          <MetaEintrag label={t('Erste Erfassung')} value={formatZeitstempel(meta.first_seen_at)} />
          <MetaEintrag label={t('Vollständige Abfragen im Zeitraum')} value={zahlOderStrich(meta.complete_polls)} />
          <MetaEintrag label={t('Abbrüche im Zeitraum')} value={zahlOderStrich(meta.incomplete_polls)} />
          <MetaEintrag
            label={t('Nachrichten-Drops (kumulativ)')}
            value={zahlOderStrich(meta.dropped_messages)}
            hinweis={t('Kumulativer Zähler seit Sammlerstart, nicht auf den Zeitraum begrenzt.')}
          />
          <MetaEintrag label={t('Speicher')} value={formatBytes(meta.storage_bytes)} />
          <MetaEintrag label={t('Rohchat-Aufbewahrung')} value={t('{tage} Tage', { tage: meta.retention_days })} hinweis={t('Snapshots und Stundenaggregate bleiben erhalten. Rohchat wird stundenweise spätestens nach Ablauf der Frist entfernt.')}  />
          <MetaEintrag label={t('Zeitzone')} value={meta.timezone || 'UTC'} />
        </dl>
      </div>
    </>
  );
}

function MetaEintrag({ label, value, hinweis }: { label: string; value: string; hinweis?: string }) {
  return (
    <div>
      <dt className="text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
        {label}
      </dt>
      <dd className="mt-0.5 font-semibold text-white">{value}</dd>
      {hinweis && <p className="mt-0.5 text-xs text-text-secondary">{hinweis}</p>}
    </div>
  );
}

/**
 * Alle Statistiksektionen. Bei collector_status "not_started" bleibt hier
 * bewusst der ehrliche Hinweis stehen, statt Nullen als Daten auszugeben.
 */
export function SammlerSektionen({ data }: { data: CategoryCollectorData }) {
  const t = useT();
  const nichtGestartet = data.meta.collector_status === 'not_started';
  const keineDaten =
    data.stream_languages.length === 0 &&
    data.message_languages.length === 0 &&
    data.trend.length === 0 &&
    data.top_channels.length === 0;

  if (nichtGestartet || keineDaten) {
    return (
      <div className="panel-card rounded-2xl p-8 text-center" data-testid="sammler-leer">
        <Globe className="mx-auto mb-4 h-12 w-12 text-primary" aria-hidden />
        <h2 className="mb-2 text-xl font-bold text-white">{t('Noch keine Daten')}</h2>
        <p className="mx-auto max-w-xl text-text-secondary">
          {t(
            'Der Sammler hat für diesen Zeitraum noch nichts beobachtet. Sobald erste Streams erfasst sind, erscheinen hier Sprachen, Trend und Top-Kanäle.',
          )}
        </p>
      </div>
    );
  }

  return (
    <>
      {data.trend.length > 0 && <TrendKarte daten={data.trend} />}
      <StreamSprachenTabelle daten={data.stream_languages} />
      <NachrichtSprachenTabelle daten={data.message_languages} />
      {data.top_channels.length > 0 && <TopKanaeleKarte daten={data.top_channels} />}
      {data.chat_heatmap.length > 0 && <ChatHeatmapKarte daten={data.chat_heatmap} />}
    </>
  );
}

export function StreamSprachenTabelle({
  daten,
}: {
  daten: CategoryCollectorData['stream_languages'];
}) {
  const t = useT();
  const sortiert = useMemo(
    () => [...daten].sort((a, b) => b.viewer_hours - a.viewer_hours),
    [daten],
  );
  return (
    <div className="panel-card rounded-2xl p-5" data-testid="sammler-streamsprachen">
      <h2 className="text-lg font-bold text-white">{t('Stream-Sprachen')}</h2>
      <p className="mt-1 text-sm text-text-secondary">
        {t(
          'Aus der eingestellten Stream-Sprache der Kanäle. Der Zuschauerschnitt ist nach Sendestunden gewichtet.',
        )}
      </p>
      <div className="mt-4 overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-[11px] uppercase tracking-[0.14em] text-text-secondary">
              <th className="pb-2 pr-4 font-semibold">{t('Sprache')}</th>
              <th className="pb-2 pr-4 font-semibold">{t('Streams')}</th>
              <th className="pb-2 pr-4 font-semibold">{t('Kanäle')}</th>
              <th className="pb-2 pr-4 font-semibold">{t('Sendestunden')}</th>
              <th className="pb-2 pr-4 font-semibold">{t('Zuschauerstunden')}</th>
              <th className="pb-2 font-semibold">{t('Ø Zuschauer (gewichtet)')}</th>
            </tr>
          </thead>
          <tbody>
            {sortiert.map((zeile) => (
              <tr key={zeile.language} className="border-t border-border">
                <td className="py-2 pr-4 font-medium text-white">{sprachLabel(zeile.language)}</td>
                <td className="py-2 pr-4 text-text-secondary">{zahlOderStrich(zeile.unique_streams)}</td>
                <td className="py-2 pr-4 text-text-secondary">{zahlOderStrich(zeile.unique_channels)}</td>
                <td className="py-2 pr-4 text-text-secondary">{zahlOderStrich(zeile.broadcast_hours, 1)}</td>
                <td className="py-2 pr-4 text-text-secondary">{zahlOderStrich(zeile.viewer_hours, 1)}</td>
                <td className="py-2 text-text-secondary">{zahlOderStrich(zeile.avg_viewers, 1)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export function NachrichtSprachenTabelle({
  daten,
}: {
  daten: CategoryCollectorData['message_languages'];
}) {
  const t = useT();
  const sortiert = useMemo(() => [...daten].sort((a, b) => b.messages - a.messages), [daten]);
  return (
    <div className="panel-card rounded-2xl p-5" data-testid="sammler-nachrichtensprachen">
      <h2 className="flex items-center gap-2 text-lg font-bold text-white">
        <MessageSquare className="h-5 w-5 text-primary" aria-hidden />
        {t('Chat-Nachrichtensprachen')}
      </h2>
      <p className="mt-1 text-sm text-text-secondary">
        {t(
          'Aus erkannten Chat-Nachrichten, unabhängig von der Stream-Sprache ermittelt. Keine Nachrichteninhalte, nur die Sprachverteilung.',
        )}
      </p>
      <div className="mt-4 overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-[11px] uppercase tracking-[0.14em] text-text-secondary">
              <th className="pb-2 pr-4 font-semibold">{t('Nachrichtensprache')}</th>
              <th className="pb-2 font-semibold">{t('Nachrichten')}</th>
            </tr>
          </thead>
          <tbody>
            {sortiert.map((zeile) => (
              <tr key={zeile.language} className="border-t border-border">
                <td className="py-2 pr-4 font-medium text-white">{sprachLabel(zeile.language)}</td>
                <td className="py-2 text-text-secondary">{zahlOderStrich(zeile.messages)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function TopKanaeleKarte({ daten }: { daten: CategoryCollectorData['top_channels'] }) {
  const t = useT();
  const [filter, setFilter] = useState<string>('alle');
  const sprachen = useMemo(
    () => [...new Set(daten.map((kanal) => kanal.language))].sort((a, b) => sprachLabel(a).localeCompare(sprachLabel(b))),
    [daten],
  );
  const gefiltert = useMemo(
    () =>
      sortiereTopKanaele(
        filter === 'alle' ? daten : daten.filter((kanal) => kanal.language === filter),
      ),
    [daten, filter],
  );

  return (
    <div className="panel-card rounded-2xl p-5" data-testid="sammler-topkanaele">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-lg font-bold text-white">{t('Top-Kanäle')}</h2>
          <p className="mt-1 text-sm text-text-secondary">
            {t(
              'Impact-Metrik ist Zuschauerstunden. Kanal- und Streamzahlen werden nicht über Stunden aufsummiert.',
            )}
          </p>
        </div>
        <label className="flex items-center gap-2 text-sm text-text-secondary">
          {t('Stream-Sprache')}
          <select
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            className="rounded-xl border border-border bg-background/80 px-3 py-1.5 text-sm font-medium text-white outline-none transition-colors focus:border-border-hover"
          >
            <option value="alle">{t('Alle Sprachen')}</option>
            {sprachen.map((sprache) => (
              <option key={sprache} value={sprache}>
                {sprachLabel(sprache)}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="mt-4 overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-[11px] uppercase tracking-[0.14em] text-text-secondary">
              <th className="pb-2 pr-4 font-semibold">{t('Kanal')}</th>
              <th className="pb-2 pr-4 font-semibold">{t('Stream-Sprache')}</th>
              <th className="pb-2 pr-4 font-semibold">{t('Sendestunden')}</th>
              <th className="pb-2 pr-4 font-semibold">{t('Zuschauerstunden')}</th>
              <th className="pb-2 font-semibold">{t('Ø Zuschauer')}</th>
            </tr>
          </thead>
          <tbody>
            {gefiltert.map((kanal) => (
              <tr key={`${kanal.language}-${kanal.user_id || kanal.login}`} className="border-t border-border">
                <td className="py-2 pr-4 font-medium text-white">
                  {kanal.display_name || kanal.login}
                  {kanal.login && kanal.display_name && kanal.login !== kanal.display_name && (
                    <span className="ml-2 text-xs text-text-secondary">{kanal.login}</span>
                  )}
                </td>
                <td className="py-2 pr-4 text-text-secondary">{sprachLabel(kanal.language)}</td>
                <td className="py-2 pr-4 text-text-secondary">{zahlOderStrich(kanal.broadcast_hours, 1)}</td>
                <td className="py-2 pr-4 font-semibold text-white">{zahlOderStrich(kanal.viewer_hours, 1)}</td>
                <td className="py-2 text-text-secondary">{zahlOderStrich(kanal.avg_viewers, 1)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function ChatHeatmapKarte({ daten }: { daten: CategoryCollectorData['chat_heatmap'] }) {
  const t = useT();
  const ansicht = useMemo(() => buildHeatmap(daten), [daten]);
  const stunden = Array.from({ length: 24 }, (_, i) => i);
  return (
    <div className="panel-card rounded-2xl p-5" data-testid="sammler-heatmap">
      <h2 className="text-lg font-bold text-white">{t('Chat-Nachrichten nach Tageszeit')}</h2>
      <p className="mt-1 text-sm text-text-secondary">
        {t('Nachrichtensprache je Stunde, alle Zeiten in UTC.')}
      </p>
      <div className="mt-4 overflow-x-auto">
        <div className="min-w-[640px]">
          <div className="mb-1 flex">
            <div className="w-40 shrink-0" />
            {stunden.map((stunde) => (
              <div key={stunde} className="flex-1 text-center text-xs text-text-secondary">
                {stunde % 3 === 0 ? `${stunde}` : ''}
              </div>
            ))}
          </div>
          {ansicht.reihen.map((reihe) => (
            <div key={reihe.language} className="mb-1 flex items-center">
              <div className="w-40 shrink-0 truncate pr-3 text-sm text-white">
                {sprachLabel(reihe.language)}
              </div>
              {reihe.stunden.map((wert, stunde) => (
                <div
                  key={stunde}
                  className="mx-0.5 h-6 flex-1 rounded-sm"
                  style={{ backgroundColor: heatmapFarbe(wert, ansicht.max) }}
                  title={
                    wert === null
                      ? `${sprachLabel(reihe.language)}, ${String(stunde).padStart(2, '0')}:00 UTC: ${t('keine Nachrichten beobachtet')}`
                      : `${sprachLabel(reihe.language)}, ${String(stunde).padStart(2, '0')}:00 UTC: ${zahlOderStrich(wert)} ${t('Nachrichten')}`
                  }
                />
              ))}
            </div>
          ))}
          <div className="mt-2 flex items-center gap-2 text-xs text-text-secondary">
            <span>{t('0')}</span>
            {[0.06, 0.3, 0.55, 0.8, 0.9].map((alpha) => (
              <span
                key={alpha}
                className="inline-block h-3 w-6 rounded-sm"
                style={{ backgroundColor: `rgba(197, 160, 89, ${alpha})` }}
              />
            ))}
            <span>{zahlOderStrich(ansicht.max)}</span>
            <span>· {t('Stunde (UTC)')}</span>
          </div>
        </div>
      </div>
    </div>
  );
}

function TrendKarte({ daten }: { daten: CategoryCollectorData['trend'] }) {
  const t = useT();
  const reihe = useMemo(() => buildTrendreihe(daten), [daten]);
  return (
    <div className="panel-card rounded-2xl p-5" data-testid="sammler-trend">
      <h2 className="flex items-center gap-2 text-lg font-bold text-white">
        <Radio className="h-5 w-5 text-primary" aria-hidden />
        {t('Gleichzeitige Streams und Zuschauer')}
      </h2>
      <p className="mt-1 text-sm text-text-secondary">
        {t(
          'Tagesmittel je Messpunkt, in UTC. Tage ohne Erfassung bleiben Lücke und werden nicht interpoliert.',
        )}
      </p>
      <div className="mt-4 h-72">
        {/* Recharts misst seinen Container und läuft deshalb nur im Browser.
            Die Datenregeln (Lücken statt Nullen) liegen in
            kategorieWeltweitViewModel.buildTrendreihe und sind dort getestet. */}
        <TrendChart reihe={reihe} />
      </div>
    </div>
  );
}
