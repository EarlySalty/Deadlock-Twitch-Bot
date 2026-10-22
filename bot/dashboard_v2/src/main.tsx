import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { onPublicTour } from './publicTour'
import { installTourGuard } from './tourGuard'
import { installStaleChunkReload } from './staleChunk'

installStaleChunkReload(window)
if (onPublicTour()) installTourGuard(window)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
