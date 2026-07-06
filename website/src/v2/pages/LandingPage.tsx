import type { JSX } from "react";
import { CtaLink, Shell } from "../components/Shell";
import { DISCORD_INVITE_URL, twitchChannelUrl, V2_FAQ } from "../lib/links";
import { formatCount, useNetworkStats, type NetworkStats } from "../lib/useNetworkStats";

/* ── Hero: Cityscape + Kategorie-Leaderboard ─────────────────── */

const BOARD_ROWS: Array<{ rank: string; width: number; viewers: string; you?: boolean }> = [
  { rank: "1", width: 92, viewers: "214" },
  { rank: "2", width: 70, viewers: "121" },
  { rank: "3", width: 52, viewers: "63", you: true },
  { rank: "4", width: 38, viewers: "41" },
  { rank: "5", width: 27, viewers: "22" },
];

function Leaderboard(): JSX.Element {
  return (
    <aside className="board" aria-label="Stilisierte Deadlock-Kategorie">
      <div className="board-title">
        <span>Kategorie</span>
        <b>Deadlock · DE</b>
      </div>
      <div className="board-rows">
        {BOARD_ROWS.map((row) => (
          <div key={row.rank} className={row.you ? "board-row is-you" : "board-row"}>
            <span className="board-rank">{row.rank}</span>
            {row.you ? (
              <span className="board-you-label">
                <span className="live-dot" aria-hidden="true" /> Du
              </span>
            ) : (
              <span className="board-bar" style={{ width: `${row.width}%` }} aria-hidden="true" />
            )}
            <span className="board-viewers">{row.viewers} Viewer</span>
          </div>
        ))}
      </div>
      <p className="board-caption">Deine Zahlen. Endlich sichtbar.</p>
    </aside>
  );
}

function Hero(): JSX.Element {
  return (
    <section className="hero">
      <div className="hero-art" aria-hidden="true" />
      <div className="hero-shade" aria-hidden="true" />
      <div className="container hero-inner">
        <div className="stagger">
          <p className="overline">Deutsches Deadlock-Partnernetzwerk</p>
          <h1>
            Die Kategorie wird gerade verteilt.{" "}
            <span className="gold">Oben ist noch frei.</span>
          </h1>
          <p className="lede">
            Dieselben Viewer, die dich woanders unsichtbar machen, bringen dich
            in Deadlock nach oben. Der Bot ist dein Mitgliedsausweis.
          </p>
          <div className="hero-actions">
            <CtaLink>Bot in deinen Kanal holen</CtaLink>
            <a className="btn btn-ghost" href="#netzwerk">
              So funktioniert&rsquo;s
            </a>
          </div>
          <p className="hero-note">Kostenlos. 30 Sekunden. Jederzeit raus.</p>
        </div>
        <Leaderboard />
      </div>
      <a className="scroll-cue" href="#signal" aria-label="Weiter scrollen">
        ▼
      </a>
    </section>
  );
}

/* ── Signal-Strip: echte Zahlen als Band ─────────────────────── */

function SignalStrip({ stats }: { stats: NetworkStats | null }): JSX.Element {
  return (
    <div className="signal-strip" id="signal" aria-label="Live-Netzwerkzahlen">
      <div>
        <span className="live-dot" aria-hidden="true" />
        <b>{formatCount(stats?.live.length)}</b> gerade live
      </div>
      <div>
        <b>{formatCount(stats?.active_partners)}</b> Partner
      </div>
      <div>
        <b>{formatCount(stats?.raids_total)}</b> Raids vermittelt
      </div>
      <div>
        <b>{formatCount(stats?.raids_7d)}</b> diese Woche
      </div>
    </div>
  );
}

/* ── Raid-Kreislauf: animierter Ring ─────────────────────────── */

function RaidRing(): JSX.Element {
  // Kreis r=44 bei viewBox 100 → Umfang ~276; dasharray/offset in ui.css darauf abgestimmt.
  return (
    <div className="ring-wrap" aria-hidden="true">
      <svg className="ring-svg" viewBox="0 0 100 100">
        <circle className="ring-track" cx="50" cy="50" r="44" />
        <circle className="ring-flow" cx="50" cy="50" r="44" />
        <g>
          <circle className="ring-node" cx="50" cy="6" r="7" />
          <text className="ring-node-label" x="50" y="6.4">1</text>
          <circle className="ring-node" cx="88" cy="72" r="7" />
          <text className="ring-node-label" x="88" y="72.4">2</text>
          <circle className="ring-node" cx="12" cy="72" r="7" />
          <text className="ring-node-label" x="12" y="72.4">3</text>
        </g>
        <text className="ring-center-label" x="50" y="47">
          DAS NETZWERK
        </text>
        <text className="ring-center-live" x="50" y="58">
          LÄUFT 24/7
        </text>
      </svg>
    </div>
  );
}

const STEPS: Array<{ title: string; text: string }> = [
  { title: "Du streamst Deadlock", text: "Der Bot kennt alle, die gerade live sind." },
  { title: "Du machst Feierabend", text: "Dein Stream raidet automatisch den passenden Partner." },
  { title: "Der Kreislauf dreht sich", text: "Hört ein anderer auf, landen seine Viewer bei dir." },
];

function Circuit(): JSX.Element {
  return (
    <section className="section" id="netzwerk">
      <div className="container">
        <div className="section-head reveal">
          <p className="overline">So funktioniert es</p>
          <h2>Kein Stream endet im Nichts.</h2>
        </div>
        <div className="circuit-grid">
          <RaidRing />
          <ol className="step-list">
            {STEPS.map((step, i) => (
              <li key={step.title} className="reveal">
                <span className="step-jewel" aria-hidden="true">
                  <i>{i + 1}</i>
                </span>
                <div>
                  <h3>{step.title}</h3>
                  <p>{step.text}</p>
                </div>
              </li>
            ))}
          </ol>
        </div>
        <p className="honest-line reveal">
          Wir machen dich nicht groß. Wir sorgen dafür, dass nichts verpufft.
        </p>
      </div>
    </section>
  );
}

/* ── Umsteiger: Kategorie-Vergleich ──────────────────────────── */

function Switcher(): JSX.Element {
  return (
    <section className="section">
      <div className="container">
        <div className="section-head reveal">
          <p className="overline">Für Wechsler</p>
          <h2>
            Gleiche Viewer. <span className="gold">Andere Liga.</span>
          </h2>
          <p className="lede">
            Ein Game-Wechsel kostet normalerweise alles. Im Netzwerk wechselst
            du nicht allein.
          </p>
        </div>
        <div className="compare-grid reveal">
          <div className="cat-card is-lose">
            <div className="cat-head">
              <span className="cat-name">Großes Game</span>
              <span className="cat-meta">4.000+ Streams</span>
            </div>
            <div className="cat-rows">
              <div className="cat-row"><span>#1</span><span className="board-bar" style={{ width: "95%" }} /></div>
              <div className="cat-row"><span>#2</span><span className="board-bar" style={{ width: "80%" }} /></div>
              <div className="cat-gap">· · ·</div>
              <div className="cat-row is-you-row"><span>#4.083</span><span className="board-bar" style={{ width: "7%" }} /></div>
            </div>
            <p className="cat-verdict">Niemand scrollt zu dir</p>
          </div>
          <div className="cat-card is-win">
            <div className="cat-head">
              <span className="cat-name">Deadlock · DE</span>
              <span className="cat-meta">Kategorie im Aufbau</span>
            </div>
            <div className="cat-rows">
              <div className="cat-row"><span>#1</span><span className="board-bar" style={{ width: "85%" }} /></div>
              <div className="cat-row"><span>#2</span><span className="board-bar" style={{ width: "62%" }} /></div>
              <div className="cat-row is-you-row"><span>#3</span><span className="board-bar" style={{ width: "48%" }} /></div>
              <div className="cat-row"><span>#4</span><span className="board-bar" style={{ width: "30%" }} /></div>
            </div>
            <p className="cat-verdict">Erste Seite der Kategorie</p>
          </div>
        </div>
        <div className="switcher-points reveal">
          <span>Raids ab deinem ersten Stream</span>
          <span>Die Szene kennt dich, bevor du ankommst</span>
          <span>Coaching &amp; Scrims, wenn du willst</span>
        </div>
      </div>
    </section>
  );
}

/* ── Live-Wall ───────────────────────────────────────────────── */

function LiveWall({ stats }: { stats: NetworkStats | null }): JSX.Element {
  const live = stats?.live ?? [];
  return (
    <section className="section" aria-label="Gerade live">
      <div className="container">
        <div className="section-head reveal">
          <p className="overline">Das Netzwerk, jetzt gerade</p>
          <h2>Gerade live</h2>
        </div>
        {live.length ? (
          <div className="live-grid reveal">
            {live.map((s) => (
              <a
                key={s.login}
                className="live-card"
                href={twitchChannelUrl(s.login)}
                target="_blank"
                rel="noreferrer"
              >
                <span className="monogram" aria-hidden="true">
                  <i>{(s.display_name || s.login).charAt(0).toUpperCase()}</i>
                </span>
                <span className="live-name">{s.display_name}</span>
                <span className="live-badge">
                  <span className="live-dot" aria-hidden="true" /> LIVE
                </span>
                <span className="live-login">twitch.tv/{s.login}</span>
              </a>
            ))}
          </div>
        ) : (
          <p className="live-empty reveal">
            Gerade streamt niemand aus dem Netzwerk. Sei der, der online ist,
            wenn andere zuschauen wollen.
          </p>
        )}
        <p className="wall-note reveal">
          Jeder Partner landet hier automatisch, sobald er live geht.
        </p>
      </div>
    </section>
  );
}

/* ── Finale: CTA-Band auf der Bühne ──────────────────────────── */

function FinalCta(): JSX.Element {
  return (
    <section className="cta-band">
      <div className="cta-band-art" aria-hidden="true" />
      <div className="container">
        <h2 className="reveal">
          Werde Teil, <span className="gold">solange oben frei ist.</span>
        </h2>
        <p className="lede reveal">
          Kostenlos, in 30 Sekunden drin — und genauso schnell wieder raus.
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
  const { stats } = useNetworkStats();
  return (
    <Shell>
      <Hero />
      <SignalStrip stats={stats} />
      <Circuit />
      <div className="deco-divider" aria-hidden="true"><span /></div>
      <Switcher />
      <div className="deco-divider" aria-hidden="true"><span /></div>
      <LiveWall stats={stats} />
      <FinalCta />
    </Shell>
  );
}
