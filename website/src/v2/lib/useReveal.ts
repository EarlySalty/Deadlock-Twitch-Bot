import { useEffect } from "react";

/**
 * Scroll-Reveal als Progressive Enhancement: ohne JS bleibt alles sichtbar
 * (`.reveal` ist per Default sichtbar, erst `.js .reveal` blendet aus).
 */
export function useReveal(): void {
  useEffect(() => {
    document.documentElement.classList.add("js");
    const elements = Array.from(document.querySelectorAll<HTMLElement>(".reveal"));
    const showAll = () => elements.forEach((el) => el.classList.add("is-visible"));
    if (!("IntersectionObserver" in window)) {
      showAll();
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-visible");
            observer.unobserve(entry.target);
          }
        }
      },
      { threshold: 0.12 },
    );
    elements.forEach((el) => observer.observe(el));
    // Sicherheitsnetz: falls der Observer nie feuert (Snapshot-Rendering,
    // exotische Browser), bleibt nichts dauerhaft versteckt.
    const failsafe = window.setTimeout(showAll, 1200);
    return () => {
      window.clearTimeout(failsafe);
      observer.disconnect();
    };
  }, []);
}
