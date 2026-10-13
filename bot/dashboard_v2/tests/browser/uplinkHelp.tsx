import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Header } from '../../src/components/layout/Header';
import { LanguageProvider } from '../../src/context/LanguageContext';
import { PlanProvider } from '../../src/context/PlanContext';
import { UplinkPage } from '../../src/pages/Uplink';
import { storeLanguage } from '../../src/i18n/dictionary';
import '../../src/index.css';

storeLanguage('en');

const client = new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } } });
const root = document.getElementById('root');
if (!root) throw new Error('Missing fixture root');

createRoot(root, {
  onUncaughtError: (error) => { root.setAttribute('data-render-error', error instanceof Error ? `${error.message}\n${error.stack}` : String(error)); },
}).render(
  <QueryClientProvider client={client}>
    <LanguageProvider onLanguageChange={() => { void client.invalidateQueries(); }}>
      <PlanProvider plan={null} isAdmin={false} isLocalhost isDemoMode>
        <div className="internal-home-vibe min-h-screen p-3">
          <Header streamer="midcore_live" streamers={[]} days={30} onStreamerChange={() => {}} onDaysChange={() => {}} />
          <UplinkPage />
        </div>
      </PlanProvider>
    </LanguageProvider>
  </QueryClientProvider>,
);
