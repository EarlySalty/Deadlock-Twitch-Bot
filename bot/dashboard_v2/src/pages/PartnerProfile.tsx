import { useEffect, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CalendarDays,
  ExternalLink,
  Eye,
  Link2,
  Palette,
  Plus,
  Save,
  Sparkles,
  UserRound,
  UsersRound,
} from 'lucide-react';
import {
  fetchPartnerProfile,
  savePartnerProfile,
  type PartnerProfileData,
  type ProfileContent,
  type ProfileEvent,
} from '../api/partnerProfile';
import { usePlan } from '../context/PlanContext';
import { isPreviewModeEnabled } from '../preview/routes';
import { berlinInput, fromBerlinInput, monthDays } from '../utils/partnerProfile';

const field =
  'w-full rounded-xl border border-border bg-background/70 px-3.5 py-2.5 text-sm text-white outline-none transition-colors placeholder:text-text-secondary/55 focus:border-primary/60 focus:ring-1 focus:ring-primary/25';
const primaryButton =
  'inline-flex min-h-11 items-center justify-center gap-2 rounded-xl bg-primary px-4 py-2.5 text-sm font-bold text-[#0D0806] transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-45';
const secondaryButton =
  'inline-flex min-h-10 items-center justify-center gap-2 rounded-xl border border-border bg-background/65 px-3.5 py-2 text-sm font-semibold text-text-secondary transition-colors hover:border-border-hover hover:text-white disabled:cursor-not-allowed disabled:opacity-45';
const panel = 'panel-card rounded-2xl p-5 md:p-6';

interface EventDraft {
  id: string;
  title: string;
  description: string;
  start: string;
  end: string;
}

function emptyEvent(day: string): EventDraft {
  return { id: '', title: '', description: '', start: `${day}T18:00`, end: `${day}T20:00` };
}

function message(error: unknown) {
  return error instanceof Error ? error.message : 'Das Speichern ist fehlgeschlagen.';
}

function SectionTitle({
  icon,
  eyebrow,
  title,
  description,
  aside,
}: {
  icon: ReactNode;
  eyebrow: string;
  title: string;
  description?: string;
  aside?: ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-4">
      <div className="flex min-w-0 items-start gap-3">
        <span className="gradient-accent flex h-10 w-10 shrink-0 items-center justify-center rounded-xl">
          {icon}
        </span>
        <div className="min-w-0">
          <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-primary">{eyebrow}</div>
          <h2 className="display-font mt-0.5 text-xl font-bold text-white">{title}</h2>
          {description ? <p className="mt-1 max-w-2xl text-sm leading-relaxed text-text-secondary">{description}</p> : null}
        </div>
      </div>
      {aside}
    </div>
  );
}

function Avatar({
  url,
  login,
  size = 'large',
}: {
  url: string;
  login: string;
  size?: 'large' | 'small';
}) {
  const box = size === 'large' ? 'h-20 w-20 text-2xl' : 'h-12 w-12 text-base';
  return (
    <span className={`${box} gradient-accent sidebar-avatar-glow flex shrink-0 items-center justify-center overflow-hidden rounded-2xl font-bold`}>
      {url ? (
        <img src={url} alt={`Twitch-Profilbild von ${login}`} className="h-full w-full object-cover" />
      ) : (
        login.slice(0, 1).toUpperCase() || '?'
      )}
    </span>
  );
}

export function ProfileEditor({
  initial,
  streamer,
  twitchAvatarUrl,
  onReload,
  onSaved,
}: {
  initial: PartnerProfileData;
  streamer?: string;
  twitchAvatarUrl?: string | null;
  onReload: () => void;
  onSaved?: (data: PartnerProfileData) => void;
}) {
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
  const effectiveAvatar = twitchAvatarUrl?.trim() || profile.avatar_url.trim();

  useEffect(() => {
    if (!dirty && !draft) return;
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty, draft]);

  function change(values: Partial<ProfileContent>) {
    setData(current => ({ ...current, profile: { ...current.profile, ...values } }));
    setDirty(true);
    setNotice('');
  }

  async function save() {
    setSaving(true);
    setError('');
    setNotice('');
    try {
      const saved = await savePartnerProfile(
        {
          ...data,
          profile: {
            ...profile,
            avatar_url: effectiveAvatar,
            featured: profile.featured.filter(value => value.trim()),
          },
        },
        streamer,
      );
      setData(saved);
      setSavedPublished(saved.published);
      setDirty(false);
      onSaved?.(saved);
      setNotice(
        saved.published
          ? 'Gespeichert. Dein Profil ist öffentlich erreichbar.'
          : 'Gespeichert. Dein Profil bleibt privat.',
      );
    } catch (err) {
      setError(message(err));
    } finally {
      setSaving(false);
    }
  }

  function addEvent() {
    if (!draft) return;
    try {
      const previous = profile.events.find(event => event.id === draft.id);
      const starts_at = fromBerlinInput(draft.start, previous?.starts_at);
      const ends_at = fromBerlinInput(draft.end, previous?.ends_at);
      const duration = Date.parse(ends_at) - Date.parse(starts_at);
      if (duration <= 0 || duration > 48 * 3_600_000) {
        throw new Error('Der Termin muss länger als null, aber höchstens 48 Stunden dauern.');
      }
      if (!draft.title.trim()) throw new Error('Bitte einen Titel eintragen.');
      const next: ProfileEvent = {
        id: draft.id || crypto.randomUUID(),
        title: draft.title.trim(),
        description: draft.description.trim(),
        starts_at,
        ends_at,
      };
      const events = [...profile.events.filter(event => event.id !== next.id), next].sort((a, b) =>
        a.starts_at.localeCompare(b.starts_at),
      );
      if (events.length > 200) {
        throw new Error('Höchstens 200 Termine speichern. Alte Termine kannst du gezielt entfernen.');
      }
      change({ events });
      setDraft(null);
      setEventError('');
    } catch (err) {
      setEventError(message(err));
    }
  }

  function openEvent(event: ProfileEvent) {
    if (draft && !window.confirm('Den noch nicht übernommenen Termineintrag verwerfen?')) return;
    setDraft({
      id: event.id,
      title: event.title,
      description: event.description,
      start: berlinInput(event.starts_at),
      end: berlinInput(event.ends_at),
    });
    setEventError('');
  }

  return (
    <div className="mx-auto max-w-[1320px] space-y-5">
      <section className={panel}>
        <div className="flex flex-col gap-5 xl:flex-row xl:items-start xl:justify-between">
          <div className="flex min-w-0 items-start gap-4">
            <Avatar url={effectiveAvatar} login={data.login} />
            <div className="min-w-0">
              <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-primary">
                Dein Platz im Partnernetzwerk
              </div>
              <h1 className="display-font mt-1 text-2xl font-extrabold text-white">Mein öffentliches Profil</h1>
              <p className="mt-2 max-w-2xl text-sm leading-relaxed text-text-secondary">
                Stell dich als Streamer vor, verlinke deine Kanäle und plane Termine für deine Community.
              </p>
              <div className="mt-3 flex flex-wrap items-center gap-2">
                <span className="rounded-full border border-border bg-background/60 px-2.5 py-1 text-xs font-semibold text-text-secondary">
                  @{data.login}
                </span>
                <span
                  className={`rounded-full border px-2.5 py-1 text-xs font-semibold ${
                    !data.active
                      ? 'border-warning/30 bg-warning/10 text-warning'
                      : savedPublished
                        ? 'border-success/30 bg-success/10 text-success'
                        : 'border-border bg-background/60 text-text-secondary'
                  }`}
                >
                  {!data.active
                    ? 'Bot derzeit nicht aktiv'
                    : savedPublished
                      ? 'Profil veröffentlicht'
                      : 'Noch nicht veröffentlicht'}
                </span>
              </div>
            </div>
          </div>

          <div className="flex w-full flex-col gap-3 xl:w-auto xl:min-w-[340px]">
            <label className="soft-elevate flex cursor-pointer items-start gap-3 rounded-xl border border-border bg-background/60 p-3.5">
              <input
                className="mt-0.5 h-5 w-5 accent-primary"
                type="checkbox"
                disabled={locked}
                checked={data.published}
                onChange={event => {
                  setData({ ...data, published: event.target.checked });
                  setDirty(true);
                  setNotice('');
                }}
              />
              <span className="min-w-0">
                <span className="block text-sm font-bold text-white">Profil veröffentlichen</span>
                <span className="mt-0.5 block text-xs leading-relaxed text-text-secondary">
                  Ausgeschaltet bleibt dein Profil ein privater Entwurf.
                </span>
              </span>
            </label>
            <button
              type="button"
              disabled={locked || !dirty || Boolean(draft)}
              className={primaryButton}
              onClick={() => void save()}
            >
              <Save className="h-4 w-4" />
              {saving ? 'Wird gespeichert …' : 'Änderungen speichern'}
            </button>
            <p className="text-right text-xs text-text-secondary" role="status">
              {draft
                ? 'Offenen Termin zuerst übernehmen oder abbrechen.'
                : dirty
                  ? 'Ungespeicherte Änderungen'
                  : 'Gespeicherter Stand'}
            </p>
          </div>
        </div>

        <div className="mt-5 grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto]">
          <div className="panel-inset flex min-w-0 flex-wrap items-center gap-3 rounded-xl px-4 py-3">
            <div className="min-w-0 flex-1">
              <p className="text-[10px] font-semibold uppercase tracking-[0.16em] text-text-secondary">Öffentliche Adresse</p>
              <code className="mt-1 block truncate text-xs text-white">deutsche-deadlock-community.de{data.public_path}</code>
            </div>
            {data.active && savedPublished ? (
              <a
                href={data.public_path}
                target="_blank"
                rel="noopener noreferrer"
                className={secondaryButton}
              >
                Profil öffnen
                <ExternalLink className="h-4 w-4" />
              </a>
            ) : null}
          </div>
          <div className="panel-inset flex items-center gap-3 rounded-xl px-4 py-3">
            <Avatar url={effectiveAvatar} login={data.login} size="small" />
            <div>
              <p className="text-sm font-semibold text-white">Profilbild von Twitch</p>
              <p className="mt-0.5 text-xs text-text-secondary">Wird automatisch aus deinem Twitch-Konto übernommen.</p>
            </div>
          </div>
        </div>

        {!data.active ? (
          <p role="status" className="mt-4 rounded-xl border border-warning/30 bg-warning/10 p-3 text-sm text-warning">
            Dein Profil bleibt gespeichert, ist aber nicht öffentlich. Du kannst es bearbeiten, sobald dein Bot wieder aktiv ist.{' '}
            <a href="/twitch/verwaltung" className="font-semibold underline">Bot-Verwaltung öffnen</a>
          </p>
        ) : null}
        {notice ? <p role="status" className="mt-4 text-sm font-medium text-success">{notice}</p> : null}
        {error ? (
          <div role="alert" className="mt-4 space-y-2 rounded-xl border border-warning/30 bg-warning/10 p-3 text-sm text-warning">
            <p>{error} Deine Eingaben bleiben hier erhalten.</p>
            <button
              type="button"
              className="font-semibold underline"
              onClick={() => {
                if (window.confirm('Deine ungespeicherten Eingaben verwerfen und den aktuellen Serverstand laden?')) {
                  onReload();
                }
              }}
            >
              Aktuellen Stand laden
            </button>
          </div>
        ) : null}
      </section>

      <fieldset disabled={locked} className="min-w-0 space-y-5 disabled:opacity-60">
        <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1.45fr)_minmax(360px,0.75fr)]">
          <section className={panel}>
            <SectionTitle
              icon={<UserRound className="h-5 w-5 text-on-gold" />}
              eyebrow="Vorstellung"
              title="Erzähl, wer du bist"
              description="Deine Überschrift ist der erste Eindruck. Im Text darunter kannst du erklären, was Zuschauer bei dir erwartet."
            />

            <div className="mt-5 space-y-4">
              <label className="block space-y-1.5 text-sm font-semibold text-white">
                Profilüberschrift
                <input
                  className={field}
                  maxLength={120}
                  placeholder="Deadlock, gute Gespräche und eine Runde mehr"
                  value={profile.headline}
                  onChange={event => change({ headline: event.target.value })}
                />
                <span className="block text-xs font-normal text-text-secondary">{profile.headline.length} / 120 Zeichen</span>
              </label>

              <label className="block space-y-1.5 text-sm font-semibold text-white">
                Über mich und meinen Stream
                <textarea
                  className={`${field} min-h-44 resize-y`}
                  maxLength={4000}
                  placeholder="Was spielst du am liebsten? Wie ist die Stimmung bei dir? Was macht deinen Stream aus?"
                  value={profile.about}
                  onChange={event => change({ about: event.target.value })}
                />
                <span className="block text-xs font-normal text-text-secondary">{profile.about.length} / 4000 Zeichen</span>
              </label>

              <div className="grid gap-4 md:grid-cols-2">
                <label className="block space-y-1.5 text-sm font-semibold text-white">
                  Akzentfarbe
                  <select
                    className={field}
                    value={profile.accent}
                    onChange={event => change({ accent: event.target.value as ProfileContent['accent'] })}
                  >
                    <option value="gold">Community-Gold</option>
                    <option value="violet">Violett</option>
                    <option value="teal">Petrol</option>
                  </select>
                </label>

                <label className="soft-elevate flex cursor-pointer items-start gap-3 rounded-xl border border-border bg-background/60 p-3.5">
                  <input
                    className="mt-0.5 h-5 w-5 accent-primary"
                    type="checkbox"
                    checked={profile.show_history}
                    onChange={event => change({ show_history: event.target.checked })}
                  />
                  <span>
                    <span className="block text-sm font-semibold text-white">Livezeiten zeigen</span>
                    <span className="mt-0.5 block text-xs leading-relaxed text-text-secondary">
                      Zeigt deine erfasste Streamhistorie ohne Zuschauerzahlen, Chats oder private Analysen.
                    </span>
                  </span>
                </label>
              </div>
            </div>
          </section>

          <section className={`${panel} xl:sticky xl:top-20`}>
            <SectionTitle
              icon={<Eye className="h-5 w-5 text-on-gold" />}
              eyebrow="Live-Vorschau"
              title="So wirkst du nach außen"
              description="Die Vorschau aktualisiert sich direkt beim Tippen."
            />
            <div className="panel-inset mt-5 overflow-hidden rounded-2xl p-4">
              <div className="flex items-center gap-3">
                <Avatar url={effectiveAvatar} login={data.login} size="small" />
                <div className="min-w-0">
                  <p className="truncate text-base font-bold text-white">@{data.login}</p>
                  <p className="text-xs text-text-secondary">Deadlock Partner</p>
                </div>
              </div>
              <div className="mt-4 border-t border-border/70 pt-4">
                <h3 className="display-font text-lg font-bold text-white">
                  {profile.headline.trim() || 'Deine Profilüberschrift'}
                </h3>
                <p className="mt-2 whitespace-pre-wrap text-sm leading-relaxed text-text-secondary">
                  {profile.about.trim() || 'Hier erscheint deine Vorstellung. Erzähl kurz, was deinen Stream und deine Community ausmacht.'}
                </p>
              </div>
              <div className="mt-4 flex flex-wrap gap-2 text-xs">
                <span className="rounded-full border border-border bg-card px-2.5 py-1 text-text-secondary">
                  {profile.socials.length} Links
                </span>
                <span className="rounded-full border border-border bg-card px-2.5 py-1 text-text-secondary">
                  {profile.featured.filter(value => value.trim()).length} Empfehlungen
                </span>
                <span className="rounded-full border border-border bg-card px-2.5 py-1 text-text-secondary">
                  {profile.events.length} Termine
                </span>
              </div>
            </div>
            <div className="mt-4 flex items-start gap-2 rounded-xl border border-primary/20 bg-primary/5 p-3 text-xs leading-relaxed text-text-secondary">
              <Palette className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
              Das Profilbild kommt direkt von Twitch. Die Akzentfarbe steuerst du hier im Dashboard.
            </div>
          </section>
        </div>

        <div className="grid items-start gap-5 xl:grid-cols-2">
          <section className={panel}>
            <SectionTitle
              icon={<Link2 className="h-5 w-5 text-on-gold" />}
              eyebrow="Links"
              title="Hier findet man dich"
              description="Dein Twitch-Kanal gehört automatisch zum Profil. Ergänze die Plattformen, auf denen du noch aktiv bist."
              aside={<span className="rounded-full border border-border bg-background/60 px-2.5 py-1 text-xs text-text-secondary">{profile.socials.length} / 12</span>}
            />
            <div className="mt-5 space-y-3">
              {profile.socials.map((social, index) => (
                <div key={index} className="panel-inset rounded-xl p-3.5">
                  <div className="grid gap-3 sm:grid-cols-[minmax(140px,0.4fr)_minmax(0,1fr)_auto] sm:items-end">
                    <label className="block space-y-1.5 text-xs font-semibold text-text-secondary">
                      Plattform
                      <input
                        aria-label={`Link ${index + 1}: Name`}
                        className={field}
                        maxLength={40}
                        placeholder="YouTube"
                        value={social.label}
                        onChange={event =>
                          change({
                            socials: profile.socials.map((value, current) =>
                              current === index ? { ...value, label: event.target.value } : value,
                            ),
                          })
                        }
                      />
                    </label>
                    <label className="block space-y-1.5 text-xs font-semibold text-text-secondary">
                      Adresse
                      <input
                        aria-label={`Link ${index + 1}: Adresse`}
                        className={field}
                        type="url"
                        maxLength={2048}
                        value={social.url}
                        placeholder="https://…"
                        onChange={event =>
                          change({
                            socials: profile.socials.map((value, current) =>
                              current === index ? { ...value, url: event.target.value } : value,
                            ),
                          })
                        }
                      />
                    </label>
                    <button
                      className="min-h-10 rounded-xl px-2 text-xs font-semibold text-text-secondary hover:text-white"
                      type="button"
                      onClick={() => change({ socials: profile.socials.filter((_, current) => current !== index) })}
                    >
                      Entfernen
                    </button>
                  </div>
                </div>
              ))}
              {profile.socials.length === 0 ? (
                <div className="rounded-xl border border-dashed border-border bg-background/35 px-4 py-5 text-sm text-text-secondary">
                  Noch keine zusätzlichen Links eingetragen.
                </div>
              ) : null}
              <button
                className={secondaryButton}
                type="button"
                disabled={profile.socials.length >= 12}
                onClick={() => change({ socials: [...profile.socials, { label: '', url: '' }] })}
              >
                <Plus className="h-4 w-4" />
                Link hinzufügen
              </button>
            </div>
          </section>

          <section className={panel}>
            <SectionTitle
              icon={<UsersRound className="h-5 w-5 text-on-gold" />}
              eyebrow="Partnernetzwerk"
              title="Leute aus deinem Umfeld"
              description="Empfiehl Streamer, mit denen du gerne spielst oder zusammen Content machst. Sichtbar werden aktive Partner mit veröffentlichtem Profil."
              aside={<span className="rounded-full border border-border bg-background/60 px-2.5 py-1 text-xs text-text-secondary">{profile.featured.filter(value => value.trim()).length} / 6</span>}
            />
            <div className="mt-5">
              <label className="block space-y-1.5 text-sm font-semibold text-white">
                Twitch-Namen
                <input
                  className={field}
                  value={profile.featured.join(', ')}
                  placeholder="partner_eins, partner_zwei"
                  onChange={event =>
                    change({
                      featured: event.target.value
                        .split(',')
                        .map(value => value.trim().replace(/^@/, '')),
                    })
                  }
                />
                <span className="block text-xs font-normal leading-relaxed text-text-secondary">
                  Mehrere Namen mit Komma trennen. Das @ musst du nicht mitschreiben.
                </span>
              </label>
              <div className="panel-inset mt-4 rounded-xl p-4">
                <div className="flex items-start gap-3">
                  <Sparkles className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
                  <p className="text-xs leading-relaxed text-text-secondary">
                    Diese Empfehlungen helfen Besuchern, dein Umfeld zu entdecken und weitere Streamer aus dem Partnernetzwerk kennenzulernen.
                  </p>
                </div>
              </div>
            </div>
          </section>
        </div>

        <section className={panel}>
          <SectionTitle
            icon={<CalendarDays className="h-5 w-5 text-on-gold" />}
            eyebrow="Streamplan"
            title="Dein Kalender"
            description="Plane Streams, Community-Abende oder besondere Termine. Alle Uhrzeiten gelten für Berlin."
            aside={
              <label className="block text-xs font-semibold text-text-secondary">
                Monat
                <input
                  aria-label="Kalendermonat"
                  type="month"
                  min="2000-01"
                  max="2100-12"
                  className={`${field} mt-1 min-w-44`}
                  value={month}
                  onChange={event => setMonth(event.target.value)}
                />
              </label>
            }
          />

          <div className="mt-5 overflow-x-auto pb-2">
            <div className="grid min-w-[680px] grid-cols-7 gap-2">
              {['Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa', 'So'].map(day => (
                <span key={day} className="px-2 py-1 text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary">
                  {day}
                </span>
              ))}
              {Array.from({ length: calendar.leading }, (_, index) => <span key={`empty-${index}`} />)}
              {calendar.days.map(day => {
                const dayEvents = profile.events.filter(event =>
                  berlinInput(event.starts_at).slice(0, 10) <= day &&
                  berlinInput(new Date(Date.parse(event.ends_at) - 1).toISOString()).slice(0, 10) >= day,
                );
                return (
                  <button
                    type="button"
                    key={day}
                    aria-label={`Termin am ${day} eintragen`}
                    onClick={() => {
                      if (!draft || window.confirm('Den noch nicht übernommenen Termineintrag verwerfen?')) {
                        setDraft(emptyEvent(day));
                        setEventError('');
                      }
                    }}
                    className="group min-h-24 rounded-xl border border-border bg-background/45 p-2.5 text-left transition-colors hover:border-primary/45 hover:bg-background/70 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary"
                  >
                    <span className="text-xs font-semibold text-text-secondary group-hover:text-white">
                      {Number(day.slice(-2))}
                    </span>
                    {dayEvents.map(event => (
                      <span
                        key={event.id}
                        className="mt-1.5 block truncate rounded-lg border border-primary/20 bg-primary/10 px-2 py-1 text-[10px] font-semibold text-primary"
                        title={event.title}
                      >
                        {event.title}
                      </span>
                    ))}
                  </button>
                );
              })}
            </div>
          </div>

          {draft ? (
            <div className="mt-4 rounded-2xl border border-primary/30 bg-primary/5 p-4 md:p-5">
              <div className="mb-4 flex items-center justify-between gap-3">
                <div>
                  <div className="text-[10px] font-bold uppercase tracking-[0.16em] text-primary">Kalendereintrag</div>
                  <h3 className="mt-0.5 text-base font-bold text-white">{draft.id ? 'Termin bearbeiten' : 'Neuer Termin'}</h3>
                </div>
              </div>
              <div className="space-y-4">
                <label className="block space-y-1.5 text-sm font-semibold text-white">
                  Titel
                  <input
                    className={field}
                    maxLength={120}
                    value={draft.title}
                    onChange={event => setDraft({ ...draft, title: event.target.value })}
                  />
                </label>
                <div className="grid gap-4 sm:grid-cols-2">
                  <label className="block space-y-1.5 text-sm font-semibold text-white">
                    Beginn, Berlin
                    <input
                      type="datetime-local"
                      className={field}
                      min="2000-01-01T00:00"
                      max="2100-12-31T23:59"
                      value={draft.start}
                      onChange={event => setDraft({ ...draft, start: event.target.value })}
                    />
                  </label>
                  <label className="block space-y-1.5 text-sm font-semibold text-white">
                    Ende, Berlin
                    <input
                      type="datetime-local"
                      className={field}
                      min="2000-01-01T00:00"
                      max="2100-12-31T23:59"
                      value={draft.end}
                      onChange={event => setDraft({ ...draft, end: event.target.value })}
                    />
                  </label>
                </div>
                <label className="block space-y-1.5 text-sm font-semibold text-white">
                  Beschreibung
                  <textarea
                    className={`${field} min-h-24 resize-y`}
                    maxLength={1000}
                    value={draft.description}
                    onChange={event => setDraft({ ...draft, description: event.target.value })}
                  />
                </label>
                {eventError ? <p role="alert" className="text-sm text-warning">{eventError}</p> : null}
                <div className="flex flex-wrap gap-2">
                  <button type="button" className={primaryButton} onClick={addEvent}>Termin übernehmen</button>
                  <button
                    type="button"
                    className={secondaryButton}
                    onClick={() => {
                      setDraft(null);
                      setEventError('');
                    }}
                  >
                    Abbrechen
                  </button>
                </div>
                <p className="text-xs text-text-secondary">
                  Termin übernehmen aktualisiert zuerst deinen Entwurf. Gespeichert wird er mit dem Button oben.
                </p>
              </div>
            </div>
          ) : null}

          <div className="mt-4 space-y-2">
            {profile.events
              .filter(event =>
                berlinInput(event.starts_at).slice(0, 7) <= month &&
                berlinInput(new Date(Date.parse(event.ends_at) - 1).toISOString()).slice(0, 7) >= month,
              )
              .map(event => (
                <div
                  key={event.id}
                  className="panel-inset flex flex-wrap items-center justify-between gap-3 rounded-xl px-4 py-3"
                >
                  <div className="min-w-0">
                    <h3 className="truncate text-sm font-bold text-white">{event.title}</h3>
                    <p className="mt-0.5 text-xs text-text-secondary">
                      {berlinInput(event.starts_at).replace('T', ' · ')} bis {berlinInput(event.ends_at).replace('T', ' · ')} Uhr, Berlin
                    </p>
                  </div>
                  <div className="flex shrink-0 gap-2">
                    <button type="button" className={secondaryButton} onClick={() => openEvent(event)}>Bearbeiten</button>
                    <button
                      type="button"
                      className="min-h-10 rounded-xl px-3 text-xs font-semibold text-text-secondary hover:text-white"
                      onClick={() => {
                        if (window.confirm(`„${event.title}“ aus deinem Kalender entfernen?`)) {
                          change({ events: profile.events.filter(current => current.id !== event.id) });
                          if (draft?.id === event.id) setDraft(null);
                        }
                      }}
                    >
                      Entfernen
                    </button>
                  </div>
                </div>
              ))}
          </div>

          {profile.events.length === 0 ? (
            <div className="mt-4 rounded-xl border border-dashed border-border bg-background/35 px-4 py-5 text-sm text-text-secondary">
              Noch keine Termine. Wähle oben deinen ersten Streamtag aus.
            </div>
          ) : null}
          <p className="mt-4 text-xs text-text-secondary">
            {profile.events.length} / 200 Termine. Keine automatische Löschung alter Termine. Erfasste Livestreams werden separat angezeigt und lassen sich hier nicht verändern.
          </p>
        </section>
      </fieldset>
    </div>
  );
}

export function PartnerProfile({
  streamer,
  twitchAvatarUrl,
}: {
  streamer?: string;
  twitchAvatarUrl?: string | null;
}) {
  const { isDemoMode } = usePlan();
  const demo = isDemoMode || isPreviewModeEnabled();
  const [generation, setGeneration] = useState(0);
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: ['partner-profile', streamer, generation],
    queryFn: () => fetchPartnerProfile(streamer),
    enabled: !demo,
    retry: false,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  });

  if (demo) {
    return (
      <div className={panel}>
        <h1 className="text-xl font-semibold">Mein Profil</h1>
        <p>Die Demo liest oder verändert keine persönlichen Partnerprofile. Melde dich mit deinem Twitch-Konto an, um dein Profil zu gestalten.</p>
      </div>
    );
  }
  if (query.isPending) return <p role="status" className={panel}>Dein Profil wird geladen …</p>;
  if (query.error || !query.data) {
    return (
      <div role="alert" className={panel}>
        <p>{query.error ? message(query.error) : 'Dein Profil konnte nicht geladen werden.'}</p>
        <button type="button" className={secondaryButton} onClick={() => void query.refetch()}>Erneut versuchen</button>
      </div>
    );
  }
  return (
    <ProfileEditor
      key={`${streamer || 'own'}-${generation}`}
      initial={query.data}
      streamer={streamer}
      twitchAvatarUrl={twitchAvatarUrl}
      onReload={() => setGeneration(value => value + 1)}
      onSaved={saved => queryClient.setQueryData(['partner-profile', streamer, generation], saved)}
    />
  );
}
