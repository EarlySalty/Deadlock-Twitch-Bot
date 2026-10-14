import { HintChip } from './HintChip';
import { useLanguage } from '@/context/LanguageContext';
import type { ClipPoolForecast } from '@/types/socialMedia';
import type { StudioOverviewData } from './StudioOverview';
import type { SocialMediaViewTarget } from './labels';
import { connectionFact } from './platformPresentation';
import { formatTerminInZone, type QueueStage } from './queuePresentation';
import { upcomingTerms } from './upcomingTerms';

export function StudioStatusBar({ data, pool, now, onNavigate }: {
  data: StudioOverviewData;
  pool: ClipPoolForecast | undefined;
  now: number;
  onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void;
}) {
  const { t, locale } = useLanguage();
  const settings = data.scheduleFailed ? undefined : data.schedule?.settings;
  const connected = data.platformsFailed || !data.platforms ? null : data.platforms.filter(status =>
    ['youtube', 'tiktok', 'instagram'].includes(status.platform) && connectionFact(status).state === 'connected',
  ).length;
  const next = upcomingTerms(data, now)[0];
  return <section className="studio-status-bar" aria-label={t('Stand auf einen Blick')}>
    <button type="button" data-tone={settings?.enabled ? settings.paused ? 'warning' : 'success' : 'neutral'}
      title={next ? `${t('Nächster Termin')}: ${formatTerminInZone(next.at, locale, next.timezone)}` : undefined}
      onClick={() => onNavigate('planen')}>
      {settings ? t(settings.enabled ? 'Auto-Pilot an' : 'Auto-Pilot aus') : `${t('Auto-Pilot')} · ${t(data.scheduleFailed ? 'Nicht abrufbar' : 'Wird geladen')}`}
      {settings?.paused && ` · ${t('Pausiert')}`}
    </button>
    <button type="button" data-tone={connected === 3 ? 'success' : 'warning'} onClick={() => onNavigate('einstellungen')}>
      {t('Konten verbunden {count}/3', { count: connected ?? '…' })}
    </button>
    <button type="button" data-tone={pool?.warnung ? 'warning' : 'neutral'} onClick={() => onNavigate('planen')}>
      {!pool ? t('Bestand wird geladen') : pool.reicht_fuer_tage === null ? t('Vorrat ohne Posting-Rhythmus') : t('Vorrat reicht {days} Tage', { days: pool.reicht_fuer_tage })}
    </button>
<HintChip hint={t("Der Vorrat wird aus dem Posting-Rhythmus für neue Twitch-Clips berechnet.")} /></section>;
}
