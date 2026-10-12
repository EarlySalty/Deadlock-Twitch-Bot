import { createRoot } from 'react-dom/client';
import { ScheduleQueuePanel } from './ScheduleQueuePanel';
import { createScheduleQueueFixture } from './fixtures';
import '@/index.css';
import '../../studio.css';
import './fixture-host.css';

const fixtures = new Map<string, ReturnType<typeof createScheduleQueueFixture>>();
const root = document.getElementById('root');
if (root) {
  const host = createRoot(root);
  const mountOwner = (owner: string) => {
    const fixture = fixtures.get(owner) ?? createScheduleQueueFixture();
    fixtures.set(owner, fixture);
    Object.assign(window, { scheduleQueueFixture: fixture });
    host.render(<main className="social-studio sq-fixture-host"><p className="sq-fixture-note">Lokale Prüfansicht mit Beispieldaten. Keine Veröffentlichung.</p><ScheduleQueuePanel twitchUserId={owner} api={fixture.api} /></main>);
  };
  Object.assign(window, { setScheduleQueueOwner: mountOwner });
  mountOwner('42');
}
