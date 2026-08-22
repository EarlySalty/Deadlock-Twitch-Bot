import { useState } from "react";
import { motion } from "framer-motion";
import { ArrowUpRight, ChevronDown, Users } from "lucide-react";
import { ProtocolSection } from "@/components/v2/NetworkChrome";
import type { PartnerChannel } from "@/hooks/useNetworkMetrics";

/**
 * Impact eines Partners: 50 % Deadlock-Stream-Häufigkeit, 50 % Schnitt-
 * Zuschauer, beides der letzten 30 Tage und je auf das Maximum im Netzwerk
 * normalisiert, damit die beiden Einheiten vergleichbar werden.
 */
function computeImpact(
  partners: PartnerChannel[],
): (channel: PartnerChannel) => number {
  const maxStreams = Math.max(1, ...partners.map((p) => p.dlStreams30d));
  const maxAvg = Math.max(1, ...partners.map((p) => p.avgViewers30d));
  return (channel) =>
    0.5 * (channel.dlStreams30d / maxStreams) +
    0.5 * (channel.avgViewers30d / maxAvg);
}

/**
 * Lebendige Bausteine der Landing V2. Kern ist das echte Twitch-Embed: der
 * gerade live laufende Partner spielt stummgeschaltet im Hero und in der
 * Partner-Sektion. Dazu klickbare Kanal-Kacheln aus den Live-Daten von
 * useNetworkMetrics (Login, Live-Status, Zuschauerzahl).
 */

const AVATAR_COLORS = [
  "#c8a86b",
  "#55978f",
  "#dd6a4d",
  "#e0912f",
  "#46c07b",
  "#7a9cc6",
];

export function avatarColor(login: string): string {
  let hash = 0;
  for (let i = 0; i < login.length; i++) {
    hash = (hash * 31 + login.charCodeAt(i)) >>> 0;
  }
  return AVATAR_COLORS[hash % AVATAR_COLORS.length];
}

export function initials(login: string): string {
  return login.replace(/[^A-Za-z0-9]/g, "").slice(0, 2).toUpperCase() || "DL";
}

function twitchUrl(login: string): string {
  return `https://twitch.tv/${login}`;
}

/** parent muss exakt der ausliefernde Host sein, sonst blockt Twitch das Embed. */
function twitchParent(): string {
  if (typeof window !== "undefined" && window.location.hostname) {
    return window.location.hostname;
  }
  return "deutsche-deadlock-community.de";
}

/**
 * Runder Avatar: echtes Twitch-Profilbild, wenn vorhanden, sonst Monogramm in
 * der Marken-Farbe des Kanals. Laedt das Bild nicht, faellt es still auf das
 * Monogramm zurueck (kein kaputter Platzhalter).
 */
function Avatar({
  login,
  avatarUrl,
  size = 40,
}: {
  login: string;
  avatarUrl?: string;
  size?: number;
}) {
  return (
    <span
      className="relative flex shrink-0 items-center justify-center overflow-hidden rounded-full font-bold text-black/85"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.4,
        background: avatarColor(login),
      }}
      aria-hidden="true"
    >
      {initials(login)}
      {avatarUrl ? (
        <img
          src={avatarUrl}
          alt=""
          loading="lazy"
          className="absolute inset-0 h-full w-full object-cover"
          onError={(e) => {
            e.currentTarget.style.display = "none";
          }}
        />
      ) : null}
    </span>
  );
}

function LiveBadge() {
  return (
    <span className="flex items-center gap-1.5 rounded bg-[#eb0400] px-2 py-0.5 text-[11px] font-bold uppercase tracking-wider text-white">
      <span className="v2-pulse h-1.5 w-1.5 rounded-full bg-white" />
      Live
    </span>
  );
}

/** Echtes, stummes Twitch-Live-Embed eines Kanals im 16:9-Bildschirm. */
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

/** Infozeile unter einem Embed: klickbarer Kanal, Live-Status, Zuschauer. */
function ChannelBar({ channel }: { channel: PartnerChannel }) {
  return (
    <a
      href={twitchUrl(channel.login)}
      target="_blank"
      rel="noopener noreferrer"
      className="group flex items-center gap-3 px-4 py-3 no-underline"
    >
      <Avatar login={channel.login} avatarUrl={channel.avatarUrl} size={34} />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate font-semibold text-[var(--color-text-primary)] group-hover:text-[var(--color-primary)]">
            {channel.displayName}
          </span>
          {channel.liveDeadlock ? <LiveBadge /> : null}
        </span>
        <span className="flex items-center gap-1 text-xs text-[var(--color-text-secondary)]">
          <Users size={12} />
          {channel.viewers} {channel.liveDeadlock ? "gerade" : "zuletzt"}
        </span>
      </span>
      <ArrowUpRight
        size={16}
        className="shrink-0 text-[var(--color-text-secondary)] transition-colors group-hover:text-[var(--color-primary)]"
      />
    </a>
  );
}

/**
 * Kompakte, klickbare Kanal-Kachel fuers Partner-Grid. Live-Kanaele bekommen
 * einen leuchtenden Rahmen und ein Abzeichen, offline tritt zurueck.
 */
function PartnerTile({ channel, index }: { channel: PartnerChannel; index: number }) {
  return (
    <motion.a
      href={twitchUrl(channel.login)}
      target="_blank"
      rel="noopener noreferrer"
      initial={{ opacity: 0, y: 12 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-40px" }}
      transition={{ duration: 0.35, delay: (index % 8) * 0.04 }}
      className={`group flex items-center gap-2.5 rounded-xl border px-3 py-2.5 no-underline transition-all ${
        channel.liveDeadlock
          ? "border-[rgba(85,151,143,0.55)] bg-[rgba(85,151,143,0.1)] shadow-[0_0_22px_-8px_rgba(85,151,143,0.7)] hover:border-[var(--color-accent)]"
          : "border-[var(--color-border)] bg-black/20 opacity-80 hover:opacity-100 hover:border-[var(--color-border-hover)]"
      }`}
    >
      <span className="relative">
        <Avatar login={channel.login} avatarUrl={channel.avatarUrl} size={34} />
        {channel.liveDeadlock ? (
          <span className="v2-pulse absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full bg-[var(--color-success)] ring-2 ring-[var(--color-card,#17130c)]" />
        ) : null}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-semibold text-[var(--color-text-primary)] group-hover:text-[var(--color-primary)]">
          {channel.displayName}
        </span>
        <span className="flex items-center gap-1 text-[11px] text-[var(--color-text-secondary)]">
          {channel.liveDeadlock ? (
            <>
              <span className="font-semibold text-[var(--color-accent)]">LIVE</span>
              <span>· {channel.viewers}</span>
            </>
          ) : channel.isLive ? (
            "gerade anderes Spiel"
          ) : (
            "offline"
          )}
        </span>
      </span>
    </motion.a>
  );
}

/**
 * Sektion "Entdecke unsere Partner": oben die gerade live laufenden Kanaele
 * als grosse echte Embeds, darunter das vollstaendige, klickbare Grid aller
 * Partner. So sieht man auf einen Blick, dass hier gerade etwas laeuft.
 */
export function PartnersSection({
  partners,
  liveNow,
  total,
  settled,
  categoryKnown,
}: {
  partners: PartnerChannel[];
  liveNow: number | null;
  total: number | null;
  settled: boolean;
  /** Siehe useNetworkMetrics: ohne Kategorie keine Deadlock-Aussage. */
  categoryKnown: boolean;
}) {
  const live = partners.filter((p) => p.liveDeadlock);
  const featured = live.slice(0, 3);

  return (
    <ProtocolSection
      id="partner"
      ambientSide="right"
      stamp="01 · Entdecke unsere Partner"
      headline={
        categoryKnown ? "Hier läuft gerade Deadlock." : "Hier ist gerade jemand live."
      }
      intro={
        categoryKnown
          ? "Kein Renderbild, kein Mockup: Das sind echte Partnerkanäle, die in diesem Moment Deadlock streamen. Klick dich rein, dann siehst du, in welche Szene du reinstreamst."
          : "Kein Renderbild, kein Mockup: Das sind echte Partnerkanäle, die in diesem Moment senden. Klick dich rein, dann siehst du, in welche Szene du reinstreamst."
      }
    >
      {featured.length > 0 ? (
        <div className="mb-6">
          <div className="mb-4 flex items-center gap-2">
            <span className="v2-pulse h-2 w-2 rounded-full bg-[var(--color-success)]" />
            <span className="v2-stamp">
              {categoryKnown ? "Gerade live in Deadlock" : "Gerade live"}
            </span>
          </div>
          <div className="grid gap-5 md:grid-cols-2 lg:grid-cols-3">
            {featured.map((channel) => (
              <motion.div
                key={channel.login}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true, margin: "-60px" }}
                transition={{ duration: 0.5 }}
                className="panel-card overflow-hidden rounded-2xl"
              >
                <TwitchEmbed login={channel.login} />
                <ChannelBar channel={channel} />
              </motion.div>
            ))}
          </div>
        </div>
      ) : (
        <div className="mb-6 rounded-2xl border border-[var(--color-border)] bg-black/25 px-6 py-5 text-sm text-[var(--color-text-secondary)]">
          {settled
            ? "Gerade streamt kein Partner Deadlock. Schau später wieder rein, oder entdecke unten alle Partner."
            : "Live-Kanäle werden geladen …"}
        </div>
      )}

      <PartnerGrid
        partners={partners}
        liveNow={liveNow}
        total={total}
        settled={settled}
        categoryKnown={categoryKnown}
      />
    </ProtocolSection>
  );
}

/**
 * Vollstaendiges Grid aller Partnerkanaele, live zuerst und hervorgehoben,
 * jede Kachel als Link zu Twitch.
 */
/** Zwei Reihen im breitesten Raster (4 Spalten) = 8 Kacheln eingeklappt. */
const COLLAPSED_TILES = 8;

export function PartnerGrid({
  partners,
  liveNow,
  total,
  settled,
  categoryKnown,
}: {
  partners: PartnerChannel[];
  liveNow: number | null;
  total: number | null;
  settled: boolean;
  categoryKnown: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  if (partners.length === 0) {
    return (
      <div className="panel-card rounded-2xl p-8 text-sm text-[var(--color-text-secondary)]">
        {settled
          ? "Die Partnerliste ist gerade nicht abrufbar."
          : "Partner werden geladen …"}
      </div>
    );
  }

  // Reihenfolge: erst die live in Deadlock (grösster Impact zuerst), dann die
  // offline/anderes Spiel (grösster Impact zuerst). Kanäle ohne Impact rutschen
  // ans Ende und landen damit im eingeklappten Rest.
  const impact = computeImpact(partners);
  const sorted = [...partners].sort((a, b) => {
    if (a.liveDeadlock !== b.liveDeadlock) return a.liveDeadlock ? -1 : 1;
    return impact(b) - impact(a);
  });

  const collapsible = sorted.length > COLLAPSED_TILES;
  const shown = expanded ? sorted : sorted.slice(0, COLLAPSED_TILES);
  const hiddenCount = sorted.length - shown.length;

  return (
    <div className="panel-card rounded-2xl p-6 sm:p-7">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <span className="v2-stamp">Alle Partner</span>
        <span className="flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
          <span className="v2-pulse h-2 w-2 rounded-full bg-[var(--color-success)]" />
          {liveNow !== null && total !== null
            ? categoryKnown
              ? `${liveNow} von ${total} gerade in Deadlock live`
              : `${liveNow} von ${total} gerade live`
            : "wird geladen …"}
        </span>
      </div>

      <div className="mt-6 grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-4">
        {shown.map((channel, i) => (
          <PartnerTile key={channel.login} channel={channel} index={i} />
        ))}
      </div>

      {collapsible ? (
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="mt-6 flex w-full items-center justify-center gap-2 rounded-xl border border-[var(--color-border)] py-3 text-sm font-semibold text-[var(--color-text-primary)] transition-all hover:border-[var(--color-border-hover)] hover:bg-white/5"
        >
          {expanded
            ? "Weniger anzeigen"
            : `Alle ${sorted.length} Partner anzeigen`}
          <ChevronDown
            size={16}
            className={`transition-transform ${expanded ? "rotate-180" : ""}`}
          />
          {!expanded && hiddenCount > 0 ? (
            <span className="text-[var(--color-text-secondary)]">
              (+{hiddenCount})
            </span>
          ) : null}
        </button>
      ) : null}
    </div>
  );
}
