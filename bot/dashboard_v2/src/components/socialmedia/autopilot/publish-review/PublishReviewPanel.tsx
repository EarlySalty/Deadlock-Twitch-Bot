import { useEffect, useRef, useState } from 'react';
import { ShieldCheck } from 'lucide-react';
import { StudioSectionHeading } from '../../StudioSectionHeading';
import { useLanguage } from '../../../../context/LanguageContext';
import type { TranslateParams } from '../../../../i18n/dictionary';
import { publishReviewClient } from './api';
import type { DestinationDecision, NotificationPreferences, PendingHandling, PublishDestination, PublishPlatform, PublishReviewClient, PublishSnapshot, PublishVideo, TikTokVersionOptions } from './api';
import './publish-review.css';

export interface PublishReviewPanelProps {
  twitchUserId: string;
  client?: PublishReviewClient;
  onChanged?: () => void;
  onOpenVideo?: (videoId: number) => void;
  onConfigureTikTok?: (videoId: number) => Promise<TikTokVersionOptions | null>;
}

const names: Record<PublishPlatform, string> = { youtube: 'YouTube', tiktok: 'TikTok', instagram: 'Instagram' };
const states: Record<PublishDestination['state'], string> = {
  pending: 'Eingeplant', preparing: 'Datei wird übertragen', effect_started: 'Veröffentlichung gestartet',
  processing: 'Beim Anbieter in Verarbeitung', published: 'Veröffentlicht', retry: 'Erneuter Versuch geplant',
  held: 'Prüfung erforderlich', uncertain: 'Ausgang unklar, kein zweiter Upload', discarded: 'Für diesen Termin verworfen', failed: 'Versuche beendet',
};
const events: { key: keyof NotificationPreferences; label: string; }[] = [
  { key: 'ready_for_review', label: 'Video fertig zur Prüfung' },
  { key: 'publication_deadline', label: 'Veröffentlichung steht bevor' },
  { key: 'processing_failed', label: 'Verarbeitung fehlgeschlagen' },
  { key: 'publication_failed', label: 'Veröffentlichung fehlgeschlagen' },
  { key: 'empty_pool', label: 'Keine geeigneten Clips mehr' },
  { key: 'review_expiring', label: 'Aufbewahrungsfrist endet bald' },
];

type PanelMessage = { key: string; params?: TranslateParams; };
type PanelError = PanelMessage | { raw: string; };

class PanelMessageError extends Error {
  readonly key: string;

  constructor(key: string) {
    super(key);
    this.key = key;
  }
}

export function PublishReviewPanel(props: PublishReviewPanelProps) {
  return <OwnerPanel key={props.twitchUserId} {...props} />;
}

function OwnerPanel({ twitchUserId, client = publishReviewClient, onChanged, onOpenVideo, onConfigureTikTok }: PublishReviewPanelProps) {
  const { t } = useLanguage();
  const [snapshot, setSnapshot] = useState<PublishSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<PanelError | null>(null);
  const [message, setMessage] = useState<PanelMessage | null>(null);
  const [modeDialog, setModeDialog] = useState(false);
  const [pending, setPending] = useState<PendingHandling | null>(null);
  const [preferences, setPreferences] = useState<NotificationPreferences | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const mounted = useRef(false);
  const saving = useRef(false);
  const preferencesDirty = preferences !== null && snapshot !== null && JSON.stringify(preferences) !== JSON.stringify(snapshot.settings.notifications);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    client.load(twitchUserId, controller.signal).then((data) => {
      if (!controller.signal.aborted) { setSnapshot(data); setPreferences(data.settings.notifications); }
    }).catch(() => {
      if (!controller.signal.aborted) setError({ key: 'Die Veröffentlichungsdaten konnten nicht geladen werden. Bitte versuche es erneut.' });
    });
    return () => controller.abort();
  }, [client, twitchUserId]);

  useEffect(() => {
    if (modeDialog && !dialog.current?.open) dialog.current?.showModal();
    if (!modeDialog && dialog.current?.open) dialog.current?.close();
  }, [modeDialog]);

  async function act(action: () => Promise<void>, success: PanelMessage, savePreferences = false) {
    if (saving.current) return false;
    saving.current = true;
    setBusy(true); setError(null); setMessage(null);
    try {
      await action();
      onChanged?.();
      const data = await client.load(twitchUserId);
      if (mounted.current) {
        setSnapshot(data);
        if (savePreferences || !preferencesDirty) setPreferences(data.settings.notifications);
        setMessage(success);
      }
      return true;
    } catch (cause) {
      if (mounted.current) setError(cause instanceof PanelMessageError ? { key: cause.key } : cause instanceof Error && !/^(http_|Unauthorized|Server-Fehler)/.test(cause.message) ? { raw: cause.message } : { key: 'Die Änderung konnte nicht gespeichert werden. Bitte prüfe die Verbindung und versuche es erneut.' });
      return false;
    } finally {
      saving.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  return <section className="publish-review" aria-label={t('Freigabe und Veröffentlichung')} aria-busy={busy} data-unsaved={preferencesDirty || pending !== null || busy ? 'true' : undefined} data-saving={busy ? 'true' : undefined}>
    <StudioSectionHeading title={t('Freigabe & Veröffentlichung')} icon={ShieldCheck}
      description={t('Prüfe die fertige Videoversion. Eine Kontoverbindung ist noch keine Freigabe.')}
      actions={<button type="button" disabled={busy} onClick={() => void act(async () => {}, { key: 'Stand aktualisiert.' })}>{t('Aktualisieren')}</button>} />
    {error && <p role="alert" className="publish-error">{'raw' in error ? error.raw : t(error.key, error.params)}</p>}
    {message && <p role="status" className="publish-message">{t(message.key, message.params)}</p>}
    {!snapshot && !error && <p role="status">{t('Veröffentlichungsdaten werden geladen.')}</p>}
    {snapshot && <>
      <div className="publish-mode-row">
        <div><strong>{t(snapshot.settings.publish_mode === 'review' ? 'Prüfung vor Veröffentlichung' : 'Vollautomatik')}</strong><p>{t(snapshot.settings.publish_mode === 'review' ? 'Ohne deine Videofreigabe wird nichts veröffentlicht.' : 'Automatische Videos werden ohne regelmäßige Freigabe veröffentlicht. Qualitätsbedenken bleiben zur Prüfung.')}</p></div>
        <div className="publish-actions">
          <button type="button" disabled={busy} onClick={() => { setPending(null); setModeDialog(true); }}>{t(snapshot.settings.publish_mode === 'review' ? 'Vollautomatik einrichten' : 'Zur Prüfung wechseln')}</button>
          <button type="button" disabled={busy} onClick={() => void act(() => client.pause(twitchUserId, !snapshot.settings.paused), { key: snapshot.settings.paused ? 'Auto-Pilot fortgesetzt. Verpasste Termine werden nicht gesammelt nachgeholt.' : 'Auto-Pilot pausiert. Laufende Verarbeitung kann fertig werden.' })}>{t(snapshot.settings.paused ? 'Fortsetzen' : 'Pausieren')}</button>
        </div>
      </div>
      {snapshot.settings.paused && <p className="publish-note" role="status">{t('Pausiert. Keine neuen geplanten Veröffentlichungen. Deine Warteschlange bleibt erhalten.')}</p>}
      <div className="publish-video-list">
        {snapshot.videos.length === 0 ? <p>{t('Noch keine fertigen Videos. Sobald ein Video gerendert ist, findest du es hier.')}</p> : snapshot.videos.map((video) => <VideoRow key={video.id} video={video} snapshot={snapshot} busy={busy} onOpenVideo={onOpenVideo} onConfigureTikTok={onConfigureTikTok} act={act} client={client} owner={twitchUserId} />)}
      </div>
      {preferences && <details className="publish-preferences"><summary>{t('Benachrichtigungen & Aufbewahrung')}</summary>
        <p>{t('Ungeprüfte Videos bleiben sieben Tage ab dem fertigen Rendern verfügbar. Freigegebene Videos sowie laufende, fehlgeschlagene oder unklare Veröffentlichungen bleiben geschützt.')}</p>
        <form onSubmit={(event) => { event.preventDefault(); void act(() => client.preferences(twitchUserId, preferences), { key: 'Benachrichtigungen gespeichert.' }, true); }}>
          <fieldset disabled={busy}><legend>{t('Wo möchtest du Hinweise erhalten?')}</legend>
            <label><input type="checkbox" role="switch" checked={preferences.dashboard} onChange={(event) => setPreferences({ ...preferences, dashboard: event.target.checked })} />{t('Im Dashboard')}</label>
            <label><input type="checkbox" role="switch" checked={preferences.discord} disabled={!snapshot.capabilities.discord} onChange={(event) => setPreferences({ ...preferences, discord: event.target.checked })} />{t('Discord, nur nach deiner Auswahl')}</label>
            <p className="publish-note">{t('E-Mail ist nicht verfügbar. Freigaben erfolgen ausschließlich im Dashboard.')}</p>
          </fieldset>
          <fieldset disabled={busy}><legend>{t('Welche Hinweise möchtest du?')}</legend>
            {events.map(({ key, label }) => <label key={key}><input type="checkbox" role="switch" checked={Boolean(preferences[key])} onChange={(event) => setPreferences({ ...preferences, [key]: event.target.checked })} />{t(label)}</label>)}
            <label>{t('Vorwarnung in Stunden')}<input aria-label={t('Vorwarnung in Stunden')} type="number" min="1" max="168" value={preferences.reminder_hours} onChange={(event) => setPreferences({ ...preferences, reminder_hours: Number(event.target.value) })} /></label>
          </fieldset>
          <button type="submit" disabled={busy}>{t('Benachrichtigungen speichern')}</button>
        </form>
      </details>}
      {snapshot.notifications.length > 0 && <section className="publish-notifications" aria-label={t('Hinweise')}><h3>{t('Deine Hinweise')}</h3>{snapshot.notifications.map((notification) => <p key={notification.id}>{notification.content}</p>)}</section>}
    </>}
    {modeDialog && <dialog ref={dialog} className="publish-mode-dialog" aria-labelledby="publish-mode-title" onCancel={(event) => { if (busy) event.preventDefault(); else { setPending(null); setModeDialog(false); } }} onClose={() => { if (!busy) { setPending(null); setModeDialog(false); } }}>
      <h3 id="publish-mode-title">{t(snapshot?.settings.publish_mode === 'review' ? 'Vollautomatik aktivieren?' : 'Zur manuellen Prüfung wechseln?')}</h3>
      {snapshot?.settings.publish_mode === 'review' ? <>
        <p>{t('Die Vollautomatik darf geeignete automatische Videos ohne weitere Freigabe veröffentlichen. Manuelle Testvideos und Videos mit Qualitätsbedenken werden nicht automatisch übernommen.')}</p>
        <fieldset disabled={busy}><legend>{t('Was passiert mit bereits wartenden Videos?')}</legend>
          <label><input type="radio" name="pending" checked={pending === 'keep_review'} onChange={() => setPending('keep_review')} />{t('Zur manuellen Prüfung behalten')}</label>
          <label><input type="radio" name="pending" checked={pending === 'transfer_eligible'} onChange={() => setPending('transfer_eligible')} />{t('Geeignete automatische Videos ausdrücklich übernehmen')}</label>
        </fieldset>
      </> : <p>{t('Noch nicht veröffentlichte Ziele benötigen wieder deine ausdrückliche Freigabe. Bereits veröffentlichte Beiträge bleiben unverändert.')}</p>}
      <div className="publish-actions"><button type="button" disabled={busy} onClick={() => { setPending(null); setModeDialog(false); }}>{t('Abbrechen')}</button><button type="button" className="publish-primary" disabled={busy || (snapshot?.settings.publish_mode === 'review' && pending === null)} onClick={() => {
        const mode = snapshot?.settings.publish_mode === 'review' ? 'full_auto' : 'review';
        void act(() => client.transition(twitchUserId, { mode, confirmed: true, ...(mode === 'full_auto' && pending ? { pending } : {}) }), { key: 'Veröffentlichungsmodus gespeichert.' }).then((saved) => { if (saved && mounted.current) { setPending(null); setModeDialog(false); } });
      }}>{t('Bewusst bestätigen')}</button></div>
    </dialog>}
  </section>;
}

interface VideoRowProps {
  video: PublishVideo;
  snapshot: PublishSnapshot;
  busy: boolean;
  owner: string;
  client: PublishReviewClient;
  act: (action: () => Promise<void>, success: PanelMessage) => Promise<boolean>;
  onOpenVideo?: PublishReviewPanelProps['onOpenVideo'];
  onConfigureTikTok?: PublishReviewPanelProps['onConfigureTikTok'];
}

function VideoRow({ video, snapshot, busy, owner, client, act, onOpenVideo, onConfigureTikTok }: VideoRowProps) {
  const { t, locale } = useLanguage();
  const date = (value: string | null) => value ? new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value)) : t('Noch kein Termin');
  const [selected, setSelected] = useState<PublishPlatform[]>([]);
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const platforms = [...new Set<PublishPlatform>([...snapshot.target_platforms, ...(video.event?.destinations.map((destination) => destination.platform) ?? []), ...video.approvals.map((approval) => approval.platform)])].filter((platform) => platform in names);
  const expired = video.status === 'expired' || video.artifact_state === 'deleted' || video.artifact_state === 'missing';
  const actionable = !expired && ['review', 'approved', 'scheduled'].includes(video.status);
  async function approve() {
    const decisions: DestinationDecision[] = [];
    for (const platform of selected) {
      if (platform === 'tiktok') {
        const options = await onConfigureTikTok?.(video.id);
        if (!options) throw new PanelMessageError('Bitte prüfe die TikTok-Einstellungen dieser Videoversion.');
        decisions.push({ platform, tiktok_options: options });
      } else decisions.push({ platform });
    }
    await client.approve(owner, video.id, decisions);
  }
  return <article className="publish-video" data-unsaved={selected.length > 0 || busy ? 'true' : undefined} aria-label={t('Videoversion {version}, Variante {variant}', { version: video.version_no, variant: video.variant_index + 1 })}>
    <header><div><h3>{t('Video #{id}', { id: video.id })} <span>{t('Version {version}, Variante {variant}', { version: video.version_no, variant: video.variant_index + 1 })}</span></h3><p>{t(video.mode === 'ai' ? 'KI-bearbeitet' : 'Statisch bearbeitet')}{video.duration_ms ? ` · ${new Intl.NumberFormat(locale, { style: 'unit', unit: 'second', unitDisplay: 'long' }).format(video.duration_ms / 1000)}` : ''}</p></div>{onOpenVideo && !expired && <button type="button" disabled={busy} onClick={() => onOpenVideo(video.id)}>{t('Video prüfen')}</button>}</header>
    {!onOpenVideo && !expired && client.videoUrl && <video className="publish-preview" controls preload="none" playsInline aria-label={t('Vorschau Video #{id}', { id: video.id })} src={client.videoUrl(owner, video.id)} />}
    {video.needs_review && <p className="publish-note">{t('Dieses Video benötigt deine Prüfung. Bitte prüfe die Bildqualität und ob die wichtige Spielsituation vollständig erhalten ist.')}</p>}
    {expired ? <p className="publish-note">{t('Aufbewahrungsfrist abgelaufen. Es wird nicht automatisch neu gerendert.')}</p> : <p className="publish-expiry">{video.status === 'review' ? t('Ohne Freigabe verfügbar bis {date}', { date: date(video.expires_at) }) : t('Datei für Freigabe, Veröffentlichung und erforderliche Wiederholungen geschützt.')}</p>}
    {video.cleanup_error && <p role="alert" className="publish-error">{t('Die Videodatei konnte nicht gelöscht werden. Wir versuchen es nicht automatisch erneut.')}</p>}
    {video.event && <p>{t('Gemeinsamer Termin:')} <strong>{date(video.event.scheduled_at)}</strong>{` · ${t(video.event.executed_at ? 'Termin ausgeführt' : 'Noch nicht ausgeführt')}`}</p>}
    <div className="publish-destinations">
      {platforms.map((platform) => {
        const connection = snapshot.connections.find((connection) => connection.platform === platform);
        const approval = video.approvals.find((approval) => approval.platform === platform);
        const destination = video.event?.destinations.find((destination) => destination.platform === platform);
        const terminal = destination?.state === 'published' || destination?.state === 'discarded';
        const canSelect = actionable && !terminal && connection?.connected && (platform !== 'tiktok' || Boolean(onConfigureTikTok));
        return <section key={platform} className="publish-destination" aria-label={names[platform]}>
          <label><input type="checkbox" checked={selected.includes(platform)} disabled={busy || !canSelect} onChange={(event) => setSelected(event.target.checked ? [...selected, platform] : selected.filter((value) => value !== platform))} />{names[platform]}</label>
          <dl><div><dt>{t('Verbindung')}</dt><dd>{t(connection?.connected ? 'Verbunden' : 'Nicht verbunden')}</dd></div><div><dt>{t('Videofreigabe')}</dt><dd>{t(approval?.state === 'approved' ? 'Für dieses Video freigegeben' : 'Nicht freigegeben')}</dd></div><div><dt>{t('Veröffentlichung')}</dt><dd>{destination ? t(states[destination.state]) : t('Noch nicht eingeplant')}</dd></div></dl>
          {platform === 'tiktok' && !snapshot.capabilities.tiktok_effect_boundary && <p className="publish-note">{t('Diese Videoversion bleibt für TikTok zur Prüfung, bis der sichere Veröffentlichungsweg verfügbar ist.')}</p>}
          {destination?.state === 'retry' && <p>{t('Nächster Versuch: {date}', { date: date(destination.next_retry_at) })}</p>}
          {destination && ['retry', 'held'].includes(destination.state) && !['account_changed', 'account_missing', 'exact_file_changed', 'tiktok_effect_boundary_pending'].includes(destination.reason ?? '') && <button type="button" disabled={busy} onClick={() => void act(() => client.retry(owner, video.id, platform), { key: '{platform} wird mit derselben Datei erneut geprüft.', params: { platform: names[platform] } })}>{t('Nur {platform} erneut versuchen', { platform: names[platform] })}</button>}
        </section>;
      })}
    </div>
    {actionable && <div className="publish-actions">
      <button type="button" disabled={busy || platforms.every((platform) => !snapshot.connections.some((connection) => connection.platform === platform && connection.connected))} onClick={() => setSelected(platforms.filter((platform) => snapshot.connections.some((connection) => connection.platform === platform && connection.connected) && (platform !== 'tiktok' || Boolean(onConfigureTikTok))))}>{t('Alle verfügbaren Ziele auswählen')}</button>
      <button type="button" className="publish-primary" disabled={busy || selected.length === 0} onClick={() => void act(approve, { key: 'Ausgewählte Ziele für genau diese Videoversion freigegeben.' }).then((saved) => { if (saved && mounted.current) setSelected([]); })}>{t('Ausgewählte Ziele freigeben')}</button>
      {!video.event && <button type="button" disabled={busy || !video.approvals.some((approval) => approval.state === 'approved')} onClick={() => void act(() => client.schedule(owner, video.id, platforms), { key: 'Fertige Datei für den nächsten freien gemeinsamen Termin eingeplant.' })}>{t('In den Zeitplan übernehmen')}</button>}
    </div>}
  </article>;
}
