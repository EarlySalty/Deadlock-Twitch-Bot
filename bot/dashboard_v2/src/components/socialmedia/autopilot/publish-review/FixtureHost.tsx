import { createRoot } from 'react-dom/client';
import { PublishReviewPanel } from './PublishReviewPanel';
import { createPublishReviewFixture } from './fixtures';
import '../../../../index.css';
import '../../studio.css';

const fixture = createPublishReviewFixture();
Object.assign(window, { publishReviewFixture: fixture });
const root = document.getElementById('root');
if (!root) throw new Error('Fixture root missing');
createRoot(root).render(<main className="social-studio" style={{ maxWidth: 1100, margin: '0 auto', padding: '28px 20px', background: 'var(--color-bg, #1b1d21)' }}><p style={{ color: 'var(--color-text-secondary)', marginBottom: 20 }}>Lokale Vorschau mit synthetischen Daten. Keine Veröffentlichung und kein Nachrichtenversand.</p><PublishReviewPanel twitchUserId="synthetic-owner" client={fixture.client} onOpenVideo={(id) => { document.title = `Lokale Videoprüfung ${id}`; }} /></main>);
