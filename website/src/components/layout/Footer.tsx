import {
  DISCORD_INVITE_URL,
  TWITCH_AGB_URL,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_FAQ_URL,
  TWITCH_DASHBOARD_URL,
  TWITCH_DEMO_DASHBOARD_URL,
  TWITCH_IMPRESSUM_URL,
  TWITCH_SECURITY_URL,
} from "@/data/externalLinks";
import { AFFILIATE_PROGRAM_PATH, WEBSITE_HOME_PATH } from "@/data/sitePaths";
import { IS_EN, STREAMER_URL_DE, STREAMER_URL_EN, tx } from "@/i18n/locale";

interface FooterLink {
  label: string;
  href: string;
  external?: boolean;
}

const NAV_SECTION_LINKS: FooterLink[] = [
  { label: 'Features', href: `${WEBSITE_HOME_PATH}#features` },
  { label: 'Community', href: `${WEBSITE_HOME_PATH}#community` },
];

const MORE_LINKS: FooterLink[] = [
  { label: tx('Partner-Dashboard', 'Partner Dashboard'), href: TWITCH_DASHBOARD_URL },
  { label: 'Bot FAQ', href: TWITCH_FAQ_URL, external: true },
  { label: tx('Affiliate-Programm', 'Affiliate program'), href: AFFILIATE_PROGRAM_PATH },
  { label: tx('Dashboard-Demo', 'Dashboard demo'), href: TWITCH_DEMO_DASHBOARD_URL, external: true },
  { label: tx('Discord beitreten', 'Join our Discord'), href: DISCORD_INVITE_URL, external: true },
  { label: tx('Impressum', 'Legal notice (German)'), href: TWITCH_IMPRESSUM_URL, external: true },
  { label: tx('Datenschutz', 'Privacy'), href: TWITCH_DATENSCHUTZ_URL, external: true },
  { label: tx('AGB', 'Terms (German)'), href: TWITCH_AGB_URL, external: true },
  { label: tx('Sicherheitskonzept', 'Security'), href: TWITCH_SECURITY_URL, external: true },
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
    <footer className="w-full border-t border-border" style={{ background: 'rgba(28, 21, 13, 0.5)' }}>
      <div className="max-w-7xl mx-auto px-6 py-12">
        {/* Three-column grid */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
          {/* Column 1 – Brand */}
          <div className="flex flex-col gap-3">
            <span className="font-display font-bold text-xl bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
              {tx('Deutsche Deadlock Community', 'Deadlock Partner Network')}
            </span>
            <p className="text-text-secondary text-sm leading-relaxed">
              {tx('Erstellt für die Deadlock-Community', 'Built for the Deadlock community')}
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
              {tx('Mehr', 'More')}
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
            {tx(
              <>&copy; 2026 Deutsche Deadlock Community. Alle Rechte vorbehalten.</>,
              <>&copy; 2026 Deadlock Partner Network. All rights reserved.</>,
            )}
          </p>
          <a
            href={IS_EN ? STREAMER_URL_DE : STREAMER_URL_EN}
            hrefLang={IS_EN ? 'de' : 'en'}
            lang={IS_EN ? 'de' : 'en'}
            className="text-text-secondary text-sm hover:text-text-primary transition-colors duration-200"
          >
            {IS_EN ? 'Deutsche Version' : 'English version'}
          </a>
        </div>
      </div>
    </footer>
  );
}
