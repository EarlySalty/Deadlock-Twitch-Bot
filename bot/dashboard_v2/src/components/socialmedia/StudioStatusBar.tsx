import { HintChip } from './HintChip';
import { useLanguage } from '@/context/LanguageContext';
import type { ClipPoolForecast } from '@/types/socialMedia';
import type { StudioOverviewData } from './StudioOverview';
import type { SocialMediaViewTarget } from './labels';
import { connectionFact } from './platformPresentation';
import type { QueueStage } from './queuePresentation';

export function StudioStatusBar({ data, pool, poolFailed, onNavigate }: {
  data: StudioOverviewData;
  pool: ClipPoolForecast | undefined;
  poolFailed?: boolean;
  onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void;
}) {
  const { t } = useLanguage();
  const connected = data.platformsFailed || !data.platforms ? null : data.platforms.filter(status =>
    ['youtube', 'tiktok', 'instagram'].includes(status.platform) && connectionFact(status).state === 'connected',
  ).length;
  return <section className="studio-status-bar" aria-label={t('Stand auf einen Blick')}>
    <button type="button" data-tone={connected === 3 ? 'success' : 'warning'} onClick={() => onNavigate('einstellungen')}>
      {connected === null ? `${t('Konten')} · ${t(data.platformsFailed ? 'Nicht abrufbar' : 'Wird geladen')}` : t('Konten verbunden {count}/3', { count: connected })}
    </button>
    <button type="button" data-tone={pool?.warnung ? 'warning' : 'neutral'} onClick={() => onNavigate('clips')}>
      {poolFailed ? `${t('Vorrat')} · ${t('Nicht abrufbar')}` : !pool ? t('Bestand wird geladen') : pool.reicht_fuer_tage === null ? t('Vorrat ohne Posting-Rhythmus') : t('Vorrat reicht {days} Tage', { days: pool.reicht_fuer_tage })}
    </button>
<HintChip hint={t("Der Vorrat wird aus dem Posting-Rhythmus für neue Twitch-Clips berechnet.")} /></section>;
}
