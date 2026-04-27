import { useState } from 'react';
import {
  AlertCircle,
  BarChart2,
  ChevronDown,
  ChevronUp,
  Loader2,
  Lock,
  Sparkles,
  ThumbsDown,
  ThumbsUp,
  TrendingUp,
} from 'lucide-react';
import { usePlan } from '@/context/PlanContext';
import { useStreamReport } from '@/hooks/useAnalytics';
import type {
  StreamReportChange,
  StreamReportPoint,
  StreamReportRecommendation,
  StreamReportWordGroup,
} from '@/types/analytics';

interface PostStreamReportCardProps {
  streamer: string | null;
  sessionId?: number;
}

function ExpandablePoint({ item, color }: { item: StreamReportPoint; color: string }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="overflow-hidden rounded-lg border border-border">
      <button
        onClick={() => setOpen((openState) => !openState)}
        className="flex w-full items-center justify-between p-3 text-left transition-colors hover:bg-white/5"
      >
        <span className={`text-sm font-medium ${color}`}>{item.punkt}</span>
        {open ? (
          <ChevronUp className="h-4 w-4 flex-shrink-0 text-text-secondary" />
        ) : (
          <ChevronDown className="h-4 w-4 flex-shrink-0 text-text-secondary" />
        )}
      </button>
      {open && (
        <div className="px-3 pb-3">
          <p className="text-xs text-text-secondary">{item.begruendung}</p>
        </div>
      )}
    </div>
  );
}

function ChangeList({ items }: { items: StreamReportChange[] }) {
  return (
    <div className="space-y-2">
      {items.map((item, index) => (
        <div key={index} className="rounded-lg border border-blue-500/20 bg-blue-500/10 p-3">
          <p className="text-sm font-medium text-blue-300">{item.aspekt}</p>
          <p className="mt-1 text-xs text-text-secondary">{item.detail}</p>
        </div>
      ))}
    </div>
  );
}

function RecommendationList({ items }: { items: StreamReportRecommendation[] }) {
  return (
    <div className="space-y-2">
      {items.map((item, index) => (
        <div key={index} className="rounded-lg border border-purple-500/20 bg-purple-500/10 p-3">
          <p className="text-sm font-medium text-purple-300">{item.trend}</p>
          <p className="mt-1 text-xs text-text-secondary">{item.empfehlung}</p>
        </div>
      ))}
    </div>
  );
}

function WordGroups({ groups }: { groups: StreamReportWordGroup[] }) {
  if (groups.length === 0) return null;

  return (
    <div className="space-y-2">
      <span className="text-xs font-bold uppercase tracking-wide text-text-secondary">
        Chat-Wortgruppen
      </span>
      <div className="flex flex-wrap gap-2">
        {groups.map((group, index) => (
          <div
            key={index}
            className="rounded-full border border-border bg-white/5 px-3 py-1.5 text-xs"
          >
            <span className="font-medium text-white/70">{group.group_name}</span>
            {group.message_count > 0 && (
              <span className="ml-1 text-white/40">({group.message_count}x)</span>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

export function PostStreamReportCard({ streamer, sessionId }: PostStreamReportCardProps) {
  const { isFeatureLocked } = usePlan();
  const locked = isFeatureLocked('post_stream_report');
  const { data, isLoading, error } = useStreamReport(locked ? null : streamer, sessionId);

  if (locked) {
    return (
      <div className="rounded-xl border border-border bg-card p-6">
        <div className="mb-4 flex items-center gap-2">
          <Sparkles className="h-5 w-5 text-purple-400" />
          <h3 className="text-sm font-bold uppercase tracking-wide text-text-secondary">
            Letzte Stream-Analyse
          </h3>
        </div>
        <div className="flex flex-col items-center justify-center gap-3 py-8">
          <Lock className="h-8 w-8 text-white/30" />
          <p className="text-center text-sm text-text-secondary">
            Automatische KI-Analyse nach jedem Stream
          </p>
          <p className="text-xs text-white/40">Verfügbar ab Basic-Plan</p>
        </div>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="rounded-xl border border-border bg-card p-6">
        <div className="mb-4 flex items-center gap-2">
          <Sparkles className="h-5 w-5 text-purple-400" />
          <h3 className="text-sm font-bold uppercase tracking-wide text-text-secondary">
            Letzte Stream-Analyse
          </h3>
        </div>
        <div className="flex items-center justify-center gap-3 py-8">
          <Loader2 className="h-5 w-5 animate-spin text-accent" />
          <span className="text-sm text-text-secondary">Lade Report...</span>
        </div>
      </div>
    );
  }

  if (error || !data || data.empty) {
    return (
      <div className="rounded-xl border border-border bg-card p-6">
        <div className="mb-4 flex items-center gap-2">
          <Sparkles className="h-5 w-5 text-purple-400" />
          <h3 className="text-sm font-bold uppercase tracking-wide text-text-secondary">
            Letzte Stream-Analyse
          </h3>
        </div>
        <p className="py-4 text-center text-sm text-text-secondary">
          {!data || data.empty
            ? 'Nach dem nächsten Stream wird hier automatisch eine KI-Analyse erstellt.'
            : 'Kein Report verfügbar.'}
        </p>
      </div>
    );
  }

  if (data.status === 'pending') {
    return (
      <div className="rounded-xl border border-border bg-card p-6">
        <div className="mb-4 flex items-center gap-2">
          <Sparkles className="h-5 w-5 text-purple-400" />
          <h3 className="text-sm font-bold uppercase tracking-wide text-text-secondary">
            Letzte Stream-Analyse
          </h3>
        </div>
        <div className="flex items-center gap-3 py-4">
          <Loader2 className="h-5 w-5 animate-spin text-accent" />
          <span className="text-sm text-text-secondary">Analyse wird erstellt...</span>
        </div>
      </div>
    );
  }

  if (data.status === 'failed') {
    return (
      <div className="rounded-xl border border-border bg-card p-6">
        <div className="mb-4 flex items-center gap-2">
          <AlertCircle className="h-5 w-5 text-danger" />
          <h3 className="text-sm font-bold uppercase tracking-wide text-text-secondary">
            Letzte Stream-Analyse
          </h3>
        </div>
        <p className="text-sm text-danger">
          Analyse fehlgeschlagen: {data.error || 'Unbekannter Fehler'}
        </p>
      </div>
    );
  }

  const report = data.report;
  const wordGroups = data.word_groups || [];
  const modelLabel = data.model === 'opus' ? 'Claude Opus' : 'Minimax';
  const dateLabel = data.generated_at
    ? new Date(data.generated_at).toLocaleString('de-DE', {
        dateStyle: 'short',
        timeStyle: 'short',
      })
    : '';

  return (
    <div className="space-y-6 rounded-xl border border-border bg-card p-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Sparkles className="h-5 w-5 text-purple-400" />
          <h3 className="text-sm font-bold uppercase tracking-wide text-text-secondary">
            Letzte Stream-Analyse
          </h3>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-xs text-white/40">{modelLabel}</span>
          {dateLabel && <span className="text-xs text-white/30">{dateLabel}</span>}
        </div>
      </div>

      {report && (
        <>
          {report.gut.length > 0 && (
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <ThumbsUp className="h-4 w-4 text-green-400" />
                <span className="text-xs font-bold uppercase tracking-wide text-green-400">
                  Was lief gut
                </span>
              </div>
              <div className="space-y-2">
                {report.gut.map((item, index) => (
                  <ExpandablePoint key={index} item={item} color="text-green-300" />
                ))}
              </div>
            </div>
          )}

          {report.schlecht.length > 0 && (
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <ThumbsDown className="h-4 w-4 text-red-400" />
                <span className="text-xs font-bold uppercase tracking-wide text-red-400">
                  Verbesserungspotenzial
                </span>
              </div>
              <div className="space-y-2">
                {report.schlecht.map((item, index) => (
                  <ExpandablePoint key={index} item={item} color="text-red-300" />
                ))}
              </div>
            </div>
          )}

          {report.veraenderungen.length > 0 && (
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <BarChart2 className="h-4 w-4 text-blue-400" />
                <span className="text-xs font-bold uppercase tracking-wide text-blue-400">
                  Erkennbare Veränderungen
                </span>
              </div>
              <ChangeList items={report.veraenderungen} />
            </div>
          )}

          {report.empfehlungen.length > 0 && (
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <TrendingUp className="h-4 w-4 text-purple-400" />
                <span className="text-xs font-bold uppercase tracking-wide text-purple-400">
                  Empfehlungen
                </span>
              </div>
              <RecommendationList items={report.empfehlungen} />
            </div>
          )}
        </>
      )}

      <WordGroups groups={wordGroups} />
    </div>
  );
}
