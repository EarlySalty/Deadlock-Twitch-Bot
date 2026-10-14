import React from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { TikTokPostDialog } from '../src/components/socialmedia/TikTokPostDialog';
import '../src/index.css';
import '../src/components/socialmedia/studio.css';

const originalFetch = window.fetch;
const queryClient = new QueryClient();
let account = 'fixture-only';
window.addEventListener('tiktok-fixture-account', (event) => {
  account = (event as CustomEvent<string>).detail;
  void queryClient.invalidateQueries({ queryKey: ['social-media', 'tiktok-creator'] });
});
let editorError = false;
window.addEventListener('tiktok-fixture-editor-error', (event) => {
  editorError = (event as CustomEvent<boolean>).detail;
});
window.addEventListener('tiktok-fixture-editor-cache', () => {
  document.body.dataset.tiktokEditorCache = JSON.stringify(queryClient.getQueryData(['social-media', 'tiktok-editor', 124789]));
});
let draft: unknown = null;
let consentAccount: string | null = new URLSearchParams(window.location.search).has('consent') ? account : null;
let defaults: unknown = { caption: 'Alte vollständige Beschreibung', standard_description: '#deadlock', privacy_level: 'SELF_ONLY', allow_comment: true, allow_duet: true, allow_stitch: false, commercial_content: false, brand_organic_toggle: false, brand_content_toggle: false };
window.fetch = async (input, init) => {
  const url = String(input);
  if (!url.includes('/social-media/api/')) return originalFetch(input, init);
  const reply = (data: unknown) => Promise.resolve(new Response(JSON.stringify(data), { headers: { 'content-type': 'application/json' } }));
  if (init?.method === 'PUT') {
    const writes = JSON.parse(document.body.dataset.tiktokEditorWrites ?? '[]');
    writes.push({ url, body: JSON.parse(String(init.body)) });
    document.body.dataset.tiktokEditorWrites = JSON.stringify(writes);
  }
  if (url.endsWith('/preview')) return reply({ status: 'ready', ready: true, clip_db_id: 124789 });
  if (url.endsWith('/creator-info')) return reply({ creator: { creator_username: 'earlysalty', creator_nickname: 'EarlySalty', privacy_level_options: ['PUBLIC_TO_EVERYONE', 'SELF_ONLY'], comment_disabled: true, duet_disabled: false, stitch_disabled: false, max_video_post_duration_sec: 300 }, caption: 'Ein guter Teamfight #deadlock', duration_seconds: 34, credential_id: 1, platform_user_id: account, approved_video_sha256: 'fixture-only' });
  if (url.endsWith('/editor')) return editorError
    ? new Response(JSON.stringify({ error: 'fixture_editor_unavailable' }), { status: 503, headers: { 'content-type': 'application/json' } })
    : reply({ draft, defaults, music_consent: consentAccount === account, music_consent_platform_user_id: account });
  if (url.endsWith('/music-consent')) {
    const request = JSON.parse(String(init?.body));
    if (request.platform_user_id !== account) return new Response(null, { status: 409 });
    consentAccount = account;
    return reply({ success: true });
  }
  if (url.endsWith('/draft')) { draft = JSON.parse(String(init?.body)); return reply({ success: true, choices: draft }); }
  if (url.endsWith('/defaults')) {
    const choices = JSON.parse(String(init?.body));
    choices.caption = '';
    choices.commercial_content = false;
    choices.brand_organic_toggle = false;
    choices.brand_content_toggle = false;
    defaults = choices;
    return reply({ success: true, choices });
  }
  return new Response(null, { status: 404 });
};
createRoot(document.getElementById('root')!).render(<QueryClientProvider client={queryClient}><main className="social-studio"><TikTokPostDialog clipDbId={124789} pending={false} error={null} onConfirm={(options) => { document.body.dataset.tiktokPostOptions = JSON.stringify(options); }} onClose={() => {}} /></main></QueryClientProvider>);
