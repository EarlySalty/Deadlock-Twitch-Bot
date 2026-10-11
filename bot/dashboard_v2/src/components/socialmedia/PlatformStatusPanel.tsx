import { CheckCircle2, CircleHelp, AlertCircle, Loader2 } from 'lucide-react';
import { useT } from '@/context/LanguageContext';
import type { PlatformStatus, SocialClipMitPosting } from '@/api/socialMedia';
import { PLATFORM_LABELS } from './labels';
import { clipPlatformPresentation, type ConnectionLoadState, type PlatformFact } from './platformPresentation';

export function PlatformStatusPanel({ clip, platforms, load }: { clip: SocialClipMitPosting; platforms: PlatformStatus[]; load: ConnectionLoadState }) {
  const t = useT();
  const headings = ['Verbindung', 'Videofreigabe', 'Übertragung', 'Veröffentlichung'];
  return (
    <div className="studio-platform-status">
      <p className="text-sm leading-6 text-text-secondary">{t('Gespeicherter Stand, keine Live-Prüfung beim Anbieter. Eine Verbindung ist keine Videofreigabe und garantiert keine Veröffentlichung.')}</p>
      {(['youtube', 'tiktok', 'instagram'] as const).map((platform) => {
        const row = clipPlatformPresentation(clip, platform, platforms.find((status) => status.platform === platform), load);
        return (
          <section key={platform} className="studio-platform-status-row" aria-label={PLATFORM_LABELS[platform]}>
            <h3>{PLATFORM_LABELS[platform]}</h3>
            <dl>
              {[row.connection, row.approval, row.transfer, row.publication].map((state, index) => (
                <div key={headings[index]}>
                  <dt>{t(headings[index])}</dt>
                  <dd><StatusLabel state={state} /></dd>
                  {index === 0 && <dd className="studio-authorization">{t(row.authorization.label)}</dd>}
                </div>
              ))}
            </dl>
          </section>
        );
      })}
    </div>
  );
}

function StatusLabel({ state }: { state: PlatformFact }) {
  const t = useT();
  const Icon = state.state === 'loading' ? Loader2 : state.tone === 'success' ? CheckCircle2 : state.tone === 'warning' || state.tone === 'danger' ? AlertCircle : CircleHelp;
  return <span className={`studio-status-label studio-tone-${state.tone}`}><Icon aria-hidden="true" className="h-4 w-4 shrink-0" />{t(state.label)}</span>;
}
