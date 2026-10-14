import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { AiJobView } from '../../ai/api';
import { createAiPreviewFetch } from '../../ai/preview';
import { createRoot } from 'react-dom/client';
import { PublishReviewPanel } from './PublishReviewPanel';
import { createPublishReviewFixture } from './fixtures';
import '../../../../index.css';
import '../../studio.css';

const fixture = createPublishReviewFixture();
const initial = fixture.snapshot();
const reviewVideo = { ...initial.videos[0], id: 2203, version_no: 3, duration_ms: 38000 };
fixture.replace({ ...initial, videos: [reviewVideo, ...initial.videos.slice(1)] });
const queryClient = new QueryClient();
const aiFetch = createAiPreviewFetch(window.fetch.bind(window), 'revisions', true);
window.fetch = async (input, init) => {
  const response = await aiFetch(input, init);
  const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, window.location.origin);
  if (url.pathname.includes('/social-media/api/ai/') && response.ok) {
    const value = await response.clone().json();
    const job = (value.job ?? value) as Partial<AiJobView>;
    if (Array.isArray(job.versions)) {
      const current = fixture.snapshot();
      fixture.replace({ ...current, videos: [...current.videos.filter((video) => video.clip_id !== reviewVideo.clip_id), ...job.versions.filter((version) => version.status === 'review').map((version) => ({ ...reviewVideo, id: version.id, version_no: version.version_no, duration_ms: version.duration_ms, approvals: [], event: null }))] });
    }
  }
  return response;
};
Object.assign(window, { publishReviewFixture: fixture });
const root = document.getElementById('root');
if (!root) throw new Error('Fixture root missing');
createRoot(root).render(<QueryClientProvider client={queryClient}><main className="social-studio" style={{ maxWidth: 1100, margin: '0 auto', padding: '28px 20px', background: 'var(--color-bg)' }}><p style={{ color: 'var(--color-text-secondary)', marginBottom: 20 }}>Lokale Vorschau mit synthetischen Daten. Keine Veröffentlichung und kein Nachrichtenversand.</p><PublishReviewPanel twitchUserId="123456789" client={fixture.client} onOpenVideo={(id) => { document.title = `Lokale Videoprüfung ${id}`; }} /></main></QueryClientProvider>);
