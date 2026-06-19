import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import AffiliateProgramPage from "./pages/AffiliateProgramPage";
import { SiteChatbot } from "@/components/layout/SiteChatbot";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AffiliateProgramPage />
    <SiteChatbot />
  </StrictMode>,
);
