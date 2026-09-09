import { useFeedbackCounts } from './useFeedbackCounts';

export function FeedbackBadge({ isAdmin = false }: { isAdmin?: boolean }) {
  const counts = useFeedbackCounts(isAdmin);
  if (counts.isError) return <span className="text-xs text-text-secondary" title="Rückmeldungen konnten gerade nicht geprüft werden" aria-label="Rückmeldungen konnten gerade nicht geprüft werden">!</span>;
  if (!counts.data?.unread) return null;
  return <span className="rounded-full bg-primary px-2 py-0.5 text-xs font-bold text-black" aria-label={`${counts.data.unread} ${isAdmin ? 'neue Einsendungen' : 'neue Antworten oder Bearbeitungsstände'}`}>
    {counts.data.unread > 99 ? '99+' : counts.data.unread}
  </span>;
}
