import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '../index.css'
import './theme.css'
import App from '../App'

// Gleiche App wie v1, nur mit Teal-Gold-Theme. CSR ohne Prerender (noindex-Preview).
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
