import type { JSX } from "react";
import { MdBlocks } from "../components/Md";
import { CtaLink, Shell } from "../components/Shell";
import { loadKnowledgeDoc } from "../lib/knowledge";
import { V2_FAQ } from "../lib/links";

interface Feature {
  title: string;
  teaser: string;
  /** Schlüssel der Wissensdatei in rust/knowledge/bot/ für den Detailtext. */
  docKey?: string;
  /** Fallback, wenn kein Detaildokument existiert: Link in die FAQ-Gruppe. */
  faqAnchor?: string;
}

const FEATURES: Feature[] = [
  {
    title: "Auto-Raids",
    teaser:
      "Dein Stream endet nie im Leeren: Der Bot raidet automatisch den passenden Partner — fair übers Netzwerk verteilt.",
    docKey: "auto-raid",
  },
  {
    title: "Analytics-Dashboard",
    teaser:
      "Viewer, Follows, Raids, Trends — dein Kanal in echten Zahlen statt Bauchgefühl.",
    docKey: "analytics-dashboard",
  },
  {
    title: "Stats & Overlay",
    teaser:
      "Deine Deadlock-Stats direkt im Stream — Overlay einbinden, fertig.",
    faqAnchor: "faq-stats-overlay",
  },
  {
    title: "Chat-Moderation & Scam-Guard",
    teaser:
      "Scam-Links und Spam-Wellen fliegen automatisch raus, bevor dein Chat sie überhaupt sieht.",
    docKey: "chat-moderation",
  },
  {
    title: "Discord-Go-Live",
    teaser:
      "Sobald du live gehst, wird dein Stream automatisch im Community-Discord angekündigt.",
    docKey: "discord-golive",
  },
  {
    title: "Einrichtung in 30 Sekunden",
    teaser:
      "Twitch-Login, fertig. Keine Downloads, kein OBS-Gefummel für den Start.",
    docKey: "einrichtung",
  },
];

function FeatureCard({ feature }: { feature: Feature }): JSX.Element {
  const doc = feature.docKey ? loadKnowledgeDoc(feature.docKey) : null;
  return (
    <article className="panel panel-corners feature-card reveal">
      <h3>{feature.title}</h3>
      <p className="feature-teaser">{feature.teaser}</p>
      {doc ? (
        <details className="feature-details">
          <summary>Im Detail</summary>
          <MdBlocks blocks={doc} />
        </details>
      ) : (
        <a className="md-link" href={`${V2_FAQ}#${feature.faqAnchor ?? ""}`}>
          Mehr dazu im FAQ
        </a>
      )}
    </article>
  );
}

export function FeaturesPage(): JSX.Element {
  return (
    <Shell>
      <section className="section">
        <div className="container">
          <p className="overline reveal">Was du bekommst</p>
          <h1 className="reveal">
            Ein Bot. <span className="gold">Die ganze Ausstattung.</span>
          </h1>
          <p className="lede reveal">
            Alles hier ist im Partnernetzwerk enthalten — kein Premium-Gate
            vor den Kernfunktionen. Die Detailtexte kommen aus derselben
            Wissensbasis, aus der auch der Bot selbst antwortet.
          </p>
          <div className="feature-grid">
            {FEATURES.map((feature) => (
              <FeatureCard key={feature.title} feature={feature} />
            ))}
          </div>
          <div className="final-inner">
            <div className="hero-actions reveal">
              <CtaLink>Bot in deinen Kanal holen</CtaLink>
              <a className="btn btn-ghost" href={V2_FAQ}>
                Noch Fragen? Zum FAQ
              </a>
            </div>
          </div>
        </div>
      </section>
    </Shell>
  );
}
