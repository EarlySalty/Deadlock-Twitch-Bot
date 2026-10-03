import { useEffect, useRef } from "react";
import { ExternalLink } from "lucide-react";
import { loadTrustpilotWidget, registerTrustpilot, TRUSTPILOT_PROFILE_URL, TRUSTPILOT_REVIEW_URL } from "@/lib/trustpilot";

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
    <section id="trustpilot" aria-labelledby="trustpilot-heading" className="max-w-3xl mx-auto px-6 pb-16">
      <div className="panel-card rounded-2xl p-6 md:p-8 text-center">
        <h2 id="trustpilot-heading" className="text-2xl font-semibold font-display text-[var(--color-text-primary)]">
          Wie erlebst du unsere Community?
        </h2>
        <p className="text-[var(--color-text-secondary)] mt-3 mb-5">
          Teile deine Erfahrung mit der Deutschen Deadlock Community auf Trustpilot.
          Wir freuen uns über ehrliches Feedback.
        </p>
        <div
          ref={widget}
          className="trustpilot-widget"
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
        <a
          href={TRUSTPILOT_REVIEW_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="mt-5 inline-flex items-center gap-2 rounded-xl px-6 py-3 gradient-accent font-semibold focus-visible:outline-2 focus-visible:outline-offset-4"
        >
          Community auf Trustpilot bewerten
          <ExternalLink size={18} aria-hidden="true" />
        </a>
      </div>
    </section>
  );
}
