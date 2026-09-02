import {
  buildTwitchBotAuthUrl,
  DISCORD_INVITE_URL,
  TWITCH_SECURITY_URL,
} from "@/data/externalLinks";
import {
  NETWORK_CLIPS,
  PARTNER_COPY,
  twitchUrl,
} from "@/data/partnerPage";
import {
  useNetworkMetrics,
  type PartnerChannel,
} from "@/hooks/useNetworkMetrics";
import { PartnerFooter } from "@/components/partner/PartnerFooter";
import { PartnerNav } from "@/components/partner/PartnerNav";
import {
  Avatar,
  clipPartners,
  livePreviewUrl,
  StreamCard,
} from "@/components/partner/StreamCard";
import "./partner.css";

function Ctas() {
  return (
    <div className="pn-hero-actions">
      <a className="pn-btn pn-btn-primary" href={buildTwitchBotAuthUrl()}>
        {PARTNER_COPY.ctaPrimary}
      </a>
      <a
        className="pn-btn pn-btn-ghost"
        href={DISCORD_INVITE_URL}
        target="_blank"
        rel="noopener noreferrer"
      >
        {PARTNER_COPY.ctaSecondary}
      </a>
    </div>
  );
}

function proofLine(partners: number | null, liveNow: number | null, settled: boolean) {
  if (!settled || partners === null) return null;
  const live = liveNow ?? 0;
  return (
    <p className="pn-proof">
      <strong>{partners.toLocaleString("de-DE")}</strong> {PARTNER_COPY.proofPartner}
      {" · "}
      <strong>{live.toLocaleString("de-DE")}</strong> {PARTNER_COPY.proofLive}
    </p>
  );
}

function heroStage(live: PartnerChannel[], clips: PartnerChannel[]) {
  const featured = live[0] ?? clips[0];
  if (!featured) return null;
  return (
    <div>
      <StreamCard
        login={featured.login}
        displayName={featured.displayName}
        live={featured.liveDeadlock}
        viewers={featured.liveDeadlock ? featured.viewers : undefined}
        avatarUrl={featured.avatarUrl}
      />
      <p className="pn-stage-caption">
        {featured.liveDeadlock ? PARTNER_COPY.stageLive : PARTNER_COPY.stageRaid}
      </p>
    </div>
  );
}

function PartnerTile({ channel }: { channel: PartnerChannel }) {
  return (
    <a
      className={`pn-tile${channel.liveDeadlock ? " is-live" : ""}`}
      href={twitchUrl(channel.login)}
      target="_blank"
      rel="noopener noreferrer"
    >
      {channel.liveDeadlock ? (
        <span className="pn-tile-shot">
          <img
            src={livePreviewUrl(channel.login)}
            alt=""
            onError={(event) => {
              event.currentTarget.style.display = "none";
            }}
          />
        </span>
      ) : null}
      <Avatar login={channel.login} avatarUrl={channel.avatarUrl} size={72} />
      <span className="pn-tile-meta">
        <strong>{channel.displayName}</strong>
        <small>
          {channel.liveDeadlock
            ? `Live · ${channel.viewers.toLocaleString("de-DE")}`
            : channel.isLive
              ? "anderes Spiel"
              : "offline"}
        </small>
      </span>
    </a>
  );
}

export function PartnerPage() {
  const metrics = useNetworkMetrics();
  const clips = clipPartners();
  const live = metrics.partnerList.filter((partner) => partner.liveDeadlock);
  const roster = metrics.partnerList.length > 0 ? metrics.partnerList : clips;
  const featuredLive = live.slice(0, 3);
  const featuredClips = clips
    .filter((clip) => !featuredLive.some((livePartner) => livePartner.login === clip.login))
    .slice(0, Math.max(0, 3 - featuredLive.length));
  const featured = [...featuredLive, ...featuredClips];
  const alone = NETWORK_CLIPS[0];
  const next = NETWORK_CLIPS[1];

  return (
    <>
      <PartnerNav />
      <main>
        <section id="hero" className="pn-hero">
          <div className="pn-wrap pn-hero-grid">
            <div>
              <p className="pn-badge">
                <i />
                {PARTNER_COPY.badge}
              </p>
              <h1>
                {PARTNER_COPY.headline.split("Deadlock-Community").map((part, index) =>
                  index === 0 ? (
                    part
                  ) : (
                    <span key="brand" className="pn-nowrap">
                      Deadlock-Community{part}
                    </span>
                  ),
                )}
              </h1>
              <p className="pn-hero-sub">{PARTNER_COPY.subline}</p>
              <Ctas />
              {proofLine(metrics.partners, metrics.liveNow, metrics.settled)}
            </div>
            {heroStage(live, clips)}
          </div>
        </section>

        <section id="problem" className="pn-section">
          <div className="pn-wrap">
            <h2>{PARTNER_COPY.problemHeadline}</h2>
            <div className="pn-split">
              <div className="pn-state is-void">
                <StreamCard
                  login={alone.login}
                  displayName={alone.displayName}
                  ended
                  dim
                />
                <h3>{PARTNER_COPY.aloneTitle}</h3>
                <p>{PARTNER_COPY.aloneBody}</p>
              </div>
              <div className="pn-state is-net">
                <StreamCard
                  login={next.login}
                  displayName={next.displayName}
                />
                <h3>{PARTNER_COPY.networkTitle}</h3>
                <p>{PARTNER_COPY.networkBody}</p>
              </div>
            </div>
          </div>
        </section>

        <section id="bedeutung" className="pn-section">
          <div className="pn-wrap">
            <h2>{PARTNER_COPY.meaningHeadline}</h2>
            <div className="pn-meaning">
              {PARTNER_COPY.meaning.map((line) => (
                <p key={line}>{line}</p>
              ))}
            </div>
          </div>
        </section>

        <section id="partner" className="pn-section">
          <div className="pn-wrap">
            <h2>{PARTNER_COPY.rosterHeadline}</h2>
            {featured.length > 0 ? (
              <div
                className={`pn-live-row${featured.length >= 3 ? " is-three" : ""}`}
              >
                {featured.map((channel) => (
                  <StreamCard
                    key={channel.login}
                    login={channel.login}
                    displayName={channel.displayName}
                    live={channel.liveDeadlock}
                    viewers={channel.liveDeadlock ? channel.viewers : undefined}
                    avatarUrl={channel.avatarUrl}
                  />
                ))}
              </div>
            ) : null}
            <p className="pn-stage-caption" style={{ marginTop: 0, marginBottom: "1rem" }}>
              {PARTNER_COPY.stageRaid}
            </p>
            <div className="pn-grid">
              {roster.map((channel) => (
                <PartnerTile key={channel.login} channel={channel} />
              ))}
            </div>
          </div>
        </section>

        <section id="sicherheit" className="pn-section">
          <div className="pn-wrap pn-safety">
            <h2>{PARTNER_COPY.safetyHeadline}</h2>
            <p>{PARTNER_COPY.safetyBody}</p>
            <a href={TWITCH_SECURITY_URL}>{PARTNER_COPY.safetyLink}</a>
          </div>
        </section>

        <section id="abschluss" className="pn-close">
          <div className="pn-wrap">
            <h2>{PARTNER_COPY.closeHeadline}</h2>
            <Ctas />
            <p className="pn-close-note">{PARTNER_COPY.closeNote}</p>
          </div>
        </section>
      </main>
      <PartnerFooter />
    </>
  );
}
