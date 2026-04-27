import { User, Calendar } from 'lucide-react';
import { useAdminAffiliateDetail } from '../../hooks/useAnalytics';

interface Props {
  login: string;
}

export function AffiliateDetailPanel({ login }: Props) {
  const { data, isLoading } = useAdminAffiliateDetail(login);

  if (isLoading) return <div className="bg-card rounded-xl border border-border p-5 animate-pulse h-64" />;
  if (!data) return null;

  const { affiliate, claims, stats } = data;
  const formatCurrency = (n: number) => `${n.toFixed(2).replace('.', ',')}€`;

  return (
    <div className="bg-card rounded-xl border border-border p-5 space-y-5">
      {/* Header */}
      <div className="flex items-center gap-3">
        <div className="w-10 h-10 rounded-full bg-purple-500/20 flex items-center justify-center">
          <User className="w-5 h-5 text-purple-400" />
        </div>
        <div>
          <div className="font-semibold text-white">{affiliate.display_name}</div>
          <div className="text-xs text-white/30">{affiliate.login}</div>
        </div>
        <span className={`ml-auto text-xs px-2 py-0.5 rounded-full ${
          affiliate.active ? 'bg-emerald-400/10 text-emerald-400' : 'bg-white/5 text-white/30'
        }`}>
          {affiliate.active ? 'Aktiv' : 'Inaktiv'}
        </span>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-2 gap-3">
        <div className="bg-white/5 rounded-lg p-3">
          <div className="text-xs text-white/40">Claims</div>
          <div className="text-lg font-bold text-white">{stats.total_claims}</div>
        </div>
        <div className="bg-white/5 rounded-lg p-3">
          <div className="text-xs text-white/40">Provision</div>
          <div className="text-lg font-bold text-white">{formatCurrency(stats.total_provision)}</div>
        </div>
        <div className="bg-white/5 rounded-lg p-3">
          <div className="text-xs text-white/40">Ø Provision</div>
          <div className="text-lg font-bold text-white">{formatCurrency(stats.avg_provision)}</div>
        </div>
        <div className="bg-white/5 rounded-lg p-3">
          <div className="text-xs text-white/40">Aktive Kunden</div>
          <div className="text-lg font-bold text-white">{stats.active_customers}</div>
        </div>
      </div>

      {/* Recent Claims */}
      <div>
        <h3 className="text-sm font-medium text-white/60 mb-2">Letzte Claims</h3>
        <div className="space-y-2">
          {claims.slice(0, 5).map((c) => (
            <div key={c.id} className="flex items-center justify-between text-sm bg-white/5 rounded-lg px-3 py-2">
              <div>
                <span className="text-white">{c.customer_login}</span>
                <div className="text-white/30 text-xs mt-1">
                  {new Date(c.claimed_at).toLocaleDateString('de-DE')} · {c.commission_count} Auszahlung(en)
                </div>
              </div>
              <span className="text-emerald-400 font-medium">
                {formatCurrency(c.commission_cents / 100)}
              </span>
            </div>
          ))}
          {claims.length === 0 && (
            <div className="text-sm text-white/30 text-center py-4">Keine Claims</div>
          )}
        </div>
      </div>

      {/* Member since */}
      <div className="flex items-center gap-2 text-xs text-white/30">
        <Calendar className="w-3 h-3" />
        Seit {new Date(affiliate.created_at).toLocaleDateString('de-DE')}
      </div>
    </div>
  );
}
