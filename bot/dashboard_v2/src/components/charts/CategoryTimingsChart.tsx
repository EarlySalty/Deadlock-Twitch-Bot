import { formatNumber } from '@/utils/formatters';
import { useLanguage } from '../../context/LanguageContext';
import { useMemo, useState } from 'react';
import {
  Area,
  CartesianGrid,
  ComposedChart,
  Legend,
  Line,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { Clock, Calendar, Info } from 'lucide-react';
import type { CategoryActivitySeries, CategoryActivitySeriesRow } from '@/types/analytics';

interface CategoryTimingsChartProps {
  data: CategoryActivitySeries;
}

const TOOLTIP_STYLE = {
  backgroundColor: 'var(--color-popover)',
  border: '1px solid var(--color-border)',
  borderRadius: '8px',
  fontSize: '12px',
};

function fmt(value: number | null | undefined, digits = 0, locale = 'de-DE') {
  if (value === null || value === undefined) return '–';
  return value.toLocaleString(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
}

function tickFmt(value: number, locale = 'de-DE') {
  if (value >= 1000) return `${formatNumber(value / 1000, 1, locale)}k`;
  return formatNumber(value, 0, locale);
}

export function CategoryTimingsChart({ data }: CategoryTimingsChartProps) {
  const { t, locale } = useLanguage();
  const [view, setView] = useState<'hourly' | 'weekly'>('hourly');
  const rows: CategoryActivitySeriesRow[] = view === 'hourly' ? data.hourly : data.weekly;

  const hasPeakSeries = useMemo(
    () => rows.some(row => row.categoryPeak !== null || row.trackedPeak !== null),
    [rows]
  );

  const CustomTooltip = ({ active, payload, label }: any) => {
    if (!active || !payload?.length) return null;
    const d = payload[0]?.payload ?? {};
    return (
      <div style={TOOLTIP_STYLE} className="p-3 space-y-1 min-w-[220px]">
        <div className="font-semibold text-white text-sm">{t(String(label ?? ''))}</div>
        <div className="flex justify-between gap-4 text-sm">
          <span className="text-text-secondary">{t("Kategorie Ø")}</span>
          <span className="font-bold text-white">{fmt(d.categoryAvg, 1, locale)}</span>
        </div>
        <div className="flex justify-between gap-4 text-sm">
          <span className="text-text-secondary">{t("Tracked Ø")}</span>
          <span className="font-bold text-white">{fmt(d.trackedAvg, 1, locale)}</span>
        </div>
        <div className="flex justify-between gap-4 text-xs">
          <span className="text-text-secondary">{t("Kategorie Peak")}</span>
          <span className="text-text-secondary">{fmt(d.categoryPeak, 0, locale)}</span>
        </div>
        <div className="flex justify-between gap-4 text-xs">
          <span className="text-text-secondary">{t("Tracked Peak")}</span>
          <span className="text-text-secondary">{fmt(d.trackedPeak, 0, locale)}</span>
        </div>
        <div className="border-t border-border/50 pt-1 mt-1 flex justify-between gap-4 text-xs">
          <span className="text-text-secondary">{t("Kategorie Samples")}</span>
          <span className="text-text-secondary">{fmt(d.categorySamples, 0, locale)}</span>
        </div>
        <div className="flex justify-between gap-4 text-xs">
          <span className="text-text-secondary">{t("Tracked Samples")}</span>
          <span className="text-text-secondary">{fmt(d.trackedSamples, 0, locale)}</span>
        </div>
      </div>
    );
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between flex-wrap gap-3">
        <div>
          <div className="flex items-center gap-2">
            <Clock className="w-4 h-4 text-accent" />
            <h3 className="font-semibold text-white">
              {view === 'hourly' ? t('Aktivität nach Uhrzeit (UTC)') : t('Aktivität nach Wochentag')}
            </h3>
          </div>
          <div className="flex items-center gap-1.5 mt-1 text-xs text-text-secondary">
            <Info className="w-3 h-3" />
            <span>
              {t("Legacy Stats Vergleich (Kategorie vs Tracked) · {days}d", { days: formatNumber(data.windowDays, 0, locale) })}
            </span>
          </div>
        </div>
        <div className="flex items-center gap-1 bg-background border border-border rounded-lg p-1">
          <button
            onClick={() => setView('hourly')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
              view === 'hourly' ? 'bg-accent text-bg' : 'text-text-secondary hover:text-white'
            }`}
          >
            <Clock className="w-3 h-3" /> {t("Stunde")}
          </button>
          <button
            onClick={() => setView('weekly')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
              view === 'weekly' ? 'bg-accent text-bg' : 'text-text-secondary hover:text-white'
            }`}
          >
            <Calendar className="w-3 h-3" /> {t("Wochentag")}
          </button>
        </div>
      </div>

      <div className="h-72">
        <ResponsiveContainer width="100%" height="100%">
          <ComposedChart data={rows as object[]} margin={{ top: 10, right: 10, bottom: 0, left: 0 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="var(--color-border)" vertical={false} />
            <XAxis
              dataKey="label"
              tickFormatter={value => t(String(value))}
              stroke="#B5A488"
              tick={{ fill: '#B5A488', fontSize: view === 'hourly' ? 10 : 12 }}
              interval={view === 'hourly' ? 1 : 0}
            />
            <YAxis
              yAxisId="yAvg"
              stroke="#B5A488"
              tick={{ fill: '#B5A488', fontSize: 11 }}
              tickFormatter={value => tickFmt(value, locale)}
              width={45}
              label={{ value: t('Ø Viewer'), angle: -90, position: 'insideLeft', fill: '#C5A059' }}
            />
            {hasPeakSeries && (
              <YAxis
                yAxisId="yPeak"
                orientation="right"
                stroke="#B5A488"
                tick={{ fill: '#B5A488', fontSize: 11 }}
                tickFormatter={value => tickFmt(value, locale)}
                width={45}
                label={{ value: t('Peak Viewer'), angle: 90, position: 'insideRight', fill: '#FF5A3C' }}
              />
            )}
            <Tooltip content={<CustomTooltip />} cursor={{ stroke: '#B5A488', strokeOpacity: 0.4 }} />
            <Legend wrapperStyle={{ color: '#EDE0C4', fontSize: '12px' }} />

            <Area
              yAxisId="yAvg"
              type="monotone"
              dataKey="categoryAvg"
              name={t("Kategorie Ø Viewer")}
              stroke="#C5A059"
              fill="#C5A059"
              fillOpacity={0.25}
              strokeWidth={2}
              connectNulls
              isAnimationActive={false}
              dot={{ r: 2 }}
              activeDot={{ r: 4 }}
            />
            <Area
              yAxisId="yAvg"
              type="monotone"
              dataKey="trackedAvg"
              name={t("Tracked Ø Viewer")}
              stroke="#00D9FF"
              fill="#00D9FF"
              fillOpacity={0.2}
              strokeWidth={2}
              connectNulls
              isAnimationActive={false}
              dot={{ r: 2 }}
              activeDot={{ r: 4 }}
            />
            {hasPeakSeries && (
              <Line
                yAxisId="yPeak"
                type="monotone"
                dataKey="categoryPeak"
                name={t("Kategorie Peak Viewer")}
                stroke="#FF5A3C"
                strokeDasharray="6 4"
                strokeWidth={2}
                connectNulls
                isAnimationActive={false}
                dot={{ r: 2 }}
                activeDot={{ r: 4 }}
              />
            )}
            {hasPeakSeries && (
              <Line
                yAxisId="yPeak"
                type="monotone"
                dataKey="trackedPeak"
                name={t("Tracked Peak Viewer")}
                stroke="#00C46A"
                strokeDasharray="4 4"
                strokeWidth={2}
                connectNulls
                isAnimationActive={false}
                dot={{ r: 2 }}
                activeDot={{ r: 4 }}
              />
            )}
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
