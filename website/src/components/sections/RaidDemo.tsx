import { useEffect, useRef } from 'react'

// ─── Typen ───────────────────────────────────────────────────────────────────
interface Streamer {
  name: string
  avatar: string
  color: string
  viewers: number
  video: string
}

// ─── Streamer-Pool ───────────────────────────────────────────────────────────
const BASE = import.meta.env.BASE_URL.replace(/\/$/, '')

const streamerPool: Streamer[] = [
  { name: 'miracleghost9',  avatar: 'M',  color: '#ff7a18', viewers: 247, video: `${BASE}/clips/miracleghost9.mp4` },
  { name: 'whysolowkey',    avatar: 'W',  color: '#10b7ad', viewers: 183, video: `${BASE}/clips/whysolowkey.mp4` },
  { name: 'kdenos',         avatar: 'K',  color: '#8b5cf6', viewers: 312, video: `${BASE}/clips/kdenos.mp4` },
  { name: 'johnnyblazedx',  avatar: 'J',  color: '#3b82f6', viewers: 421, video: `${BASE}/clips/johnnyblazedx.mp4` },
  { name: 'derechtecoolys', avatar: 'D',  color: '#f59e0b', viewers: 158, video: `${BASE}/clips/derechtecoolys.mp4` },
  { name: 'duzzel',         avatar: 'Du', color: '#ec4899', viewers: 534, video: `${BASE}/clips/duzzel.mp4` },
]

let lastPair: [number, number] = [-1, -1]

function pickTwo(): [Streamer, Streamer] {
  let a: number, b: number
  do {
    a = Math.floor(Math.random() * streamerPool.length)
    b = Math.floor(Math.random() * streamerPool.length)
  } while (a === b || (a === lastPair[0] && b === lastPair[1]))
  lastPair = [a, b]
  return [streamerPool[a], streamerPool[b]]
}

// ─── Hilfsfunktionen ─────────────────────────────────────────────────────────
function fmtDuration(secs: number): string {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

function durationToSecs(str: string): number {
  const parts = str.split(':').map(Number)
  if (parts.length === 3) return parts[0] * 3600 + parts[1] * 60 + parts[2]
  if (parts.length === 2) return parts[0] * 60 + parts[1]
  return parts[0]
}

function sleep(ms: number): Promise<void> {
  return new Promise(r => setTimeout(r, ms))
}

async function typewriter(el: HTMLElement, text: string, speed = 40): Promise<void> {
  el.textContent = ''
  for (const ch of text) {
    el.textContent += ch
    await sleep(speed)
  }
}

function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - t, 3)
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t
}

function animateCounter(from: number, to: number, duration: number, el: HTMLElement): void {
  const start = performance.now()
  const tick = (now: number) => {
    const t = Math.min((now - start) / duration, 1)
    el.textContent = String(Math.round(lerp(from, to, easeOutCubic(t))))
    if (t < 1) requestAnimationFrame(tick)
  }
  requestAnimationFrame(tick)
}

// ─── Komponente ──────────────────────────────────────────────────────────────
export function RaidDemo() {
  const sourceEmbedRef     = useRef<HTMLDivElement>(null)
  const targetEmbedRef     = useRef<HTMLDivElement>(null)
  const middleAreaRef      = useRef<HTMLDivElement>(null)
  const stepBadgeRef       = useRef<HTMLDivElement>(null)
  const searchTextRef      = useRef<HTMLDivElement>(null)
  const searchSubRef       = useRef<HTMLDivElement>(null)
  const raidCounterRef     = useRef<HTMLDivElement>(null)
  const raidCountNumRef    = useRef<HTMLDivElement>(null)
  const finalTextRef       = useRef<HTMLDivElement>(null)
  const energyBeamRef      = useRef<HTMLDivElement>(null)
  const sourceIframeRef    = useRef<HTMLVideoElement>(null)
  const targetIframeRef    = useRef<HTMLVideoElement>(null)
  const sourceLiveBadgeRef = useRef<HTMLDivElement>(null)
  const sourceLiveTextRef  = useRef<HTMLSpanElement>(null)
  const sourceDurationRef  = useRef<HTMLDivElement>(null)
  const sourceStreamerRef  = useRef<HTMLDivElement>(null)
  const sourceViewersRef   = useRef<HTMLSpanElement>(null)
  const sourceAvatarRef    = useRef<HTMLDivElement>(null)
  const sourceInfoNameRef  = useRef<HTMLAnchorElement>(null)
  const offlineOverlayRef  = useRef<HTMLDivElement>(null)
  const targetLiveBadgeRef = useRef<HTMLDivElement>(null)
  const targetLiveTextRef  = useRef<HTMLSpanElement>(null)
  const targetDurationRef  = useRef<HTMLDivElement>(null)
  const targetStreamerRef  = useRef<HTMLDivElement>(null)
  const targetViewersRef   = useRef<HTMLSpanElement>(null)
  const targetAvatarRef    = useRef<HTMLDivElement>(null)
  const targetInfoNameRef  = useRef<HTMLAnchorElement>(null)
  const pill0Ref           = useRef<HTMLDivElement>(null)
  const pill1Ref           = useRef<HTMLDivElement>(null)
  const pill2Ref           = useRef<HTMLDivElement>(null)
  const pill3Ref           = useRef<HTMLDivElement>(null)
  const raidIndexRef       = useRef<HTMLSpanElement>(null)

  useEffect(() => {
    let running = true
    let durationInterval: ReturnType<typeof setInterval> | null = null

    // Videos sofort starten
    if (sourceIframeRef.current) { sourceIframeRef.current.src = streamerPool[0].video; sourceIframeRef.current.play().catch(() => {}) }
    if (targetIframeRef.current) { targetIframeRef.current.src = streamerPool[1].video; targetIframeRef.current.play().catch(() => {}) }

    // ─── Durations ─────────────────────────────────────────────────────
    function startDurations(srcSecs: number, tgtSecs: number) {
      if (durationInterval) clearInterval(durationInterval)
      let ss = srcSecs, ts = tgtSecs
      durationInterval = setInterval(() => {
        if (!running) return
        ss++; ts++
        if (sourceDurationRef.current) sourceDurationRef.current.textContent = fmtDuration(ss)
        if (targetDurationRef.current) targetDurationRef.current.textContent = fmtDuration(ts)
      }, 1000)
    }

    // ─── Pills ─────────────────────────────────────────────────────────
    const pillRefs = [pill0Ref, pill1Ref, pill2Ref, pill3Ref]
    function resetPills() {
      pillRefs.forEach(r => { if (r.current) r.current.className = 'rd-pill' })
    }
    function setPillActive(i: number) {
      if (pillRefs[i].current) pillRefs[i].current!.className = 'rd-pill rd-pill-active'
    }
    function setPillDone(i: number) {
      if (pillRefs[i].current) pillRefs[i].current!.className = 'rd-pill rd-pill-done'
    }
    function setPillActiveRaid(i: number) {
      if (pillRefs[i].current) pillRefs[i].current!.className = 'rd-pill rd-pill-active-raid'
    }
    function setPillActiveSuccess(i: number) {
      if (pillRefs[i].current) pillRefs[i].current!.className = 'rd-pill rd-pill-active-success'
    }

    // ─── Particles ─────────────────────────────────────────────────────
    function spawnParticles(
      count: number,
      srcRect: DOMRect,
      tgtRect: DOMRect,
      containerRect: DOMRect,
      duration: number
    ) {
      const container = middleAreaRef.current
      if (!container) return
      for (let i = 0; i < count; i++) {
        const p = document.createElement('div')
        p.className = 'rd-particle'
        const delay = Math.random() * duration * 0.6
        const startX = srcRect.left - containerRect.left + srcRect.width / 2 + (Math.random() - 0.5) * 60
        const startY = srcRect.top - containerRect.top + srcRect.height / 2 + (Math.random() - 0.5) * 60
        const endX = tgtRect.left - containerRect.left + tgtRect.width / 2 + (Math.random() - 0.5) * 60
        const endY = tgtRect.top - containerRect.top + tgtRect.height / 2 + (Math.random() - 0.5) * 60
        p.style.cssText = [
          'position:absolute',
          'width:6px',
          'height:6px',
          'border-radius:50%',
          'background:var(--color-accent)',
          `left:${startX}px`,
          `top:${startY}px`,
          'opacity:0',
          'pointer-events:none',
          'z-index:20',
          'box-shadow:0 0 6px var(--color-accent)',
        ].join(';')
        container.appendChild(p)
        setTimeout(() => {
          if (!running) { p.remove(); return }
          const start = performance.now()
          const animate = (now: number) => {
            if (!running) { p.remove(); return }
            const t = Math.min((now - start) / duration, 1)
            const et = easeOutCubic(t)
            p.style.left = `${lerp(startX, endX, et)}px`
            p.style.top = `${lerp(startY, endY, et)}px`
            p.style.opacity = String(t < 0.1 ? t * 10 : t > 0.8 ? (1 - t) * 5 : 1)
            if (t < 1) requestAnimationFrame(animate)
            else p.remove()
          }
          requestAnimationFrame(animate)
        }, delay)
      }
    }

    function spawnConfetti(originX: number, originY: number, count: number) {
      const container = middleAreaRef.current
      if (!container) return
      const colors = ['#06B6D4', '#A855F7', '#f59e0b', '#8b5cf6', '#22c55e', '#ec4899']
      for (let i = 0; i < count; i++) {
        const c = document.createElement('div')
        const color = colors[Math.floor(Math.random() * colors.length)]
        const size = 4 + Math.random() * 6
        const angle = Math.random() * Math.PI * 2
        const speed = 80 + Math.random() * 120
        const vx = Math.cos(angle) * speed
        const vy = Math.sin(angle) * speed - 60
        c.style.cssText = [
          'position:absolute',
          `width:${size}px`,
          `height:${size}px`,
          `background:${color}`,
          `border-radius:${Math.random() > 0.5 ? '50%' : '2px'}`,
          `left:${originX}px`,
          `top:${originY}px`,
          'pointer-events:none',
          'z-index:30',
        ].join(';')
        container.appendChild(c)
        const startTime = performance.now()
        const dur = 800 + Math.random() * 600
        const animate = (now: number) => {
          if (!running) { c.remove(); return }
          const t = Math.min((now - startTime) / dur, 1)
          const x = originX + vx * t
          const y = originY + vy * t + 0.5 * 200 * t * t
          c.style.left = `${x}px`
          c.style.top = `${y}px`
          c.style.opacity = String(1 - t)
          c.style.transform = `rotate(${t * 360 * (Math.random() > 0.5 ? 1 : -1)}deg)`
          if (t < 1) requestAnimationFrame(animate)
          else c.remove()
        }
        requestAnimationFrame(animate)
      }
    }

    // ─── Setup Scenario ─────────────────────────────────────────────────
    function setupScenario(src: Streamer, tgt: Streamer) {
      if (sourceIframeRef.current) { sourceIframeRef.current.src = src.video; sourceIframeRef.current.play().catch(() => {}) }
      if (sourceStreamerRef.current) sourceStreamerRef.current.textContent = src.name
      if (sourceViewersRef.current) sourceViewersRef.current.textContent = `${src.viewers} Zuschauer`
      if (sourceAvatarRef.current) {
        sourceAvatarRef.current.textContent = src.avatar
        sourceAvatarRef.current.style.background = src.color
      }
      if (sourceInfoNameRef.current) { sourceInfoNameRef.current.textContent = src.name; sourceInfoNameRef.current.href = `https://twitch.tv/${src.name}` }
      if (sourceLiveTextRef.current) sourceLiveTextRef.current.textContent = 'LIVE'
      if (sourceLiveBadgeRef.current) sourceLiveBadgeRef.current.style.opacity = '1'
      if (sourceDurationRef.current)
        sourceDurationRef.current.textContent = fmtDuration(Math.floor(Math.random() * 7200))
      if (offlineOverlayRef.current) offlineOverlayRef.current.style.opacity = '0'
      if (sourceEmbedRef.current) {
        sourceEmbedRef.current.style.opacity = '1'
        sourceEmbedRef.current.style.transform = 'scale(1)'
      }

      if (targetIframeRef.current) { targetIframeRef.current.src = tgt.video; targetIframeRef.current.play().catch(() => {}) }
      if (targetStreamerRef.current) targetStreamerRef.current.textContent = tgt.name
      if (targetViewersRef.current) targetViewersRef.current.textContent = `${tgt.viewers} Zuschauer`
      if (targetAvatarRef.current) {
        targetAvatarRef.current.textContent = tgt.avatar
        targetAvatarRef.current.style.background = tgt.color
      }
      if (targetInfoNameRef.current) { targetInfoNameRef.current.textContent = tgt.name; targetInfoNameRef.current.href = `https://twitch.tv/${tgt.name}` }
      if (targetLiveTextRef.current) targetLiveTextRef.current.textContent = 'LIVE'
      if (targetLiveBadgeRef.current) targetLiveBadgeRef.current.style.opacity = '1'
      if (targetDurationRef.current)
        targetDurationRef.current.textContent = fmtDuration(Math.floor(Math.random() * 3600))
      if (targetEmbedRef.current) {
        targetEmbedRef.current.style.opacity = '0.4'
        targetEmbedRef.current.style.transform = 'scale(0.97)'
      }

      if (stepBadgeRef.current) stepBadgeRef.current.textContent = ''
      if (searchTextRef.current) { searchTextRef.current.textContent = ''; searchTextRef.current.style.opacity = '0' }
      if (searchSubRef.current) { searchSubRef.current.textContent = ''; searchSubRef.current.style.opacity = '0' }
      if (raidCounterRef.current) raidCounterRef.current.style.opacity = '0'
      if (raidCountNumRef.current) raidCountNumRef.current.textContent = '0'
      if (finalTextRef.current) { finalTextRef.current.textContent = ''; finalTextRef.current.style.opacity = '0' }
      if (energyBeamRef.current) {
        energyBeamRef.current.style.transition = 'none'
        energyBeamRef.current.style.opacity = '0'
        energyBeamRef.current.style.width = '0'
      }

      startDurations(
        durationToSecs(sourceDurationRef.current?.textContent ?? '0:00:00'),
        durationToSecs(targetDurationRef.current?.textContent ?? '0:00:00')
      )
    }

    // ─── Main Raid Loop ─────────────────────────────────────────────────
    async function runRaid(raidNum: number) {
      if (!running) return
      const [src, tgt] = pickTwo()
      setupScenario(src, tgt)
      if (raidIndexRef.current) raidIndexRef.current.textContent = String(raidNum)
      resetPills()

      // Phase 0 — Stream endet
      setPillActive(0)
      if (stepBadgeRef.current) stepBadgeRef.current.textContent = '① Stream endet'
      if (searchTextRef.current) {
        searchTextRef.current.style.opacity = '1'
        await typewriter(searchTextRef.current, `${src.name} beendet den Stream`, 35)
      }
      if (!running) return
      await sleep(600)
      if (!running) return
      if (offlineOverlayRef.current) offlineOverlayRef.current.style.opacity = '1'
      if (sourceLiveBadgeRef.current) sourceLiveBadgeRef.current.style.opacity = '0'
      if (durationInterval) clearInterval(durationInterval)
      await sleep(800)
      if (!running) return
      setPillDone(0)

      // Phase 1 — Partner suchen
      setPillActive(1)
      if (stepBadgeRef.current) stepBadgeRef.current.textContent = '② Partner suchen'
      if (searchTextRef.current) searchTextRef.current.style.opacity = '0'
      await sleep(200)
      if (!running) return
      if (searchTextRef.current) {
        searchTextRef.current.style.opacity = '1'
        await typewriter(searchTextRef.current, 'Suche aktiven Stream …', 40)
      }
      if (!running) return
      if (searchSubRef.current) {
        searchSubRef.current.style.opacity = '1'
        searchSubRef.current.textContent = 'Gleiche Kategorie · Aktiv live'
      }
      await sleep(1200)
      if (!running) return
      if (searchTextRef.current) { searchTextRef.current.style.opacity = '0'; searchTextRef.current.textContent = '' }
      if (searchSubRef.current) { searchSubRef.current.style.opacity = '0'; searchSubRef.current.textContent = '' }
      await sleep(200)
      if (!running) return
      if (stepBadgeRef.current) stepBadgeRef.current.textContent = `✓ ${tgt.name} gefunden`
      if (targetEmbedRef.current) {
        targetEmbedRef.current.style.opacity = '1'
        targetEmbedRef.current.style.transform = 'scale(1)'
      }
      await sleep(600)
      if (!running) return
      setPillDone(1)

      // Phase 2 — Raid ausführen
      setPillActiveRaid(2)
      if (stepBadgeRef.current) stepBadgeRef.current.textContent = '③ Raid läuft …'
      if (searchTextRef.current) {
        searchTextRef.current.style.opacity = '1'
        await typewriter(searchTextRef.current, `Sende ${src.viewers} Viewer zu ${tgt.name}`, 30)
      }
      if (!running) return
      if (raidCounterRef.current) raidCounterRef.current.style.opacity = '1'
      if (raidCountNumRef.current) animateCounter(0, src.viewers, 1400, raidCountNumRef.current)

      if (energyBeamRef.current) {
        energyBeamRef.current.style.opacity = '1'
        energyBeamRef.current.style.transition = 'width 1.4s cubic-bezier(0.4,0,0.2,1)'
        energyBeamRef.current.style.width = '100%'
      }

      const srcEl = sourceEmbedRef.current
      const tgtEl = targetEmbedRef.current
      const midEl = middleAreaRef.current
      if (srcEl && tgtEl && midEl) {
        spawnParticles(
          30,
          srcEl.getBoundingClientRect(),
          tgtEl.getBoundingClientRect(),
          midEl.getBoundingClientRect(),
          1400
        )
      }
      await sleep(1600)
      if (!running) return
      if (energyBeamRef.current) {
        energyBeamRef.current.style.transition = 'none'
        energyBeamRef.current.style.width = '0'
        energyBeamRef.current.style.opacity = '0'
      }
      setPillDone(2)

      // Phase 3 — Angekommen
      setPillActiveSuccess(3)
      if (stepBadgeRef.current) stepBadgeRef.current.textContent = '④ Angekommen!'
      if (searchTextRef.current) { searchTextRef.current.style.opacity = '0'; searchTextRef.current.textContent = '' }
      if (raidCounterRef.current) raidCounterRef.current.style.opacity = '0'
      if (targetViewersRef.current)
        targetViewersRef.current.textContent = `${tgt.viewers + src.viewers} Zuschauer`
      if (finalTextRef.current) {
        finalTextRef.current.style.opacity = '1'
        await typewriter(finalTextRef.current, `+${src.viewers} Viewer angekommen! 🎉`, 25)
      }
      if (!running) return

      if (midEl) {
        const r = midEl.getBoundingClientRect()
        spawnConfetti(r.width / 2, r.height / 2, 40)
      }
      await sleep(500)
      if (!running) return
      setPillDone(3)
      await sleep(2200)
      if (!running) return

      runRaid(raidNum + 1)
    }

    runRaid(1)

    return () => {
      running = false
      if (durationInterval) clearInterval(durationInterval)
    }
  }, [])

  return (
    <>
      <style>{`
        /* ── Layout ──────────────────────────────────────────────────── */
        .rd-demo-area {
          position: relative;
          margin-bottom: 32px;
          width: 100%;
        }
        .rd-embeds-row {
          display: flex;
          justify-content: center;
          gap: 22vw;
          align-items: flex-start;
          position: relative;
        }
        @media (max-width: 860px) {
          .rd-embeds-row { flex-direction: column; align-items: center; gap: 40px; }
          .rd-embed-source, .rd-embed-target { width: 100% !important; max-width: 520px !important; }
          .rd-middle-area { display: none; }
        }
        .rd-embed-source {
          width: 34vw; max-width: 540px; min-width: 300px; flex-shrink: 0;
          transition: opacity 0.5s cubic-bezier(0.4,0,0.2,1), transform 0.5s cubic-bezier(0.4,0,0.2,1);
        }
        .rd-embed-target {
          width: 34vw; max-width: 540px; min-width: 300px; flex-shrink: 0;
          transition: opacity 0.5s cubic-bezier(0.4,0,0.2,1), transform 0.5s cubic-bezier(0.4,0,0.2,1);
        }

        /* ── Twitch Embed Card ───────────────────────────────────────── */
        .rd-twitch-embed {
          border-radius: 10px;
          overflow: hidden;
          border: 1px solid var(--color-border);
          background: var(--color-card);
          box-shadow: 0 4px 24px rgba(0,0,0,0.4);
        }
        .rd-player {
          position: relative;
          aspect-ratio: 16 / 9;
          background: #0a0a0a;
          overflow: hidden;
        }
        .rd-clip-iframe {
          position: absolute;
          inset: 0;
          width: 100%;
          height: 100%;
          object-fit: cover;
          border-radius: 8px 8px 0 0;
          pointer-events: none;
        }
        .rd-player-overlays {
          position: absolute; inset: 0;
          display: flex; flex-direction: column;
          justify-content: space-between;
          padding: 10px;
          background: linear-gradient(
            to bottom,
            rgba(0,0,0,0.5) 0%,
            transparent 40%,
            transparent 60%,
            rgba(0,0,0,0.7) 100%
          );
        }
        .rd-overlay-top {
          display: flex; justify-content: space-between; align-items: center;
        }
        .rd-overlay-bottom {
          display: flex; flex-direction: column; gap: 3px;
        }
        .rd-live-badge {
          display: flex; align-items: center; gap: 5px;
          background: #ef4444; color: #fff;
          padding: 2px 8px; border-radius: 4px;
          font-size: 11px; font-weight: 700; letter-spacing: 1px;
          transition: opacity 0.4s;
        }
        .rd-live-dot {
          width: 6px; height: 6px; border-radius: 50%;
          background: #fff;
          animation: rd-pulse 1.4s infinite;
        }
        .rd-duration-pill {
          background: rgba(0,0,0,0.6); color: #fff;
          padding: 2px 8px; border-radius: 4px;
          font-size: 12px; font-variant-numeric: tabular-nums;
          backdrop-filter: blur(4px);
        }
        .rd-overlay-streamer {
          color: #fff; font-weight: 700; font-size: 14px;
          text-shadow: 0 1px 4px rgba(0,0,0,0.8);
        }
        .rd-overlay-category {
          color: rgba(255,255,255,0.7); font-size: 12px;
        }
        .rd-overlay-viewers {
          display: flex; align-items: center; gap: 5px;
          color: rgba(255,255,255,0.9); font-size: 12px;
        }
        .rd-offline-overlay {
          position: absolute; inset: 0;
          background: rgba(0,0,0,0.85);
          display: flex; flex-direction: column;
          align-items: center; justify-content: center; gap: 8px;
          opacity: 0;
          transition: opacity 0.6s;
          backdrop-filter: blur(2px);
        }
        .rd-offline-text {
          color: #fff; font-weight: 700; font-size: 15px;
        }
        .rd-offline-subtext {
          color: rgba(255,255,255,0.5); font-size: 12px;
        }
        .rd-info-bar {
          display: flex; align-items: center;
          justify-content: space-between;
          padding: 10px 12px;
        }
        .rd-info-left {
          display: flex; align-items: center; gap: 10px;
        }
        .rd-avatar {
          width: 36px; height: 36px; border-radius: 50%;
          display: flex; align-items: center; justify-content: center;
          font-weight: 700; font-size: 14px; color: #fff;
          flex-shrink: 0;
        }
        .rd-info-name {
          font-weight: 700; font-size: 13px;
          color: var(--color-text-primary);
        }
        .rd-info-link {
          text-decoration: none;
          transition: color 0.2s;
          pointer-events: auto;
        }
        .rd-info-link:hover { color: var(--color-accent); }
        .rd-info-game {
          font-size: 11px; color: var(--color-text-secondary);
        }

        /* ── Middle Area ─────────────────────────────────────────────── */
        .rd-middle-area {
          position: absolute;
          top: 50%; left: 50%;
          transform: translate(-50%, -50%);
          display: flex; flex-direction: column;
          align-items: center; justify-content: center;
          width: 18vw; min-width: 140px;
          gap: 10px;
          pointer-events: none;
          z-index: 10;
        }
        .rd-step-badge {
          font-size: 12px; font-weight: 700;
          color: var(--color-accent);
          background: rgba(16,183,173,0.12);
          border: 1px solid rgba(16,183,173,0.3);
          padding: 4px 10px; border-radius: 20px;
          text-align: center; min-height: 24px;
          transition: all 0.3s;
        }
        .rd-search-text {
          font-size: 13px; font-weight: 600;
          color: var(--color-text-primary);
          text-align: center; line-height: 1.4;
          transition: opacity 0.4s;
        }
        .rd-search-sub {
          font-size: 11px; color: var(--color-text-secondary);
          text-align: center;
          transition: opacity 0.4s;
        }
        .rd-raid-counter {
          display: flex; flex-direction: column; align-items: center; gap: 2px;
          transition: opacity 0.4s;
        }
        .rd-raid-count-num {
          font-size: 28px; font-weight: 800;
          color: var(--color-primary);
          font-variant-numeric: tabular-nums;
          line-height: 1;
        }
        .rd-raid-count-label {
          font-size: 10px; color: var(--color-text-secondary); text-align: center;
        }
        .rd-final-text {
          font-size: 14px; font-weight: 700;
          color: #22c55e; text-align: center;
          transition: opacity 0.4s;
        }
        .rd-energy-beam {
          height: 3px; width: 0;
          background: linear-gradient(90deg, var(--color-primary), var(--color-accent));
          border-radius: 2px;
          opacity: 0;
          box-shadow: 0 0 8px var(--color-accent);
        }

        /* ── Status Bar ──────────────────────────────────────────────── */
        .rd-status-bar {
          display: flex; align-items: center; justify-content: center;
          gap: 8px; flex-wrap: wrap;
          margin-top: 16px;
        }
        .rd-pill {
          padding: 6px 14px; border-radius: 20px;
          font-size: 12px; font-weight: 600;
          background: var(--color-card);
          border: 1px solid var(--color-border);
          color: var(--color-text-secondary);
          transition: all 0.35s cubic-bezier(0.4,0,0.2,1);
        }
        .rd-pill-active {
          background: rgba(255,122,24,0.15);
          border-color: var(--color-primary);
          color: var(--color-primary);
        }
        .rd-pill-done {
          background: rgba(34,197,94,0.1);
          border-color: #22c55e;
          color: #22c55e;
        }
        .rd-pill-active-raid {
          background: rgba(16,183,173,0.15);
          border-color: var(--color-accent);
          color: var(--color-accent);
          animation: rd-glow-pulse 1s infinite alternate;
        }
        .rd-pill-active-success {
          background: rgba(34,197,94,0.2);
          border-color: #22c55e;
          color: #22c55e;
          animation: rd-bounce-in 0.4s cubic-bezier(0.4,0,0.2,1);
        }
        .rd-pill-arrow {
          color: var(--color-text-secondary); font-size: 14px;
        }
        .rd-raid-display {
          text-align: center; margin-top: 10px;
          font-size: 12px; color: var(--color-text-secondary);
        }

        /* ── Keyframes ───────────────────────────────────────────────── */
        @keyframes rd-pulse {
          0%, 100% { opacity: 1; transform: scale(1); }
          50%       { opacity: 0.4; transform: scale(0.8); }
        }
        @keyframes rd-glow-pulse {
          from { box-shadow: 0 0 0 rgba(16,183,173,0); }
          to   { box-shadow: 0 0 10px rgba(16,183,173,0.5); }
        }
        @keyframes rd-bounce-in {
          0%   { transform: scale(0.8); opacity: 0; }
          60%  { transform: scale(1.1); }
          100% { transform: scale(1); opacity: 1; }
        }
      `}</style>

      <div className="rd-demo-area">
        <div className="rd-embeds-row">

          {/* Source Embed */}
          <div className="rd-twitch-embed rd-embed-source" ref={sourceEmbedRef}>
            <div className="rd-player">
              <video
                ref={sourceIframeRef}
                className="rd-clip-iframe"
                muted
                autoPlay
                loop
                playsInline
              />
              <div className="rd-player-overlays">
                <div className="rd-overlay-top">
                  <div className="rd-duration-pill" ref={sourceDurationRef}>0:00:00</div>
                </div>
                <div className="rd-overlay-bottom">
                  <div className="rd-overlay-streamer" ref={sourceStreamerRef}>Deutsche Deadlock Community</div>
                  <div className="rd-overlay-category">Deadlock</div>
                  <div className="rd-overlay-viewers">
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                      <circle cx="12" cy="12" r="3" />
                    </svg>
                    <span ref={sourceViewersRef}>247 Zuschauer</span>
                  </div>
                </div>
              </div>
              <div className="rd-offline-overlay" ref={offlineOverlayRef}>
                <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.4)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <line x1="1" y1="1" x2="23" y2="23" />
                  <path d="M16.72 11.06A10.94 10.94 0 0 1 19 12.55" />
                  <path d="M5 12.55a10.94 10.94 0 0 1 5.17-2.39" />
                  <path d="M10.71 5.05A16 16 0 0 1 22.56 9" />
                  <path d="M1.42 9a15.91 15.91 0 0 1 4.7-2.88" />
                  <path d="M8.53 16.11a6 6 0 0 1 6.95 0" />
                  <line x1="12" y1="20" x2="12.01" y2="20" />
                </svg>
                <div className="rd-offline-text">Stream Offline</div>
                <div className="rd-offline-subtext">Der Stream ist beendet</div>
              </div>
            </div>
            <div className="rd-info-bar">
              <div className="rd-info-left">
                <div className="rd-avatar" ref={sourceAvatarRef} style={{ background: '#06B6D4' }}>D</div>
                <div>
                  <a className="rd-info-name rd-info-link" ref={sourceInfoNameRef} href="https://twitch.tv/miracleghost9" target="_blank" rel="noopener noreferrer">miracleghost9</a>
                  <div className="rd-info-game">Spielt Deadlock</div>
                </div>
              </div>
            </div>
          </div>

          {/* Middle Area — overlaid between the two embeds */}
          <div className="rd-middle-area" ref={middleAreaRef}>
            <div className="rd-step-badge" ref={stepBadgeRef} />
            <div className="rd-search-text" ref={searchTextRef} style={{ opacity: 0 }} />
            <div className="rd-search-sub" ref={searchSubRef} style={{ opacity: 0 }} />
            <div className="rd-raid-counter" ref={raidCounterRef} style={{ opacity: 0 }}>
              <div className="rd-raid-count-num" ref={raidCountNumRef}>0</div>
              <div className="rd-raid-count-label">Viewer werden geraided</div>
            </div>
            <div className="rd-final-text" ref={finalTextRef} style={{ opacity: 0 }} />
            <div className="rd-energy-beam" ref={energyBeamRef} />
          </div>

          {/* Target Embed */}
          <div
            className="rd-twitch-embed rd-embed-target"
            ref={targetEmbedRef}
            style={{ opacity: 0.4, transform: 'scale(0.97)' }}
          >
            <div className="rd-player">
              <video
                ref={targetIframeRef}
                className="rd-clip-iframe"
                muted
                autoPlay
                loop
                playsInline
              />
              <div className="rd-player-overlays">
                <div className="rd-overlay-top">
                  <div className="rd-duration-pill" ref={targetDurationRef}>0:00:00</div>
                </div>
                <div className="rd-overlay-bottom">
                  <div className="rd-overlay-streamer" ref={targetStreamerRef}>Paradox</div>
                  <div className="rd-overlay-category">Deadlock</div>
                  <div className="rd-overlay-viewers">
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                      <circle cx="12" cy="12" r="3" />
                    </svg>
                    <span ref={targetViewersRef}>183 Zuschauer</span>
                  </div>
                </div>
              </div>
            </div>
            <div className="rd-info-bar">
              <div className="rd-info-left">
                <div className="rd-avatar" ref={targetAvatarRef} style={{ background: '#A855F7' }}>P</div>
                <div>
                  <a className="rd-info-name rd-info-link" ref={targetInfoNameRef} href="https://twitch.tv/whysolowkey" target="_blank" rel="noopener noreferrer">whysolowkey</a>
                  <div className="rd-info-game">Spielt Deadlock</div>
                </div>
              </div>
            </div>
          </div>

        </div>
      </div>

      {/* Status Bar */}
      <div className="rd-status-bar">
        <div className="rd-pill" ref={pill0Ref}>① Stream endet</div>
        <span className="rd-pill-arrow">→</span>
        <div className="rd-pill" ref={pill1Ref}>② Partner suchen</div>
        <span className="rd-pill-arrow">→</span>
        <div className="rd-pill" ref={pill2Ref}>③ Raid ausführen</div>
        <span className="rd-pill-arrow">→</span>
        <div className="rd-pill" ref={pill3Ref}>④ Angekommen</div>
      </div>
      <div className="rd-raid-display">Raid #<span ref={raidIndexRef}>1</span></div>
    </>
  )
}
