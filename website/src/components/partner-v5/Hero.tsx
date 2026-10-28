import { buildTwitchBotAuthUrl } from "@/data/externalLinks";

export function Hero() {
  return (
    <section className="world-hero world-poster v5-hero" aria-labelledby="v5-titel">
      <picture className="world-picture">
        <source type="image/avif" srcSet="/brand/world/club-960.avif 960w, /brand/world/club-1920.avif 1920w" sizes="100vw" />
        <source type="image/webp" srcSet="/brand/world/club-960.webp 960w, /brand/world/club-1920.webp 1920w" sizes="100vw" />
        <img src="/brand/world/club-1920.webp" alt="" width={1920} height={1080} fetchPriority="high" />
      </picture>
      <div className="world-wrap">
        <div className="world-copy">
          <p className="world-invitation">Kein Stream endet im Leeren.</p>
          <h1 id="v5-titel">
            <span>Deutsches</span>
            <strong>Deadlock</strong>
            <span>Streamer-Netz</span>
          </h1>
          <p>
            Gehst du offline, raidet der Bot deine Zuschauer zu einem deutschen Partner, der gerade Deadlock streamt.
            Gehen andere offline, kommen ihre Zuschauer zu dir.
          </p>
          <div className="world-actions">
            <a className="world-button" href={buildTwitchBotAuthUrl()}>
              Partner werden
            </a>
            <a className="world-button world-button--outline" href="#partner">
              Partner ansehen
            </a>
          </div>
          <p className="v5-hero-note">Kostenlos. Ein Klick bei Twitch, und du bist wieder raus.</p>
        </div>
      </div>
    </section>
  );
}
