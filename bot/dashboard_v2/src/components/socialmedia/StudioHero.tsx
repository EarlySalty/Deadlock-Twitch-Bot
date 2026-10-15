import { Plus, RefreshCw } from 'lucide-react';
import { useT } from '@/context/LanguageContext';

export function StudioHero({ streamer, onUpload, onRefresh, refreshing = false }: {
  streamer: string;
  onUpload: () => void;
  onRefresh: () => void;
  refreshing?: boolean;
}) {
  const t = useT();
  return <header className="studio-header">
    <div className="min-w-0"><h1 className="studio-heading">{t('Social-Media-Studio')}</h1><p className="mt-2 text-sm text-text-secondary">{streamer}</p></div>
    <div className="studio-header-actions">
      <button type="button" className="studio-button" onClick={onUpload}><Plus className="h-4 w-4" aria-hidden />{t('Clip hinzufügen')}</button>
      <button type="button" className="studio-icon-button" onClick={onRefresh} aria-label={t('Alle Daten neu laden')} title={t('Alle Daten neu laden')} aria-busy={refreshing || undefined}><RefreshCw className={`h-4 w-4${refreshing ? ' animate-spin' : ''}`} aria-hidden /></button>
    </div>
  </header>;
}
