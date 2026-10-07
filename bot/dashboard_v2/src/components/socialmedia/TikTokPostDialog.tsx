import { useEffect, useId, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { fetchTikTokCreatorInfo, previewFileUrl, SocialMediaApiError, type TikTokPostOptions } from '@/api/socialMedia';
import { WorkspaceDialog } from './WorkspaceDialog';
import { useT } from '@/context/LanguageContext';

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
  const context = useQuery({
    queryKey: ['social-media', 'tiktok-creator', clipDbId],
    queryFn: () => fetchTikTokCreatorInfo(clipDbId),
    staleTime: 0,
    gcTime: 0,
    refetchOnMount: 'always',
    refetchOnWindowFocus: false,
    retry: false,
  });
  const [caption, setCaption] = useState('');
  const [privacy, setPrivacy] = useState('');
  const [comment, setComment] = useState(false);
  const [duet, setDuet] = useState(false);
  const [stitch, setStitch] = useState(false);
  const [commercial, setCommercial] = useState(false);
  const [ownBrand, setOwnBrand] = useState(false);
  const [partnerBrand, setPartnerBrand] = useState(false);
  const [consent, setConsent] = useState(false);
  useEffect(() => {
    if (!context.data) return;
    setCaption(context.data.caption);
    setPrivacy('');
    setComment(false);
    setDuet(false);
    setStitch(false);
    setCommercial(false);
    setOwnBrand(false);
    setPartnerBrand(false);
    setConsent(false);
  }, [context.data]);
  const data = context.data;
  const issue = error ?? context.error;
  const valid = data && !context.isFetching && privacy && caption.length <= 2200 && consent
    && (!commercial || ownBrand || partnerBrand)
    && !(partnerBrand && privacy === 'SELF_ONLY');
  const change = (action: () => void) => { action(); setConsent(false); };
  return (
    <WorkspaceDialog eyebrow="" title={t('TikTok-Veröffentlichung')} onClose={onClose} busy={pending}>
      <form className="space-y-5" onSubmit={(event) => {
        event.preventDefault();
        if (!valid || pending || !data) return;
        onConfirm({
          caption, privacy_level: privacy,
          allow_comment: comment, allow_duet: duet, allow_stitch: stitch,
          commercial_content: commercial,
          brand_organic_toggle: ownBrand, brand_content_toggle: partnerBrand,
          consent, creator_username: data.creator.creator_username,
          credential_id: data.credential_id, platform_user_id: data.platform_user_id,
          approved_video_sha256: data.approved_video_sha256,
        });
      }}>
        {context.isFetching && <p role="status">{t('Aktuelle TikTok-Einstellungen werden geladen.')}</p>}
        {issue && <div role="alert" className="space-y-2 text-danger">
          <p>{issue instanceof SocialMediaApiError ? issue.message : t('Die TikTok-Freigabe konnte nicht geladen werden. Bitte versuche es erneut.')}</p>
          {context.isError && <button type="button" className="studio-button" onClick={() => context.refetch()}>{t('Erneut laden')}</button>}
        </div>}
        {data && !context.isFetching && <>
          <p className="text-sm">{t('Veröffentlichung auf')} <strong>{data.creator.creator_nickname}</strong> (@{data.creator.creator_username})</p>
          <div className="grid gap-5 md:grid-cols-[minmax(0,180px)_minmax(0,1fr)]">
            <div>
              <video className="max-h-72 w-full rounded-lg bg-black" src={previewFileUrl(clipDbId)} controls preload="metadata" aria-label={t('TikTok-Videovorschau')} />
              <p className="mt-2 text-xs text-text-secondary">{t('{duration} Sekunden, für dieses Konto höchstens {max} Sekunden.', { duration: Math.ceil(data.duration_seconds), max: data.creator.max_video_post_duration_sec })}</p>
            </div>
            <div className="space-y-4">
              <label className="block space-y-2" htmlFor={`${id}-caption`}>
                <span className="font-medium">{t('Beschreibung für TikTok')}</span>
                <textarea id={`${id}-caption`} className="studio-input min-h-32 w-full" value={caption} maxLength={2200} disabled={pending} onChange={(event) => change(() => setCaption(event.target.value))} />
                <span className="text-xs text-text-secondary">{caption.length}/2200</span>
              </label>
              <label className="block space-y-2" htmlFor={`${id}-privacy`}>
                <span className="font-medium">{t('Wer darf den Clip sehen?')}</span>
                <select id={`${id}-privacy`} className="studio-input w-full" value={privacy} disabled={pending} required onChange={(event) => change(() => setPrivacy(event.target.value))}>
                  <option value="" disabled>{t('Sichtbarkeit wählen')}</option>
                  {data.creator.privacy_level_options.map((option) => <option key={option} value={option} disabled={partnerBrand && option === 'SELF_ONLY'}>{t(PRIVACY_LABELS[option] ?? option)}</option>)}
                </select>
              </label>
              <fieldset className="space-y-2" disabled={pending}>
                <legend className="mb-2 font-medium">{t('Interaktionen erlauben')}</legend>
                {([
                  ['comment', 'Kommentare', comment, setComment, data.creator.comment_disabled],
                  ['duet', 'Duett', duet, setDuet, data.creator.duet_disabled],
                  ['stitch', 'Stitch', stitch, setStitch, data.creator.stitch_disabled],
                ] as const).map(([key, label, checked, setter, disabled]) => <label key={key} className={`flex items-center gap-2 ${disabled ? 'text-text-secondary opacity-60' : ''}`}>
                  <input type="checkbox" checked={checked} disabled={disabled || pending} onChange={(event) => change(() => setter(event.target.checked))} />
                  {t(label)}{disabled && <span className="text-xs">{t('Bei TikTok ausgeschaltet')}</span>}
                </label>)}
              </fieldset>
            </div>
          </div>
          <fieldset className="space-y-3 border-t border-border pt-4" disabled={pending}>
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
            <label className="flex items-start gap-2 text-sm">
              <input type="checkbox" className="mt-1" checked={consent} disabled={pending} onChange={(event) => setConsent(event.target.checked)} />
              <span>{t('Mit der Veröffentlichung stimmst du TikToks')} {partnerBrand && <><a className="text-accent underline" href="https://www.tiktok.com/legal/page/global/bc-policy/en" target="_blank" rel="noreferrer">{t('Richtlinie für Markeninhalte')}</a> {t('und')} </>}<a className="text-accent underline" href="https://www.tiktok.com/legal/page/global/music-usage-confirmation/en" target="_blank" rel="noreferrer">{t('Bestätigung zur Musiknutzung')}</a> {t('zu.')}</span>
            </label>
            <p className="text-sm text-text-secondary">{t('Der Clip wird zum geplanten Termin direkt veröffentlicht. TikTok kann danach einige Minuten für die Verarbeitung benötigen. Den Stand siehst du an der Clipkarte.')}</p>
            <div className="flex flex-wrap justify-end gap-2">
              <button type="button" className="studio-button" disabled={pending} onClick={onClose}>{t('Abbrechen')}</button>
              <button type="submit" className="studio-primary" disabled={!valid || pending}>{pending ? t('Wird eingeplant…') : t('Clip mit TikTok einplanen')}</button>
            </div>
          </div>
        </>}
      </form>
    </WorkspaceDialog>
  );
}
