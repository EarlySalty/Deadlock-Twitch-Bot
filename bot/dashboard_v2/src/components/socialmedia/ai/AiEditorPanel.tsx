import { useLanguage, useT } from '@/context/LanguageContext';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { AlertTriangle, Check, Clock3, Film, RotateCcw, ShieldCheck } from 'lucide-react';
import { fetchClips } from '@/api/socialMedia';
import type { AiJobView, AiTimeRange } from './api';
import { fetchAiConsent, fetchAiJob, fetchAiJobs, retryAiJob, saveAiConsent, startAiJob } from './api';
import { chooseUsableVersion } from './versions';
import { findChannelClip, isSelectableClip } from './clipLookup';
import { failureExplanation } from './failure';
import { RevisionControls } from './RevisionControls';
import { EditorOptionsPanel } from './EditorOptionsPanel';
import './AiEditorPanel.css';

interface AiEditorPanelProps {
  twitchUserId: string;
  streamer: string;
  clipDbId?: number;
}

type ScopedSelection = { channel: string; id: number };

const statusLabels: Record<AiJobView['status'], string> = {
  queued: 'Wartet auf Bearbeitung',
  preparing: 'Clip wird vorbereitet',
  transcribing: 'Sprache wird erkannt',
  analyzing: 'Spielgeschehen wird geprüft',
  planning: 'Schnitte werden geplant',
  rendering: 'Video wird erstellt',
  validating: 'Video wird geprüft',
  review: 'Bereit zur Sichtung',
  failed: 'Bearbeitung fehlgeschlagen',
  awaiting_confirmation: 'Wartet auf deine Bestätigung',
  rejected: 'Änderung verworfen',
};

const stageLabels: Record<string, string> = {
  queued: 'Warteschlange',
  preparing: 'Vorbereitung',
  transcribing: 'Transkription',
  analyzing: 'Analyse',
  planning: 'Schnittplanung',
  rendering: 'Videoerstellung',
  validating: 'Qualitätsprüfung',
  review: 'Sichtung',
};

function duration(ms: number | null, locale: string, t: ReturnType<typeof useT>): string {
  return ms === null ? t('Noch unbekannt') : `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(ms / 1000)} s`;
}

function timestamp(value: string | null, locale: string, t: ReturnType<typeof useT>): string {
  return value === null ? t('Noch unbekannt') : new Date(value).toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' });
}

function timecode(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}

function EditorError({ children, retry }: { children: ReactNode; retry?: () => void }) {
  const t = useT();
  return <div className="social-ai-alert" role="alert"><AlertTriangle aria-hidden="true" size={18} /><div><p>{children}</p>{retry && <button type="button" className="studio-button" onClick={retry}>{t("Erneut laden")}</button>}</div></div>;
}

function VideoPlayer({ url, label, shape }: { url: string | null; label: string; shape: 'source' | 'output' }) {
  const t = useT();
  const [failed, setFailed] = useState(false);
  return <div className={`social-ai-video social-ai-video--${shape}`}>
    {url && !failed ? <video key={url} src={url} controls playsInline preload="metadata" aria-label={label} onError={() => setFailed(true)} /> :
      <p role={failed ? 'alert' : 'status'}>{failed ? t("Das Video konnte nicht geladen werden. Bitte lade die Ansicht erneut.") : t("Noch kein Video verfügbar.")}</p>}
  </div>;
}

function TimeRanges({ title, items }: { title: string; items: AiTimeRange[] }) {
  const t = useT();
  return <section className="social-ai-notes" aria-label={title}>
    <h4>{title}</h4>
    {items.length ? <ol>{items.map((item, index) => <li key={`${item.start_ms}-${item.end_ms}-${index}`}><span className="social-ai-time">{timecode(item.start_ms)}  {t("bis")} {timecode(item.end_ms)}</span><span>{item.description}</span></li>)}</ol> : <p>{t("Keine Angaben vorhanden.")}</p>}
  </section>;
}

function Review({ job, selectedVersion, onVersionChange, channel, canStart, onJob, onReload }: { job: AiJobView; selectedVersion?: number; onVersionChange: (id: number) => void; channel: string; canStart: boolean; onJob: (job: AiJobView) => void; onReload: () => Promise<boolean> }) {
  const t = useT();
  const { locale } = useLanguage();
  const version = chooseUsableVersion(job.versions, selectedVersion ?? job.selected_version_id ?? undefined);
  const [comparison,setComparison] = useState<number | null>(null);
  const previous = chooseUsableVersion(job.versions.filter((item) => item.id !== version?.id), comparison ?? version?.parent_version_id ?? undefined);
  const newest = [...job.versions].sort((a, b) => b.version_no - a.version_no)[0];
  return <div className="social-ai-review">
    <div className="social-ai-review-heading">
      <div><h3>{t("Original und bearbeitete Fassung")}</h3><p>{t("Die Schnittfassung kann eine andere Zeitachse haben. Spiele die Videos einzeln ab und vergleiche die Zeitangaben unten.")}</p></div>
      {job.versions.length > 0 && <label className="social-ai-version-label">{t("Videofassung")} <select className="studio-select" value={version?.id ?? ''} onChange={(event) => onVersionChange(Number(event.target.value))} disabled={!version}>
          {!version && <option value="">{t("Keine abspielbare Fassung")}</option>}
          {job.versions.map((item) => <option key={item.id} value={item.id} disabled={!chooseUsableVersion([item])}>{t("Fassung")} {item.version_no}{item.variant_index > 0 ? t(' · Variante {variant}', { variant: item.variant_index + 1 }) : ''} · {item.file_url ? timestamp(item.created_at, locale, t) : t("nicht verfügbar")}</option>)}
        </select>
      </label>}
    </div>
    {newest && !chooseUsableVersion([newest]) && <p className="social-ai-caution" role="status">{t("Die neueste Fassung ist nicht abspielbar. Die letzte nutzbare Fassung bleibt zur Sichtung erhalten.")}</p>}
    <div className="social-ai-video-grid">
      <figure><figcaption><span>{t("Originalclip")}</span><strong>{duration(job.original_duration_ms, locale, t)}</strong></figcaption><VideoPlayer key={`source-${job.id}-${version?.original_url ?? job.original_url}`} url={version?.original_url ?? job.original_url} label={t("Originalclip abspielen")} shape="source" /></figure>
      <figure><figcaption><span>{t("Bearbeitete Fassung")} {version ? version.version_no : ''}</span><strong>{duration(version ? version.duration_ms : job.output_duration_ms, locale, t)}</strong></figcaption><VideoPlayer key={`output-${job.id}-${version?.id}`} url={version?.file_url ?? null} label={t("Bearbeitetes Video abspielen")} shape="output" /></figure>
    </div>
    <div className="social-ai-explanations">
      <TimeRanges title={t("Erkannte Höhepunkte")} items={job.highlights} />
      <TimeRanges title={t("Schnitte und Entscheidungen")} items={version?.decisions ?? job.decisions} />
    </div>
    {(job.uncertainty.length > 0 || job.review_reasons.length > 0) && <div className="social-ai-review-notices">
      {job.uncertainty.length > 0 && <section><h4>{t("Unsicher bei der Einordnung")}</h4><ul>{job.uncertainty.map((item, index) => <li key={index}>{item}</li>)}</ul></section>}
      {job.review_reasons.length > 0 && <section><h4>{t("Bitte genauer prüfen")}</h4><ul>{job.review_reasons.map((item, index) => <li key={index}>{item}</li>)}</ul></section>}
    </div>}
    {version && previous && <details className="social-ai-compare"><summary>{t("Mit früherer Fassung vergleichen")}</summary><label>{t("Vergleichsfassung")}<select className="studio-select" value={previous.id} onChange={(event) => setComparison(Number(event.target.value))}>{job.versions.filter((item) => item.id !== version.id && chooseUsableVersion([item])).map((item) => <option key={item.id} value={item.id}>{t("Fassung")} {item.version_no}{item.variant_index > 0 ? t(', Variante {variant}', { variant: item.variant_index + 1 }) : ''}</option>)}</select></label><div className="social-ai-video-grid"><figure><figcaption>{t("Vergleichsfassung")} {previous.version_no} · {duration(previous.duration_ms, locale, t)}</figcaption><VideoPlayer key={`previous-${previous.id}`} url={previous.file_url} label={t("Frühere Fassung abspielen")} shape="output" /></figure><figure><figcaption>{t("Ausgewählte Fassung")} {version.version_no} · {duration(version.duration_ms, locale, t)}</figcaption><VideoPlayer key={`current-${version.id}`} url={version.file_url} label={t("Ausgewählte Fassung abspielen")} shape="output" /></figure></div></details>}
    <RevisionControls key={`${job.id}-${version?.id ?? 'none'}`} channel={channel} job={job} version={version} canStart={canStart} onJob={onJob} onReload={onReload} />
  </div>;
}

function JobDetails({ job, canStart, onRetry, retryPending, retryError, channel, onJob, onReload }: { job: AiJobView; canStart: boolean; onRetry: () => void; retryPending: boolean; retryError: boolean; channel: string; onJob: (job: AiJobView) => void; onReload: () => Promise<boolean> }) {
  const t = useT();
  const { locale } = useLanguage();
  const numberFormat = new Intl.NumberFormat(locale);
  const moneyFormat = new Intl.NumberFormat(locale, { minimumFractionDigits: 2, maximumFractionDigits: 5 });
  const [versionSelection, setVersionSelection] = useState<{ job: number; version: number } | null>(null);
  const activeVersionId = versionSelection?.job === job.id ? versionSelection.version : undefined;
  const stage = t(stageLabels[job.stage] ?? 'Phase unbekannt');
  return <article className="social-ai-job" aria-labelledby="social-ai-current-job">
    <div className="social-ai-job-head"><div><h3 id="social-ai-current-job">{t("Auftrag")} {numberFormat.format(job.id)}</h3><p>{t("Clip")} {numberFormat.format(job.clip_db_id)}  {t("· Angelegt am")} {timestamp(job.created_at, locale, t)}</p></div><span className={`social-ai-status social-ai-status--${job.status}`}>{t(statusLabels[job.status])}</span></div>
    <div className="social-ai-stage" role="status"><Clock3 aria-hidden="true" size={17} /><span>{t("Aktuelle Phase:")} <strong>{stage}</strong></span><span>{t("Zuletzt aktualisiert:")} {timestamp(job.updated_at, locale, t)}</span></div>
    {job.status === 'failed' && <div className="social-ai-failure"><EditorError>{t('Die Bearbeitung ist in der Phase „{stage}“ fehlgeschlagen.', { stage })} {t(failureExplanation(job.failure_code))}  {t("Durch diesen Auftrag wird nichts automatisch veröffentlicht.")}</EditorError>{job.can_retry ? <button type="button" className="studio-button" disabled={!canStart || retryPending} onClick={onRetry}><RotateCcw aria-hidden="true" size={16} />{retryPending ? t("Wiederholung wird gestartet…") : t("Fehlgeschlagene Phase erneut versuchen")}</button> : <p>{t("Für diesen Auftrag ist kein weiterer Versuch vorgesehen.")}</p>}{retryError && <EditorError>{t("Der erneute Versuch konnte nicht gestartet werden. Bitte prüfe die Freigabe und versuche es später erneut.")}</EditorError>}</div>}
    <dl className="social-ai-facts" aria-label={t("Verarbeitung und Kosten")}>
      <div><dt>{t("KI-Anbieter und Modell")}</dt><dd>{job.provider && job.model ? `${job.provider} · ${job.model}` : t("Noch unbekannt")}</dd></div>
      <div><dt>{t("Gesamte Bearbeitungszeit")}</dt><dd>{duration(job.processing_ms, locale, t)}</dd></div>
      <div><dt>{t("KI-Antwortzeit")}</dt><dd>{duration(job.provider_latency_ms, locale, t)}</dd></div>
      <div><dt>{t("FFmpeg-Rechenzeit")}</dt><dd>{duration(job.rendering_ms, locale, t)}</dd></div>
      <div><dt>{t("Eingabe- und Ausgabetokens")}</dt><dd>{job.input_tokens === null || job.output_tokens === null ? t("Noch unbekannt") : `${numberFormat.format(job.input_tokens)} / ${numberFormat.format(job.output_tokens)}`}</dd></div>
      <div><dt>{t("Geschätzte Anbieter-Kosten")}</dt><dd>{job.cost_basis === 'unknown' || job.estimated_provider_cost_usd === null ? t("Noch unbekannt") : t('{cost} US-Dollar (Schätzung aus Tokenverbrauch)', { cost: moneyFormat.format(job.estimated_provider_cost_usd) })}</dd></div>
      <div><dt>{t("Vom Anbieter abgerechnete Kosten")}</dt><dd>{job.cost_basis === 'provider_billed' && job.measured_provider_cost_usd !== null ? t('{cost} US-Dollar', { cost: moneyFormat.format(job.measured_provider_cost_usd) }) : t("Noch unbekannt")}</dd></div>
      <div><dt>{t("Fertiggestellt")}</dt><dd>{timestamp(job.completed_at, locale, t)}</dd></div>
    </dl>
    {(job.original_url || job.versions.length > 0 || job.status === 'review') && <Review job={job} selectedVersion={activeVersionId} onVersionChange={(version) => setVersionSelection({ job: job.id, version })} channel={channel} canStart={canStart} onJob={onJob} onReload={onReload} />}
  </article>;
}

export function AiEditorPanel(props: AiEditorPanelProps) {
  return <AiEditorContent key={`${props.twitchUserId}-${props.clipDbId ?? ''}`} {...props} />;
}

function AiEditorContent({ twitchUserId, streamer, clipDbId }: AiEditorPanelProps) {
  const t = useT();
  const { locale } = useLanguage();
  const client = useQueryClient();
  const [clipSelection, setClipSelection] = useState<ScopedSelection | null>(null);
  const [jobSelection, setJobSelection] = useState<ScopedSelection | null>(null);
  const [confirmChannel, setConfirmChannel] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [page, setPage] = useState(1);
  const [consentRevision, setConsentRevision] = useState(0);
  const [selectionRevision, setSelectionRevision] = useState(0);
  const confirmationRevision = useRef(0);
  const jobSelectionRevision = useRef(0);
  const consentSwitch = useRef<HTMLButtonElement>(null);
  const confirmationCheckbox = useRef<HTMLInputElement>(null);
  const reloadConsentButton = useRef<HTMLButtonElement>(null);
  const focusConsentSwitch = () => requestAnimationFrame(() => consentSwitch.current?.focus());
  const focusConsentReload = () => requestAnimationFrame(() => reloadConsentButton.current?.focus());
  const advanceConsent = () => { confirmationRevision.current += 1; setConsentRevision(confirmationRevision.current); return confirmationRevision.current; };
  const advanceSelection = () => { jobSelectionRevision.current += 1; setSelectionRevision(jobSelectionRevision.current); };
  const hasChannel = twitchUserId.trim().length > 0;
  const consentKey = ['social-ai', 'consent', twitchUserId] as const;
  const jobsKey = ['social-ai', 'jobs', twitchUserId] as const;
  const consent = useQuery({ queryKey: consentKey, queryFn: ({ signal }) => fetchAiConsent(twitchUserId, signal), enabled: hasChannel, retry: false, refetchOnWindowFocus: false });
  const jobs = useQuery({ queryKey: jobsKey, queryFn: ({ signal }) => fetchAiJobs(twitchUserId, signal), enabled: hasChannel, retry: false, refetchOnWindowFocus: false });
  const clips = useQuery({ queryKey: ['social-ai', 'clips', twitchUserId, page], queryFn: ({ signal }) => fetchClips({ twitch_user_id: twitchUserId, page, page_size: 30 }, signal), enabled: hasChannel, retry: false, refetchOnWindowFocus: false });
  const clipsInvalid = clips.isSuccess && (!Array.isArray(clips.data.items) || !Number.isSafeInteger(clips.data.total) || clips.data.total < 0 || clips.data.page !== page || clips.data.page_size !== 30 || clips.data.items.length > 30);
  const availableClips = !clipsInvalid && Array.isArray(clips.data?.items) ? clips.data.items.filter(isSelectableClip) : [];
  const validHandoff = Number.isSafeInteger(clipDbId) && (clipDbId ?? 0) > 0;
  const handoffOnPage = availableClips.some((clip) => clip.clip_db_id === clipDbId);
  const handoff = useQuery({
    queryKey: ['social-ai', 'clip-handoff', twitchUserId, clipDbId],
    queryFn: ({ signal }) => findChannelClip(twitchUserId, clipDbId!, signal),
    enabled: hasChannel && validHandoff && clips.isSuccess && !clipsInvalid && !handoffOnPage,
    retry: false,
    refetchOnWindowFocus: false,
  });
  const selectedClipId = clipSelection?.channel === twitchUserId ? clipSelection.id : clipDbId;
  const handoffClip = handoff.data?.kind === 'found' ? handoff.data.clip : null;
  const selectedClip = availableClips.find((clip) => clip.clip_db_id === selectedClipId) ?? (handoffClip?.clip_db_id === selectedClipId ? handoffClip : undefined);
  const handoffOutsidePage = handoffClip && !handoffOnPage ? handoffClip : null;
  const selectedJobId = jobSelection?.channel === twitchUserId ? jobSelection.id : jobs.data?.[0]?.id;
  const jobKey = ['social-ai', 'job', twitchUserId, selectedJobId] as const;
  const job = useQuery({
    queryKey: jobKey,
    queryFn: ({ signal }) => fetchAiJob(twitchUserId, selectedJobId!, signal),
    enabled: hasChannel && Number.isSafeInteger(selectedJobId) && !jobs.isError,
    retry: false,
    refetchOnWindowFocus: false,
    refetchInterval: (query) => query.state.status === 'success' && query.state.data && !['review', 'failed', 'rejected', 'awaiting_confirmation'].includes(query.state.data.status) ? 5000 : false,
  });
  const savingConsent = useMutation({
    mutationFn: ({ channel, allow }: { channel: string; allow: boolean; revision: number }) => saveAiConsent(channel, allow),
    onSuccess: (value, { channel, revision }) => {
      client.setQueryData(['social-ai', 'consent', channel], value);
      if (revision === confirmationRevision.current) { setConfirmChannel(null); setConfirmed(false); focusConsentSwitch(); }
    },
    onError: (_error, { revision }) => {
      if (revision === confirmationRevision.current) focusConsentReload();
    },
  });
  const start = useMutation({
    mutationFn: ({ channel, clip }: { channel: string; clip: number; revision: number }) => startAiJob(channel, clip),
    onSuccess: (value, { channel, revision }) => {
      client.setQueryData(['social-ai', 'job', channel, value.id], value);
      client.invalidateQueries({ queryKey: ['social-ai', 'jobs', channel] });
      if (revision === jobSelectionRevision.current) setJobSelection({ channel, id: value.id });
    },
  });
  const retry = useMutation({
    mutationFn: ({ channel, id }: { channel: string; id: number; revision: number }) => retryAiJob(channel, id),
    onSuccess: (value, { channel, id }) => {
      client.setQueryData(['social-ai', 'job', channel, id], value);
      client.invalidateQueries({ queryKey: ['social-ai', 'jobs', channel] });
    },
  });
  const consentOpen = confirmChannel === twitchUserId;
  const currentConsentAction = savingConsent.variables?.channel === twitchUserId && savingConsent.variables.revision === consentRevision;
  const consentSaved = savingConsent.isSuccess && currentConsentAction;
  const consentError = savingConsent.isError && currentConsentAction;
  const canStart = consent.isSuccess && consent.data.external_ai_consent === true && consent.data.global_allowed === true && !savingConsent.isPending && !consentError && !consent.isFetching;
  useEffect(() => {
    if (consentOpen && !consent.data?.external_ai_consent) confirmationCheckbox.current?.focus();
  }, [consentOpen, consent.data?.external_ai_consent]);
  const reloadConsent = async () => {
    const revision = confirmationRevision.current;
    const result = await consent.refetch();
    if (result.isSuccess && revision === confirmationRevision.current) {
      savingConsent.reset();
      setConfirmChannel(null);
      setConfirmed(false);
      focusConsentSwitch();
    }
  };
  const currentStartAction = start.variables?.channel === twitchUserId && start.variables.clip === selectedClip?.clip_db_id && start.variables.revision === selectionRevision;
  const currentRetryAction = retry.variables?.channel === twitchUserId && retry.variables.id === selectedJobId && retry.variables.revision === selectionRevision;
  const clipCount = clips.data?.total ?? 0;
  return <section className="social-ai" aria-labelledby="social-ai-title">
    <header className="social-ai-header"><div><h2 id="social-ai-title"><Film aria-hidden="true" size={25} />{t("KI-Videoschnitt")}</h2><p>{t('Wähle einen Clip aus deinem Kanal')} {streamer ? <strong>{streamer}</strong> : null} {t('und starte eine einzelne Bearbeitung. Dafür muss der Auto-Pilot nicht aktiviert sein.')}</p></div><span className="social-ai-manual"><ShieldCheck aria-hidden="true" size={16} />{t("Manuelle Sichtung")}</span></header>
    <p className="social-ai-notice">{t("Ein fertiges Video wird hier zur Sichtung bereitgestellt. Es wird durch diesen Auftrag nicht automatisch veröffentlicht oder in den Zeitplan übernommen.")}</p>
    {!hasChannel ? <EditorError>{t("Bitte wähle zuerst einen Kanal, um Clips und Freigaben zu laden.")}</EditorError> : <>
      <section className="social-ai-setup" aria-label={t("Freigabe und Clipauswahl")}>
        <div className="social-ai-consent"><div className="social-ai-section-head"><h3>{t("Freigabe für externe KI")}</h3><p>{t("Diese Freigabe gilt für deinen Kanal und ist unabhängig vom Auto-Piloten.")}</p></div>
          {consent.isPending ? <p role="status">{t("Freigabe wird geladen…")}</p> : consent.isError ? <EditorError retry={reloadConsent}>{t("Die Freigabe konnte nicht geladen werden. Der aktuelle Stand ist unklar; der Start bleibt gesperrt.")}</EditorError> : <>
            <button ref={consentSwitch} type="button" className="studio-button social-ai-consent-toggle" role="switch" aria-checked={consent.data.external_ai_consent} aria-controls={consentOpen && !consent.data.external_ai_consent ? 'social-ai-consent-confirm' : undefined} disabled={savingConsent.isPending || consentError || consent.isFetching} onClick={() => { const revision = advanceConsent(); if (consent.data.external_ai_consent) savingConsent.mutate({ channel: twitchUserId, allow: false, revision }); else { setConfirmChannel(twitchUserId); setConfirmed(false); } }}>{consent.data.external_ai_consent ? t("Externe KI: zugestimmt") : t("Externe KI: nicht zugestimmt")}</button>
            <p className={`social-ai-global ${consent.data.global_allowed ? '' : 'social-ai-caution'}`}>{consent.data.global_allowed ? <Check aria-hidden="true" size={16} /> : <AlertTriangle aria-hidden="true" size={16} />}{consent.data.global_allowed ? t("Die globale Freigabe ist aktiv.") : t("Die globale Freigabe ist gesperrt. Eine Bearbeitung kann derzeit nicht gestartet werden.")}</p>
            {consentOpen && !consent.data.external_ai_consent && <div id="social-ai-consent-confirm" className="social-ai-confirm" role="group" aria-label={t("Freigabe bestätigen")} aria-describedby="social-ai-consent-description"><p id="social-ai-consent-description">{t("Wenn du zustimmst, werden Einzelbilder des Clips und die erkannte Mitschrift an den konfigurierten externen KI-Anbieter übertragen. Prüfe diese Freigabe vor dem Start bewusst.")}</p><label><input ref={confirmationCheckbox} type="checkbox" aria-describedby="social-ai-consent-description" checked={confirmed} disabled={savingConsent.isPending || consentError || consent.isFetching} onChange={(event) => setConfirmed(event.target.checked)} />{t("Ich stimme der Übertragung von Einzelbildern und Mitschrift an den konfigurierten KI-Anbieter zu.")}</label><div className="social-ai-actions"><button type="button" className="studio-primary" disabled={!confirmed || savingConsent.isPending || consentError || consent.isFetching} onClick={() => savingConsent.mutate({ channel: twitchUserId, allow: true, revision: confirmationRevision.current })}>{t("Zustimmung erteilen")}</button><button type="button" className="studio-button" disabled={savingConsent.isPending || consentError || consent.isFetching} onClick={() => { advanceConsent(); setConfirmChannel(null); setConfirmed(false); consentSwitch.current?.focus(); }}>{t("Abbrechen")}</button></div></div>}
            <div className="social-ai-consent-feedback" role="status" aria-live="polite" aria-atomic="true">
              {savingConsent.isPending && currentConsentAction && <p>{savingConsent.variables.allow ? t("Zustimmung wird gespeichert…") : t("Widerruf wird gespeichert…")}</p>}
              {consentSaved && <p>{savingConsent.data.external_ai_consent ? t("Freigabe gespeichert. Du hast der Übertragung an externe KI zugestimmt.") : t("Freigabe widerrufen. Ohne Zustimmung werden keine neuen KI-Bearbeitungen gestartet.")}</p>}
              {consentError && <div className="social-ai-alert"><AlertTriangle aria-hidden="true" size={18} /><div><p>{consent.isFetching ? t("Freigabe wird erneut geladen…") : t("Speichern fehlgeschlagen. Ob die Änderung übernommen wurde, ist unklar. Lade den aktuellen Stand neu, bevor du die Freigabe erneut änderst.")}</p><button ref={reloadConsentButton} type="button" className="studio-button" disabled={consent.isFetching} onClick={reloadConsent}>{t("Freigabe neu laden")}</button></div></div>}
            </div>
          </>}
        </div>
        <div className="social-ai-pick"><div className="social-ai-section-head"><h3>{t("Clip auswählen")}</h3><p>{t("Clips aus deinem eigenen Kanal, ohne automatische Bearbeitung.")}</p></div>
          {clips.isPending ? <p role="status">{t("Clips werden geladen…")}</p> : clips.isError || clipsInvalid ? <EditorError retry={() => clips.refetch()}>{t("Die Clips konnten nicht geladen werden. Bitte versuche es erneut.")}</EditorError> : availableClips.length === 0 && !handoffOutsidePage ? <div><p>{t("Auf dieser Seite sind keine Twitch-Clips vorhanden.")}</p><div className="social-ai-actions">{page > 1 && <button type="button" className="studio-button" onClick={() => { setPage(page - 1); setClipSelection(null); advanceSelection(); }}>{t("Zurück")}</button>}{page * 30 < clipCount && <button type="button" className="studio-button" onClick={() => { setPage(page + 1); setClipSelection(null); advanceSelection(); }}>{t("Weitere Clips ansehen")}</button>}</div></div> : <><label htmlFor="social-ai-clip">{t("Clip aus dem Kanal")}</label><select id="social-ai-clip" className="studio-select" value={selectedClip?.clip_db_id ?? ''} onChange={(event) => { advanceSelection(); setClipSelection({ channel: twitchUserId, id: Number(event.target.value) }); }}><option value="">{t("Bitte Clip wählen")}</option>{handoffOutsidePage && <option value={handoffOutsidePage.clip_db_id}>{handoffOutsidePage.title || t('Clip {id}', { id: handoffOutsidePage.clip_db_id })}  {t("· Übernommener Clip")}</option>}{availableClips.map((clip) => <option key={clip.clip_db_id} value={clip.clip_db_id}>{clip.title || t('Clip {id}', { id: clip.clip_db_id })} · {new Date(clip.created_at).toLocaleDateString(locale)}</option>)}</select><div className="social-ai-pagination"><span>{t('{count} Einträge im Clipbestand · Seite {page}', { count: clipCount, page })}</span><div><button type="button" className="studio-button" disabled={page === 1} onClick={() => { setPage(page - 1); setClipSelection(null); advanceSelection(); }}>{t("Zurück")}</button><button type="button" className="studio-button" disabled={page * 30 >= clipCount} onClick={() => { setPage(page + 1); setClipSelection(null); advanceSelection(); }}>{t("Weitere Clips")}</button></div></div></>}
          {clipSelection === null && clipDbId !== undefined && !handoffOnPage && (validHandoff ? handoff.isPending && clips.isSuccess ? <p role="status">{t("Übernommener Clip wird im Kanalbestand geprüft…")}</p> : handoff.isError ? <EditorError retry={() => handoff.refetch()}>{t("Der übernommene Clip konnte nicht geprüft werden. Wähle einen Clip aus dem Kanalbestand oder versuche es erneut.")}</EditorError> : handoff.isSuccess && handoff.data.kind === 'limit_reached' ? <EditorError>{t("Der übernommene Clip wurde auf den ersten 20 Seiten des Kanalbestands nicht gefunden. Weitere Clips wurden nicht geprüft. Wähle einen Clip aus dem Kanalbestand.")}</EditorError> : handoff.isSuccess && handoff.data.kind === 'not_found' ? <EditorError>{t("Der übernommene Clip wurde im geprüften Kanalbestand nicht als Twitch-Clip gefunden. Wähle einen Clip aus dem Kanalbestand.")}</EditorError> : null : <EditorError>{t("Die übernommene Clip-ID ist ungültig. Wähle einen Clip aus dem Kanalbestand.")}</EditorError>)}
          <div className="social-ai-actions"><button type="button" className="studio-primary" disabled={!canStart || !selectedClip || !clips.isSuccess || clipsInvalid || start.isPending || jobs.isError} onClick={() => { if (selectedClip && canStart && clips.isSuccess && !clipsInvalid) start.mutate({ channel: twitchUserId, clip: selectedClip.clip_db_id, revision: jobSelectionRevision.current }); }}>{start.isPending && currentStartAction ? t("Bearbeitung wird gestartet…") : t("KI-Bearbeitung manuell starten")}</button></div>
          {start.isError && currentStartAction && <EditorError>{t("Die Bearbeitung konnte nicht gestartet werden. Es wurde kein fertiges Video bestätigt. Bitte prüfe Freigabe und Clip und versuche es erneut.")}</EditorError>}
        </div>
      </section>
      <EditorOptionsPanel key={twitchUserId} channel={twitchUserId} />
      <div className="social-ai-history"><div><h3>{t("Bearbeitungsaufträge")}</h3><p>{t("Zuletzt gestartete Aufträge für diesen Kanal.")}</p></div>{jobs.data && jobs.data.length > 0 && <label>{t("Auftrag ansehen")}<select className="studio-select" value={selectedJobId ?? ''} onChange={(event) => { advanceSelection(); setJobSelection({ channel: twitchUserId, id: Number(event.target.value) }); }}>{jobs.data.map((item) => <option key={item.id} value={item.id}>{t("Auftrag")} {item.id} · {t(statusLabels[item.status])}</option>)}</select></label>}</div>
      {jobs.isPending ? <p role="status">{t("Aufträge werden geladen…")}</p> : jobs.isError ? <EditorError retry={() => jobs.refetch()}>{t("Die Aufträge konnten nicht geladen werden. Der Start bleibt gesperrt, bis der Stand bekannt ist.")}</EditorError> : jobs.data?.length === 0 ? <p className="social-ai-empty">{t("Noch kein Auftrag für diesen Kanal. Wähle oben einen Clip für einen manuellen Test.")}</p> : job.isPending ? <p role="status">{t("Auftrag wird geladen…")}</p> : job.isError ? <EditorError retry={() => job.refetch()}>{t("Der Auftrag konnte nicht geladen werden. Die letzte bekannte Fassung wird nicht als aktueller Stand angezeigt.")}</EditorError> : job.data && <JobDetails key={`${twitchUserId}-${job.data.id}`} job={job.data} channel={twitchUserId} onReload={async () => (await job.refetch()).isSuccess} onJob={(value) => {client.setQueryData(['social-ai','job',twitchUserId,value.id],value);client.invalidateQueries({queryKey:jobsKey});if (selectionRevision === jobSelectionRevision.current && value.id !== selectedJobId) setJobSelection({channel:twitchUserId,id:value.id});}} canStart={canStart} onRetry={() => { if (canStart && job.data?.status === 'failed' && job.data.can_retry) retry.mutate({ channel: twitchUserId, id: job.data.id, revision: jobSelectionRevision.current }); }} retryPending={retry.isPending && currentRetryAction} retryError={retry.isError && currentRetryAction} />}
    </>}
  </section>;
}
