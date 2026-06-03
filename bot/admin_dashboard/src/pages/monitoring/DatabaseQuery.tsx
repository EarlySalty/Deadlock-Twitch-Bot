import { Play, RefreshCw } from 'lucide-react';
import { useState } from 'react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';

const KNOWN_TABLES = [
  'twitch_streamers',
  'twitch_partners',
  'twitch_live_state',
  'twitch_stream_sessions',
  'twitch_stats_tracked',
  'twitch_stats_category',
  'twitch_raid_history',
  'twitch_eventsub_capacity_snapshot',
  'twitch_raw_chat_ingest_health',
  'internal_home_changelog',
  'streamer_plans',
  'dashboard_sessions',
];

interface QueryResult {
  columns: string[];
  rows: (string | null)[][];
  rowCount: number;
}

async function runQuery(sql: string): Promise<QueryResult> {
  const url = `/twitch/api/admin/system/query?sql=${encodeURIComponent(sql)}`;
  const res = await fetch(url, { credentials: 'include' });
  const body = await res.json() as Record<string, unknown>;
  if (!res.ok) {
    throw new Error(String(body.error ?? 'Unbekannter Fehler'));
  }
  return {
    columns: Array.isArray(body.columns) ? (body.columns as string[]) : [],
    rows: Array.isArray(body.rows) ? (body.rows as (string | null)[][]) : [],
    rowCount: typeof body.rowCount === 'number' ? body.rowCount : 0,
  };
}

export default function DatabaseQueryPage() {
  const [sql, setSql] = useState('SELECT * FROM twitch_streamers LIMIT 20');
  const [result, setResult] = useState<QueryResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleRun() {
    setLoading(true);
    setError(null);
    try {
      const res = await runQuery(sql);
      setResult(res);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setResult(null);
    } finally {
      setLoading(false);
    }
  }

  const columns: TableColumn<(string | null)[]>[] =
    result?.columns.map((col, i) => ({
      key: col,
      title: col,
      render: (row) => {
        const val = row[i];
        return <span className="font-mono text-xs">{val ?? <span className="text-text-secondary/50">NULL</span>}</span>;
      },
    })) ?? [];

  return (
    <section className="space-y-6">
      <PageHeader title="DB Query" description="Read-only SELECT-Abfragen gegen die Bot-Datenbank. Max. 200 Zeilen." />

      <div className="grid gap-4 lg:grid-cols-[200px_1fr]">
        <Section title="Tabellen" hint="Klicken zum Einfügen">
          <div className="space-y-1">
            {KNOWN_TABLES.map((t) => (
              <button
                key={t}
                type="button"
                className="w-full rounded-xl border border-white/10 bg-white/[0.03] px-3 py-2 text-left font-mono text-xs text-text-secondary hover:border-white/20 hover:text-white"
                onClick={() => setSql(`SELECT * FROM ${t} LIMIT 20`)}
              >
                {t}
              </button>
            ))}
          </div>
        </Section>

        <div className="space-y-4">
          <Section title="SQL" hint="Nur SELECT erlaubt — max. 200 Rows">
            <div className="space-y-3">
              <textarea
                className="admin-input min-h-[8rem] w-full resize-y font-mono text-sm leading-6"
                value={sql}
                onChange={(e) => setSql(e.target.value)}
                spellCheck={false}
                onKeyDown={(e) => {
                  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
                    e.preventDefault();
                    void handleRun();
                  }
                }}
              />
              <div className="flex items-center gap-3">
                <button
                  type="button"
                  className="admin-button admin-button-primary flex items-center gap-2"
                  disabled={loading || !sql.trim()}
                  onClick={() => void handleRun()}
                >
                  {loading
                    ? <RefreshCw className="h-4 w-4 animate-spin" />
                    : <Play className="h-4 w-4" />}
                  Ausführen
                </button>
                <span className="text-xs text-text-secondary">oder Ctrl+Enter</span>
              </div>
            </div>
          </Section>

          {error ? (
            <div className="rounded-[1.4rem] border border-red-500/30 bg-red-500/[0.06] p-4 font-mono text-sm text-red-300">
              {error}
            </div>
          ) : null}

          {result ? (
            <Section
              title={`Ergebnis — ${result.rowCount} Zeile${result.rowCount !== 1 ? 'n' : ''}${result.rowCount === 200 ? ' (Limit erreicht)' : ''}`}
              hint={`${result.columns.length} Spalten`}
            >
              <DataTable
                columns={columns}
                rows={result.rows}
                rowKey={(_, i) => String(i)}
              />
            </Section>
          ) : null}
        </div>
      </div>
    </section>
  );
}
