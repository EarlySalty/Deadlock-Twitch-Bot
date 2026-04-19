import { useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { motion, AnimatePresence } from 'framer-motion';
import {
  Sparkles,
  Copy,
  Tv2,
  Loader2,
  AlertCircle,
  CheckCircle2,
  TrendingUp,
  TrendingDown,
  Lightbulb,
  Zap,
} from 'lucide-react';
import {
  fetchTitleInsights,
  fetchTitleSuggestion,
  type TitleHistoryEntry,
  type TitleSuggestResult,
} from '@/api/title';

interface TitleGeneratorProps {
  streamer: string | null;
}

function ScoreBadge({ value }: { value: number }) {
  const pct = Math.min(Math.round(value * 100), 200);
  const color =
    value >= 1.4 ? 'text-success' : value >= 0.8 ? 'text-warning' : 'text-error';
  const label = value >= 1.4 ? '↑' : value >= 0.8 ? '→' : '↓';
  return (
    <span className={`font-mono text-xs ${color}`}>
      {label} {pct}%
    </span>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = () => {
    navigator.clipboard.writeText(text).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };
  return (
    <button
      onClick={handleCopy}
      className="flex items-center gap-1 px-2.5 py-1 text-xs rounded-lg bg-card hover:bg-card-hover border border-border transition-colors shrink-0"
    >
      {copied ? (
        <CheckCircle2 className="w-3 h-3 text-success" />
      ) : (
        <Copy className="w-3 h-3 text-text-secondary" />
      )}
      <span className={copied ? 'text-success' : 'text-text-secondary'}>
        {copied ? 'Kopiert' : 'Kopieren'}
      </span>
    </button>
  );
}

export function TitleGenerator({ streamer }: TitleGeneratorProps) {
  const [keywords, setKeywords] = useState('');
  const [includeLive, setIncludeLive] = useState(false);
  const [result, setResult] = useState<TitleSuggestResult | null>(null);
  const [setTitleStatus, setSetTitleStatus] = useState<'idle' | 'loading' | 'done' | 'error' | 'scope_missing'>('idle');

  const { data: insightData } = useQuery({
    queryKey: ['title-insights', streamer],
    queryFn: fetchTitleInsights,
    enabled: !!streamer,
    staleTime: 1000 * 60 * 60,
  });

  const mutation = useMutation({
    mutationFn: () => fetchTitleSuggestion({ keywords: keywords.trim(), include_live: includeLive }),
    onSuccess: (data) => setResult(data),
  });

  const handleSetOnTwitch = async (title: string) => {
    setSetTitleStatus('loading');
    try {
      const res = await fetch('/twitch/api/v2/channel/title', {
        method: 'PATCH',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ title }),
      });
      if (res.status === 403) {
        setSetTitleStatus('scope_missing');
      } else if (!res.ok) {
        setSetTitleStatus('error');
      } else {
        setSetTitleStatus('done');
        setTimeout(() => setSetTitleStatus('idle'), 3000);
      }
    } catch {
      setSetTitleStatus('error');
    }
  };

  const isLoading = mutation.isPending;
  const error = mutation.error as Error | null;
  const isRateLimit = error?.message?.startsWith('rate_limit');
  const retryAfter = isRateLimit ? error!.message.split(':')[1] : null;

  return (
    <div className="space-y-5 py-4 max-w-2xl">

      {/* Input Card */}
      <div className="panel-card rounded-2xl p-5 space-y-4">
        <div className="flex items-center gap-2 mb-1">
          <Sparkles className="w-4 h-4 text-accent" />
          <span className="font-semibold text-sm">Was machst du heute?</span>
        </div>

        <input
          type="text"
          value={keywords}
          onChange={(e) => setKeywords(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && keywords.trim() && !isLoading && mutation.mutate()}
          placeholder="z.B. ranked solo grind, duo mit friend, first time hero…"
          className="w-full px-3.5 py-2.5 rounded-xl bg-background border border-border focus:border-primary/60 focus:outline-none text-sm placeholder:text-text-secondary/50 transition-colors"
        />

        <div className="flex items-center justify-between">
          <label className="flex items-center gap-2 text-xs text-text-secondary cursor-pointer select-none group">
            <div className="relative">
              <input
                type="checkbox"
                checked={includeLive}
                onChange={(e) => setIncludeLive(e.target.checked)}
                className="sr-only"
              />
              <div className={`w-8 h-4 rounded-full transition-colors ${includeLive ? 'bg-primary' : 'bg-border'}`}>
                <div className={`absolute top-0.5 w-3 h-3 rounded-full bg-white transition-transform shadow ${includeLive ? 'translate-x-4' : 'translate-x-0.5'}`} />
              </div>
            </div>
            <Zap className="w-3.5 h-3.5" />
            Live-Daten einbeziehen
          </label>

          <button
            onClick={() => mutation.mutate()}
            disabled={isLoading || !keywords.trim()}
            className="flex items-center gap-2 px-4 py-2 rounded-xl bg-accent text-white text-sm font-medium disabled:opacity-40 hover:bg-accent-hover transition-colors"
          >
            {isLoading ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Sparkles className="w-4 h-4" />
            )}
            {isLoading ? 'Generiert…' : 'Titel generieren'}
          </button>
        </div>

        <AnimatePresence>
          {error && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              className="flex items-start gap-2 text-xs text-error"
            >
              <AlertCircle className="w-4 h-4 shrink-0 mt-0.5" />
              <span>
                {isRateLimit
                  ? `Zu viele Anfragen. Bitte ${retryAfter}s warten.`
                  : 'Fehler beim Generieren. Bitte erneut versuchen.'}
              </span>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* Result */}
      <AnimatePresence>
        {result && (
          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            className="panel-card rounded-2xl p-5 space-y-4 border border-accent/20"
          >
            <div className="flex items-center gap-2 text-sm font-semibold text-accent">
              <Sparkles className="w-4 h-4" />
              Empfohlener Titel
            </div>

            {/* Primary */}
            <div className="rounded-xl border border-border bg-background px-4 py-3 text-sm font-medium leading-relaxed">
              {result.primary || '(kein Titel generiert)'}
            </div>

            <div className="flex flex-wrap gap-2">
              {result.primary && <CopyButton text={result.primary} />}
              {result.primary && (
                <button
                  onClick={() => handleSetOnTwitch(result.primary)}
                  disabled={setTitleStatus === 'loading'}
                  className="flex items-center gap-1.5 px-2.5 py-1 text-xs rounded-lg bg-red-500/15 hover:bg-red-500/25 text-red-400 border border-red-500/20 transition-colors disabled:opacity-50"
                >
                  {setTitleStatus === 'loading' ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : setTitleStatus === 'done' ? (
                    <CheckCircle2 className="w-3 h-3 text-success" />
                  ) : (
                    <Tv2 className="w-3 h-3" />
                  )}
                  {setTitleStatus === 'done' ? 'Gesetzt!' : 'Auf Twitch setzen'}
                </button>
              )}
            </div>

            {setTitleStatus === 'scope_missing' && (
              <p className="text-xs text-warning flex items-center gap-1.5">
                <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                Scope <code className="font-mono">channel:manage:broadcast</code> fehlt – bitte neu mit Twitch verbinden.
              </p>
            )}
            {setTitleStatus === 'error' && (
              <p className="text-xs text-error flex items-center gap-1.5">
                <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                Fehler beim Setzen des Titels.
              </p>
            )}

            {/* Alternatives */}
            {result.alternatives.length > 0 && (
              <div className="space-y-2">
                <p className="text-xs text-text-secondary font-medium">Alternativen</p>
                {result.alternatives.map((alt, i) => (
                  <div key={i} className="flex items-center justify-between gap-3 rounded-lg bg-background px-3 py-2 border border-border/60">
                    <span className="text-sm text-text-secondary truncate">{alt}</span>
                    <CopyButton text={alt} />
                  </div>
                ))}
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* History table */}
      {result && result.title_analysis.length > 0 && (
        <div className="panel-card rounded-2xl p-5 space-y-3">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <TrendingUp className="w-4 h-4 text-text-secondary" />
            Deine letzten Titel
          </div>
          <div className="overflow-x-auto -mx-1">
            <table className="w-full text-xs">
              <thead>
                <tr className="text-text-secondary border-b border-border">
                  <th className="text-left py-2 pr-4 font-medium">Titel</th>
                  <th className="text-right py-2 pr-4 font-medium">Ø Viewer</th>
                  <th className="text-right py-2 font-medium">Performance</th>
                </tr>
              </thead>
              <tbody>
                {result.title_analysis.slice(0, 15).map((item: TitleHistoryEntry, i) => (
                  <tr key={i} className="border-b border-border/40 hover:bg-card/50 transition-colors">
                    <td className="py-2 pr-4 max-w-[260px] truncate text-text-secondary">{item.title}</td>
                    <td className="py-2 pr-4 text-right tabular-nums">{item.avg_viewers ?? '—'}</td>
                    <td className="py-2 text-right">
                      <ScoreBadge value={item.relative_perf ?? 0} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="text-xs text-text-secondary/60">
            Performance = Ø Viewer relativ zum eigenen Durchschnitt (100% = Durchschnitt).
          </p>
        </div>
      )}

      {/* Weekly insight */}
      {insightData?.insight && (
        <div className="panel-card rounded-2xl p-5 space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Lightbulb className="w-4 h-4 text-warning" />
              Letzte Analyse
            </div>
            <span className="text-xs text-text-secondary">
              {new Date(insightData.insight.generated_at).toLocaleDateString('de-DE', {
                day: '2-digit', month: '2-digit', year: 'numeric',
              })}
            </span>
          </div>

          {insightData.insight.strengths && (
            <div className="flex gap-2.5 text-xs">
              <TrendingUp className="w-4 h-4 text-success shrink-0 mt-0.5" />
              <span className="text-text-secondary">{insightData.insight.strengths}</span>
            </div>
          )}
          {insightData.insight.weaknesses && (
            <div className="flex gap-2.5 text-xs">
              <TrendingDown className="w-4 h-4 text-error shrink-0 mt-0.5" />
              <span className="text-text-secondary">{insightData.insight.weaknesses}</span>
            </div>
          )}
          {insightData.insight.recommendations && (
            <div className="flex gap-2.5 text-xs">
              <Lightbulb className="w-4 h-4 text-warning shrink-0 mt-0.5" />
              <pre className="whitespace-pre-wrap font-sans text-text-secondary leading-relaxed">
                {insightData.insight.recommendations}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
