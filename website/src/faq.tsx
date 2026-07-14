import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import "./faq-lobby.css";
import { FaqDoormanPage } from "@/pages/FaqDoormanPage";

/* Bewusst OHNE <SiteChatbot />: Diese Seite IST der Chat. Ein zweites
   Chat-Widget unten rechts, das denselben Endpoint fragt, waere ein
   zweiter Concierge im selben Raum. */
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <FaqDoormanPage />
  </StrictMode>,
);
