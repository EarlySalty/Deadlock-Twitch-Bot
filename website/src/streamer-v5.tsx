import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import "./streamer-v5.css";
import { StreamerNetworkV5Page } from "@/pages/StreamerNetworkV5Page";

if (typeof window !== "undefined") {
  const container = document.getElementById("root")!;
  const app = (
    <StrictMode>
      <StreamerNetworkV5Page />
    </StrictMode>
  );
  if (container.hasChildNodes()) {
    hydrateRoot(container, app);
  } else {
    createRoot(container).render(app);
  }
}
