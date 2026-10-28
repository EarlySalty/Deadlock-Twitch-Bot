import type { NetworkStreamer } from "@/hooks/useNetworkStreamers";

function name(s: NetworkStreamer) {
  return s.displayName ?? s.login;
}

export function Portraits({ streamers }: { streamers: NetworkStreamer[] }) {
  const auswahl = streamers.filter((s) => s.avatarUrl).slice(0, 6);
  if (auswahl.length < 3) return <div className="v5-portraits-platz" aria-hidden="true" />;
  return (
    <div className="world-portraits v5-portraits" aria-label="Partner im Netzwerk">
      {auswahl.map((s) => (
        <a key={s.login} href={`https://www.twitch.tv/${s.login}`} target="_blank" rel="noopener noreferrer">
          <img src={s.avatarUrl} alt="" width={300} height={300} loading="lazy" />
          <span>{name(s)}</span>
        </a>
      ))}
    </div>
  );
}
