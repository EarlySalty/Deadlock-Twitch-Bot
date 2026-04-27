import { motion } from 'framer-motion';
import { Target, UserPlus, TrendingUp } from 'lucide-react';
import { NoDataCard } from '@/components/cards/NoDataCard';
import type { RaidRetention as RaidRetentionData } from '@/types/analytics';

interface RaidRetentionProps {
  data: RaidRetentionData | undefined;
}

function retentionColor(pct: number): string {
  if (pct >= 50) return 'text-success';
  if (pct >= 25) return 'text-warning';
  return 'text-text-secondary';
}

export function RaidRetention({ data }: RaidRetentionProps) {
  if (!data || !data.dataAvailable) {
    return <NoDataCard message={data?.message || "Keine Raid-Retention-Daten vorhanden"} />;
  }

  const { summary, raids } = data;

  return (
    <div className="space-y-4">
      {/* Summary Cards */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="grid grid-cols-1 md:grid-cols-3 gap-4"
      >
        <SummaryCard
          icon={<TrendingUp className="w-5 h-5" />}
          label="Ø Retention (30m)"
          value={`${summary.avgRetentionPct.toFixed(1)}%`}
          color="primary"
        />
        <SummaryCard
          icon={<Target className="w-5 h-5" />}
          label="Ø Chatter-Conversion"
          value={`${summary.avgConversionPct.toFixed(1)}%`}
          color="success"
        />
        <SummaryCard
          icon={<UserPlus className="w-5 h-5" />}
          label="Neue Zuschauer aus Raids"
          value={summary.totalNewChatters.toLocaleString('de-DE')}
          sublabel={`aus ${summary.raidCount} Raids`}
          color="accent"
        />
      </motion.div>

      {/* Raids Table */}
      {raids.length > 0 && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
          className="bg-card rounded-xl border border-border p-6"
        >
          <h4 className="text-sm font-medium text-text-secondary mb-4">Raid-Details</h4>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="text-left py-2 text-text-secondary font-medium">Ziel-Streamer</th>
                  <th className="text-right py-2 text-text-secondary font-medium">Gesendet</th>
                  <th className="text-right py-2 text-text-secondary font-medium">5m</th>
                  <th className="text-right py-2 text-text-secondary font-medium">15m</th>
                  <th className="text-right py-2 text-text-secondary font-medium">30m</th>
                  <th className="text-right py-2 text-text-secondary font-medium">Retention %</th>
                  <th className="text-right py-2 text-text-secondary font-medium">Neue Chatter</th>
                </tr>
              </thead>
              <tbody>
                {raids.map((raid) => (
                  <tr key={raid.raidId} className="border-b border-border/50 hover:bg-background/50">
                    <td className="py-2 text-white">{raid.toBroadcaster}</td>
                    <td className="py-2 text-right text-text-secondary">{raid.viewersSent}</td>
                    <td className="py-2 text-right text-text-secondary">
                      {raid.chattersAt5m !== null ? raid.chattersAt5m : '-'}
                    </td>
                    <td className="py-2 text-right text-text-secondary">
                      {raid.chattersAt15m !== null ? raid.chattersAt15m : '-'}
                    </td>
                    <td className="py-2 text-right text-text-secondary">
                      {raid.chattersAt30m !== null ? raid.chattersAt30m : '-'}
                    </td>
                    <td className={`py-2 text-right font-medium ${retentionColor(raid.retention30mPct)}`}>
                      {raid.retention30mPct.toFixed(1)}%
                    </td>
                    <td className="py-2 text-right text-text-secondary">
                      {raid.newChatters !== null ? raid.newChatters : '-'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </motion.div>
      )}
    </div>
  );
}

interface SummaryCardProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  sublabel?: string;
  color: 'primary' | 'success' | 'accent';
}

function SummaryCard({ icon, label, value, sublabel, color }: SummaryCardProps) {
  const colorClasses = {
    primary: 'bg-primary/10 text-primary',
    success: 'bg-success/10 text-success',
    accent: 'bg-accent/10 text-accent',
  };

  return (
    <div className="bg-card rounded-xl border border-border p-4">
      <div className={`w-10 h-10 rounded-lg ${colorClasses[color]} flex items-center justify-center mb-3`}>
        {icon}
      </div>
      <div className="text-sm text-text-secondary mb-1">{label}</div>
      <div className="text-xl font-bold text-white">{value}</div>
      {sublabel && <div className="text-xs text-text-secondary mt-1">{sublabel}</div>}
    </div>
  );
}

export default RaidRetention;
