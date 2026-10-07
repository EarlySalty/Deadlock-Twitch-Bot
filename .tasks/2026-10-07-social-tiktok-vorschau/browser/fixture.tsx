import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { TikTokPostDialog } from '../../../bot/dashboard_v2/src/components/socialmedia/TikTokPostDialog';
import './fixture.css';
import '../../../bot/dashboard_v2/src/components/socialmedia/studio.css';

type PreviewState = null | 'pending' | 'rendering' | 'ready' | 'error';
const initial = new URLSearchParams(location.search).get('state');
let state: PreviewState = initial === 'missing' || !initial ? null : initial as PreviewState;
const requests: { method: string; path: string; state: PreviewState }[] = [];
let confirmations = 0;
const creator = {
  caption: 'Synthetischer Clip für den isolierten Vorschaunachweis',
  duration_seconds: 24,
  credential_id: 999001,
  platform_user_id: 'synthetic-only',
  approved_video_sha256: '0'.repeat(64),
  creator: {
    creator_username: 'fixture_only',
    creator_nickname: 'Isoliertes Testkonto',
    privacy_level_options: ['PUBLIC_TO_EVERYONE', 'SELF_ONLY'],
    comment_disabled: false,
    duet_disabled: true,
    stitch_disabled: false,
    max_video_post_duration_sec: 60,
  },
};
window.fetch = async (input, init) => {
  const path = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, location.origin).pathname;
  const method = init?.method ?? 'GET';
  requests.push({ method, path, state });
  if (path === '/twitch/social-media/api/admin/clips/999001/preview') {
    if (method === 'POST') state = 'pending';
    return new Response(JSON.stringify({ clip_db_id: 999001, status: state, ready: state === 'ready', error: state === 'error' ? 'Synthetischer Renderfehler' : null }), { headers: { 'Content-Type': 'application/json' } });
  }
  if (path === '/twitch/social-media/api/clips/999001/tiktok/creator-info' && method === 'GET') {
    return new Response(JSON.stringify(creator), { headers: { 'Content-Type': 'application/json' } });
  }
  return new Response(JSON.stringify({ error: 'fixture_request_blocked', message: 'Die isolierte Prüfung lässt diesen Aufruf nicht zu.' }), { status: 410, headers: { 'Content-Type': 'application/json' } });
};
const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0, refetchOnWindowFocus: false } } });
function summary() {
  return {
    state, requests: [...requests], confirmations,
    statusGets: requests.filter(r => r.method === 'GET' && r.path.endsWith('/preview')).length,
    renderPosts: requests.filter(r => r.method === 'POST' && r.path.endsWith('/preview')).length,
    creatorGets: requests.filter(r => r.path.endsWith('/creator-info')).length,
    dialogOpen: !!document.querySelector('dialog[open]'),
    text: document.querySelector('dialog')?.textContent,
    options: Array.from(document.querySelectorAll('select option')).map(n => ({ value: (n as HTMLOptionElement).value, text: n.textContent })),
    selectedPrivacy: (document.querySelector('select') as HTMLSelectElement | null)?.value,
    checkboxes: Array.from(document.querySelectorAll('dialog input[type=checkbox]')).map(n => ({ checked: (n as HTMLInputElement).checked, disabled: (n as HTMLInputElement).disabled })),
    submitDisabled: (document.querySelector('button[type=submit]') as HTMLButtonElement | null)?.disabled,
  };
}
Object.assign(window, { previewEvidence: { setState: (next: PreviewState) => { state = next; }, summary } });
function Fixture() {
  const [visible, setVisible] = useState(true);
  const [label, setLabel] = useState(state ?? 'missing');
  return <QueryClientProvider client={client}>
    <main className="p-5"><h1>Isolierter TikTok-Vorschaunachweis</h1><p>Nur synthetische Daten. Keine Verbindung zu einem Konto.</p>
      <div>{(['missing', 'pending', 'rendering', 'ready', 'error'] as const).map(next => <button key={next} className="m-2 rounded border p-2" onClick={() => { state = next === 'missing' ? null : next; setLabel(next); }}>{next}</button>)}</div>
      <p>Fixture-Auswahl: {label}</p><button onClick={() => setVisible(true)}>Dialog öffnen</button>
      {visible && <TikTokPostDialog clipDbId={999001} pending={false} error={null} onConfirm={() => { confirmations += 1; }} onClose={() => setVisible(false)} />}
    </main>
  </QueryClientProvider>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><Fixture /></StrictMode>);
