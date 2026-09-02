import { useEffect, useState } from "react";
import {
  buildTwitchBotAuthUrl,
  DISCORD_INVITE_URL,
} from "@/data/externalLinks";
import { PARTNER_COPY, PARTNER_NAV } from "@/data/partnerPage";

export function PartnerNav() {
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 16);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  const auth = buildTwitchBotAuthUrl();

  return (
    <header
      className={`pn-nav${scrolled ? " is-scrolled" : ""}${open ? " is-open" : ""}`}
    >
      <div className="pn-wrap pn-nav-inner">
        <a className="pn-brand" href="/streamer/v2/">
          <img src="/brand/logo/logo-192.png" alt="" width={32} height={32} />
          <span>{PARTNER_COPY.brand}</span>
        </a>
        <nav className="pn-nav-links" aria-label="Seite">
          {PARTNER_NAV.map((item) => (
            <a key={item.id} href={`#${item.id}`}>
              {item.label}
            </a>
          ))}
        </nav>
        <div className="pn-nav-cta">
          <a
            className="pn-btn pn-btn-ghost"
            href={DISCORD_INVITE_URL}
            target="_blank"
            rel="noopener noreferrer"
          >
            Discord
          </a>
          <a className="pn-btn pn-btn-primary" href={auth}>
            {PARTNER_COPY.ctaPrimary}
          </a>
        </div>
        <button
          className="pn-menu-btn"
          type="button"
          aria-expanded={open}
          aria-label="Menü"
          onClick={() => setOpen((value) => !value)}
        >
          {open ? "Schließen" : "Menü"}
        </button>
      </div>
      <div className="pn-wrap pn-mobile">
        {PARTNER_NAV.map((item) => (
          <a
            key={item.id}
            href={`#${item.id}`}
            onClick={() => setOpen(false)}
          >
            {item.label}
          </a>
        ))}
        <a
          className="pn-btn pn-btn-ghost"
          href={DISCORD_INVITE_URL}
          target="_blank"
          rel="noopener noreferrer"
        >
          {PARTNER_COPY.ctaSecondary}
        </a>
        <a className="pn-btn pn-btn-primary" href={auth}>
          {PARTNER_COPY.ctaPrimary}
        </a>
      </div>
    </header>
  );
}
