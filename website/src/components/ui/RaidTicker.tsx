import { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import type { RaidEvent } from "@/hooks/useRaidEvents";

interface RaidTickerProps {
  raids: RaidEvent[];
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

function RaidEventItem({ raid }: { raid: RaidEvent }) {
  return (
    <span className="inline-flex items-center gap-1 text-sm whitespace-nowrap">
      <span className="text-[var(--color-primary)] font-semibold">
        {raid.from_channel}
      </span>
      <span className="text-[var(--color-text-secondary)]">&rarr;</span>
      <span className="text-[var(--color-accent)] font-semibold">
        {raid.to_channel}
      </span>
      <span className="text-[var(--color-text-secondary)]">
        &middot; {raid.viewers} Viewer &middot; {relativeTime(raid.executed_at)}
      </span>
    </span>
  );
}

export function RaidTicker({ raids }: RaidTickerProps) {
  const [currentIndex, setCurrentIndex] = useState(0);

  useEffect(() => {
    if (raids.length === 0) return;
    const id = setInterval(() => {
      setCurrentIndex((prev) => (prev + 1) % raids.length);
    }, 8000);
    return () => clearInterval(id);
  }, [raids.length]);

  if (raids.length === 0) return null;

  // Desktop: show up to 3 events, mobile: show 1 rotating
  const desktopSlice = raids.slice(0, 3);
  const mobileRaid = raids[currentIndex % raids.length];

  return (
    <div className="glass rounded-xl px-6 py-3">
      {/* Mobile: single rotating event */}
      <div className="flex md:hidden items-center justify-center min-h-[28px]">
        <AnimatePresence mode="wait">
          <motion.div
            key={`mobile-${currentIndex}`}
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.3 }}
          >
            <RaidEventItem raid={mobileRaid} />
          </motion.div>
        </AnimatePresence>
      </div>

      {/* Desktop: 3 events visible */}
      <div className="hidden md:flex items-center justify-center gap-8 min-h-[28px]">
        {desktopSlice.map((raid, i) => (
          <RaidEventItem key={`${raid.from_channel}-${raid.to_channel}-${i}`} raid={raid} />
        ))}
      </div>
    </div>
  );
}
