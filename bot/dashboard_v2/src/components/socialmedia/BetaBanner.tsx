import { FlaskConical } from 'lucide-react';
import { useT } from '@/context/LanguageContext';

const NANI_DISCORD_URL = 'https://discord.com/users/662995601738170389';

export function SocialMediaBetaBanner({ onDismiss }: { onDismiss: () => void }) {
  const t = useT();
  return (
    <div data-recording-block className="fixed bottom-4 left-4 right-4 z-40 mx-auto max-w-2xl">
      <aside aria-labelledby="social-early-beta-title" className="panel-card rounded-2xl border-primary/30 p-4 shadow-lg">
        <div className="flex items-start gap-3">
          <FlaskConical aria-hidden="true" className="mt-0.5 h-5 w-5 shrink-0 text-primary" />
          <div className="min-w-0">
            <h2 id="social-early-beta-title" className="text-sm font-semibold text-white">{t('Early Beta')}</h2>
            <p className="mt-1 text-sm leading-6 text-text-secondary">{t('Das Dashboard ist noch ganz frisch und hat bestimmt noch viele Ecken und Kanten. Wenn dich etwas stört, falsch ist, fehlt oder du etwas nicht verstehst, schreib mir einfach auf Discord. Auch Kleinigkeiten helfen.')}</p>
            <div className="mt-3 flex flex-wrap gap-2">
              <a href={NANI_DISCORD_URL} target="_blank" rel="noopener noreferrer" className="inline-flex min-h-10 items-center justify-center rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806]">{t('Nani auf Discord schreiben')}</a>
              <button type="button" onClick={onDismiss} className="studio-button min-h-10 text-sm">{t('Verstanden')}</button>
            </div>
          </div>
        </div>
      </aside>
    </div>
  );
}
