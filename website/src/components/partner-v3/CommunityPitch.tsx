import { useState } from "react";
import { Check, Link as LinkIcon } from "lucide-react";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { DiscordLogo } from "@/components/ui/DiscordLogo";
import { DISCORD_INVITE_URL, EARLYSALTY_WEBSITE_URL } from "@/data/externalLinks";

const STREAMER_LINK = EARLYSALTY_WEBSITE_URL;

type KopierStatus = "leer" | "ok" | "fehler";

export function CommunityPitch() {
  const [kopierStatus, setKopierStatus] = useState<KopierStatus>("leer");

  async function linkKopieren() {
    try {
      await navigator.clipboard.writeText(STREAMER_LINK);
      setKopierStatus("ok");
      window.setTimeout(() => setKopierStatus("leer"), 2500);
    } catch {
      setKopierStatus("fehler");
    }
  }

  return (
    <section id="community" className="py-24">
      <div className="max-w-4xl mx-auto px-6">
        <SectionHeading
          badge="Community"
          title="Du streamst nicht selbst?"
          subtitle="Hol deine Lieblings-Streamer in das Netz: schick ihnen den Link zur Seite oder sag im Discord Bescheid."
        />

        <div className="mt-12 grid grid-cols-1 md:grid-cols-2 gap-6">
          <ScrollReveal>
            <div className="panel-card rounded-2xl p-7 h-full text-center soft-elevate">
              <h3 className="text-lg font-semibold text-[var(--color-text-primary)]">
                Den Link weitergeben
              </h3>
              <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
                {STREAMER_LINK}
              </p>
              <button
                type="button"
                onClick={linkKopieren}
                className="mt-5 gradient-accent rounded-xl px-6 py-3 font-semibold inline-flex items-center gap-2 transition-all duration-200 hover:brightness-110 hover:shadow-[0_0_24px_4px_rgba(201,168,106,0.3)]"
              >
                {kopierStatus === "ok" ? (
                  <>
                    <Check size={17} />
                    Link kopiert
                  </>
                ) : (
                  <>
                    <LinkIcon size={17} />
                    Link kopieren
                  </>
                )}
              </button>
              {kopierStatus === "fehler" ? (
                <p className="mt-3 text-xs text-[var(--color-text-secondary)]">
                  Kopieren geht hier nicht. Markiere den Link oben von Hand.
                </p>
              ) : null}
            </div>
          </ScrollReveal>

          <ScrollReveal delay={0.1}>
            <div className="panel-card rounded-2xl p-7 h-full text-center soft-elevate">
              <h3 className="text-lg font-semibold text-[var(--color-text-primary)]">
                Im Discord Bescheid geben
              </h3>
              <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
                Im Streamer-Bereich des Community-Discords erreichen dich die
                Partner direkt.
              </p>
              <a
                href={DISCORD_INVITE_URL}
                target="_blank"
                rel="noopener noreferrer"
                className="mt-5 rounded-xl px-6 py-3 font-semibold text-white inline-flex items-center gap-2 transition-opacity duration-200 hover:opacity-90"
                style={{ background: "#5865F2" }}
              >
                <DiscordLogo size={19} className="text-white" />
                Community-Discord öffnen
              </a>
            </div>
          </ScrollReveal>
        </div>
      </div>
    </section>
  );
}
