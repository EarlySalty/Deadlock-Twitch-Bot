import { useState } from 'react';
import { useLanguage } from '../../src/context/LanguageContext';
import { PartnerProfile } from '../../src/pages/PartnerProfile';
import { StreamReports } from '../../src/pages/StreamReports';
import { ModerationSection } from '../../src/components/verwaltung/ModerationSection';
import { ScamGuardSection } from '../../src/components/verwaltung/ScamGuardSection';
import { surface, client, fixture, auth } from './languageDraftsFixture';

export function LanguageDraftsSurface() {
  const { setLanguage } = useLanguage();
  const [owner, setOwner] = useState('alice');
  return (
    <main className="mx-auto max-w-[1400px] p-4 md:p-8">
      <nav className="mb-4 flex gap-4 text-white">
        <button data-testid="de" onClick={() => setLanguage('de')}>
          Deutsch
        </button>
        <button data-testid="en" onClick={() => setLanguage('en')}>
          English
        </button>
        <button data-testid="refresh" onClick={() => void client.invalidateQueries()}>
          Refresh
        </button>
        <button
          data-testid="owner"
          onClick={() => {
            fixture.owner = owner === 'alice' ? 'bob' : 'alice';
            client.setQueryData(['auth-status'], auth(fixture.owner));
            setOwner(fixture.owner);
          }}
        >
          Owner
        </button>
      </nav>
      {surface === 'profile' && <PartnerProfile />}
      {surface === 'reports' && <StreamReports streamer={owner} days={30} />}
      {surface === 'settings' && (
        <div className="space-y-5">
          <ModerationSection />
          <ScamGuardSection />
        </div>
      )}
    </main>
  );
}
