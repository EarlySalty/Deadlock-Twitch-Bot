import { describe, expect, it, vi } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderToStaticMarkup } from 'react-dom/server';
import { useAiFeatures } from './useAiFeatures';
import { AIAnalysis } from '@/pages/AIAnalysis';
import { TitleGenerator } from '@/pages/TitleGenerator';
import { AIEngagementSection } from '@/components/verwaltung/AIEngagementSection';

vi.hoisted(() => {
  vi.stubGlobal('window', { location: { pathname: '/twitch/dashboard', origin: 'http://localhost', search: '' } });
});

vi.mock('@/api/core', () => ({ fetchApi: vi.fn() }));
vi.mock('@/hooks/useAnalytics', () => ({
  useAuthStatus: () => ({ data: { authenticated: true, twitchUserId: '42' } }),
}));

function Probe({ streamer }: { streamer?: string }) {
  return <span>{String(useAiFeatures(streamer))}</span>;
}

function render(data?: { aiFeatures: boolean }, streamer?: string) {
  const client = new QueryClient();
  if (data) client.setQueryData(['ai-capabilities', '42', streamer ?? null], data);
  return renderToStaticMarkup(<QueryClientProvider client={client}><Probe streamer={streamer} /></QueryClientProvider>);
}

describe('server-controlled AI visibility', () => {
  it('shows AI only after explicit authorization', () => {
    expect(render({ aiFeatures: true })).toBe('<span>true</span>');
    expect(render({ aiFeatures: false })).toBe('<span>false</span>');
    expect(render()).toBe('<span>false</span>');
  });

  it('does not reuse the home authorization for another selected streamer', () => {
    const client = new QueryClient();
    client.setQueryData(['ai-capabilities', '42', 'home'], { aiFeatures: true });
    client.setQueryData(['ai-capabilities', '42', 'foreign'], { aiFeatures: false });
    const html = renderToStaticMarkup(<QueryClientProvider client={client}><Probe streamer="foreign" /></QueryClientProvider>);
    expect(html).toBe('<span>false</span>');
  });

  it('does not mount foreign-server AI panels', () => {
    const client = new QueryClient();
    client.setQueryData(['ai-capabilities', '42', 'foreign'], { aiFeatures: false });
    client.setQueryData(['ai-capabilities', '42', null], { aiFeatures: false });
    const html = renderToStaticMarkup(<QueryClientProvider client={client}>
      <AIAnalysis streamer="foreign" days={30} />
      <TitleGenerator streamer="foreign" />
      <AIEngagementSection />
    </QueryClientProvider>);
    expect(html).toBe('');
  });
});
