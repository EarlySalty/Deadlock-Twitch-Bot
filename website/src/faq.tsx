import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import { BotFaqPage } from "@/pages/BotFaqPage";
import { SiteChatbot } from "@/components/layout/SiteChatbot";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BotFaqPage />
    <SiteChatbot />
  </StrictMode>,
);
