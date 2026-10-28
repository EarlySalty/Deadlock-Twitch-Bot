import { TWITCH_SECURITY_URL } from "@/data/externalLinks";

const FAKTEN = [
  {
    titel: "Keine Broadcaster-Rechte.",
    text: "Der Bot moderiert, mehr nicht. Streamtitel, Kategorie oder Kanal-Einstellungen kann er technisch gar nicht anfassen.",
  },
  {
    titel: "Zugang einzeln verschlüsselt.",
    text: "Deine Zugangsdaten liegen feldweise mit AES-256-GCM verschlüsselt, der Schlüssel getrennt davon. Selbst ein Datenbank-Leak gibt sie nicht preis.",
  },
  {
    titel: "Jederzeit widerrufbar.",
    text: "Ein Klick in deinen Twitch-Einstellungen entzieht dem Bot den Zugriff. Er erkennt das und schaltet sich für deinen Kanal selbst ab.",
  },
  {
    titel: "Schutz in mehreren Schichten.",
    text: "Doppelte Firewall, Zwei-Faktor auf allen Konten, verschlüsselter Secret-Tresor und rund-um-die-Uhr-Überwachung mit Sofort-Alarm.",
  },
];

export function Sicherheit() {
  return (
    <section id="sicherheit" className="world-paper v5-sicherheit" aria-labelledby="v5-sicherheit-titel">
      <div className="world-wrap world-editorial">
        <div>
          <h2 id="v5-sicherheit-titel">Du gibst dem Bot Mod-Rechte.</h2>
          <p>
            Fair, dass du genau weißt, was wir damit tun und was wir bewusst nicht können. Hier ohne Marketing, einfach wie es
            ist.
          </p>
          <a className="world-link" href={TWITCH_SECURITY_URL}>
            Ganzes Sicherheitskonzept lesen
          </a>
        </div>
        <div>
          {FAKTEN.map((f) => (
            <p key={f.titel}>
              <strong>{f.titel}</strong> {f.text}
            </p>
          ))}
        </div>
      </div>
    </section>
  );
}
