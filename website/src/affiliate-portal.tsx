import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import AffiliatePortal from "@/pages/AffiliatePortal";
import { SiteChatbot } from "@/components/layout/SiteChatbot";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AffiliatePortal />
    <SiteChatbot />
  </StrictMode>,
);
