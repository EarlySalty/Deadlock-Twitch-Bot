import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { LanguageProvider, useLanguage } from '../../../../context/LanguageContext';
import { PublishReviewPanel } from './PublishReviewPanel';
import { createPublishReviewFixture } from './fixtures';
import '../../../../index.css';
import '../../studio.css';

const fixture = createPublishReviewFixture();
const initial = fixture.snapshot();
if (initial.videos[0]) initial.videos[0].duration_ms = 55425;
for (const connection of initial.connections) {
  if (connection.platform === 'tiktok') {
    connection.connected = true;
    connection.account_id = 'synthetic-tiktok';
  }
}
fixture.replace(initial);
const callbacks: { method: string; id: number; }[] = [];
let tikTokCancelled = false;

export function FixtureHost() {
  const { language, setLanguage, t } = useLanguage();
  const [owner, setOwner] = useState('synthetic-owner');
  const [caption, setCaption] = useState('Mein Spielzug bleibt unverändert. #Überraschung');
  useEffect(() => {
    Object.assign(window, { publishReviewFixture: { ...fixture, callbacks, setOwner, setLanguage, setTikTokCancelled: (value: boolean) => { tikTokCancelled = value; } } });
  }, [setLanguage]);
  return <main className="social-studio" style={{ maxWidth: 1100, margin: '0 auto', padding: '28px 20px', background: 'var(--color-bg)' }}>
    <p style={{ color: 'var(--color-text-secondary)', marginBottom: 20 }}>{t('Lokale Vorschau mit synthetischen Daten. Keine Veröffentlichung und kein Nachrichtenversand.')}</p>
    <label>{t('Sprache')} <select aria-label="Language" value={language} onChange={(event) => setLanguage(event.target.value === 'en' ? 'en' : 'de')}><option value="de">Deutsch</option><option value="en">English</option></select></label>
    <label>{t('Videotext (synthetisch)')} <textarea aria-label="Synthetic caption" value={caption} onChange={(event) => setCaption(event.target.value)} /></label>
    <PublishReviewPanel twitchUserId={owner} client={fixture.client} onOpenVideo={(id) => { callbacks.push({ method: 'open', id }); document.title = t('Lokale Videoprüfung {id}', { id }); }} onConfigureTikTok={async (id) => {
      callbacks.push({ method: 'configureTikTok', id });
      return tikTokCancelled ? null : { caption, privacy_level: 'SELF_ONLY', allow_comment: false, allow_duet: false, allow_stitch: false, commercial_content: false, brand_organic_toggle: false, brand_content_toggle: false, consent: true, creator_username: 'synthetic-creator', credential_id: 7, platform_user_id: 'synthetic-tiktok', approved_video_sha256: 'synthetic-hash', video_path: 'synthetic-video.mp4' };
    }} />
  </main>;
}

const root = document.getElementById('root');
if (!root) throw new Error('Fixture root missing');
createRoot(root).render(<LanguageProvider><FixtureHost /></LanguageProvider>);
