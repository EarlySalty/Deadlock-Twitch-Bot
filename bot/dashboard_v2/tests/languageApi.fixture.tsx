import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LanguageProvider, useLanguage } from '../src/context/LanguageContext';
import { DashboardAssistent } from '../src/components/assistent/DashboardAssistent';
import '../src/index.css';

const client = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: Infinity,
      retry: false,
      refetchOnMount: false,
      refetchOnWindowFocus: false,
      refetchInterval: false,
    },
  },
});
client.setQueryData(['auth-status'], {
  authenticated: true,
  displayName: 'Synthetic language contract',
});

export function Consumer() {
  const { setLanguage } = useLanguage();
  return (
    <>
      <nav aria-label="Language contract controls">
        <button type="button" onClick={() => setLanguage('de')}>
          Deutsch
        </button>
        <button type="button" onClick={() => setLanguage('en')}>
          English
        </button>
      </nav>
      <DashboardAssistent imDokumentfluss />
    </>
  );
}

const root = document.getElementById('root');
if (!root) throw new Error('Missing contract root');
createRoot(root).render(
  <QueryClientProvider client={client}>
    <LanguageProvider>
      <Consumer />
    </LanguageProvider>
  </QueryClientProvider>
);
