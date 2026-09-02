import { Check, Radio, Scissors } from "lucide-react";

/**
 * Die vier Pfeiler-Visuals.
 *
 * Jeder Pfeiler bekommt ein eigenes Bild statt einer weiteren Textkarte:
 * das Netz fuer die Raids, der mitlaufende Chat fuer den Schutz, der
 * Zuschauerverlauf fuer die Auswertung, die geschnittenen Clips fuer die
 * Clips. Alle vier arbeiten mit denselben Bausteinen aus streamer-v2.css
 * (`v2-hub`, `v2-link`, `v2-feed-*`, `v2-bar`, `v2-peak-*`, `v2-clip-*`),
 * damit die Kacheln als Familie lesbar sind und nicht als vier Einzelstuecke.
 *
 * Es steht nichts drin, was der Bot nicht kann. Namen im Chat-Feed sind
 * erkennbar erfunden, die Kurve ist ein Verlauf ohne Achsenbeschriftung.
 * Bewegung stoppt komplett bei `prefers-reduced-motion: reduce`.
 */

/** ① Auto-Raid: dein Kanal in der Mitte, Partner rundherum. */
function RaidVisual() {
  return (
    <div className="v2-visual flex h-[17rem] items-center justify-center p-6">
      <div className="relative grid w-full max-w-[19rem] grid-cols-3 place-items-center gap-y-7">
        {/* Verbindungen liegen unter den Knoten */}
        <div className="v2-link v2-link-h" style={{ top: "50%" }} aria-hidden="true" />
        <div className="v2-link v2-link-v" aria-hidden="true" />

        <span className="v2-hub v2-hub-teal relative z-10 !h-10 !w-10 text-[0.6rem] text-[var(--color-accent-hover)]">
          <Radio size={14} />
        </span>
        <span className="col-start-2 row-start-1 h-2 w-2 rounded-full bg-[rgba(201,168,106,0.45)]" />
        <span className="v2-hub v2-hub-teal relative z-10 !h-10 !w-10 text-[var(--color-accent-hover)]">
          <Radio size={14} />
        </span>

        <span className="col-start-1 row-start-2 h-2 w-2 rounded-full bg-[rgba(201,168,106,0.45)]" />
        <span className="v2-hub v2-hub-live relative z-10 col-start-2 row-start-2 text-[var(--color-primary-hover)]">
          <span className="text-[0.58rem] font-bold tracking-wider">DU</span>
        </span>
        <span className="col-start-3 row-start-2 h-2 w-2 rounded-full bg-[rgba(85,151,143,0.45)]" />

        <span className="v2-hub v2-hub-teal relative z-10 !h-10 !w-10 text-[var(--color-accent-hover)]">
          <Radio size={14} />
        </span>
        <span className="col-start-2 row-start-3 h-2 w-2 rounded-full bg-[rgba(85,151,143,0.45)]" />
        <span className="v2-hub v2-hub-teal relative z-10 !h-10 !w-10 text-[var(--color-accent-hover)]">
          <Radio size={14} />
        </span>
      </div>
    </div>
  );
}

/** ② Chat-Schutz: was durchgeht und was hängen bleibt. */
function ProtectionVisual() {
  const rows = [
    { who: "lisa_2k26", text: "gl hf leute", kind: "calm" as const },
    { who: "free_prime_gg", text: "buy viewers cheap → bit.ly/…", kind: "ban" as const },
    { who: "nordwind", text: "gönn dir, sitz nur im lurk", kind: "calm" as const },
    { who: "gift_bot_4471", text: "check my channel 100 folgers", kind: "ban" as const },
    { who: "murmelmann", text: "der ult war eklig gut", kind: "calm" as const },
  ];

  return (
    <div className="v2-visual relative flex h-[17rem] flex-col justify-center gap-3 p-6">
      <span className="v2-stamp v2-stamp-dim absolute right-4 top-3">Beispiel</span>
      {rows.map((row) => (
        <div key={row.who} className="v2-feed-row">
          <span
            className={`v2-feed-dot ${
              row.kind === "ban" ? "v2-feed-dot-ban" : "v2-feed-dot-calm"
            }`}
          />
          <span
            className={`v2-feed-who ${
              row.kind === "ban" ? "v2-feed-who-ban" : "v2-feed-who-calm"
            }`}
          >
            {row.who}
          </span>
          <span className="v2-feed-text">{row.text}</span>
          <span
            className={`v2-stamp v2-stamp-dim shrink-0 ${
              row.kind === "ban" ? "!text-[rgba(235,4,0,0.75)]" : ""
            }`}
          >
            {row.kind === "ban" ? "raus" : "ok"}
          </span>
        </div>
      ))}
    </div>
  );
}

/** ③ Auswertung: der Abend als Kurve, mit markiertem bestem Moment. */
function InsightVisual() {
  // Ein Abend in 22 Schritten. Fester Verlauf, damit die Kachel bei jedem
  // Aufruf gleich aussieht und niemand sie fuer Messwerte haelt.
  const bars = [
    12, 18, 26, 31, 29, 38, 46, 51, 47, 58, 72, 91, 78, 63, 57, 61, 54, 44, 36,
    28, 19, 13,
  ];
  const peak = bars.indexOf(Math.max(...bars));

  return (
    <div className="v2-visual flex h-[17rem] flex-col justify-end p-6">
      <div className="relative flex h-[7.5rem] items-end gap-[3px]">
        {bars.map((value, i) => (
          <span
            key={i}
            className="v2-bar flex-1"
            style={{
              height: `${value}%`,
              animationDelay: `${(i % 7) * 0.28}s`,
              opacity: i === peak ? 1 : 0.72,
            }}
          />
        ))}

        <span
          className="v2-peak"
          style={{ left: `${((peak + 0.5) / bars.length) * 100}%` }}
          aria-hidden="true"
        >
          <span className="v2-peak-line" />
          <span className="v2-peak-dot" />
          <span className="v2-peak-label">bester Moment</span>
        </span>
      </div>
      <div className="mt-3 flex items-center justify-between">
        <span className="v2-stamp v2-stamp-dim">Streamstart</span>
        <span className="v2-stamp v2-stamp-dim">Beispiel</span>
        <span className="v2-stamp v2-stamp-dim">Streamende</span>
      </div>
    </div>
  );
}

/** ④ Clips: drei Ausschnitte, einer wird gerade geschnitten. */
function ClipVisual() {
  const clips = [
    { time: "0:14", state: "done" as const },
    { time: "0:22", state: "scan" as const },
    { time: "0:09", state: "queued" as const },
  ];

  return (
    <div className="v2-visual relative flex h-[17rem] flex-col justify-center gap-4 p-6">
      <span className="v2-stamp v2-stamp-dim absolute right-4 top-3">Beispiel</span>
      <div className="grid grid-cols-3 gap-3">
        {clips.map((clip) => (
          <div
            key={clip.time}
            className={`v2-clip aspect-[3/4] ${
              clip.state === "done"
                ? "v2-clip-done"
                : clip.state === "scan"
                  ? "v2-clip-scan"
                  : ""
            }`}
          >
            <span className="v2-clip-cut" />
            <span className="v2-clip-time">{clip.time}</span>
            {clip.state === "done" ? (
              <span className="v2-clip-badge">
                <Check size={8} />
                fertig
              </span>
            ) : null}
          </div>
        ))}
      </div>
      <div className="flex items-center gap-2 text-xs text-[var(--color-text-secondary)]">
        <Scissors size={13} className="text-[var(--color-primary)]" />
        Chat ging hoch, Ausschnitt liegt im Dashboard
      </div>
    </div>
  );
}

/** Ordnet jedem Pfeiler sein Bild zu. Unbekannte id bleibt ohne Visual. */
export function PillarVisual({ id }: { id: string }) {
  switch (id) {
    case "raids":
      return <RaidVisual />;
    case "schutz":
      return <ProtectionVisual />;
    case "coaching":
      return <InsightVisual />;
    case "clips":
      return <ClipVisual />;
    default:
      return null;
  }
}
