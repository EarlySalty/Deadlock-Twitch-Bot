import { useState } from "react";
import { ExternalLink, CheckCircle2 } from "lucide-react";
import { AnimatePresence, motion } from "framer-motion";
import { ScrollReveal } from "@/components/ui/ScrollReveal";

interface EmbeddedClip {
  id: string;
  platform: "Twitch" | "Medal";
  sourceUrl: string;
  embedUrl: string;
}

const TWITCH_PARENTS = ["deutsche-deadlock-community.de"] as const;

const CLIPS: EmbeddedClip[] = [
  {
    id: "twitch-1",
    platform: "Twitch",
    sourceUrl: "https://www.twitch.tv/xradoo_/clip/VainCoweringTigerCclamChamp-2U0MMD9D4pbWGgMK",
    embedUrl: "https://clips.twitch.tv/embed?clip=VainCoweringTigerCclamChamp-2U0MMD9D4pbWGgMK",
  },
  {
    id: "twitch-2",
    platform: "Twitch",
    sourceUrl: "https://www.twitch.tv/friduzockt/clip/GlutenFreeExuberantAlmondTwitchRaid-ZGcAGDDQOrrL39JO",
    embedUrl: "https://clips.twitch.tv/embed?clip=GlutenFreeExuberantAlmondTwitchRaid-ZGcAGDDQOrrL39JO",
  },
  {
    id: "twitch-3",
    platform: "Twitch",
    sourceUrl: "https://www.twitch.tv/v4ntr1ko/clip/TubularFrailSwallowHumbleLife-rmmgDiaxVLs9QUjF",
    embedUrl: "https://clips.twitch.tv/embed?clip=TubularFrailSwallowHumbleLife-rmmgDiaxVLs9QUjF",
  },
  {
    id: "twitch-4",
    platform: "Twitch",
    sourceUrl: "https://www.twitch.tv/miracleghost9/clip/WildFilthyWitchFeelsBadMan-drdLDEzqho4rwd_z",
    embedUrl: "https://clips.twitch.tv/embed?clip=WildFilthyWitchFeelsBadMan-drdLDEzqho4rwd_z",
  },
];

function wrapIndex(index: number, length: number) {
  return (index + length) % length;
}

function clipSrc(clip: EmbeddedClip, active: boolean) {
  if (clip.platform !== "Twitch") {
    return clip.embedUrl;
  }

  const url = new URL(clip.embedUrl);
  const parents = new Set<string>(TWITCH_PARENTS);

  if (typeof window !== "undefined" && window.location.hostname) {
    parents.add(window.location.hostname);
  }

  for (const parent of parents) {
    url.searchParams.append("parent", parent);
  }

  if (active) {
    url.searchParams.set("autoplay", "true");
    url.searchParams.set("muted", "true");
  }

  return url.toString();
}

function ClipPreview({
  clip,
  onSelect,
  side,
}: {
  clip: EmbeddedClip;
  onSelect: () => void;
  side: "left" | "right";
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-label={`${side === "left" ? "Linken" : "Rechten"} Clip auswählen`}
      className={[
        "absolute hidden md:block top-1/2 -translate-y-1/2 z-0",
        "w-[75%] aspect-video rounded-xl overflow-hidden",
        "border border-[var(--color-border)] bg-black/70",
        "opacity-70 hover:opacity-95 transition duration-300",
        side === "left"
          ? "left-0 -translate-x-1/2 shadow-[-12px_0_40px_rgba(0,0,0,0.45)]"
          : "right-0 translate-x-1/2 shadow-[12px_0_40px_rgba(0,0,0,0.45)]",
      ].join(" ")}
    >
      <iframe
        src={clipSrc(clip, false)}
        title={`${clip.platform} Vorschau`}
        className="w-full h-full border-0 pointer-events-none saturate-[0.65]"
        loading="lazy"
        allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
        referrerPolicy="strict-origin-when-cross-origin"
        allowFullScreen
      />
    </button>
  );
}

export function ClipManager() {
  const [activeIndex, setActiveIndex] = useState(0);
  const [direction, setDirection] = useState<1 | -1>(1);
  const clipCount = CLIPS.length;
  const prevIndex = wrapIndex(activeIndex - 1, clipCount);
  const nextIndex = wrapIndex(activeIndex + 1, clipCount);
  const activeClip = CLIPS[activeIndex];

  function selectClip(index: number, nextDirection: 1 | -1) {
    setDirection(nextDirection);
    setActiveIndex(index);
  }

  return (
    <section id="clips" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 lg:gap-28 items-center">
          <ScrollReveal direction="left">
            <div className="rounded-2xl border border-[var(--color-border)] bg-[linear-gradient(160deg,rgba(16,38,53,0.92),rgba(10,30,42,0.92))] overflow-visible p-1.5 md:p-2">
              <div className="relative h-[205px] sm:h-[260px] lg:h-[350px] overflow-visible">
                <AnimatePresence initial={false} custom={direction} mode="sync">
                  <motion.div
                    key={activeClip.id}
                    custom={direction}
                    initial={{ opacity: 0, x: direction > 0 ? 42 : -42, scale: 0.995 }}
                    animate={{
                      opacity: 1,
                      x: 0,
                      scale: 1,
                      transition: { duration: 0.2, ease: [0.25, 1, 0.5, 1] },
                    }}
                    exit={{
                      opacity: 0,
                      x: direction > 0 ? -42 : 42,
                      scale: 0.995,
                      transition: { duration: 0.2, ease: [0.25, 1, 0.5, 1] },
                    }}
                    className="absolute inset-0 z-10 rounded-xl overflow-hidden border border-[var(--color-border-hover)] bg-black"
                  >
                    <iframe
                      src={clipSrc(activeClip, true)}
                      title={`${activeClip.platform} Hauptclip`}
                      className="w-full h-full border-0"
                      loading="lazy"
                      allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
                      referrerPolicy="strict-origin-when-cross-origin"
                      allowFullScreen
                    />
                    <a
                      href={activeClip.sourceUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="absolute right-3 bottom-3 z-30 rounded-md px-3 py-1.5 text-xs font-semibold text-white bg-black/55 border border-white/20 inline-flex items-center gap-1 hover:bg-black/75 transition"
                    >
                      Clip öffnen
                      <ExternalLink size={13} />
                    </a>
                  </motion.div>
                </AnimatePresence>

                <ClipPreview
                  clip={CLIPS[prevIndex]}
                  side="left"
                  onSelect={() => selectClip(prevIndex, -1)}
                />
                <ClipPreview
                  clip={CLIPS[nextIndex]}
                  side="right"
                  onSelect={() => selectClip(nextIndex, 1)}
                />
              </div>

              <div className="mt-2 grid grid-cols-2 gap-2 md:hidden">
                <button
                  type="button"
                  aria-label="Linke Vorschau anzeigen"
                  onClick={() => selectClip(prevIndex, -1)}
                  className="rounded-md border border-[var(--color-border)] overflow-hidden bg-black aspect-video"
                >
                  <iframe
                    src={clipSrc(CLIPS[prevIndex], false)}
                    title="Linke Vorschau"
                    className="w-full h-full border-0 pointer-events-none saturate-[0.65]"
                    loading="lazy"
                    allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
                    referrerPolicy="strict-origin-when-cross-origin"
                    allowFullScreen
                  />
                </button>
                <button
                  type="button"
                  aria-label="Rechte Vorschau anzeigen"
                  onClick={() => selectClip(nextIndex, 1)}
                  className="rounded-md border border-[var(--color-border)] overflow-hidden bg-black aspect-video"
                >
                  <iframe
                    src={clipSrc(CLIPS[nextIndex], false)}
                    title="Rechte Vorschau"
                    className="w-full h-full border-0 pointer-events-none saturate-[0.65]"
                    loading="lazy"
                    allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
                    referrerPolicy="strict-origin-when-cross-origin"
                    allowFullScreen
                  />
                </button>
              </div>
            </div>
          </ScrollReveal>

          {/* RIGHT: Text content */}
          <ScrollReveal delay={0.2} className="lg:pl-20 xl:pl-28 2xl:pl-32">
            <p className="text-sm text-[var(--color-accent)] font-medium uppercase tracking-wider mb-4">
              Clip Manager
            </p>
            <h2 className="text-3xl md:text-4xl font-bold text-[var(--color-text-primary)] mb-6 font-display">
              Clips{" "}
              <span className="bg-gradient-to-r from-[var(--color-primary)] to-[var(--color-accent)] bg-clip-text text-transparent inline">
                automatisch
              </span>{" "}
              überall teilen
            </h2>
            <p className="text-[var(--color-text-secondary)] text-lg mb-8 leading-relaxed">
              Erstelle Clips direkt aus dem Chat und verteile sie automatisch auf allen Plattformen.
            </p>

            {/* Feature list */}
            <ul className="space-y-4">
              {[
                {
                  title: "Multi-Plattform",
                  desc: "YouTube, TikTok und Instagram mit einem Befehl",
                },
                {
                  title: "Smart Templates",
                  desc: "Automatische Titel, Beschreibungen und Hashtags",
                },
                {
                  title: "Clip-Archiv",
                  desc: "Alle Clips organisiert und durchsuchbar",
                },
              ].map((item) => (
                <li key={item.title} className="flex items-start gap-3">
                  <CheckCircle2
                    size={20}
                    className="text-[var(--color-accent)] shrink-0 mt-0.5"
                  />
                  <div>
                    <span className="text-[var(--color-text-primary)] font-semibold">
                      {item.title}
                    </span>{" "}
                    <span className="text-[var(--color-text-secondary)]">
                      — {item.desc}
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          </ScrollReveal>
        </div>
      </div>
    </section>
  );
}
