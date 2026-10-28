import { useEffect, useState } from "react";

const BAN_API = "https://deutsche-deadlock-community.de/twitch/api/v2/public/recent-bans";

interface BanStats {
  heute: number;
  monat: number;
  kanaele: number;
}

function leseStats(data: unknown): BanStats | null {
  const s = (data as { stats?: Record<string, unknown> } | null)?.stats;
  if (!s) return null;
  const { today, total_30d, channels_protected } = s;
  if (typeof today !== "number" || typeof total_30d !== "number" || typeof channels_protected !== "number") return null;
  return { heute: today, monat: total_30d, kanaele: channels_protected };
}

function useBanStats() {
  const [stats, setStats] = useState<BanStats | null>(null);
  useEffect(() => {
    let aktiv = true;
    fetch(BAN_API)
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => {
        if (aktiv) setStats(leseStats(d));
      })
      .catch(() => {});
    return () => {
      aktiv = false;
    };
  }, []);
  return stats;
}

function zahl(wert: number | undefined) {
  return wert === undefined ? "–" : wert.toLocaleString("de-DE");
}

export function Schutz() {
  const stats = useBanStats();
  return (
    <section id="schutz" className="world-chapter world-chapter--rust v5-schutz" aria-labelledby="v5-schutz-titel">
      <div className="world-wrap">
        <h2 id="v5-schutz-titel" className="world-chapter-title">
          <span className="world-stamp">Dein Chat bleibt sauber.</span>
        </h2>
        <div className="world-collage world-collage--reverse">
          <picture className="world-framed">
            <source type="image/avif" srcSet="/brand/world/city-960.avif" />
            <img src="/brand/world/city-960.webp" alt="" width={960} height={540} loading="lazy" />
          </picture>
          <div className="world-collage-copy">
            <h3>Spam fliegt raus.</h3>
            <p>
              Bekannte Scam- und Spam-Konten bannt der Bot, bevor sie in deinem Chat etwas schreiben. Wer in einem
              Partner-Kanal auffällt, ist im ganzen Netzwerk raus.
            </p>
            <dl className="v5-zahlen">
              <div>
                <dt>Bans heute</dt>
                <dd>{zahl(stats?.heute)}</dd>
              </div>
              <div>
                <dt>Letzte 30 Tage</dt>
                <dd>{zahl(stats?.monat)}</dd>
              </div>
              <div>
                <dt>Geschützte Kanäle</dt>
                <dd>{zahl(stats?.kanaele)}</dd>
              </div>
            </dl>
          </div>
        </div>
      </div>
    </section>
  );
}
