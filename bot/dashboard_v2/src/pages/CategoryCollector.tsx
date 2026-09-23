import { useEffect, useMemo, useState } from 'react';
import { languageName } from '../categoryCollector';
import { useQuery } from '@tanstack/react-query';
import { Bar, BarChart, CartesianGrid, Legend, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { Activity, Languages, Radio, Users } from 'lucide-react';

const GOLD = 'var(--color-primary)';
const AMBER = 'var(--color-warning)';
const COPPER = 'var(--color-danger)';
interface LanguageRow { language: string; streams: number | null; channels: number | null; airtime_hours: number | null; avg_viewers: number | null; viewer_hours: number | null; messages: number }
interface ChannelRow { language: string; user_id: string; login: string; airtime_hours: number; avg_viewers: number; viewer_hours: number; rank: number }
interface TrendRow { at: string; streams: number; viewers: number; polls: number; peak_streams: number; peak_viewers: number }
interface Status { desired_channels?: number; confirmed_channels?: number; connected_shards?: number; discovery_state?: string; received_messages?: number; stored_this_process?: number; queue_drops?: number; storage_drops?: number; invalid_events?: number; raw_bytes?: number; retention_days?: number; retention_pressure?: boolean; raw_paused?: boolean; oldest_raw_message?: string; last_discovery?: string; media_enabled?: boolean }
export interface CategoryReport { days: number; generated_at: string; heartbeat_at: string | null; first_snapshot: string | null; last_snapshot: string | null; total_polls: number; languages: LanguageRow[]; top_channels: ChannelRow[]; trend: TrendRow[]; hourly: { language: string; hour: number; messages: number }[]; status: Status | null }
const number = (value: number | null | undefined, digits = 0) => value == null ? '—' : new Intl.NumberFormat('de-DE', { maximumFractionDigits: digits }).format(value);
const dateTime = (date: string | null | undefined) => date ? new Date(date).toLocaleString('de-DE', { timeZone: 'Europe/Berlin' }) : 'Noch keine Messung';
const tooltipStyle = { background: 'var(--color-card)', border: '1px solid var(--color-border)', borderRadius: 12, color: 'var(--color-text-primary)' };

export function CategoryCollector() {
  const [days, setDays] = useState(7);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 10_000);
    return () => clearInterval(timer);
  }, []);
  const [language, setLanguage] = useState('all');
  const query = useQuery<CategoryReport>({
    queryKey: ['category-collector', days],
    queryFn: async () => {
      const response = await fetch(`/twitch/api/v2/category-collector?days=${days}`, { credentials: 'same-origin', cache: 'no-store' });
      if (response.status === 401 || response.status === 403) throw new Error('Diese Auswertung ist nur im Admin-Modus verfügbar.');
      if (!response.ok) throw new Error('Die Kategoriedaten konnten nicht geladen werden. Bestehende Daten werden nicht als Nullwerte dargestellt.');
      return response.json() as Promise<CategoryReport>;
    },
    refetchInterval: 60_000,
    retry: false,
  });
  const data = query.data;
  const totals = useMemo(() => data?.languages.reduce((sum, row) => ({ hours: sum.hours + (row.airtime_hours ?? 0), messages: sum.messages + row.messages, viewerHours: sum.viewerHours + (row.viewer_hours ?? 0) }), { hours: 0, messages: 0, viewerHours: 0 }), [data]);
  const chart = useMemo(() => {
    const rows: Array<{ at: string; streams: number | null; viewers: number | null; label: string }> = [];
    let previous: number | null = null;
    for (const row of data?.trend ?? []) {
      const timestamp = new Date(row.at).getTime();
      if (previous !== null && timestamp - previous > 3_600_000) {
        rows.push({ at: new Date(previous + 3_600_000).toISOString(), streams: null, viewers: null, label: 'Messlücke' });
      }
      rows.push({ ...row, label: new Date(row.at).toLocaleString('de-DE', { timeZone: 'UTC', month: '2-digit', day: '2-digit', hour: '2-digit' }) });
      previous = timestamp;
    }
    return rows;
  }, [data]);
  const hourly = useMemo(() => Array.from({ length: 24 }, (_, hour) => ({ hour: `${String(hour).padStart(2, '0')}:00`, messages: data?.hourly.filter(row => row.hour === hour && (language === 'all' || row.language === language)).reduce((sum, row) => sum + row.messages, 0) ?? 0 })), [data, language]);
  const stale = !data?.heartbeat_at || now - new Date(data.heartbeat_at).getTime() > 120_000;
  const status = data?.status;
  const cards = [
    { icon: Radio, title: 'Entdeckte Live-Kanäle', value: stale ? '—' : number(status?.desired_channels), detail: 'Letzter erfolgreicher Kategorieabruf' },
    { icon: Activity, title: 'Beobachtete Sendestunden', value: number(totals?.hours, 1), detail: 'Aus Messintervallen geschätzt' },
    { icon: Users, title: 'Zuschauerstunden', value: number(totals?.viewerHours, 1), detail: 'Keine einzelnen oder eindeutigen Zuschauer' },
    { icon: Languages, title: 'Chat-Nachrichten', value: number(totals?.messages), detail: 'Ohne doppelt zugestellte Shared-Chat-Kopien' },
  ];

  return <main className="mx-auto w-full max-w-[1500px] space-y-6 p-4 md:p-8">
    <header className="flex flex-wrap items-end justify-between gap-4">
      <div><p className="text-sm font-semibold uppercase tracking-widest text-primary">Twitch · Admin-Auswertung</p><h1 className="mt-2 text-3xl font-bold text-white">Deadlock weltweit</h1><p className="mt-3 max-w-3xl text-text-secondary">Die gesamte Live-Kategorie, über alle Sprachen. Sprache ist kein Land und keine Zuschauer-Geolokation. Der Collector liest Chat anonym und sendet nichts.</p></div>
      <label className="flex items-center gap-3 text-sm text-text-secondary">Zeitraum<select aria-label="Zeitraum" value={days} onChange={event => setDays(Number(event.target.value))} className="rounded-xl border border-primary/30 bg-bg px-4 py-3 text-white">{[7, 30, 90].map(period => <option key={period} value={period}>{period} Tage</option>)}</select></label>
    </header>
    {query.isPending && <div className="panel-card rounded-2xl p-8 text-text-secondary" role="status">Kategoriedaten werden geladen …</div>}
    {query.isError && <div role="alert" className="panel-card rounded-2xl border border-primary/30 p-6"><p className="text-white">{query.error.message}</p><button className="mt-3 text-primary underline" onClick={() => void query.refetch()}>Erneut laden</button></div>}
    {data && <>
      <section className="panel-card rounded-2xl p-5 text-sm text-text-secondary" aria-label="Messabdeckung">
        <div className="flex flex-wrap justify-between gap-3"><strong className="text-white">{stale ? 'Collector-Status veraltet oder noch nicht vorhanden' : status?.discovery_state === 'ok' ? 'Sammlung aktiv' : 'Sammlung mit Einschränkungen'}</strong><span>Letzter Abruf: {dateTime(data.last_snapshot)} (Berlin)</span></div>
        <p className="mt-2">Datenbeginn: {dateTime(data.first_snapshot)}. {number(data.total_polls)} erfolgreiche Messungen insgesamt. Bestätigte Chat-Kanäle: {number(status?.confirmed_channels)} von {number(status?.desired_channels)}; anonyme Verbindungen: {number(status?.connected_shards)}.</p>
        <p className="mt-2">Rohchat: {number(status?.raw_bytes == null ? null : status.raw_bytes / (1024 ** 3), 2)} GiB. Reguläre Aufbewahrung: maximal {status?.retention_days ?? 90} Tage. Älteste Rohzeile: {dateTime(status?.oldest_raw_message)}. Stundenaggregate bleiben erhalten.</p>
        <p className="mt-2">Verworfene Warteschlangenereignisse: {number(status?.queue_drops)} · Speicherbedingt nicht gespeichert: {number(status?.storage_drops)} · Ungültige oder nicht mehr zuordenbare Ereignisse: {number(status?.invalid_events)}. Diese Zähler gelten seit dem letzten Prozessstart.</p>
        {(status?.retention_pressure || status?.raw_paused) && <p className="mt-3 font-semibold text-primary">Speichergrenze erreicht: Die Rohdatenabdeckung ist eingeschränkt. {status.raw_paused ? 'Neue Rohzeilen werden derzeit nicht gespeichert.' : 'Ältere Rohpartitionen wurden nach der Aggregation vorzeitig entfernt.'}</p>}
        <p className="mt-2">Ausfälle, Beitrittsverzögerungen und nicht gelieferte Nachrichten sind keine Null-Aktivität. Keine vollständige Zuschauer- oder Lurker-Liste. {status?.media_enabled ? 'VOD- und Clip-Metadaten werden zusätzlich gesammelt; keine Medien werden heruntergeladen.' : 'VOD- und Clip-Metadaten sind nicht aktiviert.'}</p>
      </section>
      <section className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">{cards.map(card => <article key={card.title} className="panel-card rounded-2xl p-5"><card.icon className="mb-4 h-5 w-5 text-primary" /><h2 className="text-sm text-text-secondary">{card.title}</h2><p className="mt-2 text-3xl font-bold text-white">{card.value}</p><p className="mt-2 text-xs text-text-secondary">{card.detail}</p></article>)}</section>
      {!data.total_polls ? <section className="panel-card rounded-2xl p-8 text-text-secondary">Noch keine vollständige Kategoriemessung vorhanden. Es werden keine historischen Daten vorgetäuscht.</section> : <>
        <section className="panel-card rounded-2xl p-5 md:p-6"><h2 className="text-xl font-semibold text-white">Aktivität nach Sprache</h2><p className="mt-2 text-sm text-text-secondary">Streams und Zuschauerwerte folgen der eingestellten Stream-Sprache. Chat-Nachrichten folgen der erkannten Nachrichtensprache. Beide können abweichen; kurze oder unsichere Texte bleiben unbekannt.</p><div className="mt-5 overflow-x-auto"><table className="w-full text-left text-sm"><thead className="border-b border-primary/20 text-text-secondary"><tr>{['Sprache', 'Streams', 'Kanäle', 'Sendestunden', 'Ø Zuschauer pro Stream', 'Chat-Nachrichten'].map(label => <th key={label} scope="col" className="whitespace-nowrap px-3 py-3">{label}</th>)}</tr></thead><tbody>{data.languages.map(row => <tr key={row.language} className="border-b border-white/5 text-white"><th scope="row" className="px-3 py-4 font-medium">{languageName(row.language)} <span className="text-text-secondary">({row.language})</span></th><td className="px-3 py-4">{number(row.streams)}</td><td className="px-3 py-4">{number(row.channels)}</td><td className="px-3 py-4">{number(row.airtime_hours, 1)}</td><td className="px-3 py-4">{number(row.avg_viewers, 1)}</td><td className="px-3 py-4">{number(row.messages)}</td></tr>)}</tbody></table></div></section>
        <section className="panel-card rounded-2xl p-5 md:p-6"><h2 className="text-xl font-semibold text-white">Kategorie-Trend</h2><p className="mt-2 text-sm text-text-secondary">Stundenmittel der erfolgreichen Messungen; Zeitachse UTC. Zuschauer und Streams haben getrennte Skalen.</p><div className="mt-6 h-80"><ResponsiveContainer width="100%" height="100%"><LineChart data={chart}><CartesianGrid stroke="var(--color-border)" /><XAxis dataKey="label" minTickGap={55} tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }} /><YAxis yAxisId="streams" tick={{ fill: GOLD }} /><YAxis yAxisId="viewers" orientation="right" tick={{ fill: AMBER }} /><Tooltip contentStyle={tooltipStyle} /><Legend /><Line yAxisId="streams" type="linear" dataKey="streams" name="Gleichzeitige Streams" stroke={GOLD} dot={chart.length < 3} connectNulls={false} /><Line yAxisId="viewers" type="linear" dataKey="viewers" name="Gleichzeitige Zuschauer" stroke={AMBER} dot={chart.length < 3} connectNulls={false} /></LineChart></ResponsiveContainer></div></section>
        <section className="panel-card rounded-2xl p-5 md:p-6"><div className="flex flex-wrap items-center justify-between gap-3"><div><h2 className="text-xl font-semibold text-white">Chat nach Sprache und Tageszeit</h2><p className="mt-2 text-sm text-text-secondary">Nachrichten je UTC-Stunde, summiert über den gewählten Zeitraum. Kein regionales Tageszeitprofil.</p></div><select aria-label="Sprache" value={language} onChange={event => setLanguage(event.target.value)} className="rounded-xl border border-primary/30 bg-bg px-4 py-3 text-white"><option value="all">Alle Sprachen</option>{data.languages.map(row => <option key={row.language} value={row.language}>{languageName(row.language)}</option>)}</select></div><div className="mt-6 h-72"><ResponsiveContainer width="100%" height="100%"><BarChart data={hourly}><XAxis dataKey="hour" minTickGap={25} tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }} /><YAxis tick={{ fill: 'var(--color-text-secondary)' }} /><Tooltip contentStyle={tooltipStyle} /><Bar dataKey="messages" name="Nachrichten" fill={COPPER} radius={[5, 5, 0, 0]} /></BarChart></ResponsiveContainer></div></section>
        <section className="panel-card rounded-2xl p-5 md:p-6"><h2 className="text-xl font-semibold text-white">Top-Kanäle je Stream-Sprache</h2><p className="mt-2 text-sm text-text-secondary">Bis zu zehn Kanäle pro Sprache, sortiert nach beobachteten Zuschauerstunden: Sendezeit × Zuschauerzahl. Keine Bewertung einzelner Personen.</p><div className="mt-5 overflow-x-auto"><table className="w-full text-left text-sm"><thead className="border-b border-primary/20 text-text-secondary"><tr>{['Sprache', 'Kanal', 'Sendestunden', 'Ø Zuschauer', 'Zuschauerstunden'].map(label => <th key={label} scope="col" className="px-3 py-3">{label}</th>)}</tr></thead><tbody>{data.top_channels.filter(row => language === 'all' || row.language === language).map(row => <tr key={`${row.language}-${row.user_id}`} className="border-b border-white/5 text-white"><td className="px-3 py-3">{languageName(row.language)}</td><th scope="row" className="px-3 py-3 font-medium">{row.login}</th><td className="px-3 py-3">{number(row.airtime_hours, 1)}</td><td className="px-3 py-3">{number(row.avg_viewers, 1)}</td><td className="px-3 py-3">{number(row.viewer_hours, 1)}</td></tr>)}</tbody></table></div></section>
      </>}
    </>}
  </main>;
}
