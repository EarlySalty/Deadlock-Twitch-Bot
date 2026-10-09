import React from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { TikTokPostDialog } from '../src/components/socialmedia/TikTokPostDialog';
import '../src/index.css';
import '../src/components/socialmedia/studio.css';

const originalFetch = window.fetch;
let draft: unknown = null;
let defaults: unknown = { caption: 'Meine Beschreibung #deadlock', privacy_level: 'SELF_ONLY', allow_comment: true, allow_duet: true, allow_stitch: false, commercial_content: false, brand_organic_toggle: false, brand_content_toggle: false };
window.fetch = async (input, init) => {
  const url = String(input);
  if (!url.includes('/social-media/api/')) return originalFetch(input, init);
  const reply = (data: unknown) => Promise.resolve(new Response(JSON.stringify(data), { headers: { 'content-type': 'application/json' } }));
  if (url.endsWith('/preview')) return reply({ status: 'ready', ready: true, clip_db_id: 124789 });
  if (url.endsWith('/creator-info')) return reply({ creator: { creator_username: 'earlysalty', creator_nickname: 'EarlySalty', privacy_level_options: ['PUBLIC_TO_EVERYONE', 'SELF_ONLY'], comment_disabled: true, duet_disabled: false, stitch_disabled: false, max_video_post_duration_sec: 300 }, caption: 'Ein guter Teamfight #deadlock', duration_seconds: 34, credential_id: 1, platform_user_id: 'fixture-only', approved_video_sha256: 'fixture-only' });
  if (url.endsWith('/editor')) return reply({ draft, defaults });
  if (url.endsWith('/draft')) { draft = JSON.parse(String(init?.body)); return reply({ success: true }); }
  if (url.endsWith('/defaults')) { defaults = JSON.parse(String(init?.body)); return reply({ success: true }); }
  return new Response(null, { status: 404 });
};
createRoot(document.getElementById('root')!).render(<QueryClientProvider client={new QueryClient()}><main className="social-studio"><TikTokPostDialog clipDbId={124789} pending={false} error={null} onConfirm={() => { throw new Error('No external publishing in evidence fixture'); }} onClose={() => {}} /></main></QueryClientProvider>);
