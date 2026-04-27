import { Link2, Copy, CheckCircle } from 'lucide-react';
import { useState } from 'react';
import { useAffiliatePortal } from '../hooks/useAnalytics';

export default function AffiliatePortal() {
  const [copied, setCopied] = useState(false);
  const { data, isLoading, isError, error, refetch } = useAffiliatePortal();

  const copyLink = () => {
    if (data?.affiliate.referral_url) {
      navigator.clipboard.writeText(data.affiliate.referral_url);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const formatCurrency = (n: number) => `${n.toFixed(2).replace('.', ',')}€`;

  if (isLoading) {
    return (
      <div className="max-w-4xl mx-auto px-4 py-8">
        <div className="animate-pulse space-y-4">
          <div className="h-8 bg-white/5 rounded w-64" />
          <div className="grid grid-cols-3 gap-4">
            {[1, 2, 3].map(i => <div key={i} className="h-24 bg-white/5 rounded-xl" />)}
          </div>
        </div>
      </div>
    );
  }

  if (isError) {
    const message = error instanceof Error ? error.message : 'Unbekannter Fehler';
    if (message === 'affiliate_not_found') {
      return (
        <div className="max-w-4xl mx-auto px-4 py-8 text-center">
          <h1 className="text-2xl font-bold text-white mb-2">Affiliate Portal</h1>
          <p className="text-white/50">Du bist noch kein Affiliate. Kontaktiere uns für mehr Infos.</p>
        </div>
      );
    }
    return (
      <div className="max-w-4xl mx-auto px-4 py-8 text-center">
        <h1 className="text-2xl font-bold text-white mb-2">Affiliate Portal</h1>
        <p className="text-white/50 mb-4">Portal-Daten konnten nicht geladen werden.</p>
        <p className="text-white/30 text-sm mb-6">{message}</p>
        <button
          onClick={() => void refetch()}
          className="px-4 py-2 rounded-lg bg-white/10 hover:bg-white/15 text-white text-sm font-medium transition-colors"
        >
          Erneut laden
        </button>
      </div>
    );
  }

  if (!data) {
    return (
      <div className="max-w-4xl mx-auto px-4 py-8 text-center">
        <h1 className="text-2xl font-bold text-white mb-2">Affiliate Portal</h1>
        <p className="text-white/50">Portal-Daten werden geladen.</p>
      </div>
    );
  }

  return (
    <div className="max-w-4xl mx-auto px-4 py-8">
      <h1 className="text-2xl font-bold text-white mb-6">Affiliate Portal</h1>

      {/* Referral Link */}
      <div className="bg-card rounded-xl border border-border p-5 mb-6">
        <h2 className="text-sm font-medium text-white/60 mb-3 flex items-center gap-2">
          <Link2 className="w-4 h-4" /> Dein Referral-Link
        </h2>
        <div className="flex items-center gap-3">
          <code className="flex-1 bg-white/5 rounded-lg px-4 py-2.5 text-sm text-white/70 truncate">
            {data.affiliate.referral_url}
          </code>
          <button
            onClick={copyLink}
            className="flex items-center gap-2 px-4 py-2.5 rounded-lg bg-purple-500 hover:bg-purple-400 text-white text-sm font-medium transition-colors"
          >
            {copied ? <CheckCircle className="w-4 h-4" /> : <Copy className="w-4 h-4" />}
            {copied ? 'Kopiert!' : 'Kopieren'}
          </button>
        </div>
      </div>

      {/* KPI Cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
        <div className="bg-card rounded-xl border border-border p-4">
          <div className="text-xs text-white/40 mb-1">Gesamt-Claims</div>
          <div className="text-2xl font-bold text-white">{data.stats.total_claims}</div>
        </div>
        <div className="bg-card rounded-xl border border-border p-4">
          <div className="text-xs text-white/40 mb-1">Gesamt-Provision</div>
          <div className="text-2xl font-bold text-emerald-400">{formatCurrency(data.stats.total_provision)}</div>
        </div>
        <div className="bg-card rounded-xl border border-border p-4">
          <div className="text-xs text-white/40 mb-1">Diesen Monat</div>
          <div className="text-2xl font-bold text-white">{data.stats.this_month_claims} Claims</div>
        </div>
        <div className="bg-card rounded-xl border border-border p-4">
          <div className="text-xs text-white/40 mb-1">Ausstehende Auszahlung</div>
          <div className="text-2xl font-bold text-amber-400">{formatCurrency(data.stats.pending_payout)}</div>
        </div>
      </div>

      {/* Recent Claims */}
      <div className="bg-card rounded-xl border border-border overflow-hidden">
        <div className="p-4 border-b border-white/10">
          <h2 className="font-medium text-white">Letzte Claims</h2>
        </div>
        <table className="w-full text-sm">
          <thead>
            <tr className="text-white/40 text-xs border-b border-white/5">
              <th className="text-left p-3 font-normal">Kunde</th>
              <th className="text-left p-3 font-normal">Plan</th>
              <th className="text-right p-3 font-normal">Provision</th>
              <th className="text-right p-3 font-normal">Datum</th>
            </tr>
          </thead>
          <tbody>
            {data.recent_claims.map((c, i) => (
              <tr key={i} className="border-b border-white/5">
                <td className="p-3 text-white">{c.customer_display_name}</td>
                <td className="p-3 text-white/50">{c.plan_name || '—'}</td>
                <td className="p-3 text-right text-emerald-400">{formatCurrency(c.amount)}</td>
                <td className="p-3 text-right text-white/30">
                  {new Date(c.created_at).toLocaleDateString('de-DE')}
                </td>
              </tr>
            ))}
            {data.recent_claims.length === 0 && (
              <tr>
                <td colSpan={4} className="p-8 text-center text-white/30">Noch keine Claims</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
