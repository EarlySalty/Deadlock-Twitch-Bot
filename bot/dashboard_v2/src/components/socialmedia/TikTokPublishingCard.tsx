import { useEffect, useId, useState } from 'react';
import { useLanguage } from '@/context/LanguageContext';
import { platformIconUrl } from './platformIcon';
import { failureText, planText } from './pipelinePlan';
import { loadTikTokPublishing, saveTikTokPublishing, type TikTokPrivacy, type TikTokPublishing } from './autopilot/schedule-queue/tiktokPublishingApi';

const PRIVACY: { id: TikTokPrivacy; title: string }[] = [
  { id: 'PUBLIC_TO_EVERYONE', title: 'Öffentlich' },
  { id: 'FOLLOWER_OF_CREATOR', title: 'Nur Follower' },
  { id: 'MUTUAL_FOLLOW_FRIENDS', title: 'Nur Freunde' },
  { id: 'SELF_ONLY', title: 'Nur ich' },
];

const INTERACTIONS: { key: 'allow_comment' | 'allow_duet' | 'allow_stitch'; title: string }[] = [
  { key: 'allow_comment', title: 'Kommentare erlauben' },
  { key: 'allow_duet', title: 'Duett erlauben' },
  { key: 'allow_stitch', title: 'Stitch erlauben' },
];

interface Draft extends Omit<TikTokPublishing, 'fixed_hashtags'> {
  hashtags: string;
}

function toDraft(settings: TikTokPublishing): Draft {
  const { fixed_hashtags, ...rest } = settings;
  return { ...rest, hashtags: fixed_hashtags.map(tag => `#${tag}`).join(' ') };
}

function fromDraft(draft: Draft): TikTokPublishing {
  const { hashtags, ...rest } = draft;
  return { ...rest, fixed_hashtags: hashtags.split(/[\s,]+/).map(tag => tag.replace(/^#+/, '')).filter(Boolean) };
}

export function TikTokPublishingCard({ twitchUserId }: { twitchUserId?: string }) {
  const { language } = useLanguage();
  const text = planText(language);
  const id = useId();
  const [base, setBase] = useState<Draft | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    loadTikTokPublishing(twitchUserId, controller.signal)
      .then(settings => { const next = toDraft(settings); setBase(next); setDraft(next); setError(''); })
      .catch(failure => { if (!controller.signal.aborted) setError(failureText(failure)); });
    return () => controller.abort();
  }, [twitchUserId]);

  const dirty = base !== null && draft !== null && JSON.stringify(base) !== JSON.stringify(draft);
  const edit = (change: Partial<Draft>) => { setDraft(current => current ? { ...current, ...change } : current); setNotice(''); };

  async function submit() {
    if (!draft || saving) return;
    setSaving(true);
    setError('');
    try {
      const next = toDraft(await saveTikTokPublishing(fromDraft(draft), twitchUserId));
      setBase(next);
      setDraft(next);
      setNotice('Gespeichert.');
    } catch (failure) {
      setError(failureText(failure));
    } finally {
      setSaving(false);
    }
  }

  return <section className="studio-card schedule-queue pp-plan" aria-labelledby={`${id}-title`}>
    <h2 id={`${id}-title`} className="sq-platform-label"><img src={platformIconUrl('tiktok')} width="18" height="18" alt="" aria-hidden="true" />{text('TikTok-Veröffentlichung')}</h2>
    <p className="sq-help">{text('Mit „Freigabe nötig“ landet der Clip als Entwurf in deiner TikTok-App. Bei „Vollautomatisch“ und „Veto bis Frist“ wird direkt gepostet, sobald Sichtbarkeit und Musiknutzung bestätigt sind. Bis dahin kommen auch dort nur Entwürfe an.')}</p>
    {!draft ? (error ? <p className="sq-error" role="alert">{text(error)}</p> : <p role="status">{text('Wird geladen.')}</p>) : <form onSubmit={event => { event.preventDefault(); void submit(); }}>
      <fieldset className="pp-group">
        <legend>{text('Wer darf das Video sehen?')}</legend>
        <div className="pp-choices pp-compact">
          {PRIVACY.map(option => <label className="pp-choice" key={option.id}>
            <input type="radio" name={`${id}-privacy`} value={option.id} checked={draft.privacy_level === option.id} onChange={() => edit({ privacy_level: option.id })} />
            <span><strong>{text(option.title)}</strong></span>
          </label>)}
        </div>
      </fieldset>
      <fieldset className="pp-group">
        <legend>{text('Interaktionen')}</legend>
        <div className="pp-choices pp-compact">
          {INTERACTIONS.map(option => <label className="pp-choice" key={option.key}>
            <input type="checkbox" checked={draft[option.key]} onChange={event => edit({ [option.key]: event.target.checked })} />
            <span><strong>{text(option.title)}</strong></span>
          </label>)}
        </div>
        <p className="sq-help">{text('Ist etwas in deinem TikTok-Konto gesperrt, bleibt es auch hier aus.')}</p>
      </fieldset>
      <fieldset className="pp-group">
        <legend>{text('Musiknutzung')}</legend>
        {draft.account_connected ? <label className="pp-choice">
          <input type="checkbox" checked={draft.music_consent} onChange={event => edit({ music_consent: event.target.checked })} />
          <span><strong>{text('Ich stimme TikToks Bestätigung zur Musiknutzung zu.')}</strong> <a href="https://www.tiktok.com/legal/page/global/music-usage-confirmation/en" target="_blank" rel="noreferrer">{text('Bestätigung lesen')}</a></span>
        </label> : <p className="sq-help">{text('Verbinde zuerst ein TikTok-Konto.')}</p>}
      </fieldset>
      <p className="sq-help" role="status">{text(draft.privacy_level && draft.music_consent ? 'Direktes Posten ist eingerichtet.' : 'Ohne Sichtbarkeit und Musiknutzung kommen alle Clips als Entwurf in deiner TikTok-App an.')}</p>
      <div className="pp-group pp-text">
        <label htmlFor={`${id}-tags`}>{text('Feste Hashtags')}<input id={`${id}-tags`} type="text" value={draft.hashtags} placeholder="#deadlock #deadlockdeutsch" onChange={event => edit({ hashtags: event.target.value })} /></label>
        <small className="sq-help">{text('Kommen bei jedem TikTok-Post nach den Hashtags der KI dazu. Höchstens 10, mit Leerzeichen trennen.')}</small>
        <label htmlFor={`${id}-footer`}>{text('Fester Zusatztext')}<textarea id={`${id}-footer`} rows={2} maxLength={500} value={draft.caption_footer} placeholder="Discord: discord.gg/…" onChange={event => edit({ caption_footer: event.target.value })} /></label>
        <small className="sq-help">{text('Steht unter jeder Beschreibung, zum Beispiel der Link zu deinem Discord.')}</small>
      </div>
      {error && <p className="sq-error" role="alert">{text(error)}</p>}
      <p className="sq-notice" role="status">{notice ? text(notice) : ''}</p>
      <div className="sq-actions">
        <button type="button" disabled={!dirty || saving} onClick={() => { setDraft(base); setError(''); }}>{text('Verwerfen')}</button>
        <button className="sq-primary" type="submit" disabled={!dirty || saving}>{text(saving ? 'Wird gespeichert' : 'Speichern')}</button>
      </div>
    </form>}
  </section>;
}
