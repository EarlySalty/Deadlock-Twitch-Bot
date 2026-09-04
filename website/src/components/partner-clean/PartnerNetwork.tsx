import { motion, useReducedMotion } from "framer-motion";
import { ArrowUpRight, Users } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { Avatar, LiveBadge } from "@/components/partner-clean/partnerShared";
import {
  type NetworkStatus,
  type NetworkStreamer,
} from "@/hooks/useNetworkStreamers";
import { previewImageUrl, twitchParent, twitchUrl } from "@/lib/partnerNetwork";

const EMPTY_STATE_TEXT =
  "Die Partnerliste lädt gerade nicht. Schau auf Twitch oder im Discord vorbei.";

function TwitchEmbed({ login }: { login: string }) {
  const src =
    `https://player.twitch.tv/?channel=${encodeURIComponent(login)}` +
    `&parent=${twitchParent()}&muted=true&autoplay=true`;
  return (
    <div className="relative aspect-video w-full overflow-hidden bg-black">
      <iframe
        title={`Live-Stream von ${login}`}
        src={src}
        className="absolute inset-0 h-full w-full"
        allow="autoplay; fullscreen"
        allowFullScreen
        loading="lazy"
      />
    </div>
  );
}

function LivePreview({ partner }: { partner: NetworkStreamer }) {
  return (
    <div className="relative aspect-video w-full overflow-hidden bg-black">
      <span className="absolute inset-0 flex items-center justify-center">
        <Avatar login={partner.login} avatarUrl={partner.avatarUrl} size={64} />
      </span>
      <img
        src={previewImageUrl(partner.login)}
        alt={`Vorschau von ${partner.login}`}
        loading="lazy"
        className="absolute inset-0 h-full w-full object-cover"
        onError={(e) => {
          e.currentTarget.style.display = "none";
        }}
      />
      <span className="absolute top-3 left-3">
        <LiveBadge />
      </span>
    </div>
  );
}

function ChannelBar({ partner }: { partner: NetworkStreamer }) {
  return (
    <a
      href={twitchUrl(partner.login)}
      target="_blank"
      rel="noopener noreferrer"
      className="group flex items-center gap-3 px-4 py-3 no-underline"
    >
      <Avatar login={partner.login} avatarUrl={partner.avatarUrl} size={38} />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate font-semibold text-[var(--color-text-primary)] group-hover:text-[var(--color-primary)]">
            {partner.displayName ?? partner.login}
          </span>
          {partner.isLive ? <LiveBadge /> : null}
        </span>
        <span className="flex items-center gap-1 text-xs text-[var(--color-text-secondary)]">
          <Users size={12} />
          {partner.viewers} {partner.isLive ? "gerade" : "zuletzt"}
          {partner.game ? <span>· {partner.game}</span> : null}
        </span>
      </span>
      <ArrowUpRight
        size={16}
        className="shrink-0 text-[var(--color-text-secondary)] transition-colors group-hover:text-[var(--color-primary)]"
      />
    </a>
  );
}

function LiveCard({
  partner,
  asPreview,
  index,
}: {
  partner: NetworkStreamer;
  asPreview: boolean;
  index: number;
}) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 14 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-40px" }}
      transition={{ duration: 0.4, delay: (index % 3) * 0.08 }}
      className="rounded-2xl overflow-hidden border border-[rgba(85,151,143,0.55)] bg-[var(--color-card)] shadow-[0_0_40px_-16px_rgba(201,168,106,0.85)]"
    >
      {asPreview ? <LivePreview partner={partner} /> : <TwitchEmbed login={partner.login} />}
      <ChannelBar partner={partner} />
    </motion.div>
  );
}

function OfflineTile({ partner, index }: { partner: NetworkStreamer; index: number }) {
  return (
    <motion.a
      href={twitchUrl(partner.login)}
      target="_blank"
      rel="noopener noreferrer"
      initial={{ opacity: 0, y: 12 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-40px" }}
      transition={{ duration: 0.35, delay: (index % 8) * 0.04 }}
      className="group flex items-center gap-2.5 rounded-xl border border-[var(--color-border)] bg-black/20 px-3 py-2.5 no-underline opacity-80 transition-all hover:opacity-100 hover:border-[var(--color-border-hover)]"
    >
      <Avatar login={partner.login} avatarUrl={partner.avatarUrl} size={38} />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-semibold text-[var(--color-text-primary)] group-hover:text-[var(--color-primary)]">
          {partner.displayName ?? partner.login}
        </span>
        {partner.dlStreams30d > 0 ? (
          <span className="block text-[11px] text-[var(--color-text-secondary)]">
            {partner.dlStreams30d} Deadlock-Streams in 30 Tagen
          </span>
        ) : null}
      </span>
      <ArrowUpRight
        size={15}
        className="shrink-0 text-[var(--color-text-secondary)] transition-colors group-hover:text-[var(--color-primary)]"
      />
    </motion.a>
  );
}

function EmptyState() {
  return (
    <div className="panel-card rounded-2xl p-10 text-center max-w-xl mx-auto">
      <p className="text-[var(--color-text-secondary)] leading-relaxed">
        {EMPTY_STATE_TEXT}
      </p>
    </div>
  );
}

function Skeletons() {
  return (
    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
      {[0, 1, 2].map((i) => (
        <div
          key={i}
          className="rounded-2xl overflow-hidden border border-[var(--color-border)] bg-[var(--color-card)]"
        >
          <div className="aspect-video w-full bg-black/40 animate-pulse" />
          <div className="px-4 py-3 flex items-center gap-3">
            <div className="w-9 h-9 rounded-full bg-white/5 animate-pulse" />
            <div className="h-3 w-28 rounded bg-white/5 animate-pulse" />
          </div>
        </div>
      ))}
    </div>
  );
}

export function PartnerNetwork({
  streamers,
  status,
}: {
  streamers: NetworkStreamer[];
  status: NetworkStatus;
}) {
  const reduce = useReducedMotion();

  const live = streamers.filter((s) => s.isLive);
  const offline = streamers.filter((s) => !s.isLive);
  const embedded = live.slice(0, 3);
  const previewed = live.slice(3);

  return (
    <section id="partner" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <ScrollReveal className="text-center">
          <p className="text-sm uppercase tracking-wider font-medium text-[var(--color-primary)] mb-3">
            Unsere Partner
          </p>
          <h2 className="text-4xl md:text-5xl font-bold text-[var(--color-text-primary)] font-display">
            Wer schon dabei ist
          </h2>
          {status === "ready" && streamers.length > 0 ? (
            <p className="mt-4 text-lg text-[var(--color-text-secondary)]">
              <span className="font-semibold text-[var(--color-accent)]">
                {streamers.length} Partner
              </span>{" "}
              streamen Deadlock in diesem Netzwerk. In diese Runde steigst du ein.
            </p>
          ) : (
            <p className="mt-4 text-lg text-[var(--color-text-secondary)]">
              Das sind die Kanäle, in deren Runde du als Partner einsteigst.
            </p>
          )}
        </ScrollReveal>

        <div className="mt-14">
          {status === "loading" ? (
            <Skeletons />
          ) : status === "error" || streamers.length === 0 ? (
            <EmptyState />
          ) : (
            <>
              {live.length > 0 ? (
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                  {embedded.map((p, i) => (
                    <LiveCard key={p.login} partner={p} asPreview={Boolean(reduce)} index={i} />
                  ))}
                  {previewed.map((p, i) => (
                    <LiveCard key={p.login} partner={p} asPreview index={i + 3} />
                  ))}
                </div>
              ) : null}

              {offline.length > 0 ? (
                <div className={live.length > 0 ? "mt-10" : ""}>
                  {live.length > 0 ? (
                    <p className="text-sm uppercase tracking-wider font-medium text-[var(--color-text-secondary)] mb-5">
                      Gerade offline
                    </p>
                  ) : null}
                  <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-3">
                    {offline.map((p, i) => (
                      <OfflineTile key={p.login} partner={p} index={i} />
                    ))}
                  </div>
                </div>
              ) : null}
            </>
          )}
        </div>
      </div>
    </section>
  );
}
