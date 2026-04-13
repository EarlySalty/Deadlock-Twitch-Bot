import {
  DISCORD_INVITE_URL,
  EARLYSALTY_WEBSITE_URL,
  TWITCH_AGB_URL,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_DEMO_DASHBOARD_URL,
  TWITCH_FAQ_URL,
  TWITCH_IMPRESSUM_URL,
  TWITCH_ONBOARDING_URL,
  buildTwitchDashboardLoginUrl,
} from "@/data/externalLinks";

const FOOTER_LINKS = [
  { label: "Onboarding", href: TWITCH_ONBOARDING_URL },
  { label: "FAQ", href: TWITCH_FAQ_URL },
  { label: "Login", href: buildTwitchDashboardLoginUrl() },
  { label: "Demo", href: TWITCH_DEMO_DASHBOARD_URL },
  { label: "Website", href: EARLYSALTY_WEBSITE_URL },
  { label: "Discord", href: DISCORD_INVITE_URL },
  { label: "Impressum", href: TWITCH_IMPRESSUM_URL },
  { label: "Datenschutz", href: TWITCH_DATENSCHUTZ_URL },
  { label: "AGB", href: TWITCH_AGB_URL },
];

export function PublicInfoFooter() {
  return (
    <footer className="border-t border-border bg-[rgba(7,21,29,0.56)]">
      <div className="mx-auto max-w-7xl px-6 py-10">
        <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
          <div className="max-w-xl">
            <p className="bg-gradient-to-r from-primary to-accent bg-clip-text text-xl font-bold font-display text-transparent">
              DDC
            </p>
            <p className="mt-3 text-sm leading-relaxed text-text-secondary">
              Öffentliche Produktflächen für neue und bestehende Streamer. Die
              Seite erklärt den Partner-Flow und die dokumentierten Bot-Funktionen
              auf derselben Domain wie das Dashboard.
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

        <div className="mt-8 border-t border-border pt-6 text-sm text-text-secondary">
          &copy; 2026 Deutsche Deadlock Community. Twitch-Onboarding, FAQ und Partner-Dashboard
          gehören zusammen.
        </div>
      </div>
    </footer>
  );
}


