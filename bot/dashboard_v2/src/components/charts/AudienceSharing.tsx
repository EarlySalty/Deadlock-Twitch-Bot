import { useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { Rise } from '../../motion/Rise';
import {
  BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer,
  LineChart, Line, Legend,
} from 'recharts';
import { NoDataCard } from '@/components/cards/NoDataCard';
import type { AudienceSharing as AudienceSharingData } from '@/types/analytics';
import {
  SHARING_TOPN_OPTIONS,
  readSharingTopN,
  writeSharingTopN,
  type SharingTopN,
} from '@/utils/sharingTopN';

interface AudienceSharingProps {
  data: AudienceSharingData | undefined;
}

const SEGMENT_SPRING = { type: 'spring', bounce: 0, duration: 0.32 } as const;

const PALETTE_TOKENS = [
  'var(--color-primary)',
  'var(--color-info)',
  'var(--color-danger)',
  'var(--color-success)',
  'var(--color-accent)',
  'var(--color-warning)',
  'var(--color-secondary)',
  'var(--color-primary-hover)',
  'var(--color-accent-hover)',
];

function lineColor(index: number): string {
  const base = PALETTE_TOKENS[index % PALETTE_TOKENS.length];
  const cycle = Math.floor(index / PALETTE_TOKENS.length);
  if (cycle === 0) return base;
  if (cycle === 1) return `color-mix(in srgb, ${base} 62%, black)`;
  return `color-mix(in srgb, ${base} 58%, white)`;
}

export function AudienceSharing({ data }: AudienceSharingProps) {
  const current = useMemo(() => data?.current ?? [], [data]);
  const timeline = useMemo(() => data?.timeline ?? [], [data]);
  const totalUniqueViewers = data?.totalUniqueViewers ?? 0;

  const [topN, setTopN] = useState<SharingTopN>(() => readSharingTopN());

  const selectTopN = (value: SharingTopN) => {
    setTopN(value);
    writeSharingTopN(value);
  };

  // Prepare bar chart data (top 10 partners by shared viewers)
  const barData = useMemo(
    () => [...current].sort((a, b) => b.sharedViewers - a.sharedViewers).slice(0, 10),
    [current]
  );

  // Prepare line chart data: pivot timeline into { month, streamer1, streamer2, ... }
  const topStreamers = useMemo(() => {
    const totals = new Map<string, number>();
    for (const row of timeline) {
      totals.set(row.streamer, (totals.get(row.streamer) || 0) + row.sharedViewers);
    }
    return [...totals.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, topN)
      .map(([s]) => s);
  }, [timeline, topN]);

  const lineData = useMemo(() => {
    const months = new Map<string, Record<string, number>>();
    for (const row of timeline) {
      if (!topStreamers.includes(row.streamer)) continue;
      if (!months.has(row.month)) months.set(row.month, {});
      months.get(row.month)![row.streamer] = row.sharedViewers;
    }
    return [...months.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([month, values]) => ({ month, ...values }));
  }, [timeline, topStreamers]);

  if (!data || !data.dataAvailable) {
    return <NoDataCard message={data?.message || "Keine Sharing-Daten vorhanden"} />;
  }

  return (
    <div className="space-y-4">
      {/* Summary */}
      <Rise
        className="panel-card rounded-2xl p-4"
      >
        <div className="flex items-center justify-between">
          <span className="text-sm text-text-secondary">Einzigartige Zuschauer gesamt</span>
          <span className="text-lg font-bold text-white">
            {totalUniqueViewers.toLocaleString('de-DE')}
          </span>
        </div>
      </Rise>

      {/* Horizontal Bar Chart: Top Partners */}
      {barData.length > 0 && (
        <Rise
          step={{ seconds: 0.1 }}
          className="panel-card rounded-2xl p-6"
        >
          <h4 className="text-sm font-medium text-text-secondary mb-4">
            Top Partner nach geteilten Zuschauern
          </h4>
          <div className="h-[300px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={barData} layout="vertical" margin={{ left: 80 }}>
                <XAxis type="number" stroke="#B5A488" fontSize={12} />
                <YAxis
                  type="category"
                  dataKey="streamer"
                  stroke="#B5A488"
                  fontSize={12}
                  width={75}
                />
                <Tooltip
                  contentStyle={{
                    backgroundColor: 'var(--color-popover)',
                    border: '1px solid var(--color-border)',
                    borderRadius: '8px',
                  }}
                  labelFormatter={(label: React.ReactNode, _payload) =>
                    typeof label === 'string' || typeof label === 'number' ? String(label) : ''
                  }
                  content={({ payload, label }) => {
                    if (!payload || payload.length === 0) return null;
                    const entry = barData.find(d => d.streamer === label);
                    if (!entry) return null;
                    return (
                      <div className="bg-card border border-border rounded-lg p-3 text-sm">
                        <div className="text-white font-medium mb-1">{entry.streamer}</div>
                        <div className="text-text-secondary">
                          Geteilt: {entry.sharedViewers}
                        </div>
                        <div className="flex gap-3 mt-1">
                          <span className="text-success">Inflow: {entry.inflow}</span>
                          <span className="text-danger">Outflow: {entry.outflow}</span>
                        </div>
                        <div className="text-text-secondary mt-1">
                          Jaccard: {(entry.jaccardSimilarity * 100).toFixed(1)}%
                        </div>
                      </div>
                    );
                  }}
                />
                <Bar dataKey="sharedViewers" fill="var(--color-primary)" radius={[0, 4, 4, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>

          {/* Inflow/Outflow indicators */}
          <div className="mt-4 space-y-1">
            {barData.slice(0, 5).map((entry) => (
              <div key={entry.streamer} className="flex items-center justify-between text-sm">
                <span className="text-text-secondary">{entry.streamer}</span>
                <div className="flex items-center gap-3">
                  <span className="text-success text-xs">Inflow: {entry.inflow}</span>
                  <span className="text-danger text-xs">Outflow: {entry.outflow}</span>
                </div>
              </div>
            ))}
          </div>
        </Rise>
      )}

      {/* Timeline Line Chart */}
      {lineData.length > 1 && topStreamers.length > 0 && (
        <Rise
          step={{ seconds: 0.2 }}
          className="panel-card rounded-2xl p-6"
        >
          <div className="flex flex-wrap items-center justify-between gap-3 mb-4">
            <h4 className="text-sm font-medium text-text-secondary">
              Zuschauer-Sharing Timeline (Top {topN})
            </h4>
            <div className="flex items-center bg-background/70 rounded-xl border border-border p-1.5">
              {SHARING_TOPN_OPTIONS.map(option => (
                <button
                  key={option}
                  type="button"
                  onClick={() => selectTopN(option)}
                  className={`relative px-3 py-1.5 rounded-lg text-sm font-semibold transition-colors ${
                    topN === option ? 'text-[#0D0806]' : 'text-text-secondary hover:text-white'
                  }`}
                >
                  {topN === option && (
                    <motion.span
                      layoutId="sharingTopNIndicator"
                      className="absolute inset-0 rounded-lg bg-gradient-to-r from-primary to-accent shadow-lg shadow-primary/20"
                      initial={false}
                      transition={SEGMENT_SPRING}
                    />
                  )}
                  <span className="relative z-10">Top {option}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="h-[250px]">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={lineData}>
                <XAxis dataKey="month" stroke="#B5A488" fontSize={12} />
                <YAxis stroke="#B5A488" fontSize={12} />
                <Tooltip
                  contentStyle={{
                    backgroundColor: 'var(--color-popover)',
                    border: '1px solid var(--color-border)',
                    borderRadius: '8px',
                  }}
                  labelStyle={{ color: '#fff' }}
                />
                <Legend />
                {topStreamers.map((streamer, i) => (
                  <Line
                    key={streamer}
                    type="monotone"
                    dataKey={streamer}
                    name={streamer}
                    stroke={lineColor(i)}
                    strokeWidth={2}
                    dot={{ fill: lineColor(i) }}
                    connectNulls
                  />
                ))}
              </LineChart>
            </ResponsiveContainer>
          </div>
        </Rise>
      )}
    </div>
  );
}

export default AudienceSharing;
