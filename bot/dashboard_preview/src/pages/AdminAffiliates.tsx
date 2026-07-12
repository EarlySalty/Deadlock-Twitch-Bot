import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ToggleLeft, ToggleRight, ChevronRight } from 'lucide-react';
import { useState } from 'react';
import { AffiliateDetailPanel } from '../components/cards/AffiliateDetailPanel';
import { setAffiliateCommissionRate, toggleAffiliate } from '../api/admin';
import { useAdminAffiliates, useAdminAffiliateStats, useAuthStatus } from '../hooks/useAnalytics';

export default function AdminAffiliates() {
  const [selectedLogin, setSelectedLogin] = useState<string | null>(null);
  const [rateDrafts, setRateDrafts] = useState<Record<string, string>>({});
  const [rateFeedback, setRateFeedback] = useState<{
    login: string;
    kind: 'success' | 'error';
    text: string;
  } | null>(null);
  const queryClient = useQueryClient();
  const { data: authStatus } = useAuthStatus();
  const { data: stats } = useAdminAffiliateStats();
  const { data: affiliatesData } = useAdminAffiliates();
  const csrfToken = authStatus?.csrfToken ?? authStatus?.csrf_token ?? null;

  const toggleMutation = useMutation({
    mutationFn: (login: string) => toggleAffiliate(login, csrfToken),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-affiliates'] });
      queryClient.invalidateQueries({ queryKey: ['admin-affiliate-stats'] });
      queryClient.invalidateQueries({ queryKey: ['admin-affiliate-detail'] });
    },
  });

  const rateMutation = useMutation({
    mutationFn: ({ login, rate }: { login: string; rate: number }) =>
      setAffiliateCommissionRate(login, rate, csrfToken),
    onSuccess: (result) => {
      setRateDrafts((drafts) => ({
        ...drafts,
        [result.login]: String(result.commission_rate_pct),
      }));
      setRateFeedback({ login: result.login, kind: 'success', text: 'Provisionssatz gespeichert' });
      queryClient.invalidateQueries({ queryKey: ['admin-affiliates'] });
      queryClient.invalidateQueries({ queryKey: ['admin-affiliate-detail'] });
    },
    onError: (_error, variables) => {
      setRateFeedback({
        login: variables.login,
        kind: 'error',
        text: 'Satz muss zwischen 0 und 100 liegen',
      });
    },
  });

  const saveRate = (login: string, fallbackRate: number) => {
    const rate = Number(rateDrafts[login] ?? fallbackRate);
    if (!Number.isInteger(rate) || rate < 0 || rate > 100) {
      setRateFeedback({
        login,
        kind: 'error',
        text: 'Satz muss zwischen 0 und 100 liegen',
      });
      return;
    }
    setRateFeedback(null);
    rateMutation.mutate({ login, rate });
  };

  const affiliates = affiliatesData?.affiliates ?? [];

  const formatCurrency = (n: number) => `${n.toFixed(2).replace('.', ',')}€`;

  return (
    <div className="max-w-6xl mx-auto px-4 py-8">
      <h1 className="text-2xl font-bold text-white mb-6">Affiliate-Verwaltung</h1>

      {/* KPI Cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-8">
        <div className="bg-card rounded-xl border border-border p-4">
          <div className="text-xs text-white/40 mb-1">Affiliates</div>
          <div className="text-2xl font-bold text-white">{stats?.total_affiliates ?? 0}</div>
          <div className="text-xs text-white/30">{stats?.active_affiliates ?? 0} aktiv</div>
        </div>
        <div className="bg-card rounded-xl border border-border p-4">
          <div className="text-xs text-white/40 mb-1">Claims Gesamt</div>
          <div className="text-2xl font-bold text-white">{stats?.total_claims ?? 0}</div>
        </div>
        <div className="bg-card rounded-xl border border-border p-4">
          <div className="text-xs text-white/40 mb-1">Provisionen Gesamt</div>
          <div className="text-2xl font-bold text-white">{formatCurrency(stats?.total_provision ?? 0)}</div>
        </div>
        <div className="bg-card rounded-xl border border-border p-4">
          <div className="text-xs text-white/40 mb-1">Diesen Monat</div>
          <div className="text-2xl font-bold text-white">{stats?.this_month_claims ?? 0} Claims</div>
          <div className="text-xs text-emerald-400">{formatCurrency(stats?.this_month_provision ?? 0)}</div>
        </div>
      </div>

      <div className="flex gap-6">
        {/* Affiliates Table */}
        <div className="flex-1 bg-card rounded-xl border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-white/10 text-white/40 text-xs">
                <th className="text-left p-3 font-normal">Affiliate</th>
                <th className="text-right p-3 font-normal">Claims</th>
                <th className="text-right p-3 font-normal">Provision</th>
                <th className="text-center p-3 font-normal">Provision (%)</th>
                <th className="text-center p-3 font-normal">Status</th>
                <th className="p-3"></th>
              </tr>
            </thead>
            <tbody>
              {affiliates.map((a) => (
                <tr
                  key={a.login}
                  className={`border-b border-white/5 hover:bg-white/5 cursor-pointer transition-colors ${
                    selectedLogin === a.login ? 'bg-white/5' : ''
                  }`}
                  onClick={() => setSelectedLogin(a.login === selectedLogin ? null : a.login)}
                >
                  <td className="p-3">
                    <div className="font-medium text-white">{a.display_name}</div>
                    <div className="text-xs text-white/30">{a.login}</div>
                  </td>
                  <td className="p-3 text-right text-white/70">{a.total_claims}</td>
                  <td className="p-3 text-right text-white/70">{formatCurrency(a.total_provision)}</td>
                  <td className="p-3" onClick={(e) => e.stopPropagation()}>
                    <div className="flex flex-col items-center gap-1">
                      <label className="text-xs text-white/40" htmlFor={`commission-rate-${a.login}`}>
                        Provision (%)
                      </label>
                      <div className="flex items-center gap-2">
                        <input
                          id={`commission-rate-${a.login}`}
                          type="number"
                          min={0}
                          max={100}
                          step={1}
                          value={rateDrafts[a.login] ?? String(a.commission_rate_pct)}
                          onChange={(event) =>
                            setRateDrafts((drafts) => ({ ...drafts, [a.login]: event.target.value }))
                          }
                          className="w-16 rounded border border-white/10 bg-black/20 px-2 py-1 text-right text-white"
                        />
                        <button
                          type="button"
                          onClick={() => saveRate(a.login, a.commission_rate_pct)}
                          disabled={!csrfToken || rateMutation.isPending}
                          className="rounded bg-purple-500/20 px-2 py-1 text-xs text-purple-300 disabled:opacity-40"
                        >
                          Satz speichern
                        </button>
                      </div>
                      {rateFeedback?.login === a.login && (
                        <div
                          className={`text-xs ${
                            rateFeedback.kind === 'success' ? 'text-emerald-400' : 'text-red-400'
                          }`}
                        >
                          {rateFeedback.text}
                        </div>
                      )}
                    </div>
                  </td>
                  <td className="p-3 text-center">
                    <button
                      onClick={(e) => { e.stopPropagation(); toggleMutation.mutate(a.login); }}
                      className="inline-flex items-center"
                      title={a.active ? 'Deaktivieren' : 'Aktivieren'}
                      disabled={!csrfToken || toggleMutation.isPending}
                    >
                      {a.active ? (
                        <ToggleRight className="w-6 h-6 text-emerald-400" />
                      ) : (
                        <ToggleLeft className="w-6 h-6 text-white/20" />
                      )}
                    </button>
                  </td>
                  <td className="p-3">
                    <ChevronRight className="w-4 h-4 text-white/20" />
                  </td>
                </tr>
              ))}
              {affiliates.length === 0 && (
                <tr>
                  <td colSpan={6} className="p-8 text-center text-white/30">
                    Keine Affiliates vorhanden
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        {/* Detail Panel */}
        {selectedLogin && (
          <div className="w-96 flex-shrink-0">
            <AffiliateDetailPanel login={selectedLogin} />
          </div>
        )}
      </div>
    </div>
  );
}
