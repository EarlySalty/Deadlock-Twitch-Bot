import { createRoot } from 'react-dom/client';
import { QueryClientProvider } from '@tanstack/react-query';
import App from '../../src/App';
import { LanguageProvider } from '../../src/context/LanguageContext';
import { PlanProvider } from '../../src/context/PlanContext';
import { LanguageDraftsSurface } from './LanguageDraftsSurface';
import { surface, client } from './languageDraftsFixture';
import '../../src/index.css';

const root = document.getElementById('root');
if (!root) throw new Error('Missing fixture root');
createRoot(root).render(
  surface === 'app' ? (
    <App />
  ) : (
    <QueryClientProvider client={client}>
      <LanguageProvider>
        <PlanProvider plan={null} isAdmin isLocalhost={false}>
          <LanguageDraftsSurface />
        </PlanProvider>
      </LanguageProvider>
    </QueryClientProvider>
  ),
);
