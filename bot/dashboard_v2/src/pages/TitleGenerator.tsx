import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { AnimatePresence, motion } from 'framer-motion';
import {
  AlertCircle,
  Check,
  CheckCircle2,
  Copy,
  ExternalLink,
  Lightbulb,
  Loader2,
  Save,
  Sparkles,
  ThumbsDown,
  ThumbsUp,
  Tv2,
  WandSparkles,
  Zap,
} from 'lucide-react';
import { Rise } from '../motion/Rise';
import {
  fetchTitleInsights,
  fetchTitleSettings,
  fetchTitleSuggestion,
  saveTitleSettings,
  sendTitleFeedback,
  setTwitchTitle,
  type TitleHistoryEntry,
  type TitleSettings,
  type TitleSuggestResult,
} from '@/api/title';
import { useAuthStatus } from '@/hooks/useAnalytics';

interface TitleGeneratorProps {
  streamer: string | null;
}

function ScoreBadge({ value }: { value: number }) {
  const pct = Math.min(Math.round(value * 100), 200);
  const state = value >= 1.4 ? 'text-success' : value >= 0.8 ? 'text-warning' : 'text-error';
  const marker = value >= 1.4 ? '↑' : value >= 0.8 ? '→' : '↓';
  return <span className={`font-mono text-xs ${state}`}>{marker} {pct}%</span>;
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      onClick={() => {
        navigator.clipboard.writeText(text).catch(() => {});
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1600);
      }}
      className="flex items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 py-1.5 text-xs text-text-secondary transition-colors hover:bg-card-hover hover:text-white"
    >
      {copied ? <CheckCircle2 className="h-3.5 w-3.5 text-success" /> : <Copy className="h-3.5 w-3.5" />}
      {copied ? 'Kopiert' : 'Kopieren'}
    </button>
  );
}

function Toggle({ checked, onChange, disabled = false }: { checked: boolean; onChange: (value: boolean) => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${checked ? 'bg-primary' : 'bg-border'}`}
    >
      <span className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${checked ? 'translate-x-[18px]' : 'translate-x-0.5'}`} />
    </button>
  );
}

export function TitleGenerator({ streamer }: TitleGeneratorProps) {
  const queryClient = useQueryClient();
  const { data: authStatus } = useAuthStatus();
  const csrfToken = authStatus?.csrfToken ?? authStatus?.csrf_token;
  const [keywords, setKeywords] = useState('');
  const [includeLive, setIncludeLive] = useState(true);
  const [stylePreference, setStylePreference] = useState('');
  const [autoSet, setAutoSet] = useState(false);
  const [result, setResult] = useState<TitleSuggestResult | null>(null);
  const [editableTitle, setEditableTitle] = useState('');
  const [feedbackState, setFeedbackState] = useState<'idle' | 'liked' | 'disliked' | 'saved'>('idle');
  const [setTitleStatus, setSetTitleStatus] = useState<'idle' | 'loading' | 'done' | 'error' | 'scope_missing'>('idle');

  const settingsQuery = useQuery({
    queryKey: ['title-settings', streamer],
    queryFn: () => fetchTitleSettings(streamer),
    enabled: !!streamer,
    staleTime: 60_000,
  });
  const settings = settingsQuery.data;

  useEffect(() => {
    if (!settings) return;
    setStylePreference(settings.style_preference ?? '');
    setAutoSet(settings.experimental_auto_set ?? false);
  }, [settings]);

  const insightQuery = useQuery({
    queryKey: ['title-insights', streamer],
    queryFn: () => fetchTitleInsights(streamer),
    enabled: !!streamer,
    staleTime: 60 * 60 * 1000,
  });

  const generateMutation = useMutation({
    mutationFn: () => fetchTitleSuggestion({
      keywords: keywords.trim(),
      include_live: includeLive,
      streamer,
    }, csrfToken),
    onSuccess: (data) => {
      setResult(data);
      setEditableTitle(data.primary);
      setFeedbackState('idle');
      if (data.auto_set_status === 'set') setSetTitleStatus('done');
      else if (data.auto_set_status === 'scope_missing') setSetTitleStatus('scope_missing');
      else setSetTitleStatus('idle');
    },
  });

  const settingsMutation = useMutation({
    mutationFn: (next: Pick<TitleSettings, 'style_preference' | 'experimental_auto_set'>) => saveTitleSettings({
      ...next,
      streamer,
    }, csrfToken),
    onSuccess: (data) => {
      queryClient.setQueryData(['title-settings', streamer], data);
      setStylePreference(data.style_preference);
      setAutoSet(data.experimental_auto_set);
    },
  });

  const feedbackMutation = useMutation({
    mutationFn: (payload: { feedback: 'liked' | 'disliked' | 'selected' | 'edited'; selected_title?: string; edited_title?: string }) => {
      if (!result?.generation_id) return Promise.resolve({ ok: true });
      return sendTitleFeedback({
        generation_id: result.generation_id,
        streamer,
        ...payload,
      }, csrfToken);
    },
  });

  const styleDirty = useMemo(() => {
    if (!settings) return false;
    return stylePreference.trim() !== settings.style_preference.trim() || autoSet !== settings.experimental_auto_set;
  }, [autoSet, settings, stylePreference]);

  const saveSettings = () => settingsMutation.mutate({
    style_preference: stylePreference.trim(),
    experimental_auto_set: autoSet,
  });

  const submitFeedback = (feedback: 'liked' | 'disliked') => {
    feedbackMutation.mutate({ feedback }, {
      onSuccess: () => setFeedbackState(feedback),
    });
  };

  const chooseAlternative = (title: string) => {
    setEditableTitle(title);
    feedbackMutation.mutate({ feedback: 'selected', selected_title: title }, {
      onSuccess: () => setFeedbackState('saved'),
    });
  };

  const handleSetOnTwitch = async () => {
    const title = editableTitle.trim();
    if (!title) return;
    setSetTitleStatus('loading');
    try {
      const edited = !!result?.primary && title !== result.primary;
      if (edited && result?.generation_id) {
        await sendTitleFeedback({
          generation_id: result.generation_id,
          streamer,
          feedback: 'edited',
          edited_title: title,
        }, csrfToken);
      }
      await setTwitchTitle(title, streamer, result?.generation_id, csrfToken);
      setSetTitleStatus('done');
      setFeedbackState('saved');
    } catch (error) {
      const message = error instanceof Error ? error.message : '';
      setSetTitleStatus(message.includes('scope_missing') || message.includes('reauth') ? 'scope_missing' : 'error');
    }
  };

  const error = generateMutation.error as Error | null;
  const isRateLimit = error?.message?.startsWith('rate_limit');
  const retryAfter = isRateLimit ? error?.message.split(':')[1] : null;

  return (
    <div className="mx-auto max-w-4xl space-y-5 py-4">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <div className="mb-1 flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.18em] text-accent">
            <WandSparkles className="h-4 w-4" /> Titel-Studio
          </div>
          <h1 className="text-2xl font-bold text-white">Titel, die nach dir klingen</h1>
          <p className="mt-1 max-w-2xl text-sm text-text-secondary">
            Eigene Historie, Human-Feedback und starke Deadlock-Titel fließen zusammen. 2–3 Stichwörter helfen – ohne Eingabe wird ein sinnvoller Auto-Titel gebaut.
          </p>
        </div>
        {settings?.model && (
          <span className="w-fit rounded-full border border-border bg-background/70 px-2.5 py-1 text-[11px] font-mono text-text-secondary">
            {settings.model}
          </span>
        )}
      </div>

      <div className="panel-card rounded-2xl p-5 space-y-4">
        <div className="flex items-start justify-between gap-4">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Sparkles className="h-4 w-4 text-accent" /> Dein Standard-Stil
            </div>
            <p className="mt-1 text-xs text-text-secondary">Diese Präferenz gilt dauerhaft und steht über allgemeinen Community-Mustern.</p>
          </div>
          <button
            type="button"
            onClick={saveSettings}
            disabled={!styleDirty || settingsMutation.isPending}
            className="flex items-center gap-1.5 rounded-lg border border-border bg-background px-3 py-1.5 text-xs font-medium text-text-secondary transition-colors hover:text-white disabled:opacity-40"
          >
            {settingsMutation.isPending ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Save className="h-3.5 w-3.5" />}
            Speichern
          </button>
        </div>
        <textarea
          value={stylePreference}
          onChange={(event) => setStylePreference(event.target.value)}
          maxLength={1200}
          rows={3}
          placeholder="z.B. trocken und direkt, gerne Wortspiele, keine Emojis, keine ALL CAPS, eher 60–90 Zeichen…"
          className="w-full resize-y rounded-xl border border-border bg-background px-3.5 py-3 text-sm leading-relaxed outline-none transition-colors placeholder:text-text-secondary/45 focus:border-primary/60"
        />
        {settings?.style_summary && (
          <div className="rounded-xl border border-border/60 bg-background/50 px-3.5 py-2.5 text-xs text-text-secondary">
            <span className="font-semibold text-white/80">Aus deiner Historie erkannt:</span> {settings.style_summary}
          </div>
        )}

        <div className="flex flex-col gap-3 rounded-xl border border-warning/20 bg-warning/5 p-3.5 sm:flex-row sm:items-center sm:justify-between">
          <div className="min-w-0">
            <div className="flex items-center gap-2 text-sm font-medium text-white">
              <Zap className="h-4 w-4 text-warning" /> Experimentell: automatisch auf Twitch setzen
            </div>
            <p className="mt-1 text-xs text-text-secondary">
              Nach der Generierung wird der Hauptvorschlag direkt als Twitch-Titel gesetzt. Benötigt <code className="font-mono text-[11px]">channel:manage:broadcast</code>.
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2.5">
            {settings && !settings.oauth_connected ? (
              <a href={settings.oauth_url} className="flex items-center gap-1.5 rounded-lg border border-primary/30 bg-primary/10 px-3 py-1.5 text-xs font-semibold text-primary hover:bg-primary/15">
                Twitch verbinden <ExternalLink className="h-3.5 w-3.5" />
              </a>
            ) : (
              <span className="flex items-center gap-1 text-xs text-success"><Check className="h-3.5 w-3.5" /> Schreibrecht verbunden</span>
            )}
            <Toggle checked={autoSet} disabled={!settings?.oauth_connected} onChange={setAutoSet} />
          </div>
        </div>
      </div>

      <div className="panel-card rounded-2xl p-5 space-y-4">
        <div>
          <div className="flex items-center gap-2 text-sm font-semibold"><Lightbulb className="h-4 w-4 text-accent" /> Was ist heute besonders?</div>
          <p className="mt-1 text-xs text-text-secondary">Optional. Zwei oder drei Stichwörter reichen; leer = Auto-Modus.</p>
        </div>
        <div className="flex flex-col gap-3 sm:flex-row">
          <input
            type="text"
            value={keywords}
            onChange={(event) => setKeywords(event.target.value)}
            onKeyDown={(event) => event.key === 'Enter' && !generateMutation.isPending && generateMutation.mutate()}
            maxLength={300}
            placeholder="z.B. Haze, Duo, Asc 4 — oder einfach leer lassen"
            className="min-w-0 flex-1 rounded-xl border border-border bg-background px-3.5 py-2.5 text-sm outline-none transition-colors placeholder:text-text-secondary/45 focus:border-primary/60"
          />
          <button
            type="button"
            onClick={() => generateMutation.mutate()}
            disabled={generateMutation.isPending || !streamer}
            className="flex items-center justify-center gap-2 rounded-xl bg-accent px-4 py-2.5 text-sm font-semibold text-bg transition-colors hover:bg-accent-hover disabled:opacity-40"
          >
            {generateMutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Sparkles className="h-4 w-4" />}
            {generateMutation.isPending ? 'Denke…' : keywords.trim() ? 'Titel bauen' : 'Auto-Titel bauen'}
          </button>
        </div>
        <label className="flex w-fit cursor-pointer items-center gap-2 text-xs text-text-secondary">
          <Toggle checked={includeLive} onChange={setIncludeLive} />
          Rang / Live-Hero / Party-Kontext nutzen, wenn vorhanden
        </label>
        <AnimatePresence>
          {error && (
            <motion.div initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: 'auto' }} exit={{ opacity: 0, height: 0 }} className="flex items-start gap-2 text-xs text-error">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              {isRateLimit ? `Zu viele Anfragen. Bitte ${retryAfter ?? 'kurz'} warten.` : 'Titel konnte gerade nicht generiert werden.'}
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      <AnimatePresence>
        {result && (
          <Rise className="panel-card rounded-2xl border border-accent/20 p-5 space-y-4">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-accent"><Sparkles className="h-4 w-4" /> Vorschlag</div>
              <div className="flex items-center gap-2 text-[11px] text-text-secondary">
                {result.auto_mode && <span className="rounded-full border border-border px-2 py-0.5">Auto-Modus</span>}
                {result.generated_by === 'fallback' && <span className="rounded-full border border-warning/30 bg-warning/10 px-2 py-0.5 text-warning">Fallback</span>}
                {result.live_context_used && <span className="rounded-full border border-success/30 bg-success/10 px-2 py-0.5 text-success">Live-Kontext</span>}
              </div>
            </div>

            <textarea
              value={editableTitle}
              onChange={(event) => setEditableTitle(event.target.value)}
              maxLength={140}
              rows={2}
              className="w-full resize-none rounded-xl border border-border bg-background px-4 py-3 text-base font-medium leading-relaxed outline-none transition-colors focus:border-primary/60"
            />
            <div className="flex flex-wrap items-center gap-2">
              <CopyButton text={editableTitle} />
              <button type="button" onClick={() => submitFeedback('liked')} disabled={feedbackMutation.isPending} className={`flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs transition-colors ${feedbackState === 'liked' ? 'border-success/40 bg-success/10 text-success' : 'border-border bg-card text-text-secondary hover:text-white'}`}>
                <ThumbsUp className="h-3.5 w-3.5" /> Passt zu mir
              </button>
              <button type="button" onClick={() => submitFeedback('disliked')} disabled={feedbackMutation.isPending} className={`flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs transition-colors ${feedbackState === 'disliked' ? 'border-error/40 bg-error/10 text-error' : 'border-border bg-card text-text-secondary hover:text-white'}`}>
                <ThumbsDown className="h-3.5 w-3.5" /> So nicht
              </button>
              <button
                type="button"
                onClick={handleSetOnTwitch}
                disabled={setTitleStatus === 'loading' || !editableTitle.trim()}
                className="flex items-center gap-1.5 rounded-lg border border-danger/20 bg-danger-soft px-3 py-1.5 text-xs font-medium text-danger transition-colors hover:bg-danger/25 disabled:opacity-50"
              >
                {setTitleStatus === 'loading' ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : setTitleStatus === 'done' ? <CheckCircle2 className="h-3.5 w-3.5 text-success" /> : <Tv2 className="h-3.5 w-3.5" />}
                {setTitleStatus === 'done' ? 'Auf Twitch gesetzt' : 'Auf Twitch setzen'}
              </button>
            </div>

            {setTitleStatus === 'scope_missing' && (
              <div className="flex flex-wrap items-center gap-2 rounded-lg border border-warning/20 bg-warning/5 px-3 py-2 text-xs text-warning">
                <AlertCircle className="h-3.5 w-3.5" /> Schreibrecht fehlt oder muss erneuert werden.
                <a href={result.oauth_url || settings?.oauth_url || '/twitch/raid/auth?scope_profile=dashboard_reauth'} className="font-semibold underline">Twitch verbinden</a>
              </div>
            )}
            {setTitleStatus === 'error' && <p className="text-xs text-error">Twitch hat das Setzen des Titels gerade nicht bestätigt.</p>}
            {result.auto_set_status === 'set' && <p className="text-xs text-success">Auto-Set war aktiv: Dieser Titel wurde direkt auf Twitch übernommen.</p>}

            {result.alternatives.length > 0 && (
              <div className="space-y-2 pt-1">
                <p className="text-xs font-medium text-text-secondary">Andere Richtungen</p>
                {result.alternatives.map((alternative) => (
                  <div key={alternative} className="flex flex-col gap-2 rounded-xl border border-border/60 bg-background px-3.5 py-3 sm:flex-row sm:items-center sm:justify-between">
                    <span className="text-sm text-text-secondary">{alternative}</span>
                    <button type="button" onClick={() => chooseAlternative(alternative)} className="shrink-0 rounded-lg border border-border px-2.5 py-1 text-xs text-text-secondary hover:text-white">Übernehmen</button>
                  </div>
                ))}
              </div>
            )}

            {result.style_summary && <p className="border-t border-border/60 pt-3 text-[11px] text-text-secondary">Stilbasis: {result.style_summary}</p>}
          </Rise>
        )}
      </AnimatePresence>

      {result && result.title_analysis.length > 0 && (
        <div className="panel-card rounded-2xl p-5 space-y-3">
          <div className="text-sm font-semibold">Eigene Titel-Historie</div>
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead><tr className="border-b border-border text-text-secondary"><th className="py-2 pr-4 text-left font-medium">Titel</th><th className="py-2 pr-4 text-right font-medium">Ø Viewer</th><th className="py-2 text-right font-medium">relativ</th></tr></thead>
              <tbody>
                {result.title_analysis.slice(0, 12).map((item: TitleHistoryEntry, index) => (
                  <tr key={`${item.title}-${index}`} className="border-b border-border/40">
                    <td className="max-w-[360px] truncate py-2 pr-4 text-text-secondary">{item.title}</td>
                    <td className="py-2 pr-4 text-right tabular-nums">{item.avg_viewers ?? '—'}</td>
                    <td className="py-2 text-right"><ScoreBadge value={item.relative_perf ?? 0} /></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="text-[11px] text-text-secondary/60">Viewer-Performance ist nur ein Signal. Explizites Human-Feedback und deine gespeicherte Stilpräferenz wiegen im Generator stärker.</p>
        </div>
      )}

      {insightQuery.data?.insight && (
        <div className="panel-card rounded-2xl p-5 space-y-2">
          <div className="flex items-center gap-2 text-sm font-semibold"><Lightbulb className="h-4 w-4 text-warning" /> Titel-Insight</div>
          {insightQuery.data.insight.strengths && <p className="text-xs text-text-secondary"><span className="font-semibold text-white/80">Stark:</span> {insightQuery.data.insight.strengths}</p>}
          {insightQuery.data.insight.patterns && <p className="text-xs text-text-secondary"><span className="font-semibold text-white/80">Muster:</span> {insightQuery.data.insight.patterns}</p>}
          {insightQuery.data.insight.recommendations && <pre className="whitespace-pre-wrap font-sans text-xs leading-relaxed text-text-secondary">{insightQuery.data.insight.recommendations}</pre>}
        </div>
      )}
    </div>
  );
}
