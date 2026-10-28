import { useState } from "react";
import type { NetworkStatus, NetworkStreamer } from "@/hooks/useNetworkStreamers";

const START = 12;

function Zeile({ s }: { s: NetworkStreamer }) {
  return (
    <li>
      <a href={`https://www.twitch.tv/${s.login}`} target="_blank" rel="noopener noreferrer">
        {s.avatarUrl ? <img src={s.avatarUrl} alt="" width={48} height={48} loading="lazy" /> : <span className="v5-avatar-leer" />}
        <strong>{s.displayName ?? s.login}</strong>
        <span className={s.isLive ? "v5-status is-live" : "v5-status"}>
          {s.isLive ? `Live · ${s.viewers.toLocaleString("de-DE")}` : "Offline"}
        </span>
      </a>
    </li>
  );
}

function Kopf({ streamers, status }: { streamers: NetworkStreamer[]; status: NetworkStatus }) {
  if (status === "loading") return <p className="v5-unterzeile">Partnerliste wird geladen.</p>;
  if (status === "error") return <p className="v5-unterzeile">Die Partnerliste ist gerade nicht erreichbar.</p>;
  const live = streamers.filter((s) => s.isLive).length;
  return (
    <p className="v5-unterzeile">
      {streamers.length} Partner streamen Deadlock in diesem Netzwerk, {live} davon sind gerade live.
    </p>
  );
}

export function PartnerListe({ streamers, status }: { streamers: NetworkStreamer[]; status: NetworkStatus }) {
  const [alle, setAlle] = useState(false);
  const sichtbar = alle ? streamers : streamers.slice(0, START);
  return (
    <section id="partner" className="world-index v5-partner" aria-labelledby="v5-partner-titel">
      <div className="world-wrap">
        <h2 id="v5-partner-titel">Wer schon dabei ist.</h2>
        <Kopf streamers={streamers} status={status} />
        <ul className="v5-partner-liste">
          {sichtbar.map((s) => (
            <Zeile key={s.login} s={s} />
          ))}
        </ul>
        {!alle && streamers.length > START && (
          <button type="button" className="world-button world-button--outline v5-alle" onClick={() => setAlle(true)}>
            Alle {streamers.length} anzeigen
          </button>
        )}
      </div>
    </section>
  );
}
