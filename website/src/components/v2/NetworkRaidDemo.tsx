import { useEffect, useMemo, useRef } from "react";
import { Eye } from "lucide-react";
import type { PartnerChannel } from "@/hooks/useNetworkMetrics";

/**
 * Das Hero-Visual der Landing V2: zwei Stream-Karten und die Übergabe dazwischen.
 *
 * Übernommen aus der produktiven Landing (`components/sections/RaidDemo`), aber
 * in drei Punkten anders:
 *
 * 1. Die Karten zeigen echte Partnernamen und Twitch-Profilbilder aus
 *    `useNetworkMetrics`, keinen fest verdrahteten Pool. Ohne API-Antwort
 *    greift ein kleiner Beispiel-Pool mit selbst gehosteten Profilbildern.
 * 2. Statt Video-Clips (die auf Produktion nicht ausgeliefert werden) tragen
 *    die Karten eine stilisierte Fläche aus dem unscharf gezogenen Profilbild.
 *    Nichts kann hier ins Leere laufen.
 * 3. Der Beispielablauf steht mit Zeitstempeln direkt in der Animation, nicht
 *    als Textliste daneben. Vier Schritte, der vierte ist die Gegenleistung.
 *
 * Bei `prefers-reduced-motion: reduce` läuft nichts: die Animation setzt sich
 * einmal in den Endzustand und bleibt stehen.
 */

const BASE = import.meta.env.BASE_URL.replace(/\/$/, "");

interface DemoChannel {
  login: string;
  displayName: string;
  viewers: number;
  avatarUrl: string;
}

/**
 * Rückfallebene, solange die Netzwerk-API nichts geliefert hat. Die Bilder
 * liegen selbst gehostet unter `public/clips/pfp/<login>.png`, sind also nicht
 * von Twitch abhängig.
 */
const FALLBACK_CHANNELS: DemoChannel[] = [
  { login: "miracleghost9", displayName: "miracleghost9", viewers: 247, avatarUrl: `${BASE}/clips/pfp/miracleghost9.png` },
  { login: "whysolowkey", displayName: "whysolowkey", viewers: 183, avatarUrl: `${BASE}/clips/pfp/whysolowkey.png` },
  { login: "kdenos", displayName: "kdenos", viewers: 312, avatarUrl: `${BASE}/clips/pfp/kdenos.png` },
  { login: "johnnyblazedx", displayName: "johnnyblazedx", viewers: 421, avatarUrl: `${BASE}/clips/pfp/johnnyblazedx.png` },
  { login: "derechtecoolys", displayName: "derechtecoolys", viewers: 158, avatarUrl: `${BASE}/clips/pfp/derechtecoolys.png` },
  { login: "duzzel", displayName: "duzzel", viewers: 534, avatarUrl: `${BASE}/clips/pfp/duzzel.png` },
];

/** Die vier Schritte des Ablaufs, so wie sie unter der Bühne stehen. */
const STEPS: { time: string; label: string }[] = [
  { time: "23:47:00", label: "Dein Stream endet" },
  { time: "23:47:01", label: "Partner wird gesucht" },
  { time: "23:47:03", label: "Zuschauer wandern rüber" },
  { time: "morgen", label: "Sie kommen zurück" },
];

// ── kleine Helfer ───────────────────────────────────────────────────────────

function fmtDuration(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/** Begrenzt den Fortschritt auf 0..1, auch wenn die Uhr springt. */
function clamp01(t: number): number {
  return t < 0 ? 0 : t > 1 ? 1 : t;
}

function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - t, 3);
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** Schreibt Text zeichenweise, bricht ab, sobald die Komponente weg ist. */
async function typewriter(
  el: HTMLElement,
  text: string,
  speed: number,
  alive: () => boolean,
): Promise<void> {
  el.textContent = "";
  for (const ch of text) {
    if (!alive()) return;
    el.textContent += ch;
    await sleep(speed);
  }
}

function animateCounter(
  from: number,
  to: number,
  duration: number,
  el: HTMLElement,
  alive: () => boolean,
): void {
  const start = performance.now();
  const tick = (now: number) => {
    if (!alive()) return;
    const t = clamp01((now - start) / duration);
    el.textContent = String(Math.round(lerp(from, to, easeOutCubic(t))));
    if (t < 1) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

/**
 * Wandelt die Partnerliste in Demo-Kanäle um: live in Deadlock zuerst, danach
 * die mit dem stärksten 30-Tage-Schnitt. Kanäle ohne brauchbare Zuschauerzahl
 * bekommen den 30-Tage-Schnitt, damit die Karte nie „0 Zuschauer" zeigt.
 */
function toDemoChannels(partners: PartnerChannel[]): DemoChannel[] {
  const usable = partners
    .filter((p) => p.avatarUrl)
    .sort((a, b) => {
      if (a.liveDeadlock !== b.liveDeadlock) return a.liveDeadlock ? -1 : 1;
      return b.avgViewers30d - a.avgViewers30d;
    })
    .slice(0, 6)
    .map((p) => ({
      login: p.login,
      displayName: p.displayName || p.login,
      viewers: Math.max(p.liveDeadlock ? p.viewers : 0, Math.round(p.avgViewers30d), 8),
      avatarUrl: p.avatarUrl as string,
    }));

  return usable.length >= 2 ? usable : FALLBACK_CHANNELS;
}

// ── Komponente ──────────────────────────────────────────────────────────────

export function NetworkRaidDemo({ partners }: { partners: PartnerChannel[] }) {
  const pool = useMemo(() => toDemoChannels(partners), [partners]);
  // Der Loop liest den Pool über eine Ref, damit ein nachgeladener Partner-
  // Stand nicht den laufenden Durchgang neu startet.
  const poolRef = useRef(pool);
  poolRef.current = pool;

  const stageRef = useRef<HTMLDivElement>(null);
  const midRef = useRef<HTMLDivElement>(null);
  const srcCardRef = useRef<HTMLDivElement>(null);
  const tgtCardRef = useRef<HTMLDivElement>(null);

  const srcArtRef = useRef<HTMLDivElement>(null);
  const srcNameRef = useRef<HTMLSpanElement>(null);
  const srcBarNameRef = useRef<HTMLAnchorElement>(null);
  const srcAvatarRef = useRef<HTMLSpanElement>(null);
  const srcViewersRef = useRef<HTMLSpanElement>(null);
  const srcDurRef = useRef<HTMLSpanElement>(null);
  const srcLiveRef = useRef<HTMLSpanElement>(null);
  const srcOfflineRef = useRef<HTMLDivElement>(null);

  const tgtArtRef = useRef<HTMLDivElement>(null);
  const tgtNameRef = useRef<HTMLSpanElement>(null);
  const tgtBarNameRef = useRef<HTMLAnchorElement>(null);
  const tgtAvatarRef = useRef<HTMLSpanElement>(null);
  const tgtViewersRef = useRef<HTMLSpanElement>(null);
  const tgtDurRef = useRef<HTMLSpanElement>(null);

  const stampRef = useRef<HTMLSpanElement>(null);
  const lineRef = useRef<HTMLDivElement>(null);
  const subRef = useRef<HTMLDivElement>(null);
  const counterRef = useRef<HTMLDivElement>(null);
  const counterNumRef = useRef<HTMLDivElement>(null);
  const beamRef = useRef<HTMLDivElement>(null);

  const stepRefs = [
    useRef<HTMLLIElement>(null),
    useRef<HTMLLIElement>(null),
    useRef<HTMLLIElement>(null),
    useRef<HTMLLIElement>(null),
  ];

  useEffect(() => {
    let running = true;
    const alive = () => running;
    let durationTimer: ReturnType<typeof setInterval> | null = null;

    const reduced =
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    // ── Karten befüllen ───────────────────────────────────────────────────
    function paintCard(
      side: "src" | "tgt",
      ch: DemoChannel,
      viewers: number,
      durationSecs: number,
    ) {
      const art = side === "src" ? srcArtRef : tgtArtRef;
      const name = side === "src" ? srcNameRef : tgtNameRef;
      const barName = side === "src" ? srcBarNameRef : tgtBarNameRef;
      const avatar = side === "src" ? srcAvatarRef : tgtAvatarRef;
      const view = side === "src" ? srcViewersRef : tgtViewersRef;
      const dur = side === "src" ? srcDurRef : tgtDurRef;

      if (art.current) art.current.style.backgroundImage = `url("${ch.avatarUrl}")`;
      if (avatar.current) avatar.current.style.backgroundImage = `url("${ch.avatarUrl}")`;
      if (name.current) name.current.textContent = ch.displayName;
      if (barName.current) {
        barName.current.textContent = ch.displayName;
        barName.current.href = `https://twitch.tv/${ch.login}`;
      }
      if (view.current) view.current.textContent = String(viewers);
      if (dur.current) dur.current.textContent = fmtDuration(durationSecs);
    }

    function startDurations(srcSecs: number, tgtSecs: number) {
      if (durationTimer) clearInterval(durationTimer);
      if (reduced) return;
      let a = srcSecs;
      let b = tgtSecs;
      durationTimer = setInterval(() => {
        if (!running) return;
        a++;
        b++;
        if (srcDurRef.current) srcDurRef.current.textContent = fmtDuration(a);
        if (tgtDurRef.current) tgtDurRef.current.textContent = fmtDuration(b);
      }, 1000);
    }

    // ── Schritt-Leiste ────────────────────────────────────────────────────
    function setStep(i: number, state: "idle" | "active" | "done") {
      const el = stepRefs[i].current;
      if (el) el.className = `v2-rd-step v2-rd-step-${state}`;
    }
    function resetSteps() {
      for (let i = 0; i < stepRefs.length; i++) setStep(i, "idle");
    }

    // ── Partikel und Konfetti ─────────────────────────────────────────────
    function spawnParticles(count: number, duration: number, back = false) {
      const container = midRef.current;
      const from = (back ? tgtCardRef : srcCardRef).current;
      const to = (back ? srcCardRef : tgtCardRef).current;
      if (!container || !from || !to) return;

      const box = container.getBoundingClientRect();
      const a = from.getBoundingClientRect();
      const b = to.getBoundingClientRect();

      for (let i = 0; i < count; i++) {
        const p = document.createElement("span");
        p.className = `v2-rd-particle${back ? " v2-rd-particle-back" : ""}`;
        const sx = a.left - box.left + a.width / 2 + (Math.random() - 0.5) * a.width * 0.6;
        const sy = a.top - box.top + a.height / 2 + (Math.random() - 0.5) * a.height * 0.5;
        const ex = b.left - box.left + b.width / 2 + (Math.random() - 0.5) * b.width * 0.6;
        const ey = b.top - box.top + b.height / 2 + (Math.random() - 0.5) * b.height * 0.5;
        p.style.left = `${sx}px`;
        p.style.top = `${sy}px`;
        container.appendChild(p);

        const delay = Math.random() * duration * 0.55;
        setTimeout(() => {
          if (!running) {
            p.remove();
            return;
          }
          const start = performance.now();
          const step = (now: number) => {
            if (!running) {
              p.remove();
              return;
            }
            const t = clamp01((now - start) / duration);
            const e = easeOutCubic(t);
            p.style.left = `${lerp(sx, ex, e)}px`;
            // Leichter Bogen, damit der Strom nicht wie ein Lineal aussieht.
            p.style.top = `${lerp(sy, ey, e) - Math.sin(e * Math.PI) * 26}px`;
            p.style.opacity = String(t < 0.12 ? t * 8 : t > 0.82 ? (1 - t) * 5.5 : 1);
            if (t < 1) requestAnimationFrame(step);
            else p.remove();
          };
          requestAnimationFrame(step);
        }, delay);
      }
    }

    function spawnConfetti(count: number) {
      const container = midRef.current;
      if (!container) return;
      const box = container.getBoundingClientRect();
      const ox = box.width / 2;
      const oy = box.height / 2;
      const colors = ["#efd49d", "#c8a86b", "#55978f", "#46c07b", "#e0912f"];

      for (let i = 0; i < count; i++) {
        const c = document.createElement("span");
        c.className = "v2-rd-confetti";
        const size = 4 + Math.random() * 5;
        const angle = Math.random() * Math.PI * 2;
        const speed = 70 + Math.random() * 110;
        const vx = Math.cos(angle) * speed;
        const vy = Math.sin(angle) * speed - 55;
        const spin = Math.random() > 0.5 ? 360 : -360;
        c.style.width = `${size}px`;
        c.style.height = `${size}px`;
        c.style.background = colors[Math.floor(Math.random() * colors.length)];
        c.style.borderRadius = Math.random() > 0.5 ? "50%" : "2px";
        c.style.left = `${ox}px`;
        c.style.top = `${oy}px`;
        container.appendChild(c);

        const start = performance.now();
        const dur = 750 + Math.random() * 550;
        const step = (now: number) => {
          if (!running) {
            c.remove();
            return;
          }
          const t = clamp01((now - start) / dur);
          c.style.left = `${ox + vx * t}px`;
          c.style.top = `${oy + vy * t + 0.5 * 210 * t * t}px`;
          c.style.opacity = String(1 - t);
          c.style.transform = `rotate(${t * spin}deg)`;
          if (t < 1) requestAnimationFrame(step);
          else c.remove();
        };
        requestAnimationFrame(step);
      }
    }

    // ── Mittelzone ────────────────────────────────────────────────────────
    function setStamp(time: string, label: string, tone: "gold" | "teal" | "good") {
      if (!stampRef.current) return;
      stampRef.current.className = `v2-rd-stamp v2-rd-stamp-${tone}`;
      stampRef.current.textContent = `${time} · ${label}`;
    }

    function clearLines() {
      if (lineRef.current) {
        lineRef.current.style.opacity = "0";
        lineRef.current.textContent = "";
      }
      if (subRef.current) {
        subRef.current.style.opacity = "0";
        subRef.current.textContent = "";
      }
    }

    function setBeam(state: "off" | "forward" | "back") {
      const el = beamRef.current;
      if (!el) return;
      if (state === "off") {
        el.style.transition = "none";
        el.style.opacity = "0";
        el.style.width = "0";
        return;
      }
      el.className = `v2-rd-beam-fill${state === "back" ? " v2-rd-beam-back" : ""}`;
      el.style.transition = "none";
      el.style.width = "0";
      el.style.opacity = "1";
      // Erzwingt einen Layout-Schritt, damit der Übergang wirklich läuft.
      void el.offsetWidth;
      el.style.transition = "width 1.4s cubic-bezier(0.4,0,0.2,1)";
      el.style.width = "100%";
    }

    // ── Ausgangslage eines Durchgangs ─────────────────────────────────────
    function pickPair(): [DemoChannel, DemoChannel] {
      const list = poolRef.current;
      const a = Math.floor(Math.random() * list.length);
      let b = Math.floor(Math.random() * list.length);
      if (b === a) b = (a + 1) % list.length;
      return [list[a], list[b]];
    }

    function setupRound(src: DemoChannel, tgt: DemoChannel) {
      paintCard("src", src, src.viewers, 3600 + Math.floor(Math.random() * 7200));
      paintCard("tgt", tgt, tgt.viewers, 900 + Math.floor(Math.random() * 3600));

      if (srcCardRef.current) srcCardRef.current.className = "v2-rd-card v2-rd-card-src";
      if (tgtCardRef.current) tgtCardRef.current.className = "v2-rd-card v2-rd-card-tgt v2-rd-card-dim";
      if (srcOfflineRef.current) srcOfflineRef.current.style.opacity = "0";
      if (srcLiveRef.current) srcLiveRef.current.style.opacity = "1";
      if (counterRef.current) counterRef.current.style.opacity = "0";
      if (counterNumRef.current) counterNumRef.current.textContent = "0";
      setBeam("off");
      clearLines();
      resetSteps();

      startDurations(
        3600 + Math.floor(Math.random() * 7200),
        900 + Math.floor(Math.random() * 3600),
      );
    }

    // ── Endzustand ohne Bewegung ──────────────────────────────────────────
    if (reduced) {
      const [src, tgt] = [poolRef.current[0], poolRef.current[1]];
      paintCard("src", src, src.viewers, 8123);
      paintCard("tgt", tgt, tgt.viewers + src.viewers, 2044);
      if (srcOfflineRef.current) srcOfflineRef.current.style.opacity = "1";
      if (srcLiveRef.current) srcLiveRef.current.style.opacity = "0";
      if (tgtCardRef.current) tgtCardRef.current.className = "v2-rd-card v2-rd-card-tgt";
      setStamp(STEPS[2].time, STEPS[2].label, "good");
      if (lineRef.current) {
        lineRef.current.style.opacity = "1";
        lineRef.current.textContent = `${src.viewers} Zuschauer bei ${tgt.displayName} angekommen`;
      }
      for (let i = 0; i < STEPS.length; i++) setStep(i, "done");
      return () => {
        running = false;
      };
    }

    // ── Durchgang ─────────────────────────────────────────────────────────
    async function runRound() {
      if (!running) return;
      const [src, tgt] = pickPair();
      setupRound(src, tgt);
      await sleep(1400);
      if (!running) return;

      // ① Stream endet
      setStep(0, "active");
      setStamp(STEPS[0].time, STEPS[0].label, "gold");
      if (lineRef.current) {
        lineRef.current.style.opacity = "1";
        await typewriter(lineRef.current, `${src.displayName} beendet den Stream`, 32, alive);
      }
      if (!running) return;
      await sleep(450);
      if (!running) return;
      if (srcOfflineRef.current) srcOfflineRef.current.style.opacity = "1";
      if (srcLiveRef.current) srcLiveRef.current.style.opacity = "0";
      if (durationTimer) clearInterval(durationTimer);
      await sleep(750);
      if (!running) return;
      setStep(0, "done");

      // ② Partner suchen
      setStep(1, "active");
      setStamp(STEPS[1].time, STEPS[1].label, "gold");
      clearLines();
      await sleep(180);
      if (!running) return;
      if (lineRef.current) {
        lineRef.current.style.opacity = "1";
        await typewriter(lineRef.current, "Suche einen aktiven Deadlock-Stream …", 28, alive);
      }
      if (!running) return;
      if (subRef.current) {
        subRef.current.style.opacity = "1";
        subRef.current.textContent = "gleiche Kategorie · gerade live · deutschsprachig";
      }
      await sleep(1100);
      if (!running) return;
      clearLines();
      await sleep(200);
      if (!running) return;
      if (tgtCardRef.current) tgtCardRef.current.className = "v2-rd-card v2-rd-card-tgt v2-rd-card-found";
      if (lineRef.current) {
        lineRef.current.style.opacity = "1";
        lineRef.current.textContent = `${tgt.displayName} gefunden`;
      }
      await sleep(700);
      if (!running) return;
      setStep(1, "done");

      // ③ Zuschauer wandern
      setStep(2, "active");
      setStamp(STEPS[2].time, STEPS[2].label, "teal");
      clearLines();
      if (lineRef.current) {
        lineRef.current.style.opacity = "1";
        lineRef.current.textContent = `zu ${tgt.displayName}`;
      }
      if (counterRef.current) counterRef.current.style.opacity = "1";
      if (counterNumRef.current) animateCounter(0, src.viewers, 1400, counterNumRef.current, alive);
      setBeam("forward");
      spawnParticles(34, 1400);
      await sleep(1650);
      if (!running) return;
      setBeam("off");
      if (tgtViewersRef.current)
        tgtViewersRef.current.textContent = String(tgt.viewers + src.viewers);
      if (tgtCardRef.current) tgtCardRef.current.className = "v2-rd-card v2-rd-card-tgt";
      if (counterRef.current) counterRef.current.style.opacity = "0";
      clearLines();
      if (lineRef.current) {
        lineRef.current.style.opacity = "1";
        lineRef.current.textContent = `+${src.viewers} Zuschauer angekommen`;
        lineRef.current.classList.add("v2-rd-line-good");
      }
      spawnConfetti(38);
      await sleep(1500);
      if (!running) return;
      setStep(2, "done");
      if (lineRef.current) lineRef.current.classList.remove("v2-rd-line-good");

      // ④ Und zurück
      setStep(3, "active");
      setStamp(STEPS[3].time, STEPS[3].label, "teal");
      clearLines();
      if (lineRef.current) {
        lineRef.current.style.opacity = "1";
        lineRef.current.textContent = "Ein anderer Stream endet, du bekommst Zuschauer zurück";
      }
      setBeam("back");
      spawnParticles(24, 1400, true);
      await sleep(1700);
      if (!running) return;
      setBeam("off");
      setStep(3, "done");
      await sleep(1900);
      if (!running) return;

      runRound();
    }

    runRound();

    return () => {
      running = false;
      if (durationTimer) clearInterval(durationTimer);
    };
    // Absichtlich einmalig: der Loop liest den Partner-Stand über poolRef.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="v2-stage">
      <div className="v2-rd" ref={stageRef}>
        <div className="v2-rd-head">
          <span className="v2-stamp">Übergabe im Netzwerk · Beispielablauf</span>
          <span className="v2-rd-head-live">
            <span className="v2-pulse h-2 w-2 rounded-full bg-[var(--color-success)]" />
            Netzwerk aktiv
          </span>
        </div>

        <div className="v2-stage-body">
          {/* Zeitachse: laeuft als Leitung neben der Buehne mit, nicht darunter. */}
          <ol className="v2-rd-steps">
            {STEPS.map((step, i) => (
              <li key={step.time} className="v2-rd-step v2-rd-step-idle" ref={stepRefs[i]}>
                <span className="v2-rd-step-time">{step.time}</span>
                <span className="v2-rd-step-label">{step.label}</span>
              </li>
            ))}
          </ol>

          <div className="v2-rd-stage">
            <div className="v2-rd-cards">
              {/* Quelle: der Kanal, der gleich offline geht */}
              <div className="v2-rd-card v2-rd-card-src" ref={srcCardRef}>
                <div className="v2-rd-screen">
                  <div className="v2-rd-art" ref={srcArtRef} aria-hidden="true" />
                  <div className="v2-screen-sheen" aria-hidden="true" />
                  <div className="v2-rd-ui">
                    <div className="v2-rd-ui-top">
                      <span className="v2-rd-live" ref={srcLiveRef}>
                        <span className="v2-rd-live-dot" />
                        LIVE
                      </span>
                      <span className="v2-rd-duration" ref={srcDurRef}>
                        2:14:07
                      </span>
                    </div>
                    <div className="v2-rd-ui-bottom">
                      <span className="v2-rd-screen-name" ref={srcNameRef}>
                        …
                      </span>
                      <span className="v2-rd-screen-meta">
                        <Eye size={12} />
                        <span ref={srcViewersRef}>0</span> Zuschauer · Deadlock
                      </span>
                    </div>
                  </div>
                  <div className="v2-rd-offline" ref={srcOfflineRef} aria-hidden="true">
                    <span className="v2-rd-offline-title">Stream beendet</span>
                    <span className="v2-rd-offline-sub">23:47 Uhr</span>
                  </div>
                </div>
                <div className="v2-rd-bar">
                  <span className="v2-rd-avatar" ref={srcAvatarRef} aria-hidden="true" />
                  <span className="v2-rd-bar-text">
                    <a
                      className="v2-rd-bar-name"
                      ref={srcBarNameRef}
                      href="https://twitch.tv/"
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      …
                    </a>
                    <span className="v2-rd-bar-sub">an dieser Stelle stehst du</span>
                  </span>
                </div>
              </div>

              {/* Ziel: der Partner, der die Zuschauer aufnimmt */}
              <div className="v2-rd-card v2-rd-card-tgt v2-rd-card-dim" ref={tgtCardRef}>
                <div className="v2-rd-screen">
                  <div className="v2-rd-art" ref={tgtArtRef} aria-hidden="true" />
                  <div className="v2-screen-sheen" aria-hidden="true" />
                  <div className="v2-rd-ui">
                    <div className="v2-rd-ui-top">
                      <span className="v2-rd-live">
                        <span className="v2-rd-live-dot" />
                        LIVE
                      </span>
                      <span className="v2-rd-duration" ref={tgtDurRef}>
                        0:41:22
                      </span>
                    </div>
                    <div className="v2-rd-ui-bottom">
                      <span className="v2-rd-screen-name" ref={tgtNameRef}>
                        …
                      </span>
                      <span className="v2-rd-screen-meta">
                        <Eye size={12} />
                        <span ref={tgtViewersRef}>0</span> Zuschauer · Deadlock
                      </span>
                    </div>
                  </div>
                </div>
                <div className="v2-rd-bar">
                  <span className="v2-rd-avatar" ref={tgtAvatarRef} aria-hidden="true" />
                  <span className="v2-rd-bar-text">
                    <a
                      className="v2-rd-bar-name"
                      ref={tgtBarNameRef}
                      href="https://twitch.tv/"
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      …
                    </a>
                    <span className="v2-rd-bar-sub">Partner im Netzwerk</span>
                  </span>
                </div>
              </div>

              {/* Kurze Strecke zwischen den Karten statt weiter Leerraum. */}
              <div className="v2-rd-fuse" aria-hidden="true" />
            </div>

            {/* Flaeche fuer Partikel und Konfetti, selbst unsichtbar. */}
            <div className="v2-rd-mid" ref={midRef} aria-hidden="true" />
          </div>
        </div>

        {/* Konsolenzeile: sagt, was gerade passiert, ohne die Karten zu verdecken. */}
        <div className="v2-stage-status">
          <span className="v2-rd-stamp v2-rd-stamp-gold" ref={stampRef} />
          <span className="v2-stage-status-text">
            <div className="v2-rd-line" ref={lineRef} />
            <div className="v2-rd-sub" ref={subRef} />
          </span>
          <div className="v2-rd-counter" ref={counterRef}>
            <div className="v2-rd-counter-num" ref={counterNumRef}>
              0
            </div>
            <div className="v2-rd-counter-label">Zuschauer unterwegs</div>
          </div>
        </div>

        <div className="v2-stage-beam" aria-hidden="true">
          <div className="v2-rd-beam-fill" ref={beamRef} />
        </div>
      </div>
    </div>
  );
}
