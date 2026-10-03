import { useEffect, useRef } from "react";
import { ExternalLink } from "lucide-react";
import { loadTrustpilotWidget, registerTrustpilot, TRUSTPILOT_PROFILE_URL, TRUSTPILOT_REVIEW_URL } from "@/lib/trustpilot";

export function TrustpilotLauncher() {
  useEffect(() => {
    void registerTrustpilot().catch((error: unknown) => {
      console.warn("Die Trustpilot-Einbindung konnte nicht geladen werden.", error);
    });
  }, []);

  return (
    <a
      id="trustpilot"
      href={TRUSTPILOT_REVIEW_URL}
      target="_blank"
      rel="noopener noreferrer"
      aria-label="Community auf Trustpilot bewerten"
      className="fixed right-5 top-[min(55dvh,calc(100dvh-8rem))] flex min-h-11 -translate-y-1/2 items-center gap-3 rounded-2xl border border-accent/60 bg-background px-4 py-3 text-text-primary shadow-xl sm:right-7 focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-accent"
    >
      <span>
        <span className="block text-xs font-semibold text-accent">Trustpilot</span>
        <span className="block text-sm">Community bewerten</span>
      </span>
      <ExternalLink size={16} aria-hidden="true" />
    </a>
  );
}

export function Trustpilot() {
  const widget = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (widget.current) {
      void loadTrustpilotWidget(widget.current).catch((error: unknown) => {
        console.warn("Das Trustpilot-Widget konnte nicht geladen werden.", error);
      });
    }
    void registerTrustpilot().catch((error: unknown) => {
      console.warn("Die Trustpilot-Einbindung konnte nicht geladen werden.", error);
    });
  }, []);

  return (
    <section aria-label="Community-Bewertungen auf Trustpilot" className="shrink-0 border-b border-border px-4 py-2">
      <div className="flex items-center justify-between gap-3">
        <a href={TRUSTPILOT_PROFILE_URL} target="_blank" rel="noopener noreferrer" className="flex min-h-11 min-w-0 flex-col justify-center rounded-lg text-xs text-text-secondary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent">
          <span className="font-semibold text-accent">Trustpilot</span>
          <span>Bewertungen ansehen</span>
        </a>
        <a href={TRUSTPILOT_REVIEW_URL} target="_blank" rel="noopener noreferrer" className="inline-flex min-h-11 shrink-0 items-center gap-1 rounded-lg px-2 text-xs font-semibold text-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent">
          Bewerten <ExternalLink size={13} aria-hidden="true" />
        </a>
      </div>
      <div
        ref={widget}
        className="trustpilot-widget [@media(max-height:500px)]:hidden"
        data-locale="de-DE"
        data-template-id="56278e9abfbbba0bdcd568bc"
        data-businessunit-id="6ac13d8b89ce8ac6e14678b8"
        data-style-height="52px"
        data-style-width="100%"
        data-theme="dark"
        data-token="67ca1866-b9c4-4cdb-a307-946f9817a6a5"
        style={{ minHeight: 52 }}
      >
        <a href={TRUSTPILOT_PROFILE_URL} target="_blank" rel="noopener noreferrer">Trustpilot</a>
      </div>
    </section>
  );
}
