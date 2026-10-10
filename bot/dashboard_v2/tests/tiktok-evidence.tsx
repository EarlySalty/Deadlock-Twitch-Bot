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
let draft: unknown = null;
let defaults: unknown = { caption: 'Meine Beschreibung #deadlock', privacy_level: 'SELF_ONLY', allow_comment: true, allow_duet: true, allow_stitch: false, commercial_content: false, brand_organic_toggle: false, brand_content_toggle: false };
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
  if (url.endsWith('/editor')) return reply({ draft, defaults });
  if (url.endsWith('/draft')) { draft = JSON.parse(String(init?.body)); return reply({ success: true, choices: draft }); }
  if (url.endsWith('/defaults')) {
    const choices = JSON.parse(String(init?.body));
    if (choices.music_consent) choices.music_consent_at = '2026-10-10T12:00:00Z';
    defaults = choices;
    return reply({ success: true, choices });
  }
  return new Response(null, { status: 404 });
};
createRoot(document.getElementById('root')!).render(<QueryClientProvider client={queryClient}><main className="social-studio"><TikTokPostDialog clipDbId={124789} pending={false} error={null} onConfirm={(options) => { document.body.dataset.tiktokPostOptions = JSON.stringify(options); }} onClose={() => {}} /></main></QueryClientProvider>);
