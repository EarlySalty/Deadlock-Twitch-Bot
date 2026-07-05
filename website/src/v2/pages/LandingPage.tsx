import type { JSX } from "react";
import { CtaLink, Shell } from "../components/Shell";
import { DISCORD_INVITE_URL, twitchChannelUrl, V2_FAQ } from "../lib/links";
import { formatCount, useNetworkStats, type NetworkStats } from "../lib/useNetworkStats";

function Hero(): JSX.Element {
  return (
    <section className="section hero">
      <div className="container">
        <p className="overline reveal">Deutsches Deadlock-Partnernetzwerk</p>
        <h1 className="reveal">
          Die deutsche Deadlock-Kategorie wird gerade verteilt.{" "}
          <span className="gold">Die Plätze oben sind noch frei.</span>
        </h1>
        <p className="lede reveal">
          Mit 50 Viewern bist du in Fortnite unsichtbar — in Deadlock stehst du
          damit ganz oben in der Kategorie, sichtbar für jeden, der das Spiel
          anklickt. Genau jetzt entsteht die deutsche Szene. Wir bauen die
          Infrastruktur dafür: automatische Raids, Stats-Overlay, Analytics,
          Coaching. Der Bot ist dein Mitgliedsausweis.
        </p>
        <div className="hero-actions reveal">
          <CtaLink>Bot in deinen Kanal holen</CtaLink>
          <a className="btn btn-ghost" href="#netzwerk">
            Wie das Netzwerk funktioniert
          </a>
        </div>
        <p className="hero-note reveal">
          Kostenlos für Streamer. In 30 Sekunden drin, jederzeit wieder raus.
        </p>
      </div>
    </section>
  );
}

function ProofStrip({
  stats,
  failed,
}: {
  stats: NetworkStats | null;
  failed: boolean;
}): JSX.Element {
  const tiles: Array<{ label: string; value: string }> = [
    { label: "Aktive Partner", value: formatCount(stats?.active_partners) },
    { label: "Raids vermittelt — gesamt", value: formatCount(stats?.raids_total) },
    { label: "Raids — letzte 7 Tage", value: formatCount(stats?.raids_7d) },
  ];
  if (stats?.viewers_forwarded_total != null) {
    tiles.push({
      label: "Weitergereichte Viewer",
      value: formatCount(stats.viewers_forwarded_total),
    });
  }
  return (
    <section className="section proof" aria-label="Netzwerk-Zahlen">
      <div className="container">
        <p className="overline reveal">Live aus der Datenbank</p>
        <h2 className="reveal">Keine Marketing-Zahlen. Unsere echten.</h2>
        <div className="stat-grid reveal">
          {tiles.map((tile) => (
            <div key={tile.label} className="panel panel-corners stat-tile">
              <strong>{tile.value}</strong>
              <span>{tile.label}</span>
            </div>
          ))}
        </div>
        <p className="proof-note reveal">
          {failed && !stats
            ? "Die Live-Zahlen sind gerade nicht erreichbar — schau in ein paar Minuten wieder rein."
            : "Diese Zahlen kommen ungefiltert aus der Live-Datenbank des Netzwerks. Wenn hier wenig steht, ist da wenig — wir runden nichts schön."}
        </p>
      </div>
    </section>
  );
}

function LiveWall({ stats }: { stats: NetworkStats | null }): JSX.Element {
  const live = stats?.live ?? [];
  return (
    <section className="section livewall" aria-label="Gerade live">
      <div className="container">
        <p className="overline reveal">Das Netzwerk, jetzt gerade</p>
        <h2 className="reveal">Gerade live</h2>
        {live.length ? (
          <div className="live-grid reveal">
            {live.map((s) => (
              <a
                key={s.login}
                className="panel live-card"
                href={twitchChannelUrl(s.login)}
                target="_blank"
                rel="noreferrer"
              >
                <span className="live-dot" aria-hidden="true" />
                <span className="live-name">{s.display_name}</span>
                <span className="live-login">twitch.tv/{s.login}</span>
              </a>
            ))}
          </div>
        ) : (
          <p className="live-empty panel reveal">
            Gerade streamt niemand aus dem Netzwerk. Sei der, der online ist,
            wenn andere zuschauen wollen.
          </p>
        )}
        <p className="proof-note reveal">
          Jeder Partner taucht hier automatisch auf, sobald er live geht —
          Gratis-Sichtbarkeit inklusive.
        </p>
      </div>
    </section>
  );
}

const STEPS: Array<{ title: string; text: string }> = [
  {
    title: "Du streamst Deadlock",
    text: "Der Bot weiß, wer aus dem Netzwerk gerade live ist — du musst niemanden suchen und niemanden kennen.",
  },
  {
    title: "Du machst Feierabend",
    text: "Statt ins Leere zu enden, raidet dein Stream automatisch den passenden nächsten Kanal im Netzwerk.",
  },
  {
    title: "Der Kreislauf dreht sich",
    text: "Beendet ein anderer Partner seinen Stream, landen seine Viewer bei dir. Wer streamt, wird gefunden.",
  },
];

function Circuit(): JSX.Element {
  return (
    <section className="section circuit" id="netzwerk">
      <div className="container">
        <p className="overline reveal">So funktioniert es</p>
        <h2 className="reveal">Kein Stream endet mehr im Nichts.</h2>
        <ol className="step-grid">
          {STEPS.map((step, i) => (
            <li key={step.title} className="panel panel-corners step-card reveal">
              <span className="step-jewel" aria-hidden="true">
                <i>{i + 1}</i>
              </span>
              <h3>{step.title}</h3>
              <p>{step.text}</p>
            </li>
          ))}
        </ol>
        <div className="panel panel-corners honest reveal">
          <p>
            <strong>Ehrlich gesagt:</strong> Wir machen dich nicht groß — das
            kann kein Bot. Wir sorgen dafür, dass nichts von dem verpufft, was
            du selbst reinsteckst. Jeder Stream zahlt aufs Netzwerk ein, und
            das Netzwerk zahlt zurück.
          </p>
        </div>
      </div>
    </section>
  );
}

const SWITCHER_CARDS: Array<{ title: string; text: string }> = [
  {
    title: "Sichtbarkeit ab Tag 1",
    text: "Die deutsche Deadlock-Kategorie ist klein genug, dass du mit deinen jetzigen Zahlen oben mitspielst — und wächst schnell genug, dass es sich lohnt.",
  },
  {
    title: "Anschluss statt Kaltstart",
    text: "Raids aus dem Netzwerk ab deinem ersten Stream. Die Szene kennt dich, bevor dein Overlay fertig eingerichtet ist.",
  },
  {
    title: "Besser werden, wenn du willst",
    text: "Coaching und Scrims aus der Community — für alle, die nicht nur das Spiel wechseln, sondern damit wachsen wollen.",
  },
];

function Switcher(): JSX.Element {
  return (
    <section className="section switcher">
      <div className="container">
        <p className="overline reveal">Für Wechsler</p>
        <h2 className="reveal">Du überlegst, zu Deadlock zu wechseln?</h2>
        <p className="lede reveal">
          Ein Game-Wechsel ist der Moment, in dem Streamer am meisten
          verlieren: Stammpublikum weg, fremde Kategorie, null Anschluss.
          Genau dafür ist das Netzwerk gebaut — du wechselst nicht allein.
        </p>
        <div className="card-grid reveal">
          {SWITCHER_CARDS.map((card) => (
            <div key={card.title} className="panel panel-corners info-card">
              <h3>{card.title}</h3>
              <p>{card.text}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function FinalCta(): JSX.Element {
  return (
    <section className="section final-cta">
      <div className="container final-inner">
        <h2 className="reveal">Werde Teil, solange die Plätze frei sind.</h2>
        <p className="lede reveal">
          Der Bot ist kostenlos, die Einrichtung dauert eine halbe Minute — und
          wenn es nichts für dich ist, bist du genauso schnell wieder draußen.
        </p>
        <div className="hero-actions reveal">
          <CtaLink>Bot in deinen Kanal holen</CtaLink>
          <a className="btn btn-ghost" href={V2_FAQ}>
            Erst Fragen klären
          </a>
        </div>
        <p className="hero-note reveal">
          Lieber erst reinschnuppern? <a href={DISCORD_INVITE_URL}>Komm in den Discord.</a>
        </p>
      </div>
    </section>
  );
}

export function LandingPage(): JSX.Element {
  const { stats, failed } = useNetworkStats();
  return (
    <Shell>
      <Hero />
      <div className="deco-divider" aria-hidden="true"><span /></div>
      <ProofStrip stats={stats} failed={failed} />
      <LiveWall stats={stats} />
      <div className="deco-divider" aria-hidden="true"><span /></div>
      <Circuit />
      <Switcher />
      <div className="deco-divider" aria-hidden="true"><span /></div>
      <FinalCta />
    </Shell>
  );
}
