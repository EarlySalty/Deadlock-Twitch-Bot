import { formatNumber } from '@/utils/formatters';
import { useLanguage } from '../../context/LanguageContext';
import { Rise } from '../../motion/Rise';
import { Eye, UserCheck, Users } from 'lucide-react';
import { NoDataCard } from '@/components/cards/NoDataCard';
import type { LurkerAnalysis as LurkerAnalysisData } from '@/types/analytics';

interface LurkerAnalysisProps {
  data: LurkerAnalysisData | undefined;
}

export function LurkerAnalysis({ data }: LurkerAnalysisProps) {
  const { t, locale } = useLanguage();
  if (!data || !data.dataAvailable) {
    return <NoDataCard message={t(data?.message || "Keine Lurker-Daten vorhanden")} />;
  }

  const { lurkerStats, conversionStats, regularLurkers } = data;

  return (
    <div className="space-y-4">

      <Rise
        className="grid grid-cols-1 md:grid-cols-3 gap-4"
      >
        <StatCard
          icon={<Eye className="w-5 h-5" />}
          label={t("Lurker-Ratio")}
          value={`${formatNumber((lurkerStats.ratio * 100), 1, locale)}%`}
          sublabel={t("{lurkers} von {viewers} Zuschauern", { lurkers: formatNumber(lurkerStats.totalLurkers, 0, locale), viewers: formatNumber(lurkerStats.totalViewers, 0, locale) })}
          color="primary"
        />
        <StatCard
          icon={<UserCheck className="w-5 h-5" />}
          label={t("LurkerChatter Conversion")}
          value={`${formatNumber((conversionStats.rate * 100), 1, locale)}%`}
          sublabel={t("{converted} von {eligible} konvertiert", { converted: formatNumber(conversionStats.converted, 0, locale), eligible: formatNumber(conversionStats.eligible, 0, locale) })}
          color="success"
        />
        <StatCard
          icon={<Users className="w-5 h-5" />}
          label={t('Reguläre Lurker')}
          value={formatNumber(regularLurkers.length, 0, locale)}
          sublabel={t("Ø {sessions} Sessions", { sessions: formatNumber(lurkerStats.avgSessions, 1, locale) })}
          color="accent"
        />
      </Rise>


      {regularLurkers.length > 0 && (
        <Rise
          step={{ seconds: 0.1 }}
          className="bg-card rounded-xl border border-border p-6"
        >
          <h4 className="text-sm font-medium text-text-secondary mb-4">{t("Top Lurker")}</h4>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="text-left py-2 text-text-secondary font-medium">{t("Viewer")}</th>
                  <th className="text-right py-2 text-text-secondary font-medium">{t("Sessions")}</th>
                  <th className="text-right py-2 text-text-secondary font-medium">{t("Zuletzt gesehen")}</th>
                </tr>
              </thead>
              <tbody>
                {regularLurkers.slice(0, 15).map((lurker) => (
                  <tr key={lurker.login} className="border-b border-border/50 hover:bg-background/50">
                    <td className="py-2 text-white">{lurker.login}</td>
                    <td className="py-2 text-right text-text-secondary">{formatNumber(lurker.lurkSessions, 0, locale)}</td>
                    <td className="py-2 text-right text-text-secondary">
                      {lurker.lastSeen
                        ? new Date(lurker.lastSeen).toLocaleDateString(locale)
                        : '-'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Rise>
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
