import {
  DISCORD_INVITE_URL,
  TWITCH_FAQ_URL,
  TWITCH_ONBOARDING_URL,
} from "@/data/externalLinks";
import { AFFILIATE_PROGRAM_PATH, WEBSITE_HOME_PATH } from "@/data/sitePaths";

interface FooterLink {
  label: string;
  href: string;
  external?: boolean;
}

const NAV_SECTION_LINKS: FooterLink[] = [
  { label: 'Features', href: `${WEBSITE_HOME_PATH}#features` },
  { label: 'Dashboard', href: `${WEBSITE_HOME_PATH}#dashboard` },
  { label: 'Community', href: `${WEBSITE_HOME_PATH}#community` },
];

const MORE_LINKS: FooterLink[] = [
  { label: 'Streamer Onboarding', href: TWITCH_ONBOARDING_URL, external: true },
  { label: 'Bot FAQ', href: TWITCH_FAQ_URL, external: true },
  { label: 'Affiliate-Programm', href: AFFILIATE_PROGRAM_PATH },
  { label: 'Demo Dashboard', href: 'https://deutsche-deadlock-community.de/demo/twitch/demo/', external: true },
  { label: 'Discord beitreten', href: DISCORD_INVITE_URL, external: true },
  { label: 'Impressum', href: 'https://deutsche-deadlock-community.de/twitch/impressum', external: true },
  { label: 'Datenschutz', href: 'https://deutsche-deadlock-community.de/twitch/datenschutz', external: true },
];

function FooterLinkItem({ link }: { link: FooterLink }) {
  const baseClass =
    'text-sm text-text-secondary hover:text-text-primary transition-colors duration-200';

  return (
    <a
      href={link.href}
      target={link.external ? '_blank' : undefined}
      rel={link.external ? 'noopener noreferrer' : undefined}
      className={baseClass}
    >
      {link.label}
    </a>
  );
}

export function Footer() {
  return (
    <footer className="w-full border-t border-border" style={{ background: 'rgba(7,21,29,0.5)' }}>
      <div className="max-w-7xl mx-auto px-6 py-12">
        {/* Three-column grid */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
          {/* Column 1 – Brand */}
          <div className="flex flex-col gap-3">
            <span className="font-display font-bold text-xl bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
              Deutsche Deadlock Community
            </span>
            <p className="text-text-secondary text-sm leading-relaxed">
              Erstellt für die Deadlock-Community
            </p>
          </div>

          {/* Column 2 – Navigation */}
          <div className="flex flex-col gap-3">
            <h4 className="text-sm font-semibold text-text-primary tracking-wide uppercase">
              Navigation
            </h4>
            <nav className="flex flex-col gap-2">
              {NAV_SECTION_LINKS.map((link) => (
                <FooterLinkItem key={link.label} link={link} />
              ))}
            </nav>
          </div>

          {/* Column 3 – More */}
          <div className="flex flex-col gap-3">
            <h4 className="text-sm font-semibold text-text-primary tracking-wide uppercase">
              Mehr
            </h4>
            <nav className="flex flex-col gap-2">
              {MORE_LINKS.map((link) => (
                <FooterLinkItem key={link.label} link={link} />
              ))}
            </nav>
          </div>
        </div>

        {/* Bottom bar */}
        <div className="border-t border-border mt-8 pt-8 flex flex-col sm:flex-row justify-between items-center gap-3">
          <p className="text-text-secondary text-sm">
            &copy; 2026 Deutsche Deadlock Community. Alle Rechte vorbehalten.
          </p>
        </div>
      </div>
    </footer>
  );
}
