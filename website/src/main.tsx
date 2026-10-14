import { isEnglishLanding } from "@/lib/landingLocale";
import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import "./index.css";
// Standard-Streamer-Theme (Patch-Schwarz), global ueber index.css gelegt.
import "./theme-v2.css";
import App from "./App";

if (typeof window !== "undefined") {
  const container = document.getElementById("root")!;

  if (isEnglishLanding()) container.replaceChildren();

  if (container.hasChildNodes()) {
    hydrateRoot(
      container,
      <StrictMode>
        <App />
      </StrictMode>,
    );
  } else {
    createRoot(container).render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
  }
}

export async function prerender() {
  const { renderToString } = await import("react-dom/server");
  return renderToString(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
