import { useId, useState } from 'react';
import { useT } from '@/context/LanguageContext';
import type { ReviewDecision } from './api';

interface ReviewJobActionsProps {
  busy: boolean;
  clipTitle: string;
  onResolve: (action: ReviewDecision, note: string) => void;
}

export function ReviewJobActions({ busy, clipTitle, onResolve }: ReviewJobActionsProps) {
  const t = useT();
  const id = useId();
  const [note, setNote] = useState('');
  return <div className="sq-review-actions">
    <label htmlFor={id}>{t('Begründung (optional)')}</label>
    <textarea id={id} rows={2} maxLength={500} value={note} disabled={busy}
      aria-label={t('Begründung für {title}', { title: clipTitle })} onChange={event => setNote(event.target.value)} />
    <p className="sq-help">{t('Erneut verarbeiten stellt den Clip an den Anfang der Warteschlange. Überspringen beendet diesen Auftrag.')}</p>
    <div className="sq-job-actions">
      <button type="button" disabled={busy} onClick={() => onResolve('process', note)}>{t('Trotzdem verarbeiten')}</button>
      <button type="button" disabled={busy} onClick={() => onResolve('skip', note)}>{t('Clip überspringen')}</button>
    </div>
  </div>;
}
