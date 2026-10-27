import { isEnglishLanding } from "@/lib/landingLocale";
import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import "./index.css";
import "./theme-v2.css";
import "./streamer-v3.css";
import { StreamerNetworkV4Page } from "@/pages/StreamerNetworkV4Page";

if (typeof window !== "undefined") {
  const container = document.getElementById("root")!;

  if (isEnglishLanding()) container.replaceChildren();

  if (container.hasChildNodes()) {
    hydrateRoot(
      container,
      <StrictMode>
        <StreamerNetworkV4Page />
      </StrictMode>,
    );
  } else {
    createRoot(container).render(
      <StrictMode>
        <StreamerNetworkV4Page />
      </StrictMode>,
    );
  }
}
