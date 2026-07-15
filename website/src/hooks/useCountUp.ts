import { useState, useEffect, useRef } from "react";
import type { RefObject } from "react";

function easeOutQuart(t: number): number {
  return 1 - Math.pow(1 - t, 4);
}

export interface UseCountUpResult {
  count: number;
  ref: RefObject<HTMLElement | null>;
}

/**
 * Animated counter that starts when the attached element enters the viewport.
 * Re-animiert, wenn sich `end` aendert (z.B. wenn eine live nachgeladene Zahl
 * den Fallback ersetzt) — auch dann, wenn das Element schon sichtbar war.
 * @param end      - The target number to count up to.
 * @param duration - Animation duration in milliseconds (default: 2000).
 */
export function useCountUp(end: number, duration = 2000): UseCountUpResult {
  const [count, setCount] = useState(0);
  const ref = useRef<HTMLElement | null>(null);
  const [visible, setVisible] = useState(false);
  const countRef = useRef(0);

  // Aktuellen count spiegeln, damit eine Neu-Animation vom sichtbaren Wert
  // (statt von 0) zum neuen Ziel laeuft, wenn `end` sich spaeter aendert.
  useEffect(() => {
    countRef.current = count;
  }, [count]);

  // Sichtbarkeit einmalig erkennen.
  useEffect(() => {
    const element = ref.current;
    if (!element || visible) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { threshold: 0.2 },
    );

    observer.observe(element);
    return () => observer.disconnect();
  }, [visible]);

  // Animation laeuft, sobald sichtbar — und erneut bei jeder `end`-Aenderung.
  useEffect(() => {
    if (!visible) return;

    const startValue = countRef.current;
    const startTime = performance.now();
    let raf = 0;

    function tick(now: number) {
      const progress = Math.min((now - startTime) / duration, 1);
      const eased = easeOutQuart(progress);
      setCount(Math.round(startValue + (end - startValue) * eased));
      if (progress < 1) raf = requestAnimationFrame(tick);
    }

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [visible, end, duration]);

  return { count, ref };
}
