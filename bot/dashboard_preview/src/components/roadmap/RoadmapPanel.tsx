import { motion } from 'framer-motion';
import { CheckCircle2, Circle, Loader2, Map, Zap } from 'lucide-react';
import { useRoadmap } from '@/hooks/useAnalytics';
import type { RoadmapItem } from '@/api';

const STATUS_CONFIG = {
  planned: {
    label: 'Geplant',
    icon: Circle,
    badgeClass: 'border-border/70 bg-background/60 text-text-secondary',
    dotClass: 'bg-text-secondary/50',
  },
  in_progress: {
    label: 'In Arbeit',
    icon: Zap,
    badgeClass: 'border-primary/40 bg-primary/10 text-primary',
    dotClass: 'bg-primary',
  },
  done: {
    label: 'Fertig',
    icon: CheckCircle2,
    badgeClass: 'border-success/40 bg-success/10 text-success',
    dotClass: 'bg-success',
  },
} as const;

function RoadmapCard({ item }: { item: RoadmapItem }) {
  const cfg = STATUS_CONFIG[item.status] ?? STATUS_CONFIG.planned;
  return (
    <div className="rounded-lg border border-border bg-background/50 p-3 transition-colors hover:border-border-hover">
      <div className="flex items-start gap-2">
        <span className={`mt-1 h-2 w-2 shrink-0 rounded-full ${cfg.dotClass}`} />
        <div className="min-w-0 flex-1">
          <p className="text-sm font-semibold text-white leading-snug">{item.title}</p>
          {item.description && (
            <p className="mt-0.5 text-xs text-text-secondary leading-5">{item.description}</p>
          )}
        </div>
      </div>
    </div>
  );
}

function StatusSection({
  status,
  items,
}: {
  status: keyof typeof STATUS_CONFIG;
  items: RoadmapItem[];
}) {
  const cfg = STATUS_CONFIG[status];
  if (items.length === 0) return null;
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <span className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11px] font-semibold ${cfg.badgeClass}`}>
          {cfg.label}
        </span>
        <span className="text-[11px] text-text-secondary">{items.length}</span>
      </div>
      <div className="space-y-1.5">
        {items.map((item) => (
          <RoadmapCard key={item.id} item={item} />
        ))}
      </div>
    </div>
  );
}

export function RoadmapPanel() {
  const { data, isLoading, isError } = useRoadmap();

  const planned = data?.planned ?? [];
  const inProgress = data?.in_progress ?? [];
  const done = data?.done ?? [];
  const total = planned.length + inProgress.length + done.length;

  return (
    <motion.aside
      className="panel-card rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32, delay: 0.16 }}
    >
      <div className="mb-5 flex items-start justify-between gap-2">
        <div>
          <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Features</p>
          <h2 className="display-font text-2xl font-bold text-white mb-1">Roadmap</h2>
          <p className="text-sm text-text-secondary">Geplante &amp; aktive Entwicklungen.</p>
        </div>
        <div className="flex items-center gap-1.5 rounded-full border border-border bg-background/70 px-3 py-1">
          <Map className="h-3.5 w-3.5 text-primary" />
          <span className="text-[11px] font-semibold text-text-secondary">{total}</span>
        </div>
      </div>

      {isLoading && (
        <div className="flex items-center gap-2 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          <span>Lädt...</span>
        </div>
      )}

      {isError && (
        <div className="rounded-xl border border-error/30 bg-error/10 p-3 text-sm text-error">
          Roadmap konnte nicht geladen werden.
        </div>
      )}

      {!isLoading && !isError && total === 0 && (
        <div className="rounded-xl border border-border bg-background/60 p-4 text-sm text-text-secondary">
          Noch keine Roadmap-Einträge vorhanden.
        </div>
      )}

      {!isLoading && !isError && total > 0 && (
        <div className="space-y-5">
          <StatusSection status="in_progress" items={inProgress} />
          <StatusSection status="planned" items={planned} />
          <StatusSection status="done" items={done} />
        </div>
      )}
    </motion.aside>
  );
}
