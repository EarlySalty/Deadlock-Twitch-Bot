import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { AiEditorPanel } from './AiEditorPanel';
import { aiPreviewScenarios, createAiPreviewFetch, type AiPreviewScenario } from './preview';
import '@/index.css';
import '../studio.css';

if (import.meta.env.MODE !== 'preview' || !['localhost', '127.0.0.1'].includes(window.location.hostname)) {
  throw new Error('Diese Vorschau ist ausschließlich für die lokale Entwicklung vorgesehen.');
}

const selected = new URLSearchParams(window.location.search).get('case');
const scenario: AiPreviewScenario = aiPreviewScenarios.find((item) => item === selected) ?? 'review';
window.fetch = createAiPreviewFetch(window.fetch.bind(window), scenario);

const labels: Record<AiPreviewScenario, string> = {
  'consent-off': 'Ohne Zustimmung',
  'global-off': 'Globale Sperre',
  render: 'Videoerstellung',
  review: 'Sichtung',
  failed: 'Fehlgeschlagen',
  handoff: 'Clip-Übergabe',
  race: 'Kanalwechsel',
  revisions: 'Revisionen und Freigaben',
};

export function PreviewShell() {
  const [channel, setChannel] = useState('123456789');
  return <div className="social-studio" style={{ minHeight: '100vh', background: 'var(--color-background)', padding: 'clamp(16px, 4vw, 48px)' }}>
    <div style={{ maxWidth: 1180, margin: '0 auto', display: 'grid', gap: 24 }}>
      <header style={{ borderBottom: '1px solid var(--color-border)', paddingBottom: 20 }}><p style={{ color: 'var(--color-primary)', fontWeight: 700 }}>LOKALE OBERFLÄCHENVORSCHAU · SYNTHETISCHE BEISPIELDATEN</p><nav aria-label="Vorschauzustände" style={{ display: 'flex', flexWrap: 'wrap', gap: 12, marginTop: 12 }}>{aiPreviewScenarios.map((item) => <a key={item} className="studio-button" aria-current={item === scenario ? 'page' : undefined} href={`?case=${item}`}>{labels[item]}</a>)}</nav><p style={{ color: 'var(--color-text-secondary)', fontSize: 13, marginTop: 12 }}>Diese Vorschau verwendet weder einen echten Kanal noch einen KI-Anbieter. Die Videos zeigen synthetisches Testmaterial.</p><button className="studio-button" style={{ marginTop: 12 }} onClick={() => setChannel(channel === '123456789' ? '987654321' : '123456789')}>Vorschaukanal wechseln</button></header>
      <AiEditorPanel twitchUserId={channel} streamer={channel === '123456789' ? 'Beispielkanal' : 'Zweiter Beispielkanal'} clipDbId={scenario === 'handoff' ? 135 : 501} />
    </div>
  </div>;
}

createRoot(document.getElementById('root')!).render(<React.StrictMode><QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><PreviewShell /></QueryClientProvider></React.StrictMode>);
