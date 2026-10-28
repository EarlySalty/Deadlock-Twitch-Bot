import { useNetworkStreamers } from "@/hooks/useNetworkStreamers";
import { Kopfzeile } from "@/components/partner-v5/Kopfzeile";
import { Hero } from "@/components/partner-v5/Hero";
import { Portraits } from "@/components/partner-v5/Portraits";
import { Ablauf } from "@/components/partner-v5/Ablauf";
import { Vorteile } from "@/components/partner-v5/Vorteile";
import { Schutz } from "@/components/partner-v5/Schutz";
import { PartnerListe } from "@/components/partner-v5/PartnerListe";
import { Sicherheit } from "@/components/partner-v5/Sicherheit";
import { Abschluss, Fusszeile } from "@/components/partner-v5/Abschluss";

export function StreamerNetworkV5Page() {
  const { streamers, status } = useNetworkStreamers();
  return (
    <>
      <a className="world-skip" href="#inhalt">
        Zum Inhalt
      </a>
      <Kopfzeile />
      <main id="inhalt">
        <Hero />
        <Portraits streamers={streamers} />
        <Ablauf />
        <Vorteile />
        <Schutz />
        <PartnerListe streamers={streamers} status={status} />
        <Sicherheit />
        <Abschluss />
      </main>
      <Fusszeile />
    </>
  );
}
