import { motion } from 'framer-motion';
import { Pencil, Gamepad2, ArrowDownLeft, ArrowUpRight, Heart, Calendar } from 'lucide-react';
import type { SessionEvent } from '@/types/analytics';
import { NoDataCard } from '@/components/cards/NoDataCard';
import { Loader2 } from 'lucide-react';

interface SessionEventTimelineProps {
  events: SessionEvent | undefined;
  sessionStart: string;
  loading: boolean;
}

interface TimelineEntry {
  at: string;
  relativeMin: number;
  icon: React.ReactNode;
  color: string;
  label: string;
  detail: string;
}

function formatRelativeTime(minutes: number): string {
  if (minutes < 1) return 'Start';
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  if (h > 0) return `+${h}h ${m}m`;
  return `+${m} min`;
}

function buildTimelineEntries(events: SessionEvent, sessionStart: string): TimelineEntry[] {
  const startMs = new Date(sessionStart).getTime();
  const entries: TimelineEntry[] = [];

  // Channel updates — detect what changed
  let prevTitle: string | null = null;
  let prevGame: string | null = null;

  for (const cu of events.channel_updates) {
    const relMin = (new Date(cu.at).getTime() - startMs) / 60000;
    const titleChanged = prevTitle !== null && cu.title !== prevTitle;
    const gameChanged = prevGame !== null && cu.game !== prevGame;

    if (gameChanged) {
      entries.push({
        at: cu.at,
        relativeMin: relMin,
        icon: <Gamepad2 className="w-4 h-4" />,
        color: 'text-accent',
        label: 'Spiel gewechselt',
        detail: cu.game || 'Unbekannt',
      });
    }

    if (titleChanged) {
      entries.push({
        at: cu.at,
        relativeMin: relMin,
        icon: <Pencil className="w-4 h-4" />,
        color: 'text-warning',
        label: 'Titel geandert',
        detail: cu.title || '',
      });
    }

    // If it's the first update and nothing "changed" yet, still show it if there's useful info
    if (prevTitle === null && prevGame === null && entries.length === 0) {
      // Skip the very first record as it's the initial state
    }

    prevTitle = cu.title;
    prevGame = cu.game;
  }

  // Raids
  for (const raid of events.raids) {
    const relMin = (new Date(raid.at).getTime() - startMs) / 60000;
    const isIncoming = raid.direction === 'incoming';
    entries.push({
      at: raid.at,
      relativeMin: relMin,
      icon: isIncoming
        ? <ArrowDownLeft className="w-4 h-4" />
        : <ArrowUpRight className="w-4 h-4" />,
      color: isIncoming ? 'text-success' : 'text-primary',
      label: isIncoming ? 'Eingehender Raid' : 'Ausgehender Raid',
      detail: `${raid.channel} (${raid.viewers} Viewer)`,
    });
  }

  // Follow spikes (only show minutes with >= 2 follows to reduce noise)
  for (const f of events.follows_per_minute) {
    if (f.count < 2) continue;
    const relMin = (new Date(f.minute).getTime() - startMs) / 60000;
    entries.push({
      at: f.minute,
      relativeMin: relMin,
      icon: <Heart className="w-4 h-4" />,
      color: 'text-pink-400',
      label: 'Follow-Spike',
      detail: `${f.count} neue Follower`,
    });
  }

  // Sort by time
  entries.sort((a, b) => a.relativeMin - b.relativeMin);

  return entries;
}

export function SessionEventTimeline({ events, sessionStart, loading }: SessionEventTimelineProps) {
  if (loading) {
    return (
      <div className="bg-card rounded-xl border border-border p-5">
        <h3 className="text-white font-semibold mb-4 flex items-center gap-2">
          <Calendar className="w-5 h-5 text-accent" />
          Session Events
        </h3>
        <div className="flex items-center justify-center h-32">
          <Loader2 className="w-6 h-6 animate-spin text-primary" />
        </div>
      </div>
    );
  }

  if (!events) return null;

  const entries = buildTimelineEntries(events, sessionStart);

  if (entries.length === 0) {
    return (
      <div className="bg-card rounded-xl border border-border p-5">
        <h3 className="text-white font-semibold mb-4 flex items-center gap-2">
          <Calendar className="w-5 h-5 text-accent" />
          Session Events
        </h3>
        <NoDataCard
          message="Keine Events wahrend dieser Session"
          submessage="Titel-/Spielwechsel, Raids und Follow-Spikes werden hier angezeigt."
        />
      </div>
    );
  }

  return (
    <div className="bg-card rounded-xl border border-border p-5">
      <h3 className="text-white font-semibold mb-4 flex items-center gap-2">
        <Calendar className="w-5 h-5 text-accent" />
        Session Events ({entries.length})
      </h3>
      <div className="relative pl-6">
        {/* Vertical line */}
        <div className="absolute left-[11px] top-2 bottom-2 w-px bg-border" />

        <div className="space-y-4">
          {entries.map((entry, i) => (
            <motion.div
              key={`${entry.at}-${i}`}
              initial={{ opacity: 0, x: -10 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ delay: i * 0.05 }}
              className="relative flex items-start gap-3"
            >
              {/* Dot on the line */}
              <div className={`absolute -left-6 mt-1 w-[22px] h-[22px] rounded-full bg-bg border-2 border-border flex items-center justify-center ${entry.color}`}>
                {entry.icon}
              </div>

              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="text-xs text-text-secondary font-mono">
                    {formatRelativeTime(entry.relativeMin)}
                  </span>
                  <span className={`text-sm font-medium ${entry.color}`}>
                    {entry.label}
                  </span>
                </div>
                <p className="text-sm text-white/80 mt-0.5 truncate">
                  {entry.detail}
                </p>
              </div>
            </motion.div>
          ))}
        </div>
      </div>
    </div>
  );
}

export default SessionEventTimeline;
