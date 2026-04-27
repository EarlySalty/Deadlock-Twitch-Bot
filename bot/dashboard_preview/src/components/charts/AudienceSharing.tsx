import { useMemo } from 'react';
import { motion } from 'framer-motion';
import {
  BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer,
  LineChart, Line, Legend,
} from 'recharts';
import { NoDataCard } from '@/components/cards/NoDataCard';
import type { AudienceSharing as AudienceSharingData } from '@/types/analytics';

interface AudienceSharingProps {
  data: AudienceSharingData | undefined;
}

const LINE_COLORS = ['var(--color-primary)', 'var(--color-accent)', 'var(--color-warning)'];

export function AudienceSharing({ data }: AudienceSharingProps) {
  const current = useMemo(() => data?.current ?? [], [data]);
  const timeline = useMemo(() => data?.timeline ?? [], [data]);
  const totalUniqueViewers = data?.totalUniqueViewers ?? 0;

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
      .slice(0, 3)
      .map(([s]) => s);
  }, [timeline]);

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
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="panel-card rounded-2xl p-4"
      >
        <div className="flex items-center justify-between">
          <span className="text-sm text-text-secondary">Einzigartige Zuschauer gesamt</span>
          <span className="text-lg font-bold text-white">
            {totalUniqueViewers.toLocaleString('de-DE')}
          </span>
        </div>
      </motion.div>

      {/* Horizontal Bar Chart: Top Partners */}
      {barData.length > 0 && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
          className="panel-card rounded-2xl p-6"
        >
          <h4 className="text-sm font-medium text-text-secondary mb-4">
            Top Partner nach geteilten Zuschauern
          </h4>
          <div className="h-[300px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={barData} layout="vertical" margin={{ left: 80 }}>
                <XAxis type="number" stroke="#9ca3af" fontSize={12} />
                <YAxis
                  type="category"
                  dataKey="streamer"
                  stroke="#9ca3af"
                  fontSize={12}
                  width={75}
                />
                <Tooltip
                  contentStyle={{
                    backgroundColor: '#1f2937',
                    border: '1px solid rgba(194, 221, 240, 0.25)',
                    borderRadius: '8px',
                  }}
                  formatter={(value: number | string | undefined, name?: string) => {
                    const numericValue = typeof value === 'number' ? value : Number(value ?? 0);
                    const displayName = name === 'sharedViewers' ? 'Geteilte Zuschauer' : name ?? '';
                    return [numericValue, displayName];
                  }}
                  labelFormatter={(label: React.ReactNode, _payload) =>
                    typeof label === 'string' || typeof label === 'number' ? String(label) : ''
                  }
                  content={({ payload, label }) => {
                    if (!payload || payload.length === 0) return null;
                    const entry = barData.find(d => d.streamer === label);
                    if (!entry) return null;
                    return (
                      <div className="bg-gray-800 border border-gray-700 rounded-lg p-3 text-sm">
                        <div className="text-white font-medium mb-1">{entry.streamer}</div>
                        <div className="text-text-secondary">
                          Geteilt: {entry.sharedViewers}
                        </div>
                        <div className="flex gap-3 mt-1">
                          <span className="text-green-400">Inflow: {entry.inflow}</span>
                          <span className="text-red-400">Outflow: {entry.outflow}</span>
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
                  <span className="text-green-400 text-xs">Inflow: {entry.inflow}</span>
                  <span className="text-red-400 text-xs">Outflow: {entry.outflow}</span>
                </div>
              </div>
            ))}
          </div>
        </motion.div>
      )}

      {/* Timeline Line Chart */}
      {lineData.length > 1 && topStreamers.length > 0 && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.2 }}
          className="panel-card rounded-2xl p-6"
        >
          <h4 className="text-sm font-medium text-text-secondary mb-4">
            Zuschauer-Sharing Timeline (Top 3)
          </h4>
          <div className="h-[250px]">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={lineData}>
                <XAxis dataKey="month" stroke="#9ca3af" fontSize={12} />
                <YAxis stroke="#9ca3af" fontSize={12} />
                <Tooltip
                  contentStyle={{
                    backgroundColor: '#1f2937',
                    border: '1px solid rgba(194, 221, 240, 0.25)',
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
                    stroke={LINE_COLORS[i % LINE_COLORS.length]}
                    strokeWidth={2}
                    dot={{ fill: LINE_COLORS[i % LINE_COLORS.length] }}
                    connectNulls
                  />
                ))}
              </LineChart>
            </ResponsiveContainer>
          </div>
        </motion.div>
      )}
    </div>
  );
}

export default AudienceSharing;
