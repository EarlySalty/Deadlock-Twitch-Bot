type TrustpilotQueue = ((...args: unknown[]) => void) & { q?: unknown[][] };

declare global {
  interface Window {
    Trustpilot?: { loadFromElement: (element: HTMLElement) => void };
    TrustpilotObject?: string;
    tp?: TrustpilotQueue;
  }
}

export const TRUSTPILOT_REVIEW_URL =
  "https://de.trustpilot.com/evaluate/deutsche-deadlock-community.de";
export const TRUSTPILOT_PROFILE_URL =
  "https://de.trustpilot.com/review/deutsche-deadlock-community.de";

const scripts = new Map<string, Promise<void>>();
let registered = false;

function loadScript(src: string): Promise<void> {
  const pending = scripts.get(src);
  if (pending) return pending;

  const loading = new Promise<void>((resolve, reject) => {
    const script = document.createElement("script");
    script.src = src;
    script.type = "text/javascript";
    script.async = true;
    script.addEventListener("load", () => resolve(), { once: true });
    script.addEventListener("error", () => {
      scripts.delete(src);
      script.remove();
      reject(new Error(`Trustpilot konnte nicht geladen werden: ${src}`));
    }, { once: true });
    document.head.appendChild(script);
  });
  scripts.set(src, loading);
  return loading;
}

export async function loadTrustpilotWidget(element: HTMLElement): Promise<void> {
  if (!window.Trustpilot) {
    await loadScript("https://widget.trustpilot.com/bootstrap/v5/tp.widget.bootstrap.min.js");
  }
  // Nach SPA-Navigation initialisiert Bootstrap den neu gerenderten Container.
  if (element.isConnected && !element.querySelector("iframe")) {
    if (!window.Trustpilot) throw new Error("Trustpilot-Widget ist nicht verfügbar");
    window.Trustpilot.loadFromElement(element);
  }
}

export async function registerTrustpilot(): Promise<void> {
  if (!registered) {
    window.TrustpilotObject = "tp";
    window.tp ??= Object.assign((...args: unknown[]) => {
      (window.tp!.q ??= []).push(args);
    }, { q: [] as unknown[][] });
    window.tp("register", "sHQ1P2OxAlG0YV4f");
    registered = true;
  }
  await loadScript("https://invitejs.trustpilot.com/tp.min.js");
}
