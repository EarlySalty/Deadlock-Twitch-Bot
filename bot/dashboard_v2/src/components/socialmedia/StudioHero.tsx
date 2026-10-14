import { BarChart3, Plus } from 'lucide-react';
import { useT } from '@/context/LanguageContext';
import { SOCIAL_MEDIA_TABS, type SocialMediaView, type SocialMediaViewTarget } from './labels';
import type { StudioOverviewData } from './StudioOverview';
import { queueStage, type QueueStage } from './queuePresentation';

export function StudioHero({ streamer, activeView, onUpload, onOpenAnalytics, onNavigate, onOpenLayout, data }: {
  streamer: string;
  activeView: SocialMediaView;
  onUpload: () => void;
  onOpenAnalytics: () => void;
  onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void;
  onOpenLayout: () => void;
  data: StudioOverviewData;
}) {
  const t = useT();
  const review = !data.clipsFailed && data.clips?.some(clip => queueStage(clip) === 'review');
  const primary = activeView === 'heute'
    ? review ? { label: 'Freigaben prüfen', run: () => onNavigate('freigabe', 'review') }
      : data.schedule?.review_count ? { label: 'Produktionen prüfen', run: () => onNavigate('planen') }
      : { label: 'Auto-Pilot & Zeitplan', run: () => onNavigate('planen') }
    : activeView === 'freigabe' ? { label: 'Freigaben prüfen', run: () => onNavigate('freigabe', 'review') }
    : activeView === 'planen' ? { label: 'Termine festlegen', run: () => document.querySelector('.sq-slots')?.scrollIntoView({ behavior: 'smooth', block: 'center' }) }
    : activeView === 'veroeffentlicht' ? { label: 'Auswertung', run: onOpenAnalytics }
    : { label: 'Layout anpassen', run: onOpenLayout };
  return <header className="studio-header">
    <div className="min-w-0"><h1 className="studio-heading">{t(SOCIAL_MEDIA_TABS.find(tab => tab.id === activeView)?.label ?? 'Heute')}</h1><p className="mt-2 text-sm text-text-secondary">{streamer}</p></div>
    <div className="studio-header-actions">
      <button type="button" className="studio-button" onClick={onUpload}><Plus className="h-4 w-4" aria-hidden />{t('Clip hinzufügen')}</button>
      <button type="button" className="studio-primary" onClick={primary.run}>{activeView === 'veroeffentlicht' && <BarChart3 className="h-4 w-4" aria-hidden />}{t(primary.label)}</button>
    </div>
  </header>;
}
