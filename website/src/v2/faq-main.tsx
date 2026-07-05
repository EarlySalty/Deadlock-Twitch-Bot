import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import "./brand.css";
import "./ui.css";
import { FaqPage } from "./pages/FaqPage";

if (typeof window !== "undefined") {
  const container = document.getElementById("root")!;
  const app = (
    <StrictMode>
      <FaqPage />
    </StrictMode>
  );
  if (container.hasChildNodes()) {
    hydrateRoot(container, app);
  } else {
    createRoot(container).render(app);
  }
}

export async function prerender() {
  const { renderToString } = await import("react-dom/server");
  return renderToString(
    <StrictMode>
      <FaqPage />
    </StrictMode>,
  );
}
