import { motion } from 'framer-motion';
import { Eye, UserCheck, Users } from 'lucide-react';
import { NoDataCard } from '@/components/cards/NoDataCard';
import type { LurkerAnalysis as LurkerAnalysisData } from '@/types/analytics';

interface LurkerAnalysisProps {
  data: LurkerAnalysisData | undefined;
}

export function LurkerAnalysis({ data }: LurkerAnalysisProps) {
  if (!data || !data.dataAvailable) {
    return <NoDataCard message={data?.message || "Keine Lurker-Daten vorhanden"} />;
  }

  const { lurkerStats, conversionStats, regularLurkers } = data;

  return (
    <div className="space-y-4">
      {/* Stat Cards */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="grid grid-cols-1 md:grid-cols-3 gap-4"
      >
        <StatCard
          icon={<Eye className="w-5 h-5" />}
          label="Lurker-Ratio"
          value={`${(lurkerStats.ratio * 100).toFixed(1)}%`}
          sublabel={`${lurkerStats.totalLurkers} von ${lurkerStats.totalViewers} Zuschauern`}
          color="primary"
        />
        <StatCard
          icon={<UserCheck className="w-5 h-5" />}
          label="LurkerChatter Conversion"
          value={`${(conversionStats.rate * 100).toFixed(1)}%`}
          sublabel={`${conversionStats.converted} von ${conversionStats.eligible} konvertiert`}
          color="success"
        />
        <StatCard
          icon={<Users className="w-5 h-5" />}
          label="Regulare Lurker"
          value={regularLurkers.length.toString()}
          sublabel={`Ø ${lurkerStats.avgSessions.toFixed(1)} Sessions`}
          color="accent"
        />
      </motion.div>

      {/* Lurker Table */}
      {regularLurkers.length > 0 && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
          className="bg-card rounded-xl border border-border p-6"
        >
          <h4 className="text-sm font-medium text-text-secondary mb-4">Top Lurker</h4>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="text-left py-2 text-text-secondary font-medium">Viewer</th>
                  <th className="text-right py-2 text-text-secondary font-medium">Sessions</th>
                  <th className="text-right py-2 text-text-secondary font-medium">Zuletzt gesehen</th>
                </tr>
              </thead>
              <tbody>
                {regularLurkers.slice(0, 15).map((lurker) => (
                  <tr key={lurker.login} className="border-b border-border/50 hover:bg-background/50">
                    <td className="py-2 text-white">{lurker.login}</td>
                    <td className="py-2 text-right text-text-secondary">{lurker.lurkSessions}</td>
                    <td className="py-2 text-right text-text-secondary">
                      {lurker.lastSeen
                        ? new Date(lurker.lastSeen).toLocaleDateString('de-DE')
                        : '-'}
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

interface StatCardProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  sublabel: string;
  color: 'primary' | 'success' | 'accent';
}

function StatCard({ icon, label, value, sublabel, color }: StatCardProps) {
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
      <div className="text-xs text-text-secondary mt-1">{sublabel}</div>
    </div>
  );
}

export default LurkerAnalysis;
