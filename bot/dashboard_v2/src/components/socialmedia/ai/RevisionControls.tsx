import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import type { AiJobView, AiVideoVersion } from './api';
import { approveAiVersion, decideAiProposal, decideAiVersion, reviseAiVersion, scheduleAiVersion, selectAiVersion } from './api';

type Action = 'revise' | 'confirm' | 'cancel' | 'select' | 'accept' | 'reject' | 'approve' | 'schedule';
const platforms = [{id:'youtube',label:'YouTube'},{id:'tiktok',label:'TikTok'},{id:'instagram',label:'Instagram'}];

export function RevisionControls({channel,job,version,canStart,onJob,onReload}: {
  channel: string; job: AiJobView; version: AiVideoVersion | null; canStart: boolean;
  onJob: (job: AiJobView) => void; onReload: () => Promise<boolean>;
}) {
  const [feedback,setFeedback] = useState('');
  const [selected,setSelected] = useState<string[]>([]);
  const [confirmed,setConfirmed] = useState(false);
  const [message,setMessage] = useState('');
  const [reloading,setReloading] = useState(false);
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
      return {job:result.job,message:`Ohne erneutes Rendern eingeplant für ${new Date(result.scheduledAt).toLocaleString('de-DE')} (Ortszeit deines Browsers).`};
    },
    onMutate: () => setMessage(''),
    onSuccess: (result,kind) => { setMessage(result.message); setConfirmed(false); if (kind === 'revise') setFeedback(''); onJob(result.job); },
  });
  const blocked = action.isPending || action.isError || reloading;
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
  return <section className="social-ai-revisions" aria-label="Änderungen und Veröffentlichung">
    {job.status === 'awaiting_confirmation' && job.revision_proposal && <div className="social-ai-confirm" role="group" aria-label="Aufwendige Änderung bestätigen">
      <h4>Vor dem nächsten Schritt bestätigen</h4>
      <p>{job.revision_proposal.summary}</p><p>{job.revision_proposal.impact}</p>
      <p>{job.revision_proposal.reanalyze ? 'Dafür ist eine erneute KI-Analyse nötig.' : 'Der geplante Schnitt verändert die bisherige Fassung deutlich.'} {job.revision_proposal.estimated_provider_cost_usd === null ? 'Die zusätzlichen Anbieter-Kosten sind noch nicht zuverlässig schätzbar.' : `Geschätzte zusätzliche Anbieter-Kosten: ${job.revision_proposal.estimated_provider_cost_usd.toLocaleString('de-DE')} US-Dollar.`}</p>
      <label><input type="checkbox" checked={confirmed} disabled={blocked || !canStart} onChange={(event) => setConfirmed(event.target.checked)} />Ich bestätige den vorgeschlagenen nächsten Bearbeitungsschritt und mögliche zusätzliche KI-Kosten.</label>
      <div className="social-ai-actions"><button type="button" className="studio-primary" disabled={!confirmed || !canStart || blocked} onClick={() => action.mutate('confirm')}>Änderung bestätigen</button><button type="button" className="studio-button" disabled={blocked} onClick={() => action.mutate('cancel')}>Änderung verwerfen</button></div>
      <p>Bis zur Bestätigung wird keine neue Analyse oder Videofassung erstellt. Die kurze Schnittplanung ist bereits in den Auftragskosten enthalten.</p>
    </div>}
    {job.status === 'rejected' && <p role="status">Der Änderungsvorschlag wurde verworfen. Die vorherige Fassung ist weiterhin zur Sichtung verfügbar.</p>}
    {version && <>
      <div className="social-ai-version-actions"><h4>Fassung {version.version_no}{version.variant_index > 0 ? `, Variante ${version.variant_index + 1}` : ''} beurteilen</h4><p>Bewertungen und bevorzugte Fassung sind unabhängig von der Videofreigabe.</p>
        <div className="social-ai-actions"><button type="button" className="studio-button" disabled={blocked || job.selected_version_id === version.id} onClick={() => action.mutate('select')}>{job.selected_version_id === version.id ? 'Bevorzugte Fassung' : 'Diese Fassung verwenden'}</button><button type="button" className="studio-button" disabled={blocked} onClick={() => action.mutate('accept')}>Fassung gefällt mir</button><button type="button" className="studio-button" disabled={blocked || version.status !== 'review'} onClick={() => action.mutate('reject')}>Fassung gefällt mir nicht</button></div>
      </div>
      <form onSubmit={(event) => {event.preventDefault(); if (canStart && !busyJob && !blocked && feedback.trim() && feedbackBytes <= 2000) action.mutate('revise');}}>
        <label htmlFor="social-ai-revision-feedback">Was soll sich an dieser Fassung ändern?</label>
        <textarea id="social-ai-revision-feedback" value={feedback} rows={3} maxLength={2000} disabled={blocked || busyJob} onChange={(event) => setFeedback(event.target.value)} placeholder="Zum Beispiel: Kürze den Einstieg, aber behalte die ursprüngliche Reaktion." aria-describedby="social-ai-revision-help" />
        <p id="social-ai-revision-help">Kleine Änderungen nutzen die gespeicherte Analyse. Umfangreiche Änderungen brauchen deine Bestätigung. Die bisherige Fassung wird nicht überschrieben.</p>
        {feedbackBytes > 2000 && <p className="social-ai-caution" role="alert">Bitte kürze deine Anweisung, bevor du sie abschickst.</p>}
        <button type="submit" className="studio-primary" disabled={!canStart || busyJob || blocked || !feedback.trim() || feedbackBytes > 2000}>Änderung anfordern</button>
      </form>
      <fieldset disabled={blocked} className="social-ai-platform-approval"><legend>Videofreigabe und Zeitplan</legend>
        <p>Nur ausdrücklich freigegebene Plattformen können eingeplant werden. Eine Videofreigabe verbindet kein Plattformkonto.</p>
        <div className="social-ai-platforms">{platforms.map((platform) => <label key={platform.id}><input type="checkbox" checked={selected.includes(platform.id)} onChange={(event) => setSelected(event.target.checked ? [...selected,platform.id] : selected.filter((item) => item !== platform.id))} /><span>{platform.label}</span><small>{version.approved_platforms.includes(platform.id) ? 'Video freigegeben' : 'Noch keine Videofreigabe'}</small></label>)}</div>
        <div className="social-ai-actions"><button type="button" className="studio-button" onClick={() => setSelected(platforms.map((platform) => platform.id))}>Alle Plattformen auswählen</button><button type="button" className="studio-button" disabled={selected.length === 0} onClick={() => action.mutate('approve')}>Ausgewählte Plattformen freigeben</button><button type="button" className="studio-primary" disabled={!canSchedule} onClick={() => action.mutate('schedule')}>In den Zeitplan übernehmen</button></div>
        <p>Die Übernahme verwendet genau diese fertige Datei, ohne erneute Analyse oder Videoerstellung. {version.status === 'scheduled' ? 'Diese Fassung ist bereits im Zeitplan. Weitere Plattformen brauchen eine gesonderte Freigabe und Übernahme.' : 'Nur die ausdrücklich übernommenen Plattformen werden eingeplant.'}</p>
      </fieldset>
    </>}
    {action.isPending && <p role="status">Änderung wird gespeichert…</p>}
    {message && <p role="status">{message}</p>}
    {action.isError && <div className="social-ai-alert" role="alert"><div><p>{action.error instanceof Error ? action.error.message : 'Die Änderung konnte nicht bestätigt werden.'} Lade den aktuellen Stand neu, bevor du eine weitere Aktion ausführst.</p><button type="button" className="studio-button" disabled={reloading} onClick={() => { void reload(); }}>{reloading ? 'Stand wird geladen…' : 'Aktuellen Stand neu laden'}</button></div></div>}
  </section>;
}
