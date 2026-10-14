import { studioActions } from './studioActions';
import { BarChart3, Plus } from 'lucide-react';
import { useT } from '@/context/LanguageContext';
import { SOCIAL_MEDIA_TABS, type SocialMediaView, type SocialMediaViewTarget } from './labels';
import type { StudioOverviewData } from './StudioOverview';
import { type QueueStage } from './queuePresentation';

export function StudioHero({ streamer, activeView, onUpload, onOpenAnalytics, onNavigate, onOpenLayout, data, now }: {
  streamer: string;
  activeView: SocialMediaView;
  onUpload: () => void;
  onOpenAnalytics: () => void;
  onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void;
  onOpenLayout: () => void;
  data: StudioOverviewData;
  now: number;
}) {
  const t = useT();
  const firstAction = studioActions(data, now, t)[0];
  const primary = activeView === 'heute'
    ? firstAction ? { label: firstAction.title, run: () => onNavigate(firstAction.view, firstAction.stage) }
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
