import { Rise } from '../../motion/Rise';
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip, BarChart, Bar, XAxis, YAxis } from 'recharts';
import { NoDataCard } from '@/components/cards/NoDataCard';
import type { ViewerProfiles as ViewerProfilesData } from '@/types/analytics';
import { formatNumber } from '@/utils/formatters';
import { useLanguage } from '../../context/LanguageContext';

interface ViewerProfilesProps {
  data: ViewerProfilesData | undefined;
}

const PROFILE_COLORS = [
  'var(--color-chart-1)',
  'var(--color-chart-2)',
  'var(--color-chart-3)',
  'var(--color-chart-4)',
  'var(--color-chart-5)',
];
const PROFILE_LABELS: Record<string, string> = {
  exclusive: 'Exklusiv',
  loyalMulti: 'Treue Multi',
  casual: 'Gelegentlich',
  explorer: 'Explorer',
  passive: 'Passiv',
};

export function ViewerProfiles({ data }: ViewerProfilesProps) {
  const { t, locale } = useLanguage();
  if (!data || !data.dataAvailable) {
    return <NoDataCard message={data?.message ? t(data.message) : t('Keine Profil-Daten vorhanden')} />;
  }

  const { profiles, exclusivityDistribution } = data;
  const passiveNote = profiles.passive === 0;

  const pieData = Object.entries(PROFILE_LABELS).map(([key, label]) => ({
    name: label,
    value: profiles[key as keyof typeof profiles] as number,
  }));

  return (
    <div className="space-y-4">
      <Rise
        className="panel-card rounded-2xl p-6"
      >
        <h4 className="text-sm font-medium text-text-secondary mb-4">{t('Zuschauer-Segmente')}</h4>
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
                  innerRadius="58%"
                  outerRadius="90%"
                  paddingAngle={2}
                >
                  {pieData.map((_, index) => (
                    <Cell
                      key={index}
                      fill={PROFILE_COLORS[index % PROFILE_COLORS.length]}
                      stroke="none"
                    />
                  ))}
                </Pie>
                <Tooltip
                  contentStyle={{
                    backgroundColor: 'var(--color-popover)',
                    border: '1px solid var(--color-border)',
                    borderRadius: '8px',
                  }}
                  formatter={(value) => {
                    const numericValue = Array.isArray(value) ? Number(value[0] ?? 0) : Number(value ?? 0);
                    return [formatNumber(numericValue, 0, locale), t('Zuschauer')];
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
                  <span className="text-sm text-text-secondary">{t(entry.name)}</span>
                </div>
                <span className="text-sm font-medium text-white">{formatNumber(entry.value, 0, locale)}</span>
              </div>
            ))}
          <div className="pt-2 border-t border-border">
            <div className="flex items-center justify-between">
              <span className="text-sm text-text-secondary">{t('Gesamt')}</span>
              <span className="text-sm font-bold text-white">{formatNumber(profiles.total, 0, locale)}</span>
            </div>
            {passiveNote && (
              <p className="text-[11px] text-text-secondary mt-2">
                {t('Passiv: Keine Daten (Lurker anonym).')}
              </p>
            )}
          </div>
        </div>
      </div>
    </Rise>

      {/* Exclusivity Distribution */}
      {exclusivityDistribution.length > 0 && (
        <Rise
          step={{ seconds: 0.1 }}
          className="panel-card rounded-2xl p-6"
        >
          <h4 className="text-sm font-medium text-text-secondary mb-4">
            {t('Exklusivitats-Verteilung (Anzahl verfolgter Streamer)').replace('Exklusivitats', 'Exklusivitäts')}
          </h4>
          <div className="h-[200px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={exclusivityDistribution}>
                <XAxis
                  dataKey="streamerCount"
                  stroke="#B5A488"
                  fontSize={12}
                  tickFormatter={(v: number) => t('{count} Streamer', { count: v })}
                />
                <YAxis stroke="#B5A488" fontSize={12} />
                <Tooltip
                  contentStyle={{
                    backgroundColor: 'var(--color-popover)',
                    border: '1px solid var(--color-border)',
                    borderRadius: '8px',
                  }}
                  formatter={(value) => {
                    const numericValue = Array.isArray(value) ? Number(value[0] ?? 0) : Number(value ?? 0);
                    return [formatNumber(numericValue, 0, locale), t('Zuschauer')];
                  }}
                  labelFormatter={(label: React.ReactNode) =>
                    typeof label === 'number' || typeof label === 'string'
                      ? t('{count} Streamer', { count: label })
                      : ''
                  }
                />
                <Bar dataKey="viewerCount" fill="var(--color-primary)" radius={[4, 4, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </Rise>
      )}
    </div>
  );
}

export default ViewerProfiles;
