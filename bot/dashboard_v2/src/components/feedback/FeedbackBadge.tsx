import { useT } from '../../context/LanguageContext';
import { useFeedbackCounts } from './useFeedbackCounts';

export function FeedbackBadge({ isAdmin = false }: { isAdmin?: boolean }) {
  const t = useT();
  const counts = useFeedbackCounts(isAdmin);
  if (counts.isError) return <span className="text-xs text-text-secondary" title={t('Rückmeldungen konnten gerade nicht geprüft werden')} aria-label={t('Rückmeldungen konnten gerade nicht geprüft werden')}>!</span>;
  if (!counts.data?.unread) return null;
  return <span className="rounded-full bg-primary px-2 py-0.5 text-xs font-bold text-black" aria-label={isAdmin ? t('{count} neue Einsendungen', { count: counts.data.unread }) : t('{count} neue Antworten oder Bearbeitungsstände', { count: counts.data.unread })}>
    {counts.data.unread > 99 ? '99+' : counts.data.unread}
  </span>;
}
