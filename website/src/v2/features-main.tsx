import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import "./brand.css";
import "./ui.css";
import { FeaturesPage } from "./pages/FeaturesPage";

if (typeof window !== "undefined") {
  const container = document.getElementById("root")!;
  const app = (
    <StrictMode>
      <FeaturesPage />
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
      <FeaturesPage />
    </StrictMode>,
  );
}
