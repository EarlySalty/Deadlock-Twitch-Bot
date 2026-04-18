import { useState, useRef } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import {
  Brain,
  Loader2,
  AlertCircle,
  Lock,
  Sparkles,
  ChevronDown,
  ChevronUp,
  Clock,
  Users,
  TrendingUp,
  MessageCircle,
  Shield,
  Zap,
  Target,
  History,
  RotateCcw,
} from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { useAuthStatus } from '@/hooks/useAnalytics';
import { AIChatRateLimitError, fetchAIAnalysis, fetchAIChat, fetchAIHistory } from '@/api/ai';
import { usePlan } from '@/context/PlanContext';
import type {
  AIAnalysisResult,
  AIAnalysisPoint,
  AIChatMessage,
  AIHistoryEntry,
  TimeRange,
} from '@/types/analytics';
import { dashboardRuntimeConfig, resolveEffectiveDemoMode } from '@/runtimeConfig';

interface AIAnalysisProps {
  streamer: string | null;
  days: TimeRange;
}

const PRIORITY_CONFIG = {
  kritisch: {
    bg: 'bg-error/10',
    border: 'border-error/30',
    text: 'text-error',
    numberBg: 'bg-error/15',
    dot: 'bg-error',
    label: 'Kritisch',
  },
  hoch: {
    bg: 'bg-warning/10',
    border: 'border-warning/30',
    text: 'text-warning',
    numberBg: 'bg-warning/15',
    dot: 'bg-warning',
    label: 'Hoch',
  },
  mittel: {
    bg: 'bg-primary/10',
    border: 'border-primary/30',
    text: 'text-primary',
    numberBg: 'bg-primary/15',
    dot: 'bg-primary',
    label: 'Mittel',
  },
} as const;

function getPriorityConfig(priority: string) {
  if (priority in PRIORITY_CONFIG) {
    return PRIORITY_CONFIG[priority as keyof typeof PRIORITY_CONFIG];
  }
  return PRIORITY_CONFIG.mittel;
}

type GameFilter = 'deadlock' | 'all';

export function AIAnalysis({ streamer, days }: AIAnalysisProps) {
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();
  const { hasEntitlement } = usePlan();
  const [result, setResult] = useState<AIAnalysisResult | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expandedPoint, setExpandedPoint] = useState<number | null>(null);
  const [gameFilter, setGameFilter] = useState<GameFilter>('all');
  const [userContext, setUserContext] = useState('');
  const [chatMessages, setChatMessages] = useState<AIChatMessage[]>([]);
  const [chatInput, setChatInput] = useState('');
  const [chatError, setChatError] = useState<string | null>(null);
  const [rateLimitReset, setRateLimitReset] = useState<number | null>(null);
  const [isSendingChat, setIsSendingChat] = useState(false);

  const isDemoMode = resolveEffectiveDemoMode({
    pathname: window.location.pathname,
    runtimeConfig: dashboardRuntimeConfig,
  });
  const isAdmin = authStatus?.isAdmin || authStatus?.isLocalhost;
  const hasAiAccess = hasEntitlement('analytics.ai_mini') || hasEntitlement('analytics.ai_full');
  const canUseAI = isDemoMode || isAdmin || hasAiAccess;
  const analysisInProgress = useRef(false);

  const { data: history = [], refetch: refetchHistory } = useQuery<AIHistoryEntry[]>({
    queryKey: ['ai-history', streamer],
    queryFn: () => fetchAIHistory(streamer!, 20),
    enabled: canUseAI && !!streamer,
    staleTime: 0,
  });

  if (loadingAuth) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  if (!canUseAI) {
    return (
      <div className="flex flex-col items-center justify-center h-80 gap-5">
        <div className="relative">
          <div className="w-20 h-20 rounded-2xl bg-background/80 border border-border flex items-center justify-center">
            <Brain className="w-9 h-9 text-text-secondary opacity-40" />
          </div>
          <div className="absolute -bottom-1 -right-1 w-7 h-7 rounded-full bg-border flex items-center justify-center">
            <Lock className="w-3.5 h-3.5 text-text-secondary" />
          </div>
        </div>
        <div className="text-center max-w-xs">
          <p className="text-white font-semibold text-lg mb-1">Plan-Upgrade erforderlich</p>
          <p className="text-text-secondary text-sm leading-relaxed">
            KI-Analysen sind in den Basic-, Trial-, Erweitert- und Bundle-Plänen verfügbar.
          </p>
        </div>
      </div>
    );
  }

  const handleAnalyze = async () => {
    if (!streamer || analysisInProgress.current) return;
    analysisInProgress.current = true;
    setIsLoading(true);
    setError(null);
    setResult(null);
    setChatMessages([]);
    setChatInput('');
    setChatError(null);
    setRateLimitReset(null);
    setExpandedPoint(null);
    try {
      const data = await fetchAIAnalysis(streamer, days, gameFilter, userContext);
      setResult(data);
      setExpandedPoint(1);
      refetchHistory();
    } catch (e) {
      const isAbort = e instanceof DOMException && e.name === 'AbortError';
      setError(
        isAbort
          ? 'Analyse hat zu lange gedauert (>4 Min). Ergebnis eventuell im Verlauf verfügbar.'
          : e instanceof Error
          ? e.message
          : 'Analyse fehlgeschlagen'
      );
      refetchHistory(); // backend may have saved result despite connection drop
    } finally {
      analysisInProgress.current = false;
      setIsLoading(false);
    }
  };

  const handleSendChat = async () => {
    if (!streamer || !result?.id || !chatInput.trim() || isSendingChat) return;

    const outgoingMessage = chatInput.trim();
    const userMessage: AIChatMessage = {
      role: 'user',
      content: outgoingMessage,
      timestamp: new Date().toISOString(),
    };

    setIsSendingChat(true);
    setChatError(null);
    setRateLimitReset(null);
    setChatInput('');
    setChatMessages(prev => [...prev, userMessage]);

    try {
      const response = await fetchAIChat(streamer, result.id, outgoingMessage);
      setResult(prev => prev ? ({ ...prev, followUpsRemaining: response.followUpsRemaining }) : prev);
      setRateLimitReset(response.rateLimitReset ?? null);
      setChatMessages(prev => [
        ...prev,
        {
          role: 'assistant',
          content: response.message,
          timestamp: new Date().toISOString(),
        },
      ]);
    } catch (e) {
      setChatMessages(prev => prev.filter(message => message !== userMessage));
      if (e instanceof AIChatRateLimitError) {
        setRateLimitReset(e.rateLimitReset ?? null);
        setChatError(
          e.rateLimitReset
            ? `Limit erreicht. Wieder verfügbar in ${Math.max(1, Math.ceil((e.rateLimitReset * 1000 - Date.now()) / 60000))} Minuten.`
            : 'Keine weiteren Rückfragen für diese Analyse verfügbar.'
        );
        setResult(prev => prev ? ({ ...prev, followUpsRemaining: 0 }) : prev);
      } else {
        setChatError(e instanceof Error ? e.message : 'Rückfrage fehlgeschlagen');
      }
      setChatInput(outgoingMessage);
    } finally {
      setIsSendingChat(false);
    }
  };

  return (
    <div className="space-y-5">
      {/* Header Card */}
      <div className="panel-card rounded-2xl p-6">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div className="flex items-center gap-4">
            <div className="w-12 h-12 rounded-xl bg-primary/10 border border-primary/20 flex items-center justify-center flex-shrink-0">
              <Brain className="w-6 h-6 text-primary" />
            </div>
            <div>
              <h2 className="text-xl font-bold text-white">KI Tiefenanalyse</h2>
              <p className="text-text-secondary text-sm mt-0.5">
                KI analysiert {days} Tage Streaming-Daten
                {streamer && (
                  <span className="text-primary font-medium"> · {streamer}</span>
                )}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-3 flex-shrink-0">
            {/* Game Filter Toggle */}
            <div className="flex rounded-xl border border-border overflow-hidden text-sm">
              <button
                onClick={() => setGameFilter('all')}
                disabled={isLoading}
                className={`px-3 py-2 transition-colors ${
                  gameFilter === 'all'
                    ? 'bg-primary text-white font-semibold'
                    : 'text-text-secondary hover:text-white hover:bg-background/60'
                }`}
              >
                Alle Spiele
              </button>
              <button
                onClick={() => setGameFilter('deadlock')}
                disabled={isLoading}
                className={`px-3 py-2 transition-colors border-l border-border ${
                  gameFilter === 'deadlock'
                    ? 'bg-primary text-white font-semibold'
                    : 'text-text-secondary hover:text-white hover:bg-background/60'
                }`}
              >
                Deadlock
              </button>
            </div>

            <button
              onClick={handleAnalyze}
              disabled={isLoading || !streamer}
              className="flex items-center gap-2 px-5 py-2.5 bg-primary hover:bg-primary-hover disabled:opacity-40 disabled:cursor-not-allowed text-white rounded-xl font-semibold transition-all text-sm shadow-lg shadow-primary/20"
            >
              {isLoading ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  Analysiere...
                </>
              ) : (
                <>
                  <Sparkles className="w-4 h-4" />
                  {result ? 'Neu analysieren' : 'Analyse starten'}
                </>
              )}
            </button>
          </div>
        </div>

        {!streamer && (
          <div className="mt-4 p-3 bg-warning/10 border border-warning/20 rounded-xl flex items-center gap-2 text-warning text-sm">
            <AlertCircle className="w-4 h-4 flex-shrink-0" />
            Bitte zuerst einen Streamer auswählen
          </div>
        )}

        <div className="mt-4">
          <label className="block text-sm font-medium text-white mb-2">
            Deine eigenen Eindrücke / Fragen an die KI
            <span className="text-text-secondary font-normal"> (optional)</span>
          </label>
          <textarea
            value={userContext}
            onChange={(event) => setUserContext(event.target.value.slice(0, 2000))}
            disabled={isLoading}
            placeholder="z.B. Warum performen manche Titel schlechter? Welche Streams sollte ich wiederholen?"
            className="w-full min-h-[110px] rounded-2xl border border-border bg-background/60 px-4 py-3 text-sm text-white placeholder:text-text-secondary outline-none focus:border-primary/40 focus:ring-2 focus:ring-primary/15 resize-y"
          />
          <div className="mt-2 text-xs text-text-secondary text-right">
            {userContext.length}/2000
          </div>
        </div>

        {/* Feature pills */}
        {!result && !isLoading && (
          <div className="flex flex-wrap gap-2 mt-4">
            {[
              { icon: Target, label: '10 priorisierte Handlungspunkte' },
              { icon: Zap, label: 'Daten-basierte Insights' },
              { icon: Shield, label: 'Nur für dich sichtbar' },
            ].map(({ icon: Icon, label }) => (
              <span
                key={label}
                className="flex items-center gap-1.5 px-3 py-1 text-xs text-text-secondary bg-background/60 rounded-full border border-border"
              >
                <Icon className="w-3 h-3" />
                {label === 'Nur für dich sichtbar' && isDemoMode ? 'Snapshot-basiert' : label}
              </span>
            ))}
          </div>
        )}
      </div>

      {/* Loading Animation */}
      {isLoading && (
        <div className="panel-card rounded-2xl p-10 flex flex-col items-center gap-5">
          <div className="relative">
            <div className="w-16 h-16 rounded-full border-2 border-primary/20 border-t-primary animate-spin" />
            <Brain className="w-7 h-7 text-primary absolute inset-0 m-auto" />
          </div>
          <div className="text-center">
            <p className="text-white font-semibold">KI analysiert...</p>
            <p className="text-text-secondary text-sm mt-1 max-w-xs">
              Alle Stream-Daten werden ausgewertet. Das kann 1–3 Minuten dauern.
            </p>
          </div>
          <div className="flex gap-1.5 mt-2">
            {[0, 1, 2].map(i => (
              <div
                key={i}
                className="w-2 h-2 rounded-full bg-primary animate-bounce"
                style={{ animationDelay: `${i * 0.15}s` }}
              />
            ))}
          </div>
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="panel-card rounded-2xl p-4 flex items-start gap-3 border border-error/20 bg-error/5">
          <AlertCircle className="w-5 h-5 text-error flex-shrink-0 mt-0.5" />
          <div>
            <p className="text-error font-medium text-sm">Analyse fehlgeschlagen</p>
            <p className="text-text-secondary text-xs mt-0.5">{error}</p>
          </div>
        </div>
      )}

      {/* Results */}
      {result && (
        <motion.div
          initial={{ opacity: 0, y: 16 }}
          animate={{ opacity: 1, y: 0 }}
          className="space-y-4"
        >
          {/* Data Snapshot */}
          <DataSnapshotGrid snapshot={result.dataSnapshot} />

          {/* Meta info */}
          <div className="flex items-center gap-3 flex-wrap text-xs text-text-secondary px-1">
            <div className="flex items-center gap-2">
              <Brain className="w-3 h-3 text-primary" />
              <span>
                {result.model === 'opus' ? 'Premium-Analyse' : 'KI-Analyse'} · generiert am{' '}
                {new Date(result.generatedAt).toLocaleString('de-DE', {
                  day: '2-digit',
                  month: '2-digit',
                  year: 'numeric',
                  hour: '2-digit',
                  minute: '2-digit',
                })}
              </span>
            </div>
            <span className="px-2 py-0.5 rounded-full bg-background/60 border border-border font-medium">
              {result.gameFilter === 'deadlock' ? '🎮 Deadlock' : '🌐 Alle Spiele'}
            </span>
            <span className="px-2 py-0.5 rounded-full bg-primary/10 border border-primary/20 text-primary font-medium">
              {result.model === 'opus' ? 'Premium-Analyse' : 'KI-Analyse'}
            </span>
          </div>

          {/* Priority legend */}
          <div className="flex flex-wrap gap-3 px-1">
            {(['kritisch', 'hoch', 'mittel'] as const).map(p => {
              const cfg = PRIORITY_CONFIG[p];
              return (
                <span key={p} className="flex items-center gap-1.5 text-xs text-text-secondary">
                  <span className={`w-2 h-2 rounded-full ${cfg.dot}`} />
                  {cfg.label}
                </span>
              );
            })}
          </div>

          {/* Analysis Points */}
          <div className="space-y-3">
            {result.points.map((point, i) => (
              <AnalysisPointCard
                key={point.number}
                point={point}
                index={i}
                isExpanded={expandedPoint === point.number}
                onToggle={() =>
                  setExpandedPoint(expandedPoint === point.number ? null : point.number)
                }
              />
            ))}
          </div>

          {result.sessionKey && result.id && (
            <AIChatPanel
              analysisId={result.id}
              model={result.model}
              followUpsRemaining={result.followUpsRemaining ?? 0}
              rateLimitReset={rateLimitReset}
              messages={chatMessages}
              inputValue={chatInput}
              error={chatError}
              isSending={isSendingChat}
              onInputChange={setChatInput}
              onSend={handleSendChat}
            />
          )}
        </motion.div>
      )}

      {/* History Panel */}
      {history.length > 0 && (
        <HistoryPanel
          history={history}
          activeId={result?.id ?? null}
          onRestore={(entry) => {
            setResult(entry);
            setExpandedPoint(1);
            setError(null);
            setChatMessages([]);
            setChatInput('');
            setChatError(null);
            setRateLimitReset(null);
          }}
        />
      )}
    </div>
  );
}

interface DataSnapshotGridProps {
  snapshot: AIAnalysisResult['dataSnapshot'];
}

function DataSnapshotGrid({ snapshot }: DataSnapshotGridProps) {
  const items = [
    {
      icon: <TrendingUp className="w-4 h-4" />,
      label: 'Streams',
      value: snapshot.streamCount.toString(),
      color: 'primary',
    },
    {
      icon: <Clock className="w-4 h-4" />,
      label: 'Stunden',
      value: `${snapshot.totalHours.toFixed(1)}h`,
      color: 'accent',
    },
    {
      icon: <Users className="w-4 h-4" />,
      label: 'Ø Viewer',
      value: Math.round(snapshot.avgViewers).toLocaleString('de-DE'),
      color: 'success',
    },
    {
      icon: <MessageCircle className="w-4 h-4" />,
      label: 'Ø Chatter',
      value: snapshot.avgChatters.toLocaleString('de-DE'),
      color: 'warning',
    },
  ] as const;

  const colorMap = {
    primary: 'bg-primary/10 text-primary',
    accent: 'bg-accent/10 text-accent',
    success: 'bg-success/10 text-success',
    warning: 'bg-warning/10 text-warning',
  };

  return (
    <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
      {items.map(item => (
        <div key={item.label} className="panel-card rounded-xl p-3 soft-elevate">
          <div className={`w-8 h-8 rounded-lg ${colorMap[item.color]} flex items-center justify-center mb-2`}>
            {item.icon}
          </div>
          <div className="text-xs text-text-secondary">{item.label}</div>
          <div className="text-lg font-bold text-white">{item.value}</div>
        </div>
      ))}
    </div>
  );
}

interface AnalysisPointCardProps {
  point: AIAnalysisPoint;
  index: number;
  isExpanded: boolean;
  onToggle: () => void;
}

function AnalysisPointCard({ point, index, isExpanded, onToggle }: AnalysisPointCardProps) {
  const cfg = getPriorityConfig(point.priority);

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: index * 0.04 }}
      className={`panel-card rounded-2xl overflow-hidden border ${cfg.border}`}
    >
      {/* Header */}
      <div
        className="p-4 cursor-pointer hover:bg-background/30 transition-colors select-none"
        onClick={onToggle}
      >
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-start gap-3">
            {/* Number badge */}
            <div
              className={`flex-shrink-0 w-8 h-8 rounded-lg ${cfg.numberBg} ${cfg.border} border flex items-center justify-center`}
            >
              <span className={`text-sm font-bold ${cfg.text}`}>{point.number}</span>
            </div>

            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 flex-wrap">
                <span className="font-semibold text-white leading-snug">{point.title}</span>
                <span
                  className={`text-[10px] font-bold px-1.5 py-0.5 rounded-full ${cfg.bg} ${cfg.text} border ${cfg.border} leading-none flex-shrink-0`}
                >
                  {cfg.label}
                </span>
              </div>

              {!isExpanded && (
                <p className="text-text-secondary text-sm mt-1 line-clamp-2 leading-relaxed">
                  {point.analysis}
                </p>
              )}
            </div>
          </div>

          {isExpanded ? (
            <ChevronUp className="w-5 h-5 text-text-secondary flex-shrink-0 mt-0.5" />
          ) : (
            <ChevronDown className="w-5 h-5 text-text-secondary flex-shrink-0 mt-0.5" />
          )}
        </div>
      </div>

      {/* Expanded Detail */}
      <AnimatePresence>
        {isExpanded && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="border-t border-border"
          >
            <div className="p-4 space-y-4">
              <DetailSection label="Analyse" content={point.analysis} />
              {point.action && (
                <DetailSection label="Handlungsempfehlung" content={point.action} />
              )}
              {point.expectedImpact && (
                <DetailSection
                  label="Erwarteter Impact"
                  content={point.expectedImpact}
                  className={cfg.text}
                />
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  );
}

interface DetailSectionProps {
  label: string;
  content: string;
  className?: string;
}

function DetailSection({ label, content, className = 'text-white' }: DetailSectionProps) {
  return (
    <div>
      <div className="text-[10px] font-bold uppercase tracking-widest text-text-secondary mb-1.5">
        {label}
      </div>
      <p className={`text-sm leading-relaxed ${className}`}>{content}</p>
    </div>
  );
}

interface AIChatPanelProps {
  analysisId: number;
  model?: AIAnalysisResult['model'];
  followUpsRemaining: number;
  rateLimitReset: number | null;
  messages: AIChatMessage[];
  inputValue: string;
  error: string | null;
  isSending: boolean;
  onInputChange: (value: string) => void;
  onSend: () => void;
}

function AIChatPanel({
  analysisId,
  model,
  followUpsRemaining,
  rateLimitReset,
  messages,
  inputValue,
  error,
  isSending,
  onInputChange,
  onSend,
}: AIChatPanelProps) {
  const limitLabel = model === 'opus'
    ? `Noch ${followUpsRemaining} Rückfragen verfügbar`
    : `Noch ${followUpsRemaining}/10 Rückfragen in dieser Stunde`;

  const rateLimitLabel = rateLimitReset
    ? `Limit erreicht. Wieder verfügbar in ${Math.max(1, Math.ceil((rateLimitReset * 1000 - Date.now()) / 60000))} Minuten.`
    : null;

  return (
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="text-white font-semibold">Rückfragen zur Analyse</div>
          <div className="text-sm text-text-secondary">
            Analyse #{analysisId}
          </div>
        </div>
        <div className="text-xs px-3 py-1.5 rounded-full border border-border bg-background/60 text-text-secondary">
          {limitLabel}
        </div>
      </div>

      <div className="space-y-3">
        {messages.length === 0 ? (
          <div className="rounded-xl border border-dashed border-border bg-background/40 px-4 py-5 text-sm text-text-secondary">
            Stelle gezielte Rückfragen zur bestehenden Analyse, z.B. zu Titeln, Wochentagen oder den wichtigsten Hebeln.
          </div>
        ) : (
          messages.map((message, index) => (
            <div
              key={`${message.timestamp}-${index}`}
                className={`rounded-2xl px-4 py-3 text-sm leading-relaxed border ${
                message.role === 'user'
                  ? 'ml-auto max-w-[88%] border-primary/25 bg-primary/10 text-white'
                  : 'mr-auto max-w-[88%] border-border bg-background/50 text-white'
              }`}
            >
              {message.content}
            </div>
          ))
        )}
      </div>

      {(error || rateLimitLabel) && (
        <div className="rounded-xl border border-warning/20 bg-warning/5 px-4 py-3 text-sm text-warning">
          {error || rateLimitLabel}
        </div>
      )}

      <div className="flex flex-col gap-3">
        <textarea
          value={inputValue}
          onChange={(event) => onInputChange(event.target.value)}
          disabled={isSending || followUpsRemaining <= 0}
          placeholder="Deine Rückfrage zur Analyse..."
          className="w-full min-h-[96px] rounded-2xl border border-border bg-background/60 px-4 py-3 text-sm text-white placeholder:text-text-secondary outline-none focus:border-primary/40 focus:ring-2 focus:ring-primary/15 resize-y disabled:opacity-60"
        />
        <div className="flex justify-end">
          <button
            type="button"
            onClick={onSend}
            disabled={isSending || !inputValue.trim() || followUpsRemaining <= 0}
            className="flex items-center gap-2 px-4 py-2.5 bg-primary hover:bg-primary-hover disabled:opacity-40 disabled:cursor-not-allowed text-white rounded-xl font-semibold transition-all text-sm"
          >
            {isSending ? (
              <>
                <Loader2 className="w-4 h-4 animate-spin" />
                Sende...
              </>
            ) : (
              <>
                <MessageCircle className="w-4 h-4" />
                Rückfrage senden
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── History Panel ──────────────────────────────────────────────────────

interface HistoryPanelProps {
  history: AIHistoryEntry[];
  activeId: number | null | undefined;
  onRestore: (entry: AIHistoryEntry) => void;
}

function HistoryPanel({ history, activeId, onRestore }: HistoryPanelProps) {
  const [open, setOpen] = useState(false);

  return (
    <div className="panel-card rounded-2xl overflow-hidden">
      {/* Header toggle */}
      <button
        className="w-full flex items-center justify-between p-4 hover:bg-background/30 transition-colors"
        onClick={() => setOpen(o => !o)}
      >
        <div className="flex items-center gap-2 text-sm font-semibold text-text-secondary">
          <History className="w-4 h-4" />
          Vergangene Analysen
          <span className="px-1.5 py-0.5 text-[10px] bg-background rounded-full border border-border">
            {history.length}
          </span>
        </div>
        {open ? (
          <ChevronUp className="w-4 h-4 text-text-secondary" />
        ) : (
          <ChevronDown className="w-4 h-4 text-text-secondary" />
        )}
      </button>

      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="border-t border-border"
          >
            <div className="divide-y divide-border">
              {history.map((entry) => {
                const isActive = entry.id === activeId;
                const date = new Date(entry.generatedAt).toLocaleString('de-DE', {
                  day: '2-digit',
                  month: '2-digit',
                  year: 'numeric',
                  hour: '2-digit',
                  minute: '2-digit',
                  second: '2-digit',
                });
                return (
                  <div
                    key={entry.id}
                    className={`flex items-center justify-between p-3 gap-3 transition-colors ${
                      isActive ? 'bg-primary/5' : 'hover:bg-background/30'
                    }`}
                  >
                    <div className="min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="text-xs text-white font-medium">{date}</span>
                        <span className="text-[10px] text-text-secondary px-1.5 py-0.5 rounded-full border border-border">
                          {entry.days}d
                        </span>
                        {isActive && (
                          <span className="text-[10px] text-primary px-1.5 py-0.5 rounded-full bg-primary/10 border border-primary/30">
                            aktiv
                          </span>
                        )}
                      </div>
                      {/* Priority distribution */}
                      <div className="flex items-center gap-2 mt-1">
                        {entry.kritischCount > 0 && (
                          <span className="flex items-center gap-1 text-[10px] text-error">
                            <span className="w-1.5 h-1.5 rounded-full bg-error" />
                            {entry.kritischCount}k
                          </span>
                        )}
                        {entry.hochCount > 0 && (
                          <span className="flex items-center gap-1 text-[10px] text-warning">
                            <span className="w-1.5 h-1.5 rounded-full bg-warning" />
                            {entry.hochCount}h
                          </span>
                        )}
                        {entry.mittelCount > 0 && (
                          <span className="flex items-center gap-1 text-[10px] text-primary">
                            <span className="w-1.5 h-1.5 rounded-full bg-primary" />
                            {entry.mittelCount}m
                          </span>
                        )}
                        <span className="text-[10px] text-text-secondary">
                          Ø {Math.round(entry.dataSnapshot.avgViewers)} Viewer
                        </span>
                      </div>
                    </div>

                    {!isActive && (
                      <button
                        onClick={() => onRestore(entry)}
                        className="flex-shrink-0 flex items-center gap-1 px-2.5 py-1.5 text-xs text-text-secondary hover:text-white border border-border hover:border-primary/40 rounded-lg transition-colors"
                        title="Diese Analyse laden"
                      >
                        <RotateCcw className="w-3 h-3" />
                        Laden
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

export default AIAnalysis;
