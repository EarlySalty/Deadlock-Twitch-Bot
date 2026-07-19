import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import "./theme-v2.css";
import { StreamerComparisonPage } from "@/pages/StreamerComparisonPage";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <StreamerComparisonPage />
  </StrictMode>,
);
