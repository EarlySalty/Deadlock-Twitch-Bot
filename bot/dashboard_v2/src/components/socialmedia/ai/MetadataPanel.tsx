import { useEffect, useId, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { AiCopyPlatform, AiPlatformCopy, AiVersionMetadata } from './api';
import { AI_COPY_PLATFORMS, fetchAiVersionMetadata, saveAiVersionMetadata } from './api';

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

function usage(platform: AiCopyPlatform, copy: AiPlatformCopy): string {
  const tags = formatHashtags(copy.hashtags);
  if (platform === 'youtube') return `Titel ${Array.from(copy.title).length}/100 Zeichen · ${copy.hashtags.length} Hashtags`;
  if (platform === 'tiktok') return `Beitragstext ${joined([copy.title, copy.description, tags]).length}/2200 Zeichen · ${copy.hashtags.length} Hashtags`;
  return `Beitragstext ${Array.from(joined([copy.description, tags])).length}/2200 Zeichen · ${copy.hashtags.length}/30 Hashtags`;
}

export function MetadataPanel({ channel, videoId, disabled = false, onEditingChange }: { channel: string; videoId: number; disabled?: boolean; onEditingChange?: (editing: boolean) => void }) {
  const formId = useId();
  const client = useQueryClient();
  const key = ['social-ai', 'metadata', channel, videoId] as const;
  const metadata = useQuery({ queryKey: key, queryFn: ({ signal }) => fetchAiVersionMetadata(channel, videoId, signal), retry: false, refetchOnWindowFocus: false });
  const [draft, setDraft] = useState<Draft | null>(null);
  const save = useMutation({
    mutationFn: (copy: Record<AiCopyPlatform, AiPlatformCopy>) => saveAiVersionMetadata(channel, videoId, copy),
    onSuccess: (value) => { client.setQueryData(key, value); setDraft(null); },
  });
  useEffect(() => {
    onEditingChange?.(draft !== null || save.isPending || save.isError);
    return () => onEditingChange?.(false);
  }, [draft, save.isPending, save.isError, onEditingChange]);
  if (metadata.isPending) return <p role="status">Titel und Beschreibungen werden geladen…</p>;
  if (metadata.isError) return <div className="social-ai-alert" role="alert"><div><p>Titel und Beschreibungen konnten nicht geladen werden.</p><button type="button" className="studio-button" onClick={() => { void metadata.refetch(); }}>Erneut laden</button></div></div>;
  const current = draft ?? toDraft(metadata.data);
  const copy = toCopy(current);
  const issues = AI_COPY_PLATFORMS.map((platform) => ({ platform, list: copyIssues(platform, copy[platform]) }));
  const valid = issues.every((entry) => entry.list.length === 0);
  const editable = metadata.data.editable && !save.isPending && !save.isError && !metadata.isFetching && !disabled;
  const update = (platform: AiCopyPlatform, field: keyof Draft[AiCopyPlatform], value: string) => {
    save.reset();
    setDraft({ ...current, [platform]: { ...current[platform], [field]: value } });
  };
  return <form className="social-ai-metadata" data-unsaved={draft !== null || save.isPending || save.isError ? 'true' : undefined} data-saving={save.isPending ? 'true' : undefined} aria-label="Titel, Beschreibung und Hashtags" onSubmit={(event) => { event.preventDefault(); if (editable && valid && draft) save.mutate(copy); }}>
    <h4>Titel, Beschreibung und Hashtags</h4>
    <p>{metadata.data.editable ? 'Diese Texte werden beim Veröffentlichen dieser Fassung verwendet. Hashtags trennst du mit Leerzeichen.' : 'Diese Fassung ist nicht zur Bearbeitung ihrer Veröffentlichungstexte freigegeben.'}</p>
    {AI_COPY_PLATFORMS.map((platform) => {
      const id = `${formId}-${platform}`;
      const platformIssues = issues.find((entry) => entry.platform === platform)?.list ?? [];
      return <fieldset key={platform} disabled={!editable}>
        <legend>{labels[platform]}</legend>
        {platform === 'instagram' ? <p>Instagram verwendet keinen Titel, nur Beschreibung und Hashtags.</p> : <><label htmlFor={`${id}-title`}>Titel</label><input id={`${id}-title`} type="text" value={current[platform].title} onChange={(event) => update(platform, 'title', event.target.value)} /></>}
        <label htmlFor={`${id}-description`}>Beschreibung</label>
        <textarea id={`${id}-description`} rows={3} value={current[platform].description} onChange={(event) => update(platform, 'description', event.target.value)} />
        <label htmlFor={`${id}-hashtags`}>Hashtags</label>
        <input id={`${id}-hashtags`} type="text" value={current[platform].hashtags} onChange={(event) => update(platform, 'hashtags', event.target.value)} placeholder="#deadlock #clips" />
        <small>{usage(platform, copy[platform])}</small>
        {platformIssues.map((issue) => <p key={issue} className="social-ai-caution" role="alert">{issue}</p>)}
      </fieldset>;
    })}
    {metadata.data.editable && <div className="social-ai-actions"><button type="submit" className="studio-primary" disabled={!editable || !valid || !draft}>{save.isPending ? 'Texte werden gespeichert…' : 'Texte speichern'}</button>{draft && <button type="button" className="studio-button" disabled={save.isPending || save.isError || metadata.isFetching} onClick={() => { save.reset(); setDraft(null); }}>Änderungen verwerfen</button>}</div>}
    {save.isSuccess && <p role="status">Titel, Beschreibung und Hashtags sind gespeichert.</p>}
    {save.isError && <div className="social-ai-caution" role="alert"><p>{save.error instanceof Error ? save.error.message : 'Die Texte konnten nicht gespeichert werden.'} Lade den gespeicherten Stand neu, bevor du erneut speicherst oder Änderungen verwirfst.</p><button type="button" className="studio-button" disabled={metadata.isFetching} onClick={async () => { const result = await metadata.refetch(); if (result.isSuccess) save.reset(); }}>Gespeicherte Texte neu laden</button></div>}
  </form>;
}
