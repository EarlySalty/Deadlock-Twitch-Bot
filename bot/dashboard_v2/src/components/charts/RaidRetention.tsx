import { useMemo, useState } from 'react';
import { Rise } from '../../motion/Rise';
import { ChevronDown, ChevronUp, Search, Target, UserPlus, TrendingUp } from 'lucide-react';
import { NoDataCard } from '../cards/NoDataCard';
import { filterAndSortRaids } from '@/types/analytics';
import type {
  RaidRetention as RaidRetentionData,
  RaidSortDirection,
  RaidSortKey,
} from '@/types/analytics';

interface RaidRetentionProps {
  data: RaidRetentionData | undefined;
}

const RAID_SPALTEN: { key: RaidSortKey; label: string; align: 'left' | 'right' }[] = [
  { key: 'toBroadcaster', label: 'Ziel-Streamer', align: 'left' },
  { key: 'viewersSent', label: 'Gesendet', align: 'right' },
  { key: 'chattersAt5m', label: '5m', align: 'right' },
  { key: 'chattersAt15m', label: '15m', align: 'right' },
  { key: 'chattersAt30m', label: '30m', align: 'right' },
  { key: 'retention30mPct', label: 'Retention %', align: 'right' },
  { key: 'newChatters', label: 'Neue Chatter', align: 'right' },
];

function retentionColor(pct: number): string {
  if (pct >= 50) return 'text-success';
  if (pct >= 25) return 'text-warning';
  return 'text-text-secondary';
}

export function RaidRetention({ data }: RaidRetentionProps) {
  const [suche, setSuche] = useState('');
  const [sortKey, setSortKey] = useState<RaidSortKey>('viewersSent');
  const [richtung, setRichtung] = useState<RaidSortDirection>('desc');

  const sichtbareRaids = useMemo(
    () => filterAndSortRaids(data?.raids ?? [], suche, sortKey, richtung),
    [data, suche, sortKey, richtung],
  );

  const spalteWechseln = (key: RaidSortKey) => {
    if (key === sortKey) {
      setRichtung((r) => (r === 'asc' ? 'desc' : 'asc'));
      return;
    }
    setSortKey(key);
    setRichtung(key === 'toBroadcaster' ? 'asc' : 'desc');
  };

  if (!data || !data.dataAvailable) {
    return <NoDataCard message={data?.message || "Keine Raid-Retention-Daten vorhanden"} />;
  }

  const { summary, raids } = data;

  return (
    <div className="space-y-4">
      {/* Summary Cards */}
      <Rise
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
          sublabel={`über ${summary.raidCount} Raids`}
          color="accent"
        />
      </Rise>

      {/* Raids Table */}
      {raids.length > 0 && (
        <Rise
          step={{ seconds: 0.1 }}
          className="bg-card rounded-xl border border-border p-6"
        >
          <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <h4 className="text-sm font-medium text-text-secondary">Raid-Details</h4>
            <div className="flex items-center gap-2 rounded-lg border border-border bg-background/60 px-2 py-1.5 sm:w-64">
              <Search className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
              <input
                type="text"
                value={suche}
                onChange={(event) => setSuche(event.target.value)}
                placeholder="Ziel-Streamer suchen…"
                aria-label="Ziel-Streamer suchen"
                className="w-full bg-transparent text-sm text-white outline-none placeholder:text-text-secondary"
              />
            </div>
          </div>
          <div className="mb-2 text-xs text-text-secondary">
            {sichtbareRaids.length} von {raids.length} Raids
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  {RAID_SPALTEN.map((spalte) => {
                    const aktiv = spalte.key === sortKey;
                    return (
                      <th
                        key={spalte.key}
                        aria-sort={aktiv ? (richtung === 'asc' ? 'ascending' : 'descending') : 'none'}
                        className={`${spalte.align === 'left' ? 'text-left' : 'text-right'} py-2 font-medium`}
                      >
                        <button
                          type="button"
                          onClick={() => spalteWechseln(spalte.key)}
                          className={`inline-flex items-center gap-1 transition-colors hover:text-white ${
                            spalte.align === 'right' ? 'flex-row-reverse' : ''
                          } ${aktiv ? 'text-primary' : 'text-text-secondary'}`}
                        >
                          <span>{spalte.label}</span>
                          {aktiv &&
                            (richtung === 'asc' ? (
                              <ChevronUp className="h-3.5 w-3.5" />
                            ) : (
                              <ChevronDown className="h-3.5 w-3.5" />
                            ))}
                        </button>
                      </th>
                    );
                  })}
                </tr>
              </thead>
              <tbody>
                {sichtbareRaids.map((raid) => (
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
            {sichtbareRaids.length === 0 && (
              <div className="py-6 text-center text-sm text-text-secondary">
                Kein Raid passt zur Suche „{suche.trim()}“.
              </div>
            )}
          </div>
        </Rise>
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
