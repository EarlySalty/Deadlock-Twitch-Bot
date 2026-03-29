import { motion } from "framer-motion";
import type { BanEntry } from "@/hooks/useBanFeed";

interface BanFeedEntryProps {
  ban: BanEntry;
  isNew?: boolean;
}

const rtf = new Intl.RelativeTimeFormat("de", { numeric: "auto" });

function relativeTime(isoString: string): string {
  const diffMs = Date.now() - new Date(isoString).getTime();
  const diffMin = Math.round(diffMs / 60_000);

  if (diffMin < 1) return "gerade eben";
  if (diffMin < 60) return rtf.format(-diffMin, "minute");

  const diffHours = Math.round(diffMin / 60);
  if (diffHours < 24) return rtf.format(-diffHours, "hour");

  const diffDays = Math.round(diffHours / 24);
  return rtf.format(-diffDays, "day");
}

export function BanFeedEntry({ ban, isNew = false }: BanFeedEntryProps) {
  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: -12 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 12 }}
      transition={{ duration: 0.35, ease: "easeOut" }}
      className="relative flex items-center gap-3 rounded-lg px-3 py-2.5"
    >
      {/* New-entry glow */}
      {isNew && (
        <motion.div
          className="absolute inset-0 rounded-lg"
          style={{
            boxShadow: "inset 0 0 12px rgba(239,68,68,0.25)",
            border: "1px solid rgba(239,68,68,0.3)",
          }}
          initial={{ opacity: 1 }}
          animate={{ opacity: 0 }}
          transition={{ duration: 1, ease: "easeOut" }}
        />
      )}

      {/* BANNED badge */}
      <span className="shrink-0 rounded bg-red-500/20 px-2 py-0.5 text-xs font-semibold uppercase tracking-wider text-red-400">
        Banned
      </span>

      {/* Username */}
      <span className="shrink-0 text-sm font-semibold text-[var(--color-text-primary)]">
        {ban.target_login}
      </span>

      {/* Spam message */}
      <span className="min-w-0 max-w-[200px] truncate text-xs leading-relaxed text-[var(--color-text-secondary)] line-through opacity-60 md:max-w-[300px]">
        {ban.reason}
      </span>

      {/* Relative time */}
      <span className="ml-auto shrink-0 text-xs text-[var(--color-text-secondary)]">
        {relativeTime(ban.received_at)}
      </span>
    </motion.div>
  );
}
