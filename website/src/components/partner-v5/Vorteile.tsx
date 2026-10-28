const VORTEILE = [
  {
    titel: "Auto-Raid im Netzwerk",
    text: "Endet dein Stream, landen deine Zuschauer beim passenden Live-Partner. Gehen andere offline, landen ihre bei dir.",
  },
  {
    titel: "Sichtbar ab der ersten Minute",
    text: "Du gehst live, der Discord weiß es sofort. Verschluckt Twitch das Ereignis, fragt der Bot alle 15 Sekunden selbst nach.",
  },
  {
    titel: "Deine Zahlen im Blick",
    text: "Zuschauer, Chat, Raids, Bestwerte und der Vergleich mit dem Netzwerk. Nach jedem Stream siehst du, was funktioniert hat.",
  },
  { titel: "Clips direkt aus dem Chat", text: "!clip im Chat, fertig ist der Clip." },
  {
    titel: "Eine Community im Rücken",
    text: "Treue Zuschauer werden belohnt, Lurker gezielt angesprochen. Im Streamer-Bereich im Discord tauschst du dich mit den anderen aus.",
  },
  {
    titel: "Ein Schutzschild für alle",
    text: "Scam- und Spam-Konten fliegen aus deinem Chat, bevor jemand sie sieht. Dazu die Ban-Liste des ganzen Netzwerks, alles pro Kanal abschaltbar.",
  },
];

export function Vorteile() {
  return (
    <section id="vorteile" className="world-chapter world-chapter--blue v5-vorteile" aria-labelledby="v5-vorteile-titel">
      <div className="world-wrap">
        <h2 id="v5-vorteile-titel" className="world-chapter-title">
          Was du als Partner bekommst.
        </h2>
        <div className="world-collage">
          <picture className="world-framed">
            <source type="image/avif" srcSet="/brand/world/alley-960.avif" />
            <img src="/brand/world/alley-960.webp" alt="" width={960} height={540} loading="lazy" />
          </picture>
          <div className="world-collage-copy">
            <h3>Ein Netz statt Einzelkampf.</h3>
            <p>
              Jeder Partner schickt seine Zuschauer weiter, wenn er Schluss macht. So bleibt die deutsche Deadlock-Szene auf
              Twitch zusammen, und kleine Kanäle werden gefunden.
            </p>
          </div>
        </div>
        <ol className="v5-vorteil-liste">
          {VORTEILE.map((v, i) => (
            <li key={v.titel}>
              <span className="v5-vorteil-nummer">{String(i + 1).padStart(2, "0")}</span>
              <h3>{v.titel}</h3>
              <p>{v.text}</p>
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}
