import { AnimatePresence } from "framer-motion";
import { Ban, LineChart, Shield } from "lucide-react";
import { ProtocolSection } from "@/components/v2/NetworkChrome";
import { BanFeedEntry } from "@/components/ui/BanFeedEntry";
import { useBanFeed } from "@/hooks/useBanFeed";
import { SPAM_COPY } from "@/data/networkPage";

function StatTile({
  icon,
  value,
  label,
}: {
  icon: React.ReactNode;
  value: number;
  label: string;
}) {
  return (
    <div className="panel-card rounded-2xl p-6">
      <span className="icon-tile inline-flex h-9 w-9 items-center justify-center rounded-lg">
        {icon}
      </span>
      <p className="mt-5 text-4xl font-extrabold leading-none text-[var(--color-text-primary)]">
        {value.toLocaleString("de-DE")}
      </p>
      <p className="mt-2 text-sm text-[var(--color-text-secondary)]">{label}</p>
    </div>
  );
}

export function PartnerBanFeedSection() {
  const { bans, stats } = useBanFeed();

  return (
    <ProtocolSection
      id="spamschutz"
      ambient="teal"
      ambientSide="right"
      stamp={SPAM_COPY.stamp}
      headline={SPAM_COPY.headline}
      intro={SPAM_COPY.intro}
    >
      <div className="grid gap-5 sm:grid-cols-3">
        <StatTile
          icon={<Shield size={17} />}
          value={stats.today}
          label={SPAM_COPY.statToday}
        />
        <StatTile
          icon={<Ban size={17} />}
          value={stats.total_30d}
          label={SPAM_COPY.stat30d}
        />
        <StatTile
          icon={<LineChart size={17} />}
          value={stats.channels_protected}
          label={SPAM_COPY.statChannels}
        />
      </div>

      <div className="panel-card mt-6 overflow-hidden rounded-2xl">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] px-6 py-4">
          <span className="v2-stamp">{SPAM_COPY.feedTitle}</span>
          <span className="flex items-center gap-2 text-xs text-[var(--color-text-secondary)]">
            <span className="v2-pulse h-2 w-2 rounded-full bg-[var(--color-success)]" />
            live
          </span>
        </div>

        {bans.length > 0 ? (
          <div className="relative max-h-[420px] space-y-1 overflow-hidden p-3">
            <AnimatePresence initial={false}>
              {bans.map((ban) => (
                <BanFeedEntry
                  key={`${ban.target_login}-${ban.received_at}`}
                  ban={ban}
                />
              ))}
            </AnimatePresence>
            <div className="pointer-events-none absolute inset-x-0 bottom-0 h-14 bg-gradient-to-t from-[var(--color-card)] to-transparent" />
          </div>
        ) : (
          <p className="px-6 py-8 text-sm text-[var(--color-text-secondary)]">
            {SPAM_COPY.empty}
          </p>
        )}
      </div>
    </ProtocolSection>
  );
}
