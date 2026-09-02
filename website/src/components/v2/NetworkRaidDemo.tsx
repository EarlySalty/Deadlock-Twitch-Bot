import { useEffect, useMemo, useRef } from "react";
import { Eye, Play } from "lucide-react";
import type { PartnerChannel } from "@/hooks/useNetworkMetrics";

interface DemoChannel {
  login: string;
  displayName: string;
  viewers: number;
  avatarUrl?: string;
  video?: string;
  poster?: string;
  pfp?: string;
  sample: boolean;
}

const BASE = import.meta.env.BASE_URL.replace(/\/$/, "");

const CLIP_POOL: DemoChannel[] = [
  { login: "miracleghost9", displayName: "miracleghost9", viewers: 247, video: `${BASE}/clips/miracleghost9.mp4`, poster: `${BASE}/clips/poster/miracleghost9.jpg`, pfp: `${BASE}/clips/pfp/miracleghost9.png`, sample: true },
  { login: "whysolowkey", displayName: "whysolowkey", viewers: 183, video: `${BASE}/clips/whysolowkey.mp4`, poster: `${BASE}/clips/poster/whysolowkey.jpg`, pfp: `${BASE}/clips/pfp/whysolowkey.png`, sample: true },
  { login: "kdenos", displayName: "kdenos", viewers: 312, video: `${BASE}/clips/kdenos.mp4`, poster: `${BASE}/clips/poster/kdenos.jpg`, pfp: `${BASE}/clips/pfp/kdenos.png`, sample: true },
  { login: "johnnyblazedx", displayName: "johnnyblazedx", viewers: 421, video: `${BASE}/clips/johnnyblazedx.mp4`, poster: `${BASE}/clips/poster/johnnyblazedx.jpg`, pfp: `${BASE}/clips/pfp/johnnyblazedx.png`, sample: true },
  { login: "coolysdl", displayName: "coolysdl", viewers: 158, video: `${BASE}/clips/coolysdl.mp4`, poster: `${BASE}/clips/poster/coolysdl.jpg`, pfp: `${BASE}/clips/pfp/coolysdl.png`, sample: true },
  { login: "duzzel", displayName: "duzzel", viewers: 534, video: `${BASE}/clips/duzzel.mp4`, poster: `${BASE}/clips/poster/duzzel.jpg`, pfp: `${BASE}/clips/pfp/duzzel.png`, sample: true },
];

const STEPS: { time: string; label: string }[] = [
  { time: "23:47:00", label: "Dein Stream endet" },
  { time: "23:47:01", label: "Partner wird gesucht" },
  { time: "23:47:03", label: "Zuschauer wandern rüber" },
  { time: "morgen", label: "Sie kommen zurück" },
];

function fmtDuration(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function clamp01(t: number): number {
  return t < 0 ? 0 : t > 1 ? 1 : t;
}

function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - t, 3);
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

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

function toDemoChannels(partners: PartnerChannel[]): DemoChannel[] {
  const usable = partners
    .filter((p) => p.avatarUrl && p.liveDeadlock && p.viewers > 0)
    .sort((a, b) => {
      if (a.liveDeadlock !== b.liveDeadlock) return a.liveDeadlock ? -1 : 1;
      return b.avgViewers30d - a.avgViewers30d;
    })
    .slice(0, 6)
    .map((p) => ({
      login: p.login,
      displayName: p.displayName || p.login,
      viewers: p.viewers,
      avatarUrl: p.avatarUrl as string,
      sample: false,
    }));

  return usable.length >= 2 ? usable : CLIP_POOL;
}

export function NetworkRaidDemo({ partners }: { partners: PartnerChannel[] }) {
  const pool = useMemo(() => toDemoChannels(partners), [partners]);
  const poolRef = useRef(pool);
  poolRef.current = pool;

  const midRef = useRef<HTMLDivElement>(null);
  const srcCardRef = useRef<HTMLDivElement>(null);
  const tgtCardRef = useRef<HTMLDivElement>(null);

  const srcScreenRef = useRef<HTMLDivElement>(null);
  const srcVideoRef = useRef<HTMLVideoElement>(null);
  const srcArtRef = useRef<HTMLDivElement>(null);
  const srcNameRef = useRef<HTMLSpanElement>(null);
  const srcBarNameRef = useRef<HTMLAnchorElement>(null);
  const srcBarSubRef = useRef<HTMLSpanElement>(null);
  const srcAvatarRef = useRef<HTMLSpanElement>(null);
  const srcViewersRef = useRef<HTMLSpanElement>(null);
  const srcMetaRef = useRef<HTMLSpanElement>(null);
  const srcDurRef = useRef<HTMLSpanElement>(null);
  const srcLiveRef = useRef<HTMLSpanElement>(null);
  const srcBadgeTextRef = useRef<HTMLSpanElement>(null);
  const srcOfflineRef = useRef<HTMLDivElement>(null);

  const tgtScreenRef = useRef<HTMLDivElement>(null);
  const tgtVideoRef = useRef<HTMLVideoElement>(null);
  const tgtArtRef = useRef<HTMLDivElement>(null);
  const tgtNameRef = useRef<HTMLSpanElement>(null);
  const tgtBarNameRef = useRef<HTMLAnchorElement>(null);
  const tgtBarSubRef = useRef<HTMLSpanElement>(null);
  const tgtAvatarRef = useRef<HTMLSpanElement>(null);
  const tgtViewersRef = useRef<HTMLSpanElement>(null);
  const tgtMetaRef = useRef<HTMLSpanElement>(null);
  const tgtDurRef = useRef<HTMLSpanElement>(null);
  const tgtLiveRef = useRef<HTMLSpanElement>(null);
  const tgtBadgeTextRef = useRef<HTMLSpanElement>(null);

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

    function markPaused(side: "src" | "tgt", paused: boolean) {
      const screen = side === "src" ? srcScreenRef : tgtScreenRef;
      if (screen.current) screen.current.classList.toggle("v2-rd-screen-paused", paused);
    }

    function paintCard(
      side: "src" | "tgt",
      ch: DemoChannel,
      viewers: number,
      durationSecs: number,
    ) {
      const video = side === "src" ? srcVideoRef : tgtVideoRef;
      const art = side === "src" ? srcArtRef : tgtArtRef;
      const name = side === "src" ? srcNameRef : tgtNameRef;
      const barName = side === "src" ? srcBarNameRef : tgtBarNameRef;
      const barSub = side === "src" ? srcBarSubRef : tgtBarSubRef;
      const avatar = side === "src" ? srcAvatarRef : tgtAvatarRef;
      const view = side === "src" ? srcViewersRef : tgtViewersRef;
      const meta = side === "src" ? srcMetaRef : tgtMetaRef;
      const dur = side === "src" ? srcDurRef : tgtDurRef;
      const live = side === "src" ? srcLiveRef : tgtLiveRef;
      const badgeText = side === "src" ? srcBadgeTextRef : tgtBadgeTextRef;

      if (ch.video && video.current) {
        if (art.current) art.current.style.backgroundImage = "none";
        const v = video.current;
        v.style.display = "";
        if (v.getAttribute("src") !== ch.video) {
          if (ch.poster) v.poster = ch.poster;
          v.src = ch.video;
          v.load();
        }
        if (reduced) {
          markPaused(side, true);
        } else {
          const p = v.play();
          if (p && typeof p.then === "function") {
            p.then(() => markPaused(side, false)).catch(() => markPaused(side, true));
          } else {
            markPaused(side, false);
          }
        }
      } else {
        if (video.current) {
          video.current.style.display = "none";
          video.current.removeAttribute("src");
          video.current.load();
        }
        if (art.current)
          art.current.style.backgroundImage = ch.avatarUrl ? `url("${ch.avatarUrl}")` : "none";
        markPaused(side, false);
      }

      const avatarBg = ch.avatarUrl
        ? `url("${ch.avatarUrl}")`
        : ch.pfp
          ? `url("${ch.pfp}")`
          : "none";
      if (avatar.current) avatar.current.style.backgroundImage = avatarBg;
      if (name.current) name.current.textContent = ch.displayName;
      if (barName.current) {
        barName.current.textContent = ch.displayName;
        if (!ch.sample && ch.login) barName.current.href = `https://twitch.tv/${ch.login}`;
        else barName.current.removeAttribute("href");
      }
      if (barSub.current)
        barSub.current.textContent = ch.sample
          ? "Clip aus dem Netzwerk"
          : "Partner im Netzwerk";
      if (live.current) live.current.classList.toggle("v2-rd-clip", ch.sample);
      if (badgeText.current) badgeText.current.textContent = ch.sample ? "CLIP" : "LIVE";
      if (view.current) view.current.textContent = ch.sample ? "" : String(viewers);
      if (meta.current) meta.current.textContent = ch.sample ? "Deadlock" : "Zuschauer · Deadlock";
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

    function setStep(i: number, state: "idle" | "active" | "done") {
      const el = stepRefs[i].current;
      if (el) el.className = `v2-rd-step v2-rd-step-${state}`;
    }
    function resetSteps() {
      for (let i = 0; i < stepRefs.length; i++) setStep(i, "idle");
    }

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
      void el.offsetWidth;
      el.style.transition = "width 1.4s cubic-bezier(0.4,0,0.2,1)";
      el.style.width = "100%";
    }

    function pickPair(): [DemoChannel, DemoChannel] {
      const list = poolRef.current;
      const a = Math.floor(Math.random() * list.length);
      let b = Math.floor(Math.random() * list.length);
      if (b === a) b = (a + 1) % list.length;
      return [list[a], list[b]];
    }

    function setupRound(src: DemoChannel, tgt: DemoChannel) {
      const srcSecs = 3600 + Math.floor(Math.random() * 7200);
      const tgtSecs = 900 + Math.floor(Math.random() * 3600);
      paintCard("src", src, src.viewers, srcSecs);
      paintCard("tgt", tgt, tgt.viewers, tgtSecs);

      if (srcCardRef.current) srcCardRef.current.className = "v2-rd-card v2-rd-card-src";
      if (tgtCardRef.current) tgtCardRef.current.className = "v2-rd-card v2-rd-card-tgt v2-rd-card-dim";
      if (srcOfflineRef.current) srcOfflineRef.current.style.opacity = "0";
      if (srcLiveRef.current) srcLiveRef.current.style.opacity = "1";
      if (counterRef.current) counterRef.current.style.opacity = "0";
      if (counterNumRef.current) counterNumRef.current.textContent = "0";
      setBeam("off");
      clearLines();
      resetSteps();

      startDurations(srcSecs, tgtSecs);
    }

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
        lineRef.current.textContent = src.sample
          ? `Deine Zuschauer sind bei ${tgt.displayName} angekommen`
          : `${src.viewers} Zuschauer bei ${tgt.displayName} angekommen`;
      }
      for (let i = 0; i < STEPS.length; i++) setStep(i, "done");
      return () => {
        running = false;
      };
    }

    async function runRound() {
      if (!running) return;
      const [src, tgt] = pickPair();
      setupRound(src, tgt);
      await sleep(1400);
      if (!running) return;

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

      setStep(2, "active");
      setStamp(STEPS[2].time, STEPS[2].label, "teal");
      clearLines();
      if (lineRef.current) {
        lineRef.current.style.opacity = "1";
        lineRef.current.textContent = `zu ${tgt.displayName}`;
      }
      if (counterRef.current) counterRef.current.style.opacity = "1";
      if (counterNumRef.current)
        animateCounter(0, src.viewers, 1400, counterNumRef.current, alive);
      setBeam("forward");
      spawnParticles(34, 1400);
      await sleep(1650);
      if (!running) return;
      setBeam("off");
      if (tgtViewersRef.current && !tgt.sample) {
        tgtViewersRef.current.textContent = String(tgt.viewers + src.viewers);
      }
      if (tgtCardRef.current) tgtCardRef.current.className = "v2-rd-card v2-rd-card-tgt";
      if (counterRef.current) counterRef.current.style.opacity = "0";
      clearLines();
      if (lineRef.current) {
        lineRef.current.style.opacity = "1";
        lineRef.current.textContent = src.sample
          ? "Deine Zuschauer sind angekommen"
          : `+${src.viewers} Zuschauer angekommen`;
        lineRef.current.classList.add("v2-rd-line-good");
      }
      spawnConfetti(38);
      await sleep(1500);
      if (!running) return;
      setStep(2, "done");
      if (lineRef.current) lineRef.current.classList.remove("v2-rd-line-good");

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

    const onSrcError = () => markPaused("src", true);
    const onTgtError = () => markPaused("tgt", true);
    srcVideoRef.current?.addEventListener("error", onSrcError, true);
    tgtVideoRef.current?.addEventListener("error", onTgtError, true);

    runRound();

    return () => {
      running = false;
      if (durationTimer) clearInterval(durationTimer);
      srcVideoRef.current?.removeEventListener("error", onSrcError, true);
      tgtVideoRef.current?.removeEventListener("error", onTgtError, true);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="v2-stage">
      <div className="v2-rd">
        <div className="v2-rd-head">
          <span className="v2-stamp">Übergabe im Netzwerk · Beispielablauf</span>
          <span className="v2-rd-head-live">
            <span className="v2-pulse h-2 w-2 rounded-full bg-[var(--color-success)]" />
            Netzwerk aktiv
          </span>
        </div>

        <div className="v2-stage-body">
          <div className="v2-rd-stage">
            <div className="v2-rd-cards">
              <div className="v2-rd-card v2-rd-card-src" ref={srcCardRef}>
                <div className="v2-rd-screen" ref={srcScreenRef}>
                  <video
                    className="v2-rd-video"
                    ref={srcVideoRef}
                    muted
                    loop
                    playsInline
                    preload="metadata"
                    aria-hidden="true"
                  />
                  <div className="v2-rd-art" ref={srcArtRef} aria-hidden="true" />
                  <div className="v2-screen-sheen" aria-hidden="true" />
                  <span className="v2-rd-play" aria-hidden="true">
                    <Play size={20} fill="currentColor" />
                  </span>
                  <div className="v2-rd-ui">
                    <div className="v2-rd-ui-top">
                      <span className="v2-rd-live v2-rd-clip" ref={srcLiveRef}>
                        <span className="v2-rd-live-dot" />
                        <span ref={srcBadgeTextRef}>CLIP</span>
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
                        <span ref={srcViewersRef} /> <span ref={srcMetaRef}>Deadlock</span>
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
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      …
                    </a>
                    <span className="v2-rd-bar-sub" ref={srcBarSubRef}>
                      Spielt Deadlock
                    </span>
                  </span>
                </div>
              </div>

              <div className="v2-rd-card v2-rd-card-tgt v2-rd-card-dim" ref={tgtCardRef}>
                <div className="v2-rd-screen" ref={tgtScreenRef}>
                  <video
                    className="v2-rd-video"
                    ref={tgtVideoRef}
                    muted
                    loop
                    playsInline
                    preload="metadata"
                    aria-hidden="true"
                  />
                  <div className="v2-rd-art" ref={tgtArtRef} aria-hidden="true" />
                  <div className="v2-screen-sheen" aria-hidden="true" />
                  <span className="v2-rd-play" aria-hidden="true">
                    <Play size={20} fill="currentColor" />
                  </span>
                  <div className="v2-rd-ui">
                    <div className="v2-rd-ui-top">
                      <span className="v2-rd-live v2-rd-clip" ref={tgtLiveRef}>
                        <span className="v2-rd-live-dot" />
                        <span ref={tgtBadgeTextRef}>CLIP</span>
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
                        <span ref={tgtViewersRef} /> <span ref={tgtMetaRef}>Deadlock</span>
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
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      …
                    </a>
                    <span className="v2-rd-bar-sub" ref={tgtBarSubRef}>
                      Partner im Netzwerk
                    </span>
                  </span>
                </div>
              </div>

              <div className="v2-rd-fuse" aria-hidden="true" />
            </div>

            <div className="v2-rd-mid" ref={midRef} aria-hidden="true" />
          </div>
        </div>

        <ol className="v2-rd-steps">
          {STEPS.map((step, i) => (
            <li key={step.time} className="v2-rd-step v2-rd-step-idle" ref={stepRefs[i]}>
              <span className="v2-rd-step-time">{step.time}</span>
              <span className="v2-rd-step-label">{step.label}</span>
            </li>
          ))}
        </ol>

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
