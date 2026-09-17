import { StrictMode } from 'react'
import { createRoot, hydrateRoot } from 'react-dom/client'
import './index.css'
import './theme-v2.css'
import './streamer-v3.css'
import { StreamerNetworkV3Page } from '@/pages/StreamerNetworkV3Page'

if (typeof window !== 'undefined') {
  const container = document.getElementById('root')!

  if (container.hasChildNodes()) {
    hydrateRoot(
      container,
      <StrictMode>
        <StreamerNetworkV3Page />
      </StrictMode>,
    )
  } else {
    createRoot(container).render(
      <StrictMode>
        <StreamerNetworkV3Page />
      </StrictMode>,
    )
  }
}

export async function prerender() {
  const { renderToString } = await import('react-dom/server')
  return renderToString(
    <StrictMode>
      <StreamerNetworkV3Page />
    </StrictMode>,
  )
}
