import {
  DISCORD_INVITE_URL,
  EARLYSALTY_WEBSITE_URL,
  TWITCH_AGB_URL,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_DASHBOARD_URL,
  TWITCH_DEMO_DASHBOARD_URL,
  TWITCH_FAQ_URL,
  TWITCH_IMPRESSUM_URL,
  TWITCH_ONBOARDING_URL,
} from "@/data/externalLinks";
import { IS_EN, STREAMER_URL_DE, STREAMER_URL_EN, tx } from "@/i18n/locale";

const FOOTER_LINKS = [
  { label: tx("Streamer-Vergleich", "Streamer comparison"), href: "/streamer/vergleich/" },
  { label: "Onboarding", href: TWITCH_ONBOARDING_URL },
  { label: "FAQ", href: TWITCH_FAQ_URL },
  { label: tx("Partner-Dashboard", "Partner Dashboard"), href: TWITCH_DASHBOARD_URL },
  { label: tx("Dashboard-Demo", "Dashboard demo"), href: TWITCH_DEMO_DASHBOARD_URL },
  { label: "Website", href: EARLYSALTY_WEBSITE_URL },
  { label: "Discord", href: DISCORD_INVITE_URL },
  { label: tx("Impressum", "Legal notice (German)"), href: TWITCH_IMPRESSUM_URL },
  { label: tx("Datenschutz", "Privacy"), href: TWITCH_DATENSCHUTZ_URL },
  { label: tx("AGB", "Terms (German)"), href: TWITCH_AGB_URL },
];

export function PublicInfoFooter() {
  return (
    <footer className="border-t border-border bg-[rgba(28, 21, 13, 0.56)]">
      <div className="mx-auto max-w-7xl px-6 py-10">
        <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
          <div className="max-w-xl">
            <p className="bg-gradient-to-r from-primary to-accent bg-clip-text text-xl font-bold font-display text-transparent">
              {tx("Deutsche Deadlock Community", "Deadlock Partner Network")}
</p>
            <p className="mt-3 text-sm leading-relaxed text-text-secondary">
              {tx(
                <>
                  Öffentliche Produktflächen für neue und bestehende Streamer. Die
                  Seite erklärt den Partner-Flow und die dokumentierten Bot-Funktionen
                  auf derselben Domain wie das Dashboard.
                </>,
                <>
                  Public pages for new and existing streamers. They explain the
                  partner flow and the documented bot features, on the same domain
                  as the dashboard.
                </>,
              )}
            </p>
          </div>

          <div className="grid grid-cols-2 gap-x-6 gap-y-3 text-sm sm:grid-cols-3">
            {FOOTER_LINKS.map((link) => (
              <a
                key={link.href}
                href={link.href}
                className="text-text-secondary transition-colors duration-200 hover:text-text-primary no-underline"
              >
                {link.label}
              </a>
            ))}
          </div>
        </div>

        <div className="mt-8 flex flex-col gap-3 border-t border-border pt-6 text-sm text-text-secondary sm:flex-row sm:items-center sm:justify-between">
          <p>
            {tx(
              <>
                &copy; 2026 Deutsche Deadlock Community. Twitch-Onboarding, FAQ und Partner-Dashboard
                gehören zusammen.
              </>,
              <>
                &copy; 2026 Deadlock Partner Network. Twitch onboarding, FAQ and Partner Dashboard
                belong together.
              </>,
            )}
          </p>
          <a
            href={IS_EN ? STREAMER_URL_DE : STREAMER_URL_EN}
            hrefLang={IS_EN ? "de" : "en"}
            lang={IS_EN ? "de" : "en"}
            className="text-text-secondary no-underline transition-colors duration-200 hover:text-text-primary"
          >
            {IS_EN ? "Deutsche Version" : "English version"}
          </a>
        </div>
      </div>
    </footer>
  );
}
