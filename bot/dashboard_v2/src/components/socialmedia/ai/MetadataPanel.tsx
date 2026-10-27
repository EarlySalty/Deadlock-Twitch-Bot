import { useLanguage, useT } from '../../../context/LanguageContext';
import { editorErrorExplanation } from './failure';
import { useEffect, useId, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { AiCopyPlatform, AiPlatformCopy, AiVersionMetadata } from './api';
import { fetchAiVersionMetadata, saveAiVersionMetadata } from './api';

type Draft = Record<AiCopyPlatform, { title: string; description: string; hashtags: string }>;

const labels: Record<AiCopyPlatform, string> = { youtube: 'YouTube', tiktok: 'TikTok', instagram: 'Instagram' };
const encoder = new TextEncoder();

function trimWhitespace(value: string): string {
  return value.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '');
}

function parseHashtags(value: string): string[] {
  return value.split(/[\p{White_Space},]+/u).map((tag) => trimWhitespace(tag.replace(/^#+/, ''))).filter(Boolean);
}

function formatHashtags(tags: string[]): string {
  return tags.map((tag) => `#${tag}`).join(' ');
}

function joined(parts: string[]): string {
  return parts.filter(Boolean).join('\n\n');
}

function toDraft(metadata: AiVersionMetadata): Draft {
  const entry = (copy: AiPlatformCopy) => ({ title: copy.title, description: copy.description, hashtags: formatHashtags(copy.hashtags) });
  return { youtube: entry(metadata.platforms.youtube), tiktok: entry(metadata.platforms.tiktok), instagram: entry(metadata.platforms.instagram) };
}

function toCopy(draft: Draft): Record<AiCopyPlatform, AiPlatformCopy> {
  const entry = (platform: AiCopyPlatform) => ({ title: trimWhitespace(draft[platform].title), description: trimWhitespace(draft[platform].description), hashtags: parseHashtags(draft[platform].hashtags) });
  return { youtube: entry('youtube'), tiktok: entry('tiktok'), instagram: entry('instagram') };
}

function copyIssues(platform: AiCopyPlatform, copy: AiPlatformCopy): string[] {
  const issues: string[] = [];
  const tags = formatHashtags(copy.hashtags);
  if (copy.hashtags.length > 30) issues.push('Bitte verwende höchstens 30 Hashtags.');
  if (copy.hashtags.some((tag) => Array.from(tag).length > 100 || tag.includes('#'))) issues.push('Hashtags dürfen keine weiteren Rauten enthalten und höchstens 100 Zeichen lang sein.');
  if (platform === 'youtube') {
    if (!copy.title) issues.push('Bitte gib einen Titel ein.');
    else if (Array.from(copy.title).length > 100) issues.push('Der YouTube-Titel darf höchstens 100 Zeichen lang sein.');
    else if (/[<>]/.test(copy.title)) issues.push('YouTube erlaubt im Titel keine spitzen Klammern.');
    const description = `${copy.description}\n\n${tags}`.replace(/[<>]/g, '').replace(/\p{White_Space}+$/u, '');
    if (encoder.encode(`${description}\n\n#Shorts`).length > 5000) issues.push('Beschreibung und Hashtags sind für YouTube zusammen zu lang. Bitte kürze sie.');
  } else if (platform === 'tiktok') {
    if (!copy.title) issues.push('Bitte gib einen Titel ein.');
    if (joined([copy.title, copy.description, tags]).length > 2200) issues.push('Titel, Beschreibung und Hashtags dürfen bei TikTok zusammen höchstens 2200 Zeichen lang sein.');
  } else {
    if (!copy.description && copy.hashtags.length === 0) issues.push('Bitte gib eine Beschreibung oder Hashtags ein.');
    if (Array.from(joined([copy.description, tags])).length > 2200) issues.push('Beschreibung und Hashtags dürfen bei Instagram zusammen höchstens 2200 Zeichen lang sein.');
  }
  return issues;
}

function usage(platform: AiCopyPlatform, copy: AiPlatformCopy, t: ReturnType<typeof useT>): string {
  const tags = formatHashtags(copy.hashtags);
  if (platform === 'youtube') return t('Titel {count}/100 Zeichen · {tags} Hashtags', { count: Array.from(copy.title).length, tags: copy.hashtags.length });
  if (platform === 'tiktok') return t('Beitragstext {count}/2200 Zeichen · {tags} Hashtags', { count: joined([copy.title, copy.description, tags]).length, tags: copy.hashtags.length });
  return t('Beitragstext {count}/2200 Zeichen · {tags}/30 Hashtags', { count: Array.from(joined([copy.description, tags])).length, tags: copy.hashtags.length });
}

function emptyCopy(copy: AiPlatformCopy): boolean {
  return !copy.title && !copy.description && copy.hashtags.length === 0;
}

function SaveBar({ dirty, pending, saved, editable, canSave }: { dirty: boolean; pending: boolean; saved: boolean; editable: boolean; canSave: boolean }) {
  const t = useT();
  const state = pending ? 'saving' : dirty ? 'dirty' : saved ? 'saved' : 'clean';
  const text = { saving: t("Texte werden gespeichert…"), dirty: t("Ungespeicherte Änderungen"), saved: t("Titel, Beschreibung und Hashtags sind gespeichert."), clean: t("Keine ungespeicherten Änderungen") }[state];
  return <div className="social-ai-savebar" data-state={state}>
    <p role="status">{text}</p>
    {editable && <button type="submit" className="studio-primary" disabled={!canSave}>{t("Texte speichern")}</button>}
  </div>;
}

export function MetadataPanel({ channel, videoId, disabled = false, onEditingChange, onSaved }: { channel: string; videoId: number; disabled?: boolean; onEditingChange?: (editing: boolean) => void; onSaved?: () => void }) {
  const { language, t } = useLanguage();
  const formId = useId();
  const client = useQueryClient();
  const key = ['social-ai', 'metadata', channel, videoId] as const;
  const metadata = useQuery({ queryKey: key, queryFn: ({ signal }) => fetchAiVersionMetadata(channel, videoId, signal), retry: false, refetchOnWindowFocus: false, staleTime: 0 });
  const [draft, setDraft] = useState<Draft | null>(null);
  const [mode, setMode] = useState<{ videoId: number; channel: string; shared: boolean } | null>(null);
  const save = useMutation({
    mutationFn: (copy: Record<AiCopyPlatform, AiPlatformCopy>) => saveAiVersionMetadata(channel, videoId, copy),
    onSuccess: (value) => { client.setQueryData(key, value); setDraft(null); onSaved?.(); },
  });
  useEffect(() => {
    onEditingChange?.(draft !== null || save.isPending || save.isError);
    return () => onEditingChange?.(false);
  }, [draft, save.isPending, save.isError, onEditingChange]);
  if (metadata.isPending) return <p role="status">{t("Titel und Beschreibungen werden geladen…")}</p>;
  if (metadata.isError) return <div className="social-ai-alert" role="alert"><div><p>{t("Titel und Beschreibungen konnten nicht geladen werden.")}</p><button type="button" className="studio-button" onClick={() => { void metadata.refetch(); }}>{t("Erneut laden")}</button></div></div>;
  const stored = toDraft(metadata.data);
  const current = draft ?? stored;
  const targets = metadata.data.targets;
  const lead = targets[0];
  const shared = mode?.videoId === videoId && mode.channel === channel ? mode.shared : targets.every((platform) => JSON.stringify(stored[platform]) === JSON.stringify(stored[lead]));
  const copy = toCopy(current);
  const issues = targets.map((platform) => ({ platform, list: copyIssues(platform, copy[platform]) }));
  const valid = issues.every((entry) => entry.list.length === 0);
  const editable = metadata.data.editable && !save.isPending && !save.isError && !metadata.isFetching && !disabled;
  const missing = draft === null && targets.every((platform) => emptyCopy(metadata.data.platforms[platform]));
  const update = (platform: AiCopyPlatform, field: keyof Draft[AiCopyPlatform], value: string) => {
    save.reset();
    const entry = { ...current[platform], [field]: value };
    setDraft(shared ? { youtube: { ...entry }, tiktok: { ...entry }, instagram: { ...entry } } : { ...current, [platform]: entry });
  };
  const chooseShared = () => {
    setMode({ videoId, channel, shared: true });
    setDraft({ youtube: { ...current[lead] }, tiktok: { ...current[lead] }, instagram: { ...current[lead] } });
    save.reset();
  };
  return <form className="social-ai-metadata" data-unsaved={draft !== null || save.isPending || save.isError ? 'true' : undefined} data-saving={save.isPending ? 'true' : undefined} aria-label={t("Titel, Beschreibung und Hashtags")} onSubmit={(event) => { event.preventDefault(); if (editable && valid && draft) save.mutate(copy); }}>
    <header className="social-ai-metadata-head">
      <h4>{t("Titel, Beschreibung und Hashtags")}</h4>
      <p>{metadata.data.editable ? t("Diese Texte werden beim Veröffentlichen dieser Fassung verwendet. Hashtags trennst du mit Leerzeichen.") : t("Diese Fassung ist nicht zur Bearbeitung ihrer Veröffentlichungstexte freigegeben.")}</p>
    </header>
    {missing && <p className="social-ai-caution" role="status">{t("Für diese Fassung liegen noch keine KI-Texte vor. Sie erscheinen hier, sobald die Fassung fertig ist. Du kannst die Felder auch selbst ausfüllen.")}</p>}
    <fieldset className="social-ai-copy-mode" disabled={!editable}>
      <legend>{t("Texte für die Plattformen")}</legend>
      <div className="social-ai-copy-options">
        <label data-checked={shared ? 'true' : undefined}><input type="radio" name={`${formId}-mode`} checked={shared} onChange={chooseShared} /><span>{t("Gleicher Text für alle Plattformen")}</span><small>{t("Du schreibst einen Text, der überall verwendet wird.")}</small></label>
        <label data-checked={!shared ? 'true' : undefined}><input type="radio" name={`${formId}-mode`} checked={!shared} onChange={() => { setMode({ videoId, channel, shared: false }); }} /><span>{t("Eigener Text je Plattform")}</span><small>{t("Du passt die Texte für jede Plattform einzeln an.")}</small></label>
      </div>
      {!shared && <p>{t("Wechselst du zu einem gleichen Text, übernehmen alle Plattformen den Text von {platform}.", { platform: labels[lead] })}</p>}
    </fieldset>
    {(shared ? [lead] : targets).map((platform) => {
      const id = `${formId}-${platform}`;
      const platformIssues = shared ? [...new Set(issues.flatMap((entry) => entry.list))] : issues.find((entry) => entry.platform === platform)?.list ?? [];
      return <fieldset key={platform} className="social-ai-copy-card" disabled={!editable}>
        <legend>{shared ? t("Alle Plattformen") : labels[platform]}</legend>
        <ul className="social-ai-copy-usage">{(shared ? targets : [platform]).map((target) => <li key={target} data-over={copyIssues(target, copy[target]).length > 0 ? 'true' : undefined}>{shared ? `${labels[target]}: ` : ''}{usage(target, copy[target], t)}</li>)}</ul>
        {platform === 'instagram' ? <p className="social-ai-copy-hint">{t("Instagram verwendet keinen Titel, nur Beschreibung und Hashtags.")}</p> : <div className="social-ai-copy-field"><label htmlFor={`${id}-title`}>{t("Titel")}</label><input id={`${id}-title`} className="studio-input" type="text" value={current[platform].title} onChange={(event) => update(platform, 'title', event.target.value)} /></div>}
        <div className="social-ai-copy-field"><label htmlFor={`${id}-description`}>{t("Beschreibung")}</label><textarea id={`${id}-description`} className="studio-input" rows={4} value={current[platform].description} onChange={(event) => update(platform, 'description', event.target.value)} /></div>
        <div className="social-ai-copy-field"><label htmlFor={`${id}-hashtags`}>{t("Hashtags")}</label><input id={`${id}-hashtags`} className="studio-input" type="text" value={current[platform].hashtags} onChange={(event) => update(platform, 'hashtags', event.target.value)} placeholder="#deadlock #clips" /></div>
        {(shared ? targets.includes('youtube') : platform === 'youtube') && <p className="social-ai-copy-hint">{t("YouTube: Beschreibung und Hashtags einschließlich #Shorts höchstens 5000 Bytes.")}</p>}
        {platformIssues.map((issue) => <p key={issue} className="social-ai-caution" role="alert">{t(issue)}</p>)}
      </fieldset>;
    })}
    {(metadata.data.editable || draft) && <div className="social-ai-metadata-footer" data-pending={draft !== null || save.isPending ? 'true' : undefined}>
      <SaveBar dirty={draft !== null} pending={save.isPending} saved={save.isSuccess} editable={metadata.data.editable} canSave={editable && valid && draft !== null} />
      {draft && <button type="button" className="studio-button" disabled={save.isPending || save.isError || metadata.isFetching} onClick={() => { if (!save.isPending && !save.isError && !metadata.isFetching) { save.reset(); setDraft(null); setMode(null); } }}>{t("Änderungen verwerfen")}</button>}
    </div>}
    {save.isError && <div className="social-ai-caution" role="alert"><p>{language === 'en' ? editorErrorExplanation(save.error, t) : save.error instanceof Error ? save.error.message : t("Die Texte konnten nicht gespeichert werden.")} {t("Lade den gespeicherten Stand neu, bevor du erneut speicherst oder Änderungen verwirfst.")}</p><button type="button" className="studio-button" disabled={metadata.isFetching} onClick={async () => { const result = await metadata.refetch(); if (result.isSuccess) save.reset(); }}>{t("Gespeicherte Texte neu laden")}</button></div>}
  </form>;
}
