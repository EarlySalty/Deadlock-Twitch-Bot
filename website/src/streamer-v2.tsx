import { StrictMode } from 'react'
import { createRoot, hydrateRoot } from 'react-dom/client'
import './index.css'
import './theme-v2.css'
import './streamer-v2.css'
import { StreamerNetworkPage } from '@/pages/StreamerNetworkPage'

if (typeof window !== 'undefined') {
  const container = document.getElementById('root')!

  if (container.hasChildNodes()) {
    hydrateRoot(
      container,
      <StrictMode>
        <StreamerNetworkPage />
      </StrictMode>,
    )
  } else {
    createRoot(container).render(
      <StrictMode>
        <StreamerNetworkPage />
      </StrictMode>,
    )
  }
}

export async function prerender() {
  const { renderToString } = await import('react-dom/server')
  return renderToString(
    <StrictMode>
      <StreamerNetworkPage />
    </StrictMode>,
  )
}
