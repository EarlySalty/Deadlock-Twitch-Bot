import {
  DISCORD_INVITE_URL,
  TWITCH_AGB_URL,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_IMPRESSUM_URL,
  TWITCH_SECURITY_URL,
} from "@/data/externalLinks";
import { PARTNER_COPY } from "@/data/partnerPage";

export function PartnerFooter() {
  return (
    <footer className="pn-footer">
      <div className="pn-wrap pn-footer-grid">
        <div>
          <strong>{PARTNER_COPY.brand}</strong>
          <p>Das Partner-Netzwerk der deutschen Deadlock-Community.</p>
        </div>
        <nav aria-label="Rechtliches">
          <a href={DISCORD_INVITE_URL} target="_blank" rel="noopener noreferrer">
            Discord
          </a>
          <a href={TWITCH_SECURITY_URL}>Sicherheit</a>
          <a href={TWITCH_IMPRESSUM_URL}>Impressum</a>
          <a href={TWITCH_DATENSCHUTZ_URL}>Datenschutz</a>
          <a href={TWITCH_AGB_URL}>AGB</a>
        </nav>
      </div>
    </footer>
  );
}
