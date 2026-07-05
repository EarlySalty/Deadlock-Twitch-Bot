import { useEffect } from "react";

/**
 * Scroll-Reveal als Progressive Enhancement: ohne JS bleibt alles sichtbar
 * (`.reveal` ist per Default sichtbar, erst `.js .reveal` blendet aus).
 */
export function useReveal(): void {
  useEffect(() => {
    document.documentElement.classList.add("js");
    const elements = Array.from(document.querySelectorAll<HTMLElement>(".reveal"));
    if (!("IntersectionObserver" in window)) {
      elements.forEach((el) => el.classList.add("is-visible"));
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
    return () => observer.disconnect();
  }, []);
}
