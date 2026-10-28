import { useEffect, useRef, useState } from "react";
import { buildTwitchBotAuthUrl } from "@/data/externalLinks";

const WEGE = [
  { href: "#ablauf", label: "So läuft's" },
  { href: "#vorteile", label: "Partner-Vorteile" },
  { href: "#schutz", label: "Chat-Schutz" },
  { href: "#partner", label: "Partner" },
  { href: "#sicherheit", label: "Sicherheit" },
];

export function Kopfzeile() {
  const [offen, setOffen] = useState(false);
  const knopf = useRef<HTMLButtonElement>(null);
  const schliessenKnopf = useRef<HTMLButtonElement>(null);
  const warOffen = useRef(false);

  useEffect(() => {
    if (offen) schliessenKnopf.current?.focus();
    else if (warOffen.current) knopf.current?.focus();
    warOffen.current = offen;
  }, [offen]);

  useEffect(() => {
    if (!offen) return;
    const schliessen = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOffen(false);
    };
    document.addEventListener("keydown", schliessen);
    document.body.style.overflow = "hidden";
    return () => {
      document.removeEventListener("keydown", schliessen);
      document.body.style.overflow = "";
    };
  }, [offen]);

  return (
    <header className="world-header v5-header">
      <a className="brand" href="/" aria-label="Deutsche Deadlock Community">
        <img src="/brand/logo/logo-192.png" className="brand-logo" alt="" width={54} height={54} />
        <img src="/brand/logo/wordmark.svg" className="brand-wordmark-img" alt="" width={220} height={48} />
      </a>
      <nav className="world-nav v5-nav" aria-label="Seitenbereiche">
        {WEGE.map((w) => (
          <a key={w.href} href={w.href}>
            {w.label}
          </a>
        ))}
        <a className="world-button" href={buildTwitchBotAuthUrl()}>
          Partner werden
        </a>
      </nav>
      <button
        type="button"
        ref={knopf}
        className="v5-menu-button"
        aria-expanded={offen}
        aria-controls="v5-drawer"
        onClick={() => setOffen(true)}
      >
        Menü
      </button>
      <div className={offen ? "v5-drawer-backdrop is-open" : "v5-drawer-backdrop"} onClick={() => setOffen(false)} aria-hidden="true" />
      <nav id="v5-drawer" className={offen ? "v5-drawer is-open" : "v5-drawer"} aria-label="Menü" aria-hidden={!offen}>
        <button ref={schliessenKnopf} type="button" className="v5-drawer-close" onClick={() => setOffen(false)} tabIndex={offen ? 0 : -1}>
          Schließen
        </button>
        {WEGE.map((w) => (
          <a key={w.href} href={w.href} onClick={() => setOffen(false)} tabIndex={offen ? 0 : -1}>
            {w.label}
          </a>
        ))}
        <a className="world-button" href={buildTwitchBotAuthUrl()} tabIndex={offen ? 0 : -1}>
          Partner werden
        </a>
      </nav>
    </header>
  );
}
