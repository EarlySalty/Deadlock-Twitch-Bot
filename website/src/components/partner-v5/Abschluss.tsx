import {
  DISCORD_INVITE_URL,
  TWITCH_AGB_URL,
  TWITCH_DASHBOARD_URL,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_FAQ_URL,
  TWITCH_IMPRESSUM_URL,
  buildTwitchBotAuthUrl,
} from "@/data/externalLinks";

export function Abschluss() {
  return (
    <section className="world-scene world-scene--ghost" aria-labelledby="v5-abschluss-titel">
      <picture className="world-picture">
        <source type="image/avif" srcSet="/brand/world/ghost-960.avif 960w, /brand/world/ghost-1920.avif 1920w" sizes="100vw" />
        <img src="/brand/world/ghost-1920.webp" alt="" width={1920} height={1080} loading="lazy" />
      </picture>
      <div className="world-wrap">
        <div className="world-copy">
          <h2 id="v5-abschluss-titel">Bis zum nächsten Raid.</h2>
          <p>Dein nächster Stream endet sowieso. Die Frage ist, ob allein oder als Partner.</p>
          <div className="world-actions">
            <a className="world-button" href={buildTwitchBotAuthUrl()}>
              Partner werden
            </a>
            <a className="world-button world-button--outline" href={DISCORD_INVITE_URL} target="_blank" rel="noopener noreferrer">
              Community-Discord
            </a>
          </div>
        </div>
      </div>
    </section>
  );
}

export function Fusszeile() {
  return (
    <footer className="world-footer v5-footer">
      <div className="world-footer-top">
        <p>
          Das Streamer-Netzwerk der Deutschen Deadlock Community.
        </p>
        <nav aria-label="Rechtliches und Links">
          <a href={TWITCH_IMPRESSUM_URL}>Impressum</a>
          <a href={TWITCH_DATENSCHUTZ_URL}>Datenschutz</a>
          <a href={TWITCH_AGB_URL}>AGB</a>
          <a href={TWITCH_FAQ_URL}>FAQ</a>
          <a href={TWITCH_DASHBOARD_URL}>Partner-Dashboard</a>
          <a href={DISCORD_INVITE_URL} target="_blank" rel="noopener noreferrer">
            Discord
          </a>
        </nav>
      </div>
    </footer>
  );
}
