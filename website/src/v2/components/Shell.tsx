import type { JSX, ReactNode } from "react";
import {
  buildBotAuthUrl,
  DISCORD_INVITE_URL,
  ROADMAP_URL,
  TWITCH_AFFILIATE_URL,
  TWITCH_AGB_URL,
  TWITCH_BOT_AUTH_FALLBACK,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_IMPRESSUM_URL,
  TWITCH_SECURITY_URL,
  V2_BASE,
  V2_FAQ,
  V2_FEATURES,
} from "../lib/links";
import { useReveal } from "../lib/useReveal";

/**
 * CTA-Link: die ts-Query wird erst beim Klick gebaut (frischer OAuth-State),
 * das statische href ist der No-JS-Fallback.
 */
export function CtaLink({
  className = "btn btn-primary",
  children,
}: {
  className?: string;
  children: ReactNode;
}): JSX.Element {
  return (
    <a
      className={className}
      href={TWITCH_BOT_AUTH_FALLBACK}
      onClick={(event) => {
        event.preventDefault();
        window.location.assign(buildBotAuthUrl());
      }}
    >
      {children}
    </a>
  );
}

const NAV_ITEMS: Array<{ href: string; label: string }> = [
  { href: `${V2_BASE}#netzwerk`, label: "Netzwerk" },
  { href: V2_FEATURES, label: "Features" },
  { href: V2_FAQ, label: "FAQ" },
];

export function Shell({ children }: { children: ReactNode }): JSX.Element {
  useReveal();
  return (
    <>
      <div className="grain" aria-hidden="true" />
      <div className="vignette" aria-hidden="true" />

      <header className="site-header">
        <div className="container header-inner">
          <a className="wordmark" href={V2_BASE}>
            <span className="wordmark-jewel" aria-hidden="true" />
            Deadlock <b>Partner-Netzwerk</b>
          </a>
          <nav className="main-nav" aria-label="Hauptnavigation">
            {NAV_ITEMS.map((item) => (
              <a key={item.href} href={item.href}>
                {item.label}
              </a>
            ))}
            <CtaLink className="nav-cta">Bot reinholen</CtaLink>
          </nav>
        </div>
      </header>

      <main>{children}</main>

      <footer className="site-footer">
        <div className="deco-divider" aria-hidden="true">
          <span />
        </div>
        <div className="container footer-inner">
          <p className="footer-brand">
            Deutsche Deadlock Community — Partner-Netzwerk für Deadlock-Streamer.
          </p>
          <nav className="footer-nav" aria-label="Rechtliches und weitere Seiten">
            <a href={DISCORD_INVITE_URL}>Discord</a>
            <a href={ROADMAP_URL}>Roadmap</a>
            <a href={TWITCH_AFFILIATE_URL}>Affiliate</a>
            <a href={TWITCH_IMPRESSUM_URL}>Impressum</a>
            <a href={TWITCH_DATENSCHUTZ_URL}>Datenschutz</a>
            <a href={TWITCH_AGB_URL}>AGB</a>
            <a href={TWITCH_SECURITY_URL}>Sicherheit</a>
          </nav>
        </div>
      </footer>
    </>
  );
}
