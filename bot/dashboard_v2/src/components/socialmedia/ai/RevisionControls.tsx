import { useLanguage, useT } from '@/context/LanguageContext';
import { useEffect, useId, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { AiJobView, AiVideoVersion } from './api';
import { approveAiVersion, decideAiProposal, decideAiVersion, fetchAiPublishingContext, reviseAiVersion, scheduleAiVersion, selectAiVersion } from './api';
import { MetadataPanel } from './MetadataPanel';
import { editorErrorExplanation } from './failure';
import { proposalCost } from './proposalCost';

type Action = 'revise' | 'confirm' | 'cancel' | 'select' | 'accept' | 'reject' | 'approve' | 'schedule';

const ratingActions: Action[] = ['select', 'accept', 'reject'];

const platforms = [{id:'youtube',label:'YouTube'},{id:'tiktok',label:'TikTok'},{id:'instagram',label:'Instagram'}];

export function RevisionControls({channel,job,version,canStart,onJob,onReload,publishing = true,onBusyChange}: {
  channel: string; job: AiJobView; version: AiVideoVersion | null; canStart: boolean; publishing?: boolean;
  onJob: (job: AiJobView) => void; onReload: () => Promise<boolean>; onBusyChange?: (busy: boolean) => void;
}) {
  const t = useT();
  const { locale } = useLanguage();
  const feedbackId = useId();
  const client = useQueryClient();
  const videoId = version?.id;
  const videoStatus = version?.status;
  useEffect(() => {
    if (videoId !== undefined) void client.invalidateQueries({ queryKey: ['social-ai', 'metadata', channel, videoId] });
  }, [client, channel, videoId, videoStatus]);
  const publishingContext = useQuery({queryKey: ['social-ai-publishing-context', channel, job.id], queryFn: () => fetchAiPublishingContext(channel, job.id), enabled: publishing, staleTime: Infinity});
  const showPublishing = publishing && publishingContext.data === false;
  const historical = version?.status === 'rejected';
  const [metadataEditing, setMetadataEditing] = useState(false);
  const [feedback,setFeedback] = useState('');
  const [selected,setSelected] = useState<string[]>([]);
  const [confirmed,setConfirmed] = useState(false);
  const [message,setMessage] = useState<string | { scheduledAt: string }>('');
  const [reloading,setReloading] = useState(false);
  const [lastAction,setLastAction] = useState<Action | null>(null);
  const busyJob = !['review','failed','rejected'].includes(job.status);
  const feedbackBytes = new TextEncoder().encode(feedback).length;
  const action = useMutation({
    mutationFn: async (kind: Action) => {
      if (kind === 'confirm' || kind === 'cancel') return {job:await decideAiProposal(channel,job.id,kind === 'confirm'),message:kind === 'confirm' ? 'Änderung bestätigt. Die Bearbeitung wird fortgesetzt.' : 'Änderung verworfen. Die vorherige Fassung bleibt erhalten.'};
      if (!version) throw new Error('Keine nutzbare Fassung ausgewählt.');
      if (kind === 'revise') return {job:await reviseAiVersion(channel,job.id,version.id,feedback),message:'Änderungswunsch gespeichert. Die bisherige Fassung bleibt erhalten.'};
      if (kind === 'select') return {job:await selectAiVersion(channel,job.id,version.id),message:'Diese Fassung ist als bevorzugte Fassung gespeichert. Sie wurde dadurch nicht freigegeben oder eingeplant.'};
      if (kind === 'accept' || kind === 'reject') return {job:await decideAiVersion(channel,job.id,version.id,kind === 'accept'),message:kind === 'accept' ? 'Positive Bewertung gespeichert. Zum Veröffentlichen sind gesonderte Plattformfreigaben erforderlich.' : 'Negative Bewertung gespeichert. Die Fassung bleibt zum Vergleichen erhalten.'};
      if (kind === 'approve') return {job:await approveAiVersion(channel,job.id,version.id,selected),message:'Videofreigabe für die ausgewählten Plattformen gespeichert. Das Video ist noch nicht neu eingeplant.'};
      const result = await scheduleAiVersion(channel,job.id,version.id,selected);
      return {job:result.job,message: { scheduledAt: result.scheduledAt }};
    },
    onMutate: (kind) => { setMessage(''); setLastAction(kind); },
    onSuccess: (result,kind) => { setMessage(result.message); setConfirmed(false); if (kind === 'revise') setFeedback(''); onJob(result.job); },
  });
  const blocked = action.isPending || action.isError || reloading || metadataEditing;
  const busy = action.isPending || reloading;
  useEffect(() => {
    onBusyChange?.(busy);
    return () => onBusyChange?.(false);
  }, [busy, onBusyChange]);
  const rating = version?.rating ?? null;
  const preferred = version !== null && job.selected_version_id === version.id;
  const ratingFeedback = lastAction !== null && ratingActions.includes(lastAction);
  const reload = async () => {
    setReloading(true);
    try {
      if (await onReload()) { action.reset(); setMessage('Aktueller Stand geladen.'); }
      else setMessage('Der aktuelle Stand konnte nicht geladen werden. Bitte versuche es erneut.');
    } catch {
      setMessage('Der aktuelle Stand konnte nicht geladen werden. Bitte versuche es erneut.');
    } finally {
      setReloading(false);
    }
  };
  const canSchedule = version && selected.length > 0 && selected.every((platform) => version.approved_platforms.includes(platform));
  return <section className="social-ai-revisions" aria-label={t("Änderungen und Veröffentlichung")}>
    {job.status === 'awaiting_confirmation' && job.revision_proposal && <div className="social-ai-confirm" role="group" aria-label={t("Aufwendige Änderung bestätigen")}>
      <h4>{t("Vor dem nächsten Schritt bestätigen")}</h4>
      <p>{job.revision_proposal.summary}</p><p>{job.revision_proposal.impact}</p>
      <p>{job.revision_proposal.reanalyze ? t("Dafür ist eine erneute KI-Analyse nötig.") : t("Der geplante Schnitt verändert die bisherige Fassung deutlich.")} {proposalCost(job.revision_proposal, locale, t)}</p>
      <label><input type="checkbox" checked={confirmed} disabled={blocked || !canStart} onChange={(event) => setConfirmed(event.target.checked)} />{t("Ich bestätige den vorgeschlagenen nächsten Bearbeitungsschritt und mögliche zusätzliche KI-Kosten.")}</label>
      <div className="social-ai-actions"><button type="button" className="studio-primary" disabled={!confirmed || !canStart || blocked} onClick={() => action.mutate('confirm')}>{t("Änderung bestätigen")}</button><button type="button" className="studio-button" disabled={blocked} onClick={() => action.mutate('cancel')}>{t("Änderung verwerfen")}</button></div>
      <p>{t("Bis zur Bestätigung wird keine neue Analyse oder Videofassung erstellt. Die kurze Schnittplanung ist bereits in den Auftragskosten enthalten.")}</p>
    </div>}
    <RevisionOutcome job={job} />
    {job.status === 'rejected' && <p role="status">{t("Der Änderungsvorschlag wurde verworfen. Die vorherige Fassung ist weiterhin zur Sichtung verfügbar.")}</p>}
    {version && <>
      <div className="social-ai-version-actions"><h4>{t('Fassung {version}{variant} beurteilen', { version: version.version_no, variant: version.variant_index > 0 ? t(', Variante {variant}', { variant: version.variant_index + 1 }) : '' })}</h4><p>{t("Bewertungen und bevorzugte Fassung sind unabhängig von der Videofreigabe.")}</p>
        <div className="social-ai-actions"><button type="button" className="studio-button" aria-pressed={preferred} disabled={blocked || historical} onClick={() => { if (!preferred) action.mutate('select'); }}>{preferred ? t("Bevorzugte Fassung") : t("Diese Fassung verwenden")}</button><button type="button" className="studio-button" aria-pressed={rating === 'accepted'} disabled={blocked || historical} onClick={() => action.mutate('accept')}>{t("Fassung gefällt mir")}</button><button type="button" className="studio-button" aria-pressed={rating === 'rejected'} disabled={blocked || version.status !== 'review'} onClick={() => action.mutate('reject')}>{t("Fassung gefällt mir nicht")}</button></div>
        {ratingFeedback && <ActionFeedback pending={action.isPending} message={message} />}
      </div>
      <form onSubmit={(event) => {event.preventDefault(); if (canStart && !historical && !busyJob && !blocked && job.revisions_left !== 0 && feedback.trim() && feedbackBytes <= 2000) action.mutate('revise');}}>
        <label htmlFor={feedbackId}>{t("Was soll sich an dieser Fassung ändern?")}</label>
        <textarea id={feedbackId} value={feedback} rows={3} maxLength={2000} disabled={blocked || busyJob || historical} onChange={(event) => setFeedback(event.target.value)} placeholder={t("Zum Beispiel: Kürze den Einstieg, aber behalte die ursprüngliche Reaktion.")} aria-describedby={`${feedbackId}-help`} />
        <p id={`${feedbackId}-help`}>{t("Kleine Änderungen nutzen die gespeicherte Analyse. Umfangreiche Änderungen brauchen deine Bestätigung. Die bisherige Fassung wird nicht überschrieben.")}</p>
        {feedbackBytes > 2000 && <p className="social-ai-caution" role="alert">{t("Bitte kürze deine Anweisung, bevor du sie abschickst.")}</p>}
        {typeof job.revisions_left === 'number' && <p className={job.revisions_left === 0 ? 'social-ai-caution' : undefined} role="status">{job.revisions_left === 0 ? t("Für diesen Clip sind alle Änderungswünsche aufgebraucht. Gib eine der Fassungen frei oder verwirf das Video.") : t("Noch {count} Änderungswünsche für diesen Clip.", { count: job.revisions_left })}</p>}
        <button type="submit" className="studio-primary" disabled={!canStart || historical || busyJob || blocked || job.revisions_left === 0 || !feedback.trim() || feedbackBytes > 2000}>{t("Änderung anfordern")}</button>
      </form>
      <MetadataPanel key={version.id} channel={channel} videoId={version.id} disabled={action.isPending || busyJob || reloading || version.status !== 'review'} onEditingChange={setMetadataEditing} />
      {metadataEditing && <p role="status">{t("Speichere oder verwirf die Textänderungen vor der nächsten Aktion.")}</p>}
      {publishingContext.isError && publishing && <p className="social-ai-caution" role="alert">{t("Der Veröffentlichungsweg konnte nicht geprüft werden.")} <button type="button" className="studio-button" onClick={() => { void publishingContext.refetch(); }}>{t("Erneut prüfen")}</button></p>}
      {(!publishing || publishingContext.data === true) && <p>{t("Die Plattformfreigabe dieser Fassung erfolgt im Veröffentlichungspanel.")}</p>}
      {showPublishing && <fieldset disabled={blocked || historical || busyJob} className="social-ai-platform-approval"><legend>{t("Videofreigabe und Zeitplan")}</legend>
        <p>{t("Nur ausdrücklich freigegebene Plattformen können eingeplant werden. Eine Videofreigabe verbindet kein Plattformkonto.")}</p>
        <div className="social-ai-platforms">{platforms.map((platform) => <label key={platform.id}><input type="checkbox" checked={selected.includes(platform.id)} onChange={(event) => setSelected(event.target.checked ? [...selected,platform.id] : selected.filter((item) => item !== platform.id))} /><span>{platform.label}</span><small>{version.approved_platforms.includes(platform.id) ? t("Video freigegeben") : t("Noch keine Videofreigabe")}</small></label>)}</div>
        <div className="social-ai-actions"><button type="button" className="studio-button" onClick={() => setSelected(platforms.map((platform) => platform.id))}>{t("Alle Plattformen auswählen")}</button><button type="button" className="studio-button" disabled={selected.length === 0} onClick={() => action.mutate('approve')}>{t("Ausgewählte Plattformen freigeben")}</button><button type="button" className="studio-primary" disabled={!canSchedule} onClick={() => action.mutate('schedule')}>{t("In den Zeitplan übernehmen")}</button></div>
        <p>{t("Die Übernahme verwendet genau diese fertige Datei, ohne erneute Analyse oder Videoerstellung.")} {version.status === 'scheduled' ? t("Diese Fassung ist bereits im Zeitplan. Weitere Plattformen brauchen eine gesonderte Freigabe und Übernahme.") : t("Nur die ausdrücklich übernommenen Plattformen werden eingeplant.")}</p>
      </fieldset>}
    </>}
    {!ratingFeedback && <ActionFeedback pending={action.isPending} message={message} />}
    {action.isError && <div className="social-ai-alert" role="alert"><div><p>{editorErrorExplanation(action.error, t)} {t("Lade den aktuellen Stand neu, bevor du eine weitere Aktion ausführst.")}</p><button type="button" className="studio-button" disabled={reloading} onClick={() => { void reload(); }}>{reloading ? t("Stand wird geladen…") : t("Aktuellen Stand neu laden")}</button></div></div>}
  </section>;
}

function RevisionOutcome({job}: {job: AiJobView}) {
  const t = useT();
  const proposal = job.revision_proposal;
  if (job.parent_version_id === null || !proposal || ['awaiting_confirmation', 'rejected'].includes(job.status)) return null;
  const produced = job.versions.some((item) => item.job_id === job.id);
  const outcome = produced ? 'answered' : ['review', 'failed'].includes(job.status) ? 'unchanged' : 'running';
  const text = { answered: t('Rückmeldung der KI zu deinem Änderungswunsch: {summary}', { summary: proposal.summary }), unchanged: t('Die KI konnte das nicht umsetzen: {summary}', { summary: proposal.summary }), running: t('Geplante Änderung: {summary}', { summary: proposal.summary }) }[outcome];
  return <div className="social-ai-revision-outcome" data-outcome={outcome} role="status"><p>{text}</p>{proposal.impact && <p>{proposal.impact}</p>}</div>;
}

function ActionFeedback({pending,message}: {pending: boolean; message: string | { scheduledAt: string }}) {
  const t = useT();
  const { locale } = useLanguage();
  if (pending) return <p role="status">{t("Änderung wird gespeichert…")}</p>;
  if (!message) return null;
  return <p role="status">{typeof message === 'string' ? t(message) : t('Ohne erneutes Rendern eingeplant für {date} (Ortszeit deines Browsers).', { date: new Date(message.scheduledAt).toLocaleString(locale) })}</p>;
}
