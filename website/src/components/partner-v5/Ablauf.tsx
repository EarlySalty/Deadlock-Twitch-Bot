const PARTNER = ["Kanal A", "Kanal B", "Kanal C"];

function Schritt({ nummer, titel, text, children }: { nummer: string; titel: string; text: string; children: React.ReactNode }) {
  return (
    <li className="v5-schritt">
      <span className="v5-schritt-nummer" aria-hidden="true">
        {nummer}
      </span>
      <h3>{titel}</h3>
      <p>{text}</p>
      <div className="v5-kachel">{children}</div>
    </li>
  );
}

export function Ablauf() {
  return (
    <section id="ablauf" className="world-paper v5-ablauf" aria-labelledby="v5-ablauf-titel">
      <div className="world-wrap">
        <div className="v5-kopf">
          <h2 id="v5-ablauf-titel">So läuft ein Raid.</h2>
          <span className="world-stamp v5-beispiel">Beispiel, kein Live-Stand</span>
        </div>
        <ol className="v5-schritte">
          <Schritt nummer="1" titel="Du beendest deinen Stream" text="Du klickst wie immer auf Stream beenden. Mehr musst du nicht tun.">
            <p className="v5-kachel-kanal">Dein Kanal</p>
            <p className="v5-kachel-titel">Letzte Runde für heute</p>
            <p className="v5-kachel-zeile">Stream wird beendet</p>
          </Schritt>
          <Schritt nummer="2" titel="Der Bot sucht einen Partner" text="Er nimmt einen deutschen Partner, der gerade Deadlock streamt.">
            <ul className="v5-kachel-liste">
              {PARTNER.map((p, i) => (
                <li key={p} className={i === 1 ? "is-gewaehlt" : undefined}>
                  <span>{p}</span>
                  <span>{i === 1 ? "ausgewählt" : "Deadlock"}</span>
                </li>
              ))}
            </ul>
          </Schritt>
          <Schritt nummer="3" titel="Deine Zuschauer sind direkt drin" text="Sie landen in einem laufenden Stream statt vor einem schwarzen Bild.">
            <p className="v5-kachel-kanal">
              Kanal B <span className="v5-live">Live</span>
            </p>
            <p className="v5-kachel-titel">Ranked mit der Community</p>
            <p className="v5-kachel-zeile">+42 Zuschauer aus deinem Raid</p>
          </Schritt>
        </ol>
        <p className="v5-hinweis">Manuell raiden kannst du weiterhin jederzeit. Der Bot springt nur ein, wenn du es nicht tust.</p>
      </div>
    </section>
  );
}
