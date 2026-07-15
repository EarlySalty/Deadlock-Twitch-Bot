import { StrictMode } from 'react'
import { createRoot, hydrateRoot } from 'react-dom/client'
import './index.css'
// Inert ohne data-theme="v2" (nur die /streamer/v2/-Preview setzt das Attribut).
import './theme-v2.css'
import App from './App'

if (typeof window !== 'undefined') {
  const container = document.getElementById('root')!

  if (container.hasChildNodes()) {
    hydrateRoot(
      container,
      <StrictMode>
        <App />
      </StrictMode>,
    )
  } else {
    createRoot(container).render(
      <StrictMode>
        <App />
      </StrictMode>,
    )
  }
}

export async function prerender() {
  const { renderToString } = await import('react-dom/server')
  return renderToString(
    <StrictMode>
      <App />
    </StrictMode>,
  )
}
