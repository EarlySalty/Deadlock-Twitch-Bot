import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { CalendarDays, ExternalLink, Plus, Save, UserRound } from 'lucide-react';
import { fetchPartnerProfile, savePartnerProfile, type PartnerProfileData, type ProfileContent, type ProfileEvent } from '../api/partnerProfile';
import { usePlan } from '../context/PlanContext';
import { isPreviewModeEnabled } from '../preview/routes';
import { berlinInput, fromBerlinInput, monthDays } from '../utils/partnerProfile';

const field = 'w-full rounded-xl border border-white/15 bg-black/20 px-3 py-2.5 text-sm text-white focus:border-primary focus:outline-none';
const button = 'inline-flex items-center justify-center gap-2 rounded-xl border border-primary/30 px-4 py-2.5 text-sm font-semibold text-primary hover:bg-primary/10 disabled:opacity-50 disabled:cursor-not-allowed';
const panel = 'panel-card rounded-2xl p-5 sm:p-6 space-y-4';
interface EventDraft { id: string; title: string; description: string; start: string; end: string }
function emptyEvent(day: string): EventDraft { return { id: '', title: '', description: '', start: `${day}T18:00`, end: `${day}T20:00` }; }
function message(error: unknown) { return error instanceof Error ? error.message : 'Das Speichern ist fehlgeschlagen.'; }

export function ProfileEditor({ initial, streamer, onReload, onSaved }: { initial: PartnerProfileData; streamer?: string; onReload: () => void; onSaved?: (data: PartnerProfileData) => void }) {
  const [data, setData] = useState(initial);
  const [dirty, setDirty] = useState(false);
  const [savedPublished, setSavedPublished] = useState(initial.published);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState('');
  const [error, setError] = useState('');
  const [month, setMonth] = useState(() => berlinInput(new Date().toISOString()).slice(0, 7));
  const [draft, setDraft] = useState<EventDraft | null>(null);
  const [eventError, setEventError] = useState('');
  const profile = data.profile;
  const calendar = monthDays(month);
  const locked = saving || !data.active;
  useEffect(() => {
    if (!dirty && !draft) return;
    const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty, draft]);
  function change(values: Partial<ProfileContent>) {
    setData(current => ({ ...current, profile: { ...current.profile, ...values } }));
    setDirty(true); setNotice('');
  }
  async function save() {
    setSaving(true); setError(''); setNotice('');
    try {
      const saved = await savePartnerProfile({ ...data, profile: { ...profile, featured: profile.featured.filter(s => s.trim()) } }, streamer);
      setData(saved); setSavedPublished(saved.published); setDirty(false); onSaved?.(saved);
      setNotice(saved.published ? 'Gespeichert. Dein Profil ist öffentlich erreichbar.' : 'Gespeichert. Dein Profil ist nicht öffentlich erreichbar.');
    } catch (err) { setError(message(err)); } finally { setSaving(false); }
  }
  function addEvent() {
    if (!draft) return;
    try {
      const previous = profile.events.find(e => e.id === draft.id);
      const starts_at = fromBerlinInput(draft.start, previous?.starts_at);
      const ends_at = fromBerlinInput(draft.end, previous?.ends_at);
      const duration = Date.parse(ends_at) - Date.parse(starts_at);
      if (duration <= 0 || duration > 48 * 3_600_000) throw new Error('Der Termin muss länger als null, aber höchstens 48 Stunden dauern.');
      if (!draft.title.trim()) throw new Error('Bitte einen Titel eintragen.');
      const next: ProfileEvent = { id: draft.id || crypto.randomUUID(), title: draft.title.trim(), description: draft.description.trim(), starts_at, ends_at };
      const events = [...profile.events.filter(e => e.id !== next.id), next].sort((a, b) => a.starts_at.localeCompare(b.starts_at));
      if (events.length > 200) throw new Error('Höchstens 200 Termine speichern. Alte Termine kannst du gezielt entfernen.');
      change({ events }); setDraft(null); setEventError('');
    } catch (err) { setEventError(message(err)); }
  }
  function openEvent(event: ProfileEvent) {
    if (draft && !window.confirm('Den noch nicht übernommenen Termineintrag verwerfen?')) return;
    setDraft({ id: event.id, title: event.title, description: event.description, start: berlinInput(event.starts_at), end: berlinInput(event.ends_at) });
    setEventError('');
  }
  return <div className="space-y-6">
    <section className={panel}>
      <div className="flex flex-wrap items-start justify-between gap-4"><div>
        <p className="text-xs uppercase tracking-widest text-primary">Dein Platz im Partnernetzwerk</p>
        <h1 className="mt-2 flex items-center gap-2 text-2xl font-semibold text-white"><UserRound className="h-6 w-6 text-primary" />Mein öffentliches Profil</h1>
        <p className="mt-2 text-sm text-text-secondary">Stell dich vor, teile deine Streamzeiten und zeig, wer zu deinem Umfeld gehört.</p>
      </div><span className="rounded-full border border-white/15 px-3 py-1.5 text-xs">{!data.active ? 'Bot derzeit nicht aktiv' : savedPublished ? 'Veröffentlichter Stand vorhanden' : 'Noch nicht veröffentlicht'}</span></div>
      <div className="flex flex-wrap items-center gap-3 rounded-xl bg-black/20 p-3 text-sm">
        <code className="break-all">deutsche-deadlock-community.de{data.public_path}</code>
        {data.active && savedPublished && <a href={data.public_path} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-primary">Profil öffnen<ExternalLink className="h-4 w-4" /></a>}
      </div>
      <p className="text-xs leading-relaxed text-text-secondary">Bei Austritt, Bot-Trennung oder Deaktivierung geht die Seite automatisch offline. Deine Inhalte bleiben erhalten. Bei erneuter Aktivierung wird ein zuvor veröffentlichtes Profil wieder sichtbar.</p>
      {!data.active && <p role="status" className="rounded-xl border border-warning/30 p-3 text-sm">Dein Profil bleibt gespeichert, ist aber nicht öffentlich. Du kannst es bearbeiten, sobald dein Bot wieder aktiv ist. <a href="/twitch/verwaltung" className="text-primary underline">Bot-Verwaltung öffnen</a></p>}
    </section>
    <fieldset disabled={locked} className="min-w-0 space-y-6 disabled:opacity-60">
      <div className="grid items-start gap-6 xl:grid-cols-2">
        <section className={panel}>
          <h2 className="text-lg font-semibold">So zeigst du dich</h2>
          <label className="block space-y-1.5 text-sm">Überschrift<input className={field} maxLength={120} placeholder="Deadlock, gute Gespräche und eine Runde mehr" value={profile.headline} onChange={e => change({ headline: e.target.value })} /></label>
          <label className="block space-y-1.5 text-sm">Über mich<textarea className={`${field} min-h-40`} maxLength={4000} placeholder="Was erwartet Zuschauer bei dir? Was spielst du gerne?" value={profile.about} onChange={e => change({ about: e.target.value })} /></label>
          <div className="flex justify-end text-xs text-text-secondary">{profile.about.length} / 4000</div>
          <label className="block space-y-1.5 text-sm">Akzentfarbe<select className={field} value={profile.accent} onChange={e => change({ accent: e.target.value as ProfileContent['accent'] })}><option value="gold">Community-Gold</option><option value="violet">Violett</option><option value="teal">Petrol</option></select></label>
          <label className="block space-y-1.5 text-sm">Twitch-Profilbild (optional)<input className={field} type="url" maxLength={2048} placeholder="https://static-cdn.jtvnw.net/…" value={profile.avatar_url} onChange={e => change({ avatar_url: e.target.value })} /><span className="block text-xs text-text-secondary">Bildadresse deines Twitch-Profilbilds. Ohne Bild zeigen wir deinen Anfangsbuchstaben.</span></label>
          <label className="flex items-start gap-3 text-sm"><input className="mt-1" type="checkbox" checked={profile.show_history} onChange={e => change({ show_history: e.target.checked })} /><span>Meine erfassten Livezeiten öffentlich zeigen<span className="mt-1 block text-xs text-text-secondary">Monatliche Historie und 90-Tage-Wochenübersicht. Keine Zuschauerzahlen, Chats oder privaten Analysen.</span></span></label>
        </section>
        <section className={panel}>
          <div className="flex items-center justify-between gap-3"><h2 className="text-lg font-semibold">Hier findet man dich</h2><span className="text-xs text-text-secondary">{profile.socials.length} / 12 Links</span></div>
          <p className="text-sm text-text-secondary">YouTube, TikTok, Instagram, Discord oder deine Website. Dein Twitch-Kanal ist automatisch verlinkt.</p>
          {profile.socials.map((social, i) => <div key={i} className="space-y-2 rounded-xl border border-white/10 p-3">
            <label className="block text-xs text-text-secondary">Name des Links<input aria-label={`Link ${i + 1}: Name`} className={`${field} mt-1`} maxLength={40} value={social.label} onChange={e => change({ socials: profile.socials.map((s, n) => n === i ? { ...s, label: e.target.value } : s) })} /></label>
            <label className="block text-xs text-text-secondary">HTTPS-Adresse<input aria-label={`Link ${i + 1}: Adresse`} className={`${field} mt-1`} type="url" maxLength={2048} value={social.url} placeholder="https://…" onChange={e => change({ socials: profile.socials.map((s, n) => n === i ? { ...s, url: e.target.value } : s) })} /></label>
            <button className="text-xs text-text-secondary underline" type="button" onClick={() => change({ socials: profile.socials.filter((_, n) => n !== i) })}>Link entfernen</button>
          </div>)}
          <button className={button} type="button" disabled={profile.socials.length >= 12} onClick={() => change({ socials: [...profile.socials, { label: '', url: '' }] })}><Plus className="h-4 w-4" />Link hinzufügen</button>
          <div className="border-t border-white/10 pt-4">
            <h3 className="font-semibold">Aus deinem Umfeld</h3><p className="mt-2 text-xs text-text-secondary">Empfiehl bis zu sechs Partner. Öffentlich erscheinen nur aktive Partner mit veröffentlichtem Profil.</p>
            <label className="mt-3 block space-y-1.5 text-sm">Twitch-Namen, durch Kommas getrennt<input className={field} value={profile.featured.join(', ')} placeholder="partner_eins, partner_zwei" onChange={e => change({ featured: e.target.value.split(',').map(s => s.trim().replace(/^@/, '')) })} /></label>
          </div>
        </section>
      </div>
      <section className={panel}>
        <div className="flex flex-wrap items-center justify-between gap-3"><div><h2 className="flex items-center gap-2 text-lg font-semibold"><CalendarDays className="h-5 w-5 text-primary" />Dein Kalender</h2><p className="mt-1 text-sm text-text-secondary">Klicke auf einen Tag, um einen Stream oder Community-Abend einzutragen. Alle Uhrzeiten gelten für Berlin.</p></div><label className="text-xs text-text-secondary">Monat<input aria-label="Kalendermonat" type="month" min="2000-01" max="2100-12" className={`${field} mt-1`} value={month} onChange={e => setMonth(e.target.value)} /></label></div>
        <div className="overflow-x-auto pb-2"><div className="grid min-w-[580px] grid-cols-7 gap-1.5">
          {['Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa', 'So'].map(day => <span key={day} className="p-2 text-xs text-text-secondary">{day}</span>)}
          {Array.from({ length: calendar.leading }, (_, i) => <span key={`empty-${i}`} />)}
          {calendar.days.map(day => <button type="button" key={day} aria-label={`Termin am ${day} eintragen`} onClick={() => { if (!draft || window.confirm('Den noch nicht übernommenen Termineintrag verwerfen?')) { setDraft(emptyEvent(day)); setEventError(''); } }} className="min-h-20 rounded-lg border border-white/10 bg-white/[0.02] p-2 text-left hover:border-primary/50 focus-visible:outline-primary">
            <span className="text-xs text-text-secondary">{Number(day.slice(-2))}</span>
            {profile.events.filter(event => berlinInput(event.starts_at).slice(0, 10) <= day && berlinInput(new Date(Date.parse(event.ends_at) - 1).toISOString()).slice(0, 10) >= day).map(event => <span key={event.id} className="mt-1 block truncate rounded bg-primary/10 p-1 text-[10px] text-primary" title={event.title}>{event.title}</span>)}
          </button>)}
        </div></div>
        {draft && <div className="space-y-4 rounded-xl border border-primary/30 bg-primary/5 p-4">
          <h3 className="font-semibold">{draft.id ? 'Termin bearbeiten' : 'Neuer Termin'}</h3>
          <label className="block space-y-1.5 text-sm">Titel<input className={field} maxLength={120} value={draft.title} onChange={e => setDraft({ ...draft, title: e.target.value })} /></label>
          <div className="grid gap-4 sm:grid-cols-2"><label className="block space-y-1.5 text-sm">Beginn (Berlin)<input type="datetime-local" className={field} min="2000-01-01T00:00" max="2100-12-31T23:59" value={draft.start} onChange={e => setDraft({ ...draft, start: e.target.value })} /></label><label className="block space-y-1.5 text-sm">Ende (Berlin)<input type="datetime-local" className={field} min="2000-01-01T00:00" max="2100-12-31T23:59" value={draft.end} onChange={e => setDraft({ ...draft, end: e.target.value })} /></label></div>
          <label className="block space-y-1.5 text-sm">Beschreibung<textarea className={field} maxLength={1000} value={draft.description} onChange={e => setDraft({ ...draft, description: e.target.value })} /></label>
          {eventError && <p role="alert" className="text-sm text-warning">{eventError}</p>}
          <div className="flex flex-wrap gap-3"><button type="button" className={button} onClick={addEvent}>Termin übernehmen</button><button type="button" className="text-sm text-text-secondary underline" onClick={() => { setDraft(null); setEventError(''); }}>Abbrechen</button></div>
          <p className="text-xs text-text-secondary">Übernehmen ändert deinen Entwurf. Öffentlich wird die Änderung erst mit „Profil speichern“.</p>
        </div>}
        <div className="space-y-2">{profile.events.filter(event => berlinInput(event.starts_at).slice(0, 7) <= month && berlinInput(new Date(Date.parse(event.ends_at) - 1).toISOString()).slice(0, 7) >= month).map(event => <div key={event.id} className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-white/10 p-3">
          <div><h3 className="text-sm font-semibold">{event.title}</h3><p className="text-xs text-text-secondary">{berlinInput(event.starts_at).replace('T', ' · ')} bis {berlinInput(event.ends_at).replace('T', ' · ')} (Berlin)</p></div>
          <div className="flex gap-3"><button type="button" className="text-sm text-primary" onClick={() => openEvent(event)}>Bearbeiten</button><button type="button" className="text-sm text-text-secondary" onClick={() => { if (window.confirm(`„${event.title}“ aus deinem Kalender entfernen?`)) { change({ events: profile.events.filter(e => e.id !== event.id) }); if (draft?.id === event.id) setDraft(null); } }}>Entfernen</button></div>
        </div>)}</div>
        {profile.events.length === 0 && <p className="text-sm text-text-secondary">Noch keine Termine. Wähle oben deinen ersten Streamtag aus.</p>}
        <p className="text-xs text-text-secondary">{profile.events.length} / 200 Termine. Keine automatische Löschung alter Termine. Erfasste Livestreams werden separat angezeigt und lassen sich hier nicht verändern.</p>
      </section>
    </fieldset>
    <section className={`${panel} sticky bottom-3 z-10 border border-primary/30 bg-bg/95 shadow-xl backdrop-blur`}>
      <div className="flex flex-wrap items-center justify-between gap-4">
        <label className="flex items-center gap-3 text-sm"><input type="checkbox" disabled={locked} checked={data.published} onChange={e => { setData({ ...data, published: e.target.checked }); setDirty(true); setNotice(''); }} /><span>Profil veröffentlichen<span className="block text-xs text-text-secondary">Ausgeschaltet bleibt dein Profil ein privater Entwurf.</span></span></label>
        <div className="flex items-center gap-4"><span className="text-xs text-text-secondary" role="status">{dirty ? 'Ungespeicherte Änderungen' : 'Gespeicherter Stand'}</span><button type="button" disabled={locked || !dirty || Boolean(draft)} className={button} onClick={() => void save()}><Save className="h-4 w-4" />{saving ? 'Wird gespeichert …' : 'Profil speichern'}</button></div>
      </div>
      {draft && <p className="text-xs text-text-secondary">Bitte den offenen Termin zuerst übernehmen oder abbrechen.</p>}
      {notice && <p role="status" className="text-sm text-primary">{notice}</p>}
      {error && <div role="alert" className="space-y-2 text-sm text-warning"><p>{error} Deine Eingaben bleiben hier erhalten.</p><button type="button" className="underline" onClick={() => { if (window.confirm('Deine ungespeicherten Eingaben verwerfen und den aktuellen Serverstand laden?')) onReload(); }}>Aktuellen Stand laden</button></div>}
    </section>
  </div>;
}

export function PartnerProfile({ streamer }: { streamer?: string }) {
  const { isDemoMode } = usePlan();
  const demo = isDemoMode || isPreviewModeEnabled();
  const [generation, setGeneration] = useState(0);
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ['partner-profile', streamer, generation], queryFn: () => fetchPartnerProfile(streamer), enabled: !demo, retry: false, staleTime: Infinity, refetchOnWindowFocus: false });
  if (demo) return <div className={panel}><h1 className="text-xl font-semibold">Mein Profil</h1><p>Die Demo liest oder verändert keine persönlichen Partnerprofile. Melde dich mit deinem Twitch-Konto an, um dein Profil zu gestalten.</p></div>;
  if (query.isPending) return <p role="status" className={panel}>Dein Profil wird geladen …</p>;
  if (query.error || !query.data) return <div role="alert" className={panel}><p>{query.error ? message(query.error) : 'Dein Profil konnte nicht geladen werden.'}</p><button type="button" className={button} onClick={() => void query.refetch()}>Erneut versuchen</button></div>;
  return <ProfileEditor key={`${streamer || 'own'}-${generation}`} initial={query.data} streamer={streamer} onReload={() => setGeneration(value => value + 1)} onSaved={saved => queryClient.setQueryData(['partner-profile', streamer, generation], saved)} />;
}
