import { StrictMode } from 'react'
import { createRoot, hydrateRoot } from 'react-dom/client'
import './index.css'
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
