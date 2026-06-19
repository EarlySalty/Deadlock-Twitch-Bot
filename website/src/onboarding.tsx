import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import { StreamerOnboardingPage } from "@/pages/StreamerOnboardingPage";
import { SiteChatbot } from "@/components/layout/SiteChatbot";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <StreamerOnboardingPage />
    <SiteChatbot />
  </StrictMode>,
);
