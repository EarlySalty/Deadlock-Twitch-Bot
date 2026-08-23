import { useState } from "react";
import { ArrowUpRight, Ban, LineChart, Users } from "lucide-react";
import { ProtocolSection } from "@/components/v2/NetworkChrome";
import { DISCORD_INVITE_URL } from "@/data/externalLinks";
import type { NetworkMetrics } from "@/hooks/useNetworkMetrics";

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const minutes = Math.max(0, Math.round(diff / 60_000));
  if (minutes < 1) return "gerade eben";
  if (minutes < 60) return `vor ${minutes} min`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `vor ${hours} h`;
  return `vor ${Math.round(hours / 24)} d`;
}

function MetricTile({
  icon,
  value,
  label,
  hint,
  settled,
}: {
  icon: React.ReactNode;
  value: number | null;
  label: string;
  hint: string;
  settled: boolean;
}) {
  return (
    <div className="panel-card rounded-2xl p-6">
      <span className="icon-tile inline-flex h-9 w-9 items-center justify-center rounded-lg">
        {icon}
      </span>
      <p className="mt-5 text-4xl font-extrabold leading-none text-[var(--color-text-primary)]">
        {value !== null ? (
          value.toLocaleString("de-DE")
        ) : (
          <span className="text-2xl text-[var(--color-text-secondary)]">
            {settled ? "gerade nicht abrufbar" : "…"}
          </span>
        )}
      </p>
      <p className="mt-2 font-semibold text-[var(--color-text-primary)]">{label}</p>
      <p className="mt-1 text-sm text-[var(--color-text-secondary)]">{hint}</p>
    </div>
  );
}

/**
 * Open Metrics: die Zahlen, die das Versprechen belegen, plus der laufende
 * Ban-Feed. Faellt die API aus, steht das offen da statt einer Beispielzahl.
 */
export function OpenMetricsSection({ metrics }: { metrics: NetworkMetrics }) {
  const marqueeNames =
    metrics.partnerNames.length > 0
      ? [...metrics.partnerNames, ...metrics.partnerNames]
      : [];

  return (
    <ProtocolSection
      id="zahlen"
      ambient="teal"
      ambientSide="left"
      stamp="07 · Offene Zahlen"
      headline="Wir behaupten nichts, was wir nicht zeigen können."
      intro="Diese Werte kommen direkt aus dem laufenden Betrieb und ändern sich, während du hier liest. Wenn eine Zahl schlecht aussieht, steht sie trotzdem da."
    >
      <div className="grid gap-5 sm:grid-cols-3">
        <MetricTile
          icon={<Users size={17} />}
          value={metrics.partners}
          label="Streamer im Netzwerk"
          hint="verbundene Partnerkanäle"
          settled={metrics.settled}
        />
        <MetricTile
          icon={<Ban size={17} />}
          value={metrics.banStats?.total_30d ?? null}
          label="Spam-Accounts entfernt"
          hint="in den letzten 30 Tagen"
          settled={metrics.settled}
        />
        <MetricTile
          icon={<LineChart size={17} />}
          value={metrics.banStats?.channels_protected ?? null}
          label="geschützte Kanäle"
          hint="Chats mit aktivem Schutz"
          settled={metrics.settled}
        />
      </div>

      {/* Laufender Ban-Feed */}
      <div className="panel-card mt-6 overflow-hidden rounded-2xl">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] px-6 py-4">
          <span className="v2-stamp">Live aus den Partner-Chats</span>
          <span className="flex items-center gap-2 text-xs text-[var(--color-text-secondary)]">
            <span className="v2-pulse h-2 w-2 rounded-full bg-[var(--color-success)]" />
            aktualisiert alle 45 Sekunden
          </span>
        </div>

        {metrics.bans.length > 0 ? (
          <ul className="divide-y divide-[rgba(201,168,106,0.1)]">
            {metrics.bans.map((ban, i) => (
              <li
                key={`${ban.target_login}-${i}`}
                className="flex flex-wrap items-center gap-x-4 gap-y-1 px-6 py-3.5 text-sm"
              >
                <span className="font-mono text-[var(--color-danger)]">
                  {ban.target_login}
                </span>
                <span className="min-w-0 flex-1 truncate text-[var(--color-text-secondary)]">
                  {ban.reason}
                </span>
                <span className="v2-stamp v2-stamp-dim">
                  {relativeTime(ban.received_at)}
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="px-6 py-8 text-sm text-[var(--color-text-secondary)]">
            {metrics.settled
              ? "Der Feed ist gerade nicht abrufbar."
              : "Feed wird geladen."}
          </p>
        )}
      </div>

      {/* Partner-Laufband */}
      {marqueeNames.length > 0 ? (
        <div className="v2-marquee-mask mt-6 overflow-hidden">
          <div className="v2-marquee gap-3">
            {marqueeNames.map((name, i) => (
              <span
                key={`${name}-${i}`}
                className="whitespace-nowrap rounded-full border border-[var(--color-border)] bg-[var(--color-card)] px-4 py-1.5 text-sm text-[var(--color-text-secondary)]"
              >
                {name}
              </span>
            ))}
          </div>
        </div>
      ) : null}
    </ProtocolSection>
  );
}

/**
 * Lead-Magnet nach docs/strategie/31 §3.2. Der Report wird zurzeit von Hand
 * erstellt, deshalb fuehrt das Formular in den Discord statt in eine
 * Automatik, die es noch nicht gibt.
 */
export function ChannelReportSection() {
  const [channel, setChannel] = useState("");
  const trimmed = channel.trim().replace(/^@/, "");
  const target = trimmed
    ? `${DISCORD_INVITE_URL}?utm_source=streamer_v2&utm_medium=report`
    : DISCORD_INVITE_URL;

  return (
    <ProtocolSection
      id="report"
      ambientSide="right"
      stamp="08 · Vor der Entscheidung"
      headline="Erst der Blick auf deinen Kanal, dann der Rest."
      intro="Du musst nichts verbinden, um etwas zu bekommen. Sag uns deinen Kanal und du bekommst eine Einschätzung: wann deine Zuschauer abspringen, wie deine Streamzeiten zu den Deadlock-Spitzen liegen und welche Stellen sich als Clip lohnen."
    >
      <div className="grid gap-6 lg:grid-cols-[1.1fr_0.9fr]">
        <div className="panel-card rounded-2xl p-8">
          <label
            htmlFor="channel-report"
            className="block text-sm font-semibold text-[var(--color-text-primary)]"
          >
            Dein Twitch-Kanal
          </label>
          <div className="mt-3 flex flex-wrap gap-3">
            <input
              id="channel-report"
              type="text"
              value={channel}
              onChange={(e) => setChannel(e.target.value)}
              placeholder="z. B. deinkanalname"
              autoComplete="off"
              className="min-w-0 flex-1 rounded-xl border border-[var(--color-border)] bg-black/30 px-4 py-3 text-[var(--color-text-primary)] outline-none transition-colors placeholder:text-[rgba(183,170,145,0.45)] focus:border-[var(--color-border-strong)]"
            />
            <a
              href={target}
              target="_blank"
              rel="noopener noreferrer"
              className="gradient-accent inline-flex items-center gap-2 rounded-xl px-6 py-3 font-semibold no-underline transition-all hover:brightness-110"
            >
              Report anfordern
              <ArrowUpRight size={17} />
            </a>
          </div>
          <p className="mt-4 text-sm leading-relaxed text-[var(--color-text-secondary)]">
            Der Report wird zurzeit von Hand erstellt, deshalb läuft die Anfrage
            über den Discord. Du bekommst ihn dort als Nachricht, ohne dass du
            vorher etwas verbindest.
          </p>
        </div>

        <div className="rounded-2xl border border-[rgba(255,255,255,0.07)] bg-black/25 p-8">
          <span className="v2-stamp">Was drinsteht</span>
          <ul className="mt-5 space-y-3 text-sm text-[var(--color-text-secondary)]">
            <li>Deine Streamzeiten gegen die Zeiten, zu denen Deadlock läuft</li>
            <li>Raid-Bilanz: wie viele gehen raus, wie viele kommen rein</li>
            <li>Die Punkte im Stream, an denen Zuschauer aussteigen</li>
            <li>Momente aus den letzten Streams, die sich als Clip lohnen</li>
          </ul>
        </div>
      </div>
    </ProtocolSection>
  );
}
