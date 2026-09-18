import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer } from 'recharts';
import type { TrendReihenpunkt } from './kategorieWeltweitViewModel';

/**
 * Trend gleichzeitiger Streams und Zuschauer für "Kategorien weltweit".
 * Warme Markenfarben (Gold und Bernsteinton), keine Ampelfarben.
 * Lückenstunden kommen als null herein und brechen die Linie bewusst.
 */
export function TrendChart({ reihe }: { reihe: TrendReihenpunkt[] }) {
  const streamsLabel = 'Gleichzeitige Streams (Ø)';
  const zuschauerLabel = 'Zuschauer (Ø)';

  return (
    <ResponsiveContainer width="100%" height="100%">
      <AreaChart data={reihe} margin={{ top: 10, right: 10, left: 0, bottom: 0 }}>
        <defs>
          <linearGradient id="kategorieSammlerStreams" x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor="#E8A33D" stopOpacity={0.35} />
            <stop offset="95%" stopColor="#E8A33D" stopOpacity={0} />
          </linearGradient>
          <linearGradient id="kategorieSammlerZuschauer" x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor="var(--color-primary)" stopOpacity={0.3} />
            <stop offset="95%" stopColor="var(--color-primary)" stopOpacity={0} />
          </linearGradient>
        </defs>
        <CartesianGrid strokeDasharray="3 3" stroke="rgba(197, 160, 89, 0.24)" />
        <XAxis
          dataKey="label"
          stroke="#B5A488"
          fontSize={12}
          tickLine={false}
          axisLine={false}
          minTickGap={24}
        />
        <YAxis yAxisId="streams" stroke="#E8A33D" fontSize={12} tickLine={false} axisLine={false} width={48} allowDecimals={false} />
        <YAxis yAxisId="viewers" orientation="right" stroke="#B5A488" fontSize={12} tickLine={false} axisLine={false} width={56} />
        <Tooltip
          cursor={{ stroke: 'rgba(197, 160, 89, 0.4)' }}
          content={({ active, payload, label }) => {
            if (!active || !payload || payload.length === 0) {
              return null;
            }
            const punkt = payload[0]?.payload as TrendReihenpunkt | undefined;
            if (!punkt || punkt.luecke) {
              return (
                <div className="rounded-lg border border-border bg-card p-3 text-sm shadow-xl">
                  <p className="font-medium text-white">{label}</p>
                  <p className="text-text-secondary">Keine vollständige Erfassung in dieser Stunde (UTC)</p>
                </div>
              );
            }
            return (
              <div className="rounded-lg border border-border bg-card p-3 text-sm shadow-xl">
                <p className="mb-2 font-medium text-white">{label}</p>
                <p className="text-text-secondary">
                  Streams (Ø): <span className="font-semibold text-white">{formatWert(punkt.avg_streams)}</span>
                </p>
                <p className="text-text-secondary">
                  Zuschauer (Ø): <span className="font-semibold text-white">{formatWert(punkt.avg_viewers)}</span>
                </p>
                <p className="text-text-secondary">
                  Messpunkte: <span className="font-semibold text-white">{punkt.poll_samples}</span>
                </p>
              </div>
            );
          }}
        />
        <Legend
          verticalAlign="top"
          height={36}
          formatter={(value) => (
            <span className="text-sm text-text-secondary">
              {value}
            </span>
          )}
        />
        <Area
          type="monotone"
          dataKey="avg_streams"
          yAxisId="streams"
          name={streamsLabel}
          stroke="#E8A33D"
          strokeWidth={2}
          fill="url(#kategorieSammlerStreams)"
          fillOpacity={1}
          connectNulls={false}
          dot={false}
        />
        <Area
          type="monotone"
          dataKey="avg_viewers"
          yAxisId="viewers"
          name={zuschauerLabel}
          stroke="var(--color-primary)"
          strokeWidth={2}
          fill="url(#kategorieSammlerZuschauer)"
          fillOpacity={1}
          connectNulls={false}
          dot={false}
        />
      </AreaChart>
    </ResponsiveContainer>
  );
}

function formatWert(wert: number | null): string {
  if (wert === null || !Number.isFinite(wert)) {
    return '-';
  }
  return wert.toLocaleString('de-DE', { maximumFractionDigits: 1 });
}
