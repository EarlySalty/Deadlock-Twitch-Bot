import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { fetchTikTokCreatorInfo, fetchTikTokEditor, saveTikTokChoices, saveTikTokMusicConsent, getPreviewStatus, requestPreview, previewFileUrl, SocialMediaApiError, type TikTokDraft, type TikTokEditorState, type TikTokPostOptions } from '@/api/socialMedia';
import { WorkspaceDialog } from './WorkspaceDialog';
import { VideoFullscreenButton } from './VideoFullscreenButton';
import { useT } from '@/context/LanguageContext';
import { fetchTikTokVersion, type TikTokVersionContext, type TikTokVersionOptions } from './autopilot/publish-review/api';

const PRIVACY_LABELS: Record<string, string> = {
  PUBLIC_TO_EVERYONE: 'Alle',
  MUTUAL_FOLLOW_FRIENDS: 'Freunde',
  FOLLOWER_OF_CREATOR: 'Follower',
  SELF_ONLY: 'Nur ich',
};

export function TikTokPostDialog({ clipDbId, pending, error, onConfirm, onClose }: {
  clipDbId: number;
  pending: boolean;
  error: unknown;
  onConfirm: (options: TikTokPostOptions) => void;
  onClose: () => void;
}) {
  const t = useT();
  const id = useId();
  const videoRef = useRef<HTMLVideoElement>(null);
  const queryClient = useQueryClient();
  const previewKey = ['social-media', 'preview', clipDbId];
  const requested = useRef<number | null>(null);
  const session = useRef(0);
  const inFlight = useRef<number | null>(null);
  useEffect(() => {
    session.current += 1;
    requested.current = null;
    inFlight.current = null;
    return () => { session.current += 1; };
  }, [clipDbId]);
  const preview = useQuery({
    queryKey: previewKey,
    queryFn: () => getPreviewStatus(clipDbId),
    staleTime: 0,
    gcTime: 0,
    refetchOnMount: 'always',
    refetchOnWindowFocus: false,
    retry: false,
    refetchInterval: (query) => !query.state.error
      && ['pending', 'rendering'].includes(query.state.data?.status ?? '') ? 3000 : false,
  });
  const render = useMutation({
    mutationFn: ({ clip }: { clip: number; session: number }) => requestPreview(clip),
    onSuccess: (response, request) => {
      if (request.session !== session.current) return;
      const key = ['social-media', 'preview', request.clip];
      queryClient.setQueryData(key, {
        clip_db_id: request.clip,
        status: response.status,
        ready: response.status === 'ready',
      });
      void queryClient.invalidateQueries({ queryKey: key });
    },
    onSettled: (_response, _error, request) => {
      if (inFlight.current === request.session) inFlight.current = null;
    },
  });
  const previewReady = preview.data?.status === 'ready' && !preview.isError && !preview.isFetching && !render.isPending;
  const context = useQuery({
    queryKey: ['social-media', 'tiktok-creator', clipDbId],
    queryFn: () => fetchTikTokCreatorInfo(clipDbId),
    enabled: previewReady,
    staleTime: 0,
    gcTime: 0,
    refetchOnMount: 'always',
    refetchOnWindowFocus: false,
    retry: false,
  });
  const [caption, setCaption] = useState('');
  const [standardDescription, setStandardDescription] = useState('');
  const [privacy, setPrivacy] = useState('');
  const [comment, setComment] = useState(false);
  const [duet, setDuet] = useState(false);
  const [stitch, setStitch] = useState(false);
  const [commercial, setCommercial] = useState(false);
  const [ownBrand, setOwnBrand] = useState(false);
  const [partnerBrand, setPartnerBrand] = useState(false);
  const [consent, setConsent] = useState(false);
  const [saveMessage, setSaveMessage] = useState('');
  const editor = useQuery({
    queryKey: ['social-media', 'tiktok-editor', clipDbId],
    queryFn: () => fetchTikTokEditor(clipDbId),
    staleTime: 0, gcTime: 0, refetchOnMount: 'always', refetchOnWindowFocus: false, retry: false,
  });
  const save = useMutation({
    mutationFn: (request: { clip: number; kind: 'draft' | 'defaults'; choices: TikTokDraft }) =>
      saveTikTokChoices(request.clip, request.kind, request.choices),
    onSuccess: async (response, request) => {
      const key = ['social-media', 'tiktok-editor', request.clip];
      await queryClient.cancelQueries({ queryKey: key });
      queryClient.setQueryData<TikTokEditorState>(key, (previous) => ({
        music_consent: previous?.music_consent ?? false,
        music_consent_platform_user_id: previous?.music_consent_platform_user_id ?? null,
        draft: previous?.draft ?? null,
        defaults: previous?.defaults ?? null,
        [request.kind]: response.choices,
      }));
      if (request.clip !== clipDbId) return;
      setSaveMessage(request.kind === 'draft' ? 'Gespeichert. Der Clip wurde nicht eingeplant.' : 'Deine Standardwerte sind gespeichert.');
      void queryClient.invalidateQueries({ queryKey: ['social-media', 'tiktok-editor', request.clip] });
    },
  });
  const musicConsent = useMutation({
    mutationFn: (request: { clip: number; account: string }) => saveTikTokMusicConsent(request.clip, request.account),
    onSuccess: async (_response, request) => {
      const key = ['social-media', 'tiktok-editor', request.clip];
      await queryClient.cancelQueries({ queryKey: key });
      queryClient.setQueryData<TikTokEditorState>(key, (previous) => previous ? {
        ...previous, music_consent: true, music_consent_platform_user_id: request.account,
      } : previous);
      void queryClient.invalidateQueries({ queryKey: key });
    },
  });
  const busy = pending || save.isPending || musicConsent.isPending;
  const combinedCaption = [caption.trim(), standardDescription.trim()].filter(Boolean).join('\n\n');
  const captionLength = Array.from(combinedCaption).length;
  const choices: TikTokDraft = {
    caption, standard_description: standardDescription, privacy_level: privacy, allow_comment: comment, allow_duet: duet, allow_stitch: stitch,
    commercial_content: commercial, brand_organic_toggle: ownBrand, brand_content_toggle: partnerBrand,
  };
  const saveChoices = (kind: 'draft' | 'defaults') => {
    setSaveMessage('');
    const payload = kind === 'draft' ? choices : {
      ...choices, caption: '', commercial_content: false, brand_organic_toggle: false, brand_content_toggle: false,
    };
    save.mutate({ clip: clipDbId, kind, choices: payload });
  };
  const mutateRender = render.mutate;
  const requestRender = useCallback(() => {
    if (inFlight.current !== null || pending) return;
    inFlight.current = session.current;
    requested.current = clipDbId;
    setConsent(false);
    mutateRender({ clip: clipDbId, session: session.current });
  }, [clipDbId, pending, mutateRender]);
  useEffect(() => {
    if (!preview.isSuccess || preview.data.status !== null || requested.current === clipDbId) return;
    requestRender();
  }, [clipDbId, preview.isSuccess, preview.data?.status, requestRender]);
  const initializedClip = useRef('');
  const approvedContext = useRef('');
  useEffect(() => { setConsent(false); }, [clipDbId, context.data?.platform_user_id]);
  useEffect(() => {
    const data = context.data;
    if (!data || !editor.data || editor.isFetching || editor.isError) return;
    const clipAccount = JSON.stringify([clipDbId, data.platform_user_id]);
    if (initializedClip.current !== clipAccount) {
      initializedClip.current = clipAccount;
      const draft = editor.data.draft;
      const values = draft ?? editor.data.defaults;
      setCaption(draft?.caption ?? data.caption);
      setStandardDescription(values?.standard_description ?? '');
      setPrivacy(values && data.creator.privacy_level_options.includes(values.privacy_level) ? values.privacy_level : '');
      setComment(!!values?.allow_comment && !data.creator.comment_disabled);
      setDuet(!!values?.allow_duet && !data.creator.duet_disabled);
      setStitch(!!values?.allow_stitch && !data.creator.stitch_disabled);
      setCommercial(!!draft?.commercial_content);
      setOwnBrand(!!draft?.commercial_content && draft.brand_organic_toggle);
      setPartnerBrand(!!draft?.commercial_content && draft.brand_content_toggle);
      setConsent(false);
      setSaveMessage('');
    }
    const identity = JSON.stringify([
      clipDbId, data.credential_id, data.platform_user_id, data.approved_video_sha256, data.creator,
    ]);
    if (approvedContext.current !== identity) {
      approvedContext.current = identity;
      setConsent(false);
      setPrivacy((value) => data.creator.privacy_level_options.includes(value) ? value : '');
      if (data.creator.comment_disabled) setComment(false);
      if (data.creator.duet_disabled) setDuet(false);
      if (data.creator.stitch_disabled) setStitch(false);
    }
  }, [clipDbId, context.data, editor.data, editor.isFetching, editor.isError]);
  const data = context.data;
  const issue = error ?? render.error ?? preview.error ?? context.error;
  const storedMusicConsent = !!data?.platform_user_id && !!editor.data?.music_consent
    && editor.data.music_consent_platform_user_id === data.platform_user_id;
  const postConsent = storedMusicConsent && (!partnerBrand || consent);
  const valid = data && previewReady && !context.isError && !context.isFetching
    && !editor.isError && !editor.isFetching && privacy && captionLength <= 2200 && postConsent
    && (!commercial || ownBrand || partnerBrand)
    && !(partnerBrand && privacy === 'SELF_ONLY');
  const change = (action: () => void) => { action(); setConsent(false); setSaveMessage(''); save.reset(); };
  const consentNotice = <>{t('Mit der Veröffentlichung stimmst du TikToks')} <a className="text-accent underline" href="https://www.tiktok.com/legal/page/global/music-usage-confirmation/en" target="_blank" rel="noreferrer">{t('Bestätigung zur Musiknutzung')}</a> {t('zu.')}</>;
  return (
    <WorkspaceDialog eyebrow="" title={t('TikTok-Veröffentlichung')} onClose={onClose} busy={busy}>
      <form className="space-y-5" onSubmit={(event) => {
        event.preventDefault();
        if (!valid || busy || !data) return;
        onConfirm({
          caption: combinedCaption, privacy_level: privacy,
          allow_comment: comment, allow_duet: duet, allow_stitch: stitch,
          commercial_content: commercial,
          brand_organic_toggle: ownBrand, brand_content_toggle: partnerBrand,
          consent: postConsent, creator_username: data.creator.creator_username,
          credential_id: data.credential_id, platform_user_id: data.platform_user_id,
          approved_video_sha256: data.approved_video_sha256,
        });
      }}>
        {!preview.data && preview.isFetching && <p role="status">{t('Die Videovorschau wird geprüft.')}</p>}
        {render.isPending && <p role="status">{t('Die Videovorschau wird angefordert.')}</p>}
        {preview.data?.status === null && !render.isPending && !render.isError && <div role="status" className="space-y-2">
          <p>{t('Die Videovorschau ist nicht mehr verfügbar. Bitte erstelle sie erneut.')}</p>
          <button type="button" className="studio-button" disabled={pending || preview.isFetching} onClick={requestRender}>{t('Vorschau erneut erstellen')}</button>
        </div>}
        {preview.data?.status === 'pending' && !render.isPending && <p role="status">{t('Die Videovorschau ist angefordert und wird gleich erstellt.')}</p>}
        {preview.data?.status === 'rendering' && <p role="status">{t('Die Videovorschau wird erstellt. Du kannst dieses Fenster schließen und später wieder öffnen.')}</p>}
        {preview.data?.status === 'error' && !render.isPending && <div role="alert" className="space-y-2 text-danger">
          <p>{t('Die Videovorschau konnte nicht erstellt werden. Bitte versuche es erneut.')}</p>
          <button type="button" className="studio-button" disabled={pending || render.isPending} onClick={() => requestRender()}>{t('Vorschau erneut erstellen')}</button>
        </div>}
        {previewReady && context.isFetching && <p role="status">{t('Aktuelle TikTok-Einstellungen werden geladen.')}</p>}
        {issue && <div role="alert" className="space-y-2 text-danger">
          <p>{issue instanceof SocialMediaApiError && issue.code === 'tiktok_posting' ? issue.message : t('Die TikTok-Freigabe konnte nicht geladen werden. Bitte versuche es erneut.')}</p>
          {render.isError && <button type="button" className="studio-button" disabled={pending || render.isPending} onClick={() => requestRender()}>{t('Vorschau erneut anfordern')}</button>}
          {preview.isError && <button type="button" className="studio-button" disabled={pending || preview.isFetching} onClick={() => preview.refetch()}>{t('Vorschau erneut prüfen')}</button>}
          {context.isError && previewReady && <button type="button" className="studio-button" disabled={pending || context.isFetching} onClick={() => context.refetch()}>{t('Erneut laden')}</button>}
        </div>}
        {editor.isFetching && <p role="status" className="text-sm">{t('Gespeicherte Einstellungen werden geladen.')}</p>}
        {editor.isError && <div role="alert" className="space-y-2 text-danger"><p>{t('Deine gespeicherten Einstellungen konnten nicht geladen werden.')}</p><button type="button" className="studio-button" disabled={busy || editor.isFetching} onClick={() => editor.refetch()}>{t('Erneut laden')}</button></div>}
        {data && previewReady && !context.isError && !context.isFetching && editor.data && !editor.isError && !editor.isFetching && !storedMusicConsent && <section className="space-y-4">
          <h3 className="font-medium">{t('Einmalig: Musiknutzung bestätigen')}</h3>
          <p>{t('Damit wir deine Clips auf TikTok veröffentlichen dürfen, musst du einmal TikToks Bestätigung zur Musiknutzung zustimmen. Wir speichern deine Zustimmung dauerhaft für dieses TikTok-Konto.')}</p>
          <a className="text-accent underline" href="https://www.tiktok.com/legal/page/global/music-usage-confirmation/en" target="_blank" rel="noreferrer">{t('Bestätigung zur Musiknutzung')}</a>
          {musicConsent.isError && <p role="alert" className="text-danger">{musicConsent.error instanceof SocialMediaApiError ? musicConsent.error.message : t('Speichern hat nicht geklappt. Versuch es nochmal.')}</p>}
          <div className="flex justify-end"><button type="button" className="studio-primary" disabled={busy || !data.platform_user_id} onClick={() => musicConsent.mutate({ clip: clipDbId, account: data.platform_user_id })}>{musicConsent.isPending ? t('Wird gespeichert…') : t('Zustimmen und speichern')}</button></div>
        </section>}
        {data && previewReady && !context.isError && !context.isFetching && storedMusicConsent && editor.data && !editor.isFetching && !editor.isError && <>
          <p className="text-sm">{t('Veröffentlichung auf')} <strong>{data.creator.creator_nickname}</strong> (@{data.creator.creator_username})</p>
          {save.isError && <p role="alert" className="text-danger">{t('Speichern hat nicht geklappt. Versuch es nochmal.')}</p>}
          {saveMessage && <p role="status" className="text-sm">{t(saveMessage)}</p>}
          <div className="grid gap-5 md:grid-cols-[minmax(0,180px)_minmax(0,1fr)]">
            <div>
              <video ref={videoRef} className="max-h-72 w-full rounded-lg bg-black" src={`${previewFileUrl(clipDbId)}?v=${encodeURIComponent(data.approved_video_sha256)}`} controls preload="metadata" aria-label={t('TikTok-Videovorschau')} />
              <VideoFullscreenButton videoRef={videoRef} className="mt-2" />
              <p className="mt-2 text-xs text-text-secondary">{t('{duration} Sekunden, für dieses Konto höchstens {max} Sekunden.', { duration: Math.ceil(data.duration_seconds), max: data.creator.max_video_post_duration_sec })}</p>
            </div>
            <div className="space-y-4">
              <label className="block space-y-2" htmlFor={`${id}-caption`}>
                <span className="font-medium">{t('Beschreibung des Clips')}</span>
                <textarea id={`${id}-caption`} className="studio-input min-h-32 w-full" value={caption} disabled={busy} onChange={(event) => change(() => setCaption(event.target.value))} />
                <span className="text-xs text-text-secondary">{captionLength}/2200</span>
              </label>
              <label className="block space-y-2" htmlFor={`${id}-standard-description`}>
                <span className="font-medium">{t('Standard-Beschreibung')}</span>
                <textarea id={`${id}-standard-description`} className="studio-input min-h-24 w-full" value={standardDescription} disabled={busy} onChange={(event) => change(() => setStandardDescription(event.target.value))} />
                <span className="text-xs text-text-secondary">{t('Wird unter jede Clip-Beschreibung gesetzt.')}</span>
              </label>
              <button type="button" className="studio-button" disabled={busy || captionLength > 2200} onClick={() => saveChoices('defaults')}>{t('Als Standard speichern')}</button>
              <label className="block space-y-2" htmlFor={`${id}-privacy`}>
                <span className="font-medium">{t('Wer darf den Clip sehen?')}</span>
                <select id={`${id}-privacy`} className="studio-input w-full" value={privacy} disabled={busy} required onChange={(event) => change(() => setPrivacy(event.target.value))}>
                  <option value="" disabled>{t('Sichtbarkeit wählen')}</option>
                  {data.creator.privacy_level_options.map((option) => <option key={option} value={option} disabled={partnerBrand && option === 'SELF_ONLY'}>{t(PRIVACY_LABELS[option] ?? option)}</option>)}
                </select>
              </label>
              <fieldset className="space-y-2" disabled={busy}>
                <legend className="mb-2 font-medium">{t('Interaktionen erlauben')}</legend>
                {([
                  ['comment', 'Kommentare', comment, setComment, data.creator.comment_disabled],
                  ['duet', 'Duett', duet, setDuet, data.creator.duet_disabled],
                  ['stitch', 'Stitch', stitch, setStitch, data.creator.stitch_disabled],
                ] as const).map(([key, label, checked, setter, disabled]) => <label key={key} className={`flex items-center gap-2 ${disabled ? 'text-text-secondary opacity-60' : ''}`}>
                  <input type="checkbox" checked={checked} disabled={disabled || busy} onChange={(event) => change(() => setter(event.target.checked))} />
                  {t(label)}{disabled && <span className="text-xs">{t('Bei TikTok ausgeschaltet')}</span>}
                </label>)}
              </fieldset>
            </div>
          </div>
          <fieldset className="space-y-3 border-t border-border pt-4" disabled={busy}>
            <legend className="sr-only">{t('Werbung kennzeichnen')}</legend>
            <label className="flex items-center gap-2 font-medium">
              <input type="checkbox" checked={commercial} onChange={(event) => change(() => {
                setCommercial(event.target.checked);
                if (!event.target.checked) { setOwnBrand(false); setPartnerBrand(false); }
              })} />
              {t('Dieser Clip enthält Werbung')}
            </label>
            <p className="text-sm text-text-secondary">{t('Kennzeichne Inhalte, die deine eigene Marke oder Produkte und Leistungen anderer Unternehmen bewerben.')}</p>
            {commercial && <div className="space-y-3 pl-6">
              <label className="flex items-center gap-2"><input type="checkbox" checked={ownBrand} onChange={(event) => change(() => setOwnBrand(event.target.checked))} />{t('Eigene Marke')}</label>
              <p className="text-xs text-text-secondary">{t('Du bewirbst dich selbst oder dein eigenes Unternehmen. TikTok kennzeichnet den Clip als „Werbeinhalt“.')}</p>
              <label className="flex items-center gap-2"><input type="checkbox" checked={partnerBrand} onChange={(event) => change(() => setPartnerBrand(event.target.checked))} />{t('Markenpartner')}</label>
              <p className="text-xs text-text-secondary">{t('Du bewirbst eine andere Marke oder ein anderes Unternehmen. TikTok kennzeichnet den Clip als „Bezahlte Partnerschaft“.')}</p>
              {ownBrand && partnerBrand && <p className="text-xs">{t('TikTok kennzeichnet den Clip als „Bezahlte Partnerschaft“.')}</p>}
              {!ownBrand && !partnerBrand && <p role="status" className="text-warning">{t('Wähle mindestens eine Art von Werbung aus.')}</p>}
              {partnerBrand && <p role="status" className="text-sm text-warning">{t('Inhalte für Markenpartner dürfen bei TikTok nicht auf „Nur ich“ stehen.')}</p>}
            </div>}
          </fieldset>
          <div className="space-y-3 border-t border-border pt-4">
            <p className="text-sm">{consentNotice}</p>
            {partnerBrand && <label className="flex items-start gap-2 text-sm">
              <input type="checkbox" className="mt-1" checked={consent} disabled={busy} onChange={(event) => setConsent(event.target.checked)} />
              <span>{t('Für diesen Clip stimme ich TikToks')} <a className="text-accent underline" href="https://www.tiktok.com/legal/page/global/bc-policy/en" target="_blank" rel="noreferrer">{t('Richtlinie für Markeninhalte')}</a> {t('zu.')}</span>
            </label>}
            <p className="text-sm text-text-secondary">{t('Der Clip wird zum geplanten Termin direkt veröffentlicht. TikTok kann danach einige Minuten für die Verarbeitung benötigen. Den Stand siehst du an der Clipkarte.')}</p>
            <div className="flex flex-wrap justify-end gap-2">
              <button type="button" className="studio-button" disabled={busy || captionLength > 2200} onClick={() => saveChoices('draft')}>{save.isPending && save.variables?.kind === 'draft' ? t('Wird gespeichert…') : t('Speichern')}</button>
              <button type="button" className="studio-button" disabled={busy} onClick={onClose}>{t('Abbrechen')}</button>
              <button type="submit" className="studio-primary" disabled={!valid || busy}>{pending ? t('Wird eingeplant…') : t('Clip mit TikTok einplanen')}</button>
            </div>
          </div>
        </>}
      </form>
    </WorkspaceDialog>
  );
}

type VersionChoices = Pick<TikTokVersionOptions, 'privacy_level' | 'allow_comment' | 'allow_duet' | 'allow_stitch' | 'commercial_content' | 'brand_organic_toggle' | 'brand_content_toggle'>;

function initialChoices(data: TikTokVersionContext): VersionChoices {
  const bound = data.bound_options;
  const saved = data.draft ?? data.defaults;
  const commercial = bound ? bound.commercial_content : !!data.draft?.commercial_content;
  const source = bound ?? saved;
  const privacy = source?.privacy_level ?? '';
  return {
    privacy_level: data.creator.privacy_level_options.includes(privacy) ? privacy : '',
    allow_comment: !!source?.allow_comment && !data.creator.comment_disabled,
    allow_duet: !!source?.allow_duet && !data.creator.duet_disabled,
    allow_stitch: !!source?.allow_stitch && !data.creator.stitch_disabled,
    commercial_content: commercial,
    brand_organic_toggle: commercial && !!(bound ?? data.draft)?.brand_organic_toggle,
    brand_content_toggle: commercial && !!(bound ?? data.draft)?.brand_content_toggle,
  };
}

function versionOptions(data: TikTokVersionContext, choices: VersionChoices, partnerConsent: boolean): TikTokVersionOptions | null {
  const { privacy_level, commercial_content, brand_organic_toggle, brand_content_toggle } = choices;
  if (!data.music_consent || !privacy_level || !data.caption.trim() || Array.from(data.caption).length > 2200) return null;
  if (commercial_content && !brand_organic_toggle && !brand_content_toggle) return null;
  if (brand_content_toggle && (privacy_level === 'SELF_ONLY' || !partnerConsent)) return null;
  return {
    caption: data.caption,
    ...choices,
    consent: true,
    creator_username: data.creator.creator_username,
    credential_id: data.credential_id,
    platform_user_id: data.platform_user_id,
    approved_video_sha256: data.approved_video_sha256,
    video_path: '',
  };
}

export function TikTokVersionForm({ clipDbId, videoId, revision, disabled, onChange }: {
  clipDbId: number;
  videoId: number;
  revision: number;
  disabled: boolean;
  onChange: (options: TikTokVersionOptions | null) => void;
}) {
  const t = useT();
  const id = useId();
  const queryClient = useQueryClient();
  const key = ['social-publish', 'tiktok-version', clipDbId, videoId, revision];
  const context = useQuery({
    queryKey: key,
    queryFn: ({ signal }) => fetchTikTokVersion(clipDbId, videoId, signal),
    staleTime: 0, gcTime: 0, refetchOnWindowFocus: false, retry: false,
  });
  const [choices, setChoices] = useState<VersionChoices | null>(null);
  const [partnerConsent, setPartnerConsent] = useState(false);
  const [saveMessage, setSaveMessage] = useState('');
  const data = context.data;
  useEffect(() => {
    if (!data) return;
    setChoices(initialChoices(data));
    setPartnerConsent(false);
  }, [data]);
  const musicConsent = useMutation({
    mutationFn: (account: string) => saveTikTokMusicConsent(clipDbId, account),
    onSuccess: () => { void queryClient.invalidateQueries({ queryKey: key }); },
  });
  const saveDefaults = useMutation({
    mutationFn: (values: VersionChoices) => saveTikTokChoices(clipDbId, 'defaults', {
      ...values, caption: '', standard_description: data?.defaults?.standard_description ?? '', commercial_content: false, brand_organic_toggle: false, brand_content_toggle: false,
    }),
    onSuccess: () => setSaveMessage('Deine Standardwerte sind gespeichert.'),
  });
  const options = data && choices ? versionOptions(data, choices, partnerConsent) : null;
  const serialized = JSON.stringify(options);
  useEffect(() => {
    onChange(serialized === 'null' ? null : JSON.parse(serialized) as TikTokVersionOptions);
  }, [serialized, onChange]);
  useEffect(() => () => onChange(null), [onChange]);
  const busy = disabled || musicConsent.isPending || saveDefaults.isPending;
  const change = (next: Partial<VersionChoices>) => {
    setChoices((current) => current ? { ...current, ...next } : current);
    setPartnerConsent(false);
    setSaveMessage('');
  };
  if (context.isPending) return <p role="status">{t('Aktuelle TikTok-Einstellungen werden geladen.')}</p>;
  if (context.isError || !data) return <div role="alert" className="space-y-2 text-danger">
    <p>{context.error instanceof SocialMediaApiError ? context.error.message : t('Die TikTok-Freigabe konnte nicht geladen werden. Bitte versuche es erneut.')}</p>
    <button type="button" className="studio-button" disabled={context.isFetching} onClick={() => void context.refetch()}>{t('Erneut laden')}</button>
  </div>;
  if (!data.music_consent) return <section className="space-y-3" aria-label={t('Einmalig: Musiknutzung bestätigen')}>
    <h4 className="font-medium">{t('Einmalig: Musiknutzung bestätigen')}</h4>
    <p>{t('Damit wir deine Clips auf TikTok veröffentlichen dürfen, musst du einmal TikToks Bestätigung zur Musiknutzung zustimmen. Wir speichern deine Zustimmung dauerhaft für dieses TikTok-Konto.')}</p>
    <a className="text-accent underline" href="https://www.tiktok.com/legal/page/global/music-usage-confirmation/en" target="_blank" rel="noreferrer">{t('Bestätigung zur Musiknutzung')}</a>
    {musicConsent.isError && <p role="alert" className="text-danger">{musicConsent.error instanceof SocialMediaApiError ? musicConsent.error.message : t('Speichern hat nicht geklappt. Versuch es nochmal.')}</p>}
    <button type="button" className="studio-primary" disabled={busy || !data.platform_user_id} onClick={() => musicConsent.mutate(data.platform_user_id)}>{musicConsent.isPending ? t('Wird gespeichert…') : t('Zustimmen und speichern')}</button>
  </section>;
  if (!choices) return null;
  const captionLength = Array.from(data.caption).length;
  return <fieldset className="space-y-4 tiktok-version-form" disabled={busy}>
    <legend className="sr-only">{t('TikTok-Veröffentlichung')}</legend>
    <p className="text-sm">{t('Veröffentlichung auf')} <strong>{data.creator.creator_nickname}</strong> (@{data.creator.creator_username})</p>
    <div className="space-y-1">
      <p className="font-medium">{t('Beschreibung des Clips')}</p>
      <p className="whitespace-pre-wrap break-words text-sm">{data.caption || t('Noch kein Text für TikTok. Ergänze ihn oben bei den Texten.')}</p>
      <p className="text-xs text-text-secondary">{captionLength}/2200 · {t('Der Text kommt aus den Texten dieser Fassung.')}</p>
    </div>
    <label className="block space-y-2" htmlFor={`${id}-privacy`}>
      <span className="font-medium">{t('Wer darf den Clip sehen?')}</span>
      <select id={`${id}-privacy`} className="studio-input w-full" value={choices.privacy_level} required onChange={(event) => change({ privacy_level: event.target.value })}>
        <option value="" disabled>{t('Sichtbarkeit wählen')}</option>
        {data.creator.privacy_level_options.map((option) => <option key={option} value={option} disabled={choices.brand_content_toggle && option === 'SELF_ONLY'}>{t(PRIVACY_LABELS[option] ?? option)}</option>)}
      </select>
    </label>
    <div className="space-y-2" role="group" aria-label={t('Interaktionen erlauben')}>
      <p className="font-medium">{t('Interaktionen erlauben')}</p>
      {([
        ['allow_comment', 'Kommentare', data.creator.comment_disabled],
        ['allow_duet', 'Duett', data.creator.duet_disabled],
        ['allow_stitch', 'Stitch', data.creator.stitch_disabled],
      ] as const).map(([field, label, off]) => <label key={field} className={`flex items-center gap-2 ${off ? 'text-text-secondary opacity-60' : ''}`}>
        <input type="checkbox" checked={choices[field]} disabled={off} onChange={(event) => change({ [field]: event.target.checked })} />
        {t(label)}{off && <span className="text-xs">{t('Bei TikTok ausgeschaltet')}</span>}
      </label>)}
    </div>
    <div className="space-y-2">
      <label className="flex items-center gap-2 font-medium">
        <input type="checkbox" checked={choices.commercial_content} onChange={(event) => change({ commercial_content: event.target.checked, ...(event.target.checked ? {} : { brand_organic_toggle: false, brand_content_toggle: false }) })} />
        {t('Dieser Clip enthält Werbung')}
      </label>
      {choices.commercial_content && <div className="space-y-2 pl-6">
        <label className="flex items-center gap-2"><input type="checkbox" checked={choices.brand_organic_toggle} onChange={(event) => change({ brand_organic_toggle: event.target.checked })} />{t('Eigene Marke')}</label>
        <label className="flex items-center gap-2"><input type="checkbox" checked={choices.brand_content_toggle} onChange={(event) => change({ brand_content_toggle: event.target.checked })} />{t('Markenpartner')}</label>
        {!choices.brand_organic_toggle && !choices.brand_content_toggle && <p role="status" className="text-warning">{t('Wähle mindestens eine Art von Werbung aus.')}</p>}
        {choices.brand_content_toggle && <p role="status" className="text-sm text-warning">{t('Inhalte für Markenpartner dürfen bei TikTok nicht auf „Nur ich“ stehen.')}</p>}
      </div>}
    </div>
    <p className="text-sm">{t('Mit der Veröffentlichung stimmst du TikToks')} <a className="text-accent underline" href="https://www.tiktok.com/legal/page/global/music-usage-confirmation/en" target="_blank" rel="noreferrer">{t('Bestätigung zur Musiknutzung')}</a> {t('zu.')}</p>
    {choices.brand_content_toggle && <label className="flex items-start gap-2 text-sm">
      <input type="checkbox" className="mt-1" checked={partnerConsent} onChange={(event) => setPartnerConsent(event.target.checked)} />
      <span>{t('Für diesen Clip stimme ich TikToks')} <a className="text-accent underline" href="https://www.tiktok.com/legal/page/global/bc-policy/en" target="_blank" rel="noreferrer">{t('Richtlinie für Markeninhalte')}</a> {t('zu.')}</span>
    </label>}
    <div className="flex flex-wrap items-center gap-2">
      <button type="button" className="studio-button" disabled={busy || !choices.privacy_level} onClick={() => { setSaveMessage(''); saveDefaults.mutate(choices); }}>{t('Als Standard speichern')}</button>
      {saveMessage && <span role="status" className="text-sm">{t(saveMessage)}</span>}
      {saveDefaults.isError && <span role="alert" className="text-sm text-danger">{t('Speichern hat nicht geklappt. Versuch es nochmal.')}</span>}
    </div>
  </fieldset>;
}
