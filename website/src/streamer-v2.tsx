import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
// Gleiches Patch-Schwarz-Theme wie die produktive Landing.
import './theme-v2.css'
import './streamer-v2.css'
import { StreamerNetworkPage } from '@/pages/StreamerNetworkPage'

const container = document.getElementById('root')!
createRoot(container).render(
  <StrictMode>
    <StreamerNetworkPage />
  </StrictMode>,
)
