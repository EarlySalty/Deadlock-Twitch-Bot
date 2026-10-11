import { createRoot } from 'react-dom/client';
import { ScheduleQueuePanel } from './ScheduleQueuePanel';
import { createScheduleQueueFixture } from './fixtures';
import '@/index.css';
import '../../studio.css';
import './fixture-host.css';

const fixture = createScheduleQueueFixture();
Object.assign(window, { scheduleQueueFixture: fixture });
const root = document.getElementById('root');
if (root) createRoot(root).render(<main className="social-studio sq-fixture-host"><p className="sq-fixture-note">Lokale Prüfansicht mit Beispieldaten. Keine Veröffentlichung.</p><ScheduleQueuePanel twitchUserId="42" api={fixture.api} /></main>);
