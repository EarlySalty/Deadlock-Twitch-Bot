import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Viewers } from '../../src/pages/Viewers';
import type { TimeRange } from '../../src/types/analytics';
import '../../src/index.css';

const client = new QueryClient({
  defaultOptions: { queries: { retry: false, gcTime: 0, refetchOnWindowFocus: false } },
});

export function Fixture() {
  const [streamer, setStreamer] = useState('test_channel');
  const [days, setDays] = useState<TimeRange>(30);
  const [visible, setVisible] = useState(true);
  return (
    <QueryClientProvider client={client}>
      <button onClick={() => setStreamer('other_channel')}>Kanal wechseln</button>
      <button onClick={() => setDays(7)}>Zeitraum wechseln</button>
      <button onClick={() => setVisible(false)}>Ansicht schließen</button>
      {visible && <Viewers streamer={streamer} days={days} />}
    </QueryClientProvider>
  );
}

createRoot(document.getElementById('root')!).render(<StrictMode><Fixture /></StrictMode>);
document.body.dataset.fixture = new URLSearchParams(location.search).get('case') || '';
