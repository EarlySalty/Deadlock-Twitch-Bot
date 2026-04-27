import { motion } from 'framer-motion';
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip, BarChart, Bar, XAxis, YAxis } from 'recharts';
import { NoDataCard } from '@/components/cards/NoDataCard';
import type { ViewerProfiles as ViewerProfilesData } from '@/types/analytics';

interface ViewerProfilesProps {
  data: ViewerProfilesData | undefined;
}

const PROFILE_COLORS = ['var(--color-primary)', 'var(--color-accent)', '#f5b642', '#2ecc71', 'var(--color-secondary)'];
const PROFILE_LABELS: Record<string, string> = {
  exclusive: 'Exklusiv',
  loyalMulti: 'Treue Multi',
  casual: 'Gelegentlich',
  explorer: 'Explorer',
  passive: 'Passiv',
};

export function ViewerProfiles({ data }: ViewerProfilesProps) {
  if (!data || !data.dataAvailable) {
    return <NoDataCard message={data?.message || "Keine Profil-Daten vorhanden"} />;
  }

  const { profiles, exclusivityDistribution } = data;
  const passiveNote = profiles.passive === 0;

  const pieData = Object.entries(PROFILE_LABELS).map(([key, label]) => ({
    name: label,
    value: profiles[key as keyof typeof profiles] as number,
  }));

  return (
    <div className="space-y-4">
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="panel-card rounded-2xl p-6"
      >
        <h4 className="text-sm font-medium text-text-secondary mb-4">Zuschauer-Segmente</h4>
        <div className="flex flex-col lg:flex-row items-center gap-6">
          {/* Pie Chart */}
          <div className="w-48 h-48">
            <ResponsiveContainer width="100%" height="100%">
              <PieChart>
                <Pie
                  data={pieData}
                  dataKey="value"
                  nameKey="name"
                  cx="50%"
                  cy="50%"
                  innerRadius={35}
                  outerRadius={70}
                  paddingAngle={2}
                >
                  {pieData.map((_, index) => (
                    <Cell key={index} fill={PROFILE_COLORS[index % PROFILE_COLORS.length]} />
                  ))}
                </Pie>
                <Tooltip
                  contentStyle={{
                    backgroundColor: '#1f2937',
                    border: '1px solid rgba(194, 221, 240, 0.25)',
                    borderRadius: '8px',
                  }}
                  formatter={(value: number | string | undefined) => {
                    const numericValue = typeof value === 'number' ? value : Number(value ?? 0);
                    return [numericValue, 'Zuschauer'];
                  }}
                />
              </PieChart>
            </ResponsiveContainer>
          </div>

          {/* Legend */}
          <div className="flex-1 space-y-2">
            {pieData.map((entry, i) => (
              <div key={entry.name} className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <div
                    className="w-3 h-3 rounded-full"
                    style={{ backgroundColor: PROFILE_COLORS[i] }}
                  />
                  <span className="text-sm text-text-secondary">{entry.name}</span>
                </div>
                <span className="text-sm font-medium text-white">{entry.value}</span>
              </div>
            ))}
          <div className="pt-2 border-t border-border">
            <div className="flex items-center justify-between">
              <span className="text-sm text-text-secondary">Gesamt</span>
              <span className="text-sm font-bold text-white">{profiles.total}</span>
            </div>
            {passiveNote && (
              <p className="text-[11px] text-text-secondary mt-2">
                Passiv: Keine Daten (Lurker anonym).
              </p>
            )}
          </div>
        </div>
      </div>
    </motion.div>

      {/* Exclusivity Distribution */}
      {exclusivityDistribution.length > 0 && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
          className="panel-card rounded-2xl p-6"
        >
          <h4 className="text-sm font-medium text-text-secondary mb-4">
            Exklusivitats-Verteilung (Anzahl verfolgter Streamer)
          </h4>
          <div className="h-[200px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={exclusivityDistribution}>
                <XAxis
                  dataKey="streamerCount"
                  stroke="#9ca3af"
                  fontSize={12}
                  tickFormatter={(v: number) => `${v} Streamer`}
                />
                <YAxis stroke="#9ca3af" fontSize={12} />
                <Tooltip
                  contentStyle={{
                    backgroundColor: '#1f2937',
                    border: '1px solid rgba(194, 221, 240, 0.25)',
                    borderRadius: '8px',
                  }}
                  formatter={(value: number | string | undefined) => {
                    const numericValue = typeof value === 'number' ? value : Number(value ?? 0);
                    return [numericValue, 'Zuschauer'];
                  }}
                  labelFormatter={(label: React.ReactNode, _payload) =>
                    typeof label === 'number' || typeof label === 'string'
                      ? `${label} Streamer`
                      : ''
                  }
                />
                <Bar dataKey="viewerCount" fill="var(--color-primary)" radius={[4, 4, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </motion.div>
      )}
    </div>
  );
}

export default ViewerProfiles;
