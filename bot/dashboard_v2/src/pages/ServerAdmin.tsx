// Server-Admin-Dashboard (Multi-Server Paket A, Issue #1064).
//
// Eigenes Panel fuer Admins fremder Discord-Server ("Server verwalten"). Es
// nutzt Shell, Karten und Stil des Partner-Dashboards, aber weder dessen
// Sidebar noch Profil-/Admin-Hooks: kein Admin-Modus, keine Partner-Seiten,
// keine globalen Werkzeuge. Alle Rechte prueft der Server.
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowRight,
  Ban,
  Globe2,
  Hash,
  Loader2,
  Lock,
  LogIn,
  Server,
  Swords,
  UserCheck,
  UserX,
  Users,
} from 'lucide-react';
import { Rise } from '@/motion/Rise';
import { KpiCard } from '@/components/cards/KpiCard';
import { DashboardShell } from '@/components/layout/DashboardShell';
import { ServerAdminSidebar } from '@/components/layout/ServerAdminSidebar';
import {
  SERVER_ADMIN_LOGIN_URL,
  SERVER_ADMIN_SECTIONS,
  ServerAdminLoginRequired,
  errorText,
  fetchGuildStreamers,
  fetchServerAdminMe,
  isSnowflakeOrEmpty,
  patchGuildSettings,
  setStreamerActive,
  type GuildSettings,
  type GuildSettingsPatch,
  type GuildStreamer,
  type ServerAdminMe,
  type ServerAdminSection,
} from '@/api/serverAdmin';

const ME_KEY = ['server-admin', 'me'] as const;

const LABEL = 'text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary';
const OUTLINE_BUTTON =
  'inline-flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover disabled:cursor-not-allowed disabled:opacity-70';
const GOLD_BUTTON =
  'gradient-accent inline-flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-bold no-underline shadow-lg shadow-primary/20 transition-[transform,translate,scale] hover:-translate-y-0.5 disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:translate-y-0';
const INPUT =
  'w-full rounded-xl border border-border bg-background/60 px-3 py-2 text-sm text-white placeholder:text-text-secondary/60 focus:border-primary/50 focus:outline-none';

/** Bereiche, die zentral verwaltet werden und hier gesperrt sind (PLAN.md 4.5). */
const LOCKED_AREAS = ['Tags', 'Globale Bans', 'Blacklist', 'Abrechnung', 'KI-Funktionen'] as const;

function sectionFromHash(hash: string): ServerAdminSection {
  const value = hash.replace(/^#/, '') as ServerAdminSection;
  return SERVER_ADMIN_SECTIONS.includes(value) ? value : 'overview';
}

function useActiveSection(): ServerAdminSection {
  const [section, setSection] = useState<ServerAdminSection>(() => sectionFromHash(window.location.hash));
  useEffect(() => {
    const onHash = () => setSection(sectionFromHash(window.location.hash));
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);
  return section;
}

function Card({ id, eyebrow, title, icon: Icon, children, step = 0 }: {
  id?: string;
  eyebrow: string;
  title: string;
  icon: typeof Users;
  children: ReactNode;
  step?: number;
}) {
  return (
    <Rise step={step} as="section" id={id} className="panel-card card-glow card-glow-soft scroll-mt-5 rounded-2xl p-5">
      <div className="mb-5 flex items-center gap-3">
        <div className="gradient-accent sidebar-avatar-glow flex h-9 w-9 items-center justify-center rounded-xl">
          <Icon className="h-4 w-4 text-on-gold" />
        </div>
        <div>
          <div className={LABEL}>{eyebrow}</div>
          <h2 className="text-lg font-semibold text-white">{title}</h2>
        </div>
      </div>
      {children}
    </Rise>
  );
}

function Toggle({ checked, onChange, label, hint, disabled }: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  hint: string;
  disabled?: boolean;
}) {
  return (
    <label className="flex cursor-pointer items-start justify-between gap-4 rounded-xl border border-border bg-background/60 px-4 py-3">
      <span className="min-w-0">
        <span className="block text-sm font-semibold text-white">{label}</span>
        <span className="mt-0.5 block text-xs text-text-secondary">{hint}</span>
      </span>
      <input
        type="checkbox"
        role="switch"
        className="mt-1 h-4 w-4 shrink-0 accent-[var(--color-primary)]"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
    </label>
  );
}

function StatusLine({ error, saved }: { error: unknown; saved: boolean }) {
  if (error) return <p className="text-sm text-danger">{errorText(error)}</p>;
  if (saved) return <p className="text-sm text-success">Gespeichert.</p>;
  return null;
}

/** Einstellungs-Formular einer Karte: schickt nur die eigenen Felder. */
function useSettingsForm<K extends keyof GuildSettingsPatch>(
  guild: GuildSettings,
  csrfToken: string,
  fields: readonly K[],
) {
  const queryClient = useQueryClient();
  const initial = useMemo(() => {
    const values = {} as Pick<GuildSettings, K>;
    for (const field of fields) values[field] = guild[field];
    return values;
  }, [guild, fields]);
  const [draft, setDraft] = useState(initial);
  useEffect(() => setDraft(initial), [initial]);
  const dirty = fields.some((field) => draft[field] !== initial[field]);

  const mutation = useMutation({
    mutationFn: () => {
      const patch: GuildSettingsPatch = {};
      for (const field of fields) {
        if (draft[field] !== initial[field]) Object.assign(patch, { [field]: draft[field] });
      }
      return patchGuildSettings(guild.guildId, patch, csrfToken);
    },
    onSuccess: (updated) => {
      queryClient.setQueryData<ServerAdminMe>(ME_KEY, (me) =>
        me ? { ...me, guilds: me.guilds.map((g) => (g.guildId === updated.guildId ? updated : g)) } : me,
      );
    },
  });

  const set = <F extends K>(field: F, value: GuildSettings[F]) => {
    mutation.reset();
    setDraft((current) => ({ ...current, [field]: value }));
  };

  return { draft, set, dirty, mutation };
}

function SaveRow({ dirty, invalid, mutation }: {
  dirty: boolean;
  invalid?: boolean;
  mutation: { mutate: () => void; isPending: boolean; isSuccess: boolean; error: unknown };
}) {
  return (
    <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
      <StatusLine error={mutation.error} saved={mutation.isSuccess && !dirty} />
      <button
        type="button"
        className={`${GOLD_BUTTON} ml-auto`}
        disabled={!dirty || invalid || mutation.isPending}
        onClick={() => mutation.mutate()}
      >
        {mutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
        Speichern
      </button>
    </div>
  );
}

const CHANNEL_FIELDS = ['announceChannelId', 'alertChannelId', 'streamerRoleId', 'pingRoleId'] as const;

function ChannelsCard({ guild, csrfToken }: { guild: GuildSettings; csrfToken: string }) {
  const { draft, set, dirty, mutation } = useSettingsForm(guild, csrfToken, CHANNEL_FIELDS);
  const rows: Array<{ field: (typeof CHANNEL_FIELDS)[number]; label: string; hint: string }> = [
    { field: 'announceChannelId', label: 'Ankündigungs-Kanal', hint: 'Hier postet der Bot, wenn ein Streamer live geht.' },
    { field: 'alertChannelId', label: 'Hinweis-Kanal', hint: 'Interne Hinweise für dein Team.' },
    { field: 'streamerRoleId', label: 'Streamer-Rolle', hint: 'Rolle für aktive Streamer deines Servers.' },
    { field: 'pingRoleId', label: 'Ping-Rolle', hint: 'Wird bei Live-Ankündigungen erwähnt.' },
  ];
  const invalid = rows.some(({ field }) => !isSnowflakeOrEmpty(draft[field] ?? ''));
  return (
    <Card id="channels" eyebrow="Kanäle & Rollen" title="Wohin der Bot postet" icon={Hash} step={3}>
      <div className="grid gap-4 md:grid-cols-2">
        {rows.map(({ field, label, hint }) => {
          const value = draft[field] ?? '';
          const bad = !isSnowflakeOrEmpty(value);
          return (
            <label key={field} className="block space-y-1.5">
              <span className={LABEL}>{label}</span>
              <input
                className={`${INPUT} ${bad ? 'border-danger/60' : ''}`}
                inputMode="numeric"
                placeholder="Discord-ID, z. B. 1289721245281292288"
                value={value}
                onChange={(event) => set(field, event.target.value.trim() === '' ? null : event.target.value.trim())}
              />
              <span className={`block text-xs ${bad ? 'text-danger' : 'text-text-secondary'}`}>
                {bad ? 'Bitte eine gültige Discord-ID eintragen oder leer lassen.' : hint}
              </span>
            </label>
          );
        })}
      </div>
      <SaveRow dirty={dirty} invalid={invalid} mutation={mutation} />
    </Card>
  );
}

const LANGUAGE_FIELDS = ['language', 'promoInviteUrl', 'autoPartnerDefault'] as const;

function LanguageCard({ guild, csrfToken }: { guild: GuildSettings; csrfToken: string }) {
  const { draft, set, dirty, mutation } = useSettingsForm(guild, csrfToken, LANGUAGE_FIELDS);
  return (
    <Card id="language" eyebrow="Sprache & Einladung" title="Auftritt deines Servers" icon={Globe2} step={4}>
      <div className="grid gap-4 md:grid-cols-2">
        <label className="block space-y-1.5">
          <span className={LABEL}>Sprache</span>
          <select
            className={INPUT}
            value={draft.language}
            onChange={(event) => set('language', event.target.value === 'de' ? 'de' : 'en')}
          >
            <option value="de">Deutsch</option>
            <option value="en">English</option>
          </select>
          <span className="block text-xs text-text-secondary">Sprache der Bot-Nachrichten auf deinem Server.</span>
        </label>
        <label className="block space-y-1.5">
          <span className={LABEL}>Einladungslink</span>
          <input
            className={INPUT}
            placeholder="https://discord.gg/deinserver"
            value={draft.promoInviteUrl ?? ''}
            onChange={(event) => set('promoInviteUrl', event.target.value.trim() === '' ? null : event.target.value)}
          />
          <span className="block text-xs text-text-secondary">Nur Discord-Einladungen (discord.gg/…).</span>
        </label>
      </div>
      <div className="mt-4">
        <Toggle
          checked={draft.autoPartnerDefault}
          onChange={(next) => set('autoPartnerDefault', next)}
          label="Neue Streamer automatisch aktivieren"
          hint="Standard für Streamer, die sich über deinen Server anmelden."
        />
      </div>
      <SaveRow dirty={dirty} mutation={mutation} />
    </Card>
  );
}

const RAID_FIELDS = ['crossServerRaids'] as const;

function RaidsCard({ guild, csrfToken }: { guild: GuildSettings; csrfToken: string }) {
  const { draft, set, dirty, mutation } = useSettingsForm(guild, csrfToken, RAID_FIELDS);
  return (
    <Card id="raids" eyebrow="Raids" title="Raids über Servergrenzen" icon={Swords} step={5}>
      <Toggle
        checked={draft.crossServerRaids}
        onChange={(next) => set('crossServerRaids', next)}
        label="Raids zu Streamern anderer Server erlauben"
        hint="Aus: Deine Streamer raiden nur innerhalb deines Servers."
      />
      <SaveRow dirty={dirty} mutation={mutation} />
    </Card>
  );
}

function StreamerRow({ guildId, streamer, csrfToken }: { guildId: string; streamer: GuildStreamer; csrfToken: string }) {
  const queryClient = useQueryClient();
  const mutation = useMutation({
    mutationFn: (active: boolean) => setStreamerActive(guildId, streamer.twitchUserId, active, csrfToken),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['server-admin', 'streamers', guildId] });
      void queryClient.invalidateQueries({ queryKey: ME_KEY });
    },
  });
  const name = streamer.twitchLogin ?? `ID ${streamer.twitchUserId}`;
  return (
    <li className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-background/60 px-4 py-3">
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold text-white">{name}</div>
        <div className="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-text-secondary">
          <span
            className={`inline-flex items-center rounded-full border px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] ${
              streamer.active ? 'border-success/40 bg-success/10 text-success' : 'border-border bg-background/60 text-text-secondary'
            }`}
          >
            {streamer.active ? 'Aktiv' : 'Deaktiviert'}
          </span>
          {streamer.joinedAt ? <span>seit {new Date(streamer.joinedAt).toLocaleDateString('de-DE')}</span> : null}
        </div>
        {mutation.error ? <p className="mt-1 text-xs text-danger">{errorText(mutation.error)}</p> : null}
      </div>
      <button
        type="button"
        className={OUTLINE_BUTTON}
        disabled={mutation.isPending}
        onClick={() => mutation.mutate(!streamer.active)}
      >
        {mutation.isPending ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : streamer.active ? (
          <UserX className="h-4 w-4" />
        ) : (
          <UserCheck className="h-4 w-4" />
        )}
        {streamer.active ? 'Deaktivieren' : 'Aktivieren'}
      </button>
    </li>
  );
}

function StreamersCard({ guild, csrfToken }: { guild: GuildSettings; csrfToken: string }) {
  const { data, isLoading, error } = useQuery({
    queryKey: ['server-admin', 'streamers', guild.guildId],
    queryFn: () => fetchGuildStreamers(guild.guildId),
  });
  const streamers = data?.streamers ?? [];
  return (
    <Card id="streamers" eyebrow="Streamer" title="Streamer deines Servers" icon={Users} step={2}>
      <p className="mb-4 text-sm text-text-secondary">
        Deaktivieren wirkt nur auf deinem Server. Auf anderen Servern und im restlichen Netzwerk bleibt der Streamer unverändert.
      </p>
      {isLoading ? (
        <div className="flex items-center gap-2 text-sm text-text-secondary">
          <Loader2 className="h-4 w-4 animate-spin" /> Lade Streamer…
        </div>
      ) : error ? (
        <p className="text-sm text-danger">{errorText(error)}</p>
      ) : streamers.length === 0 ? (
        <p className="text-sm text-text-secondary">Auf diesem Server sind noch keine Streamer angemeldet.</p>
      ) : (
        <ul className="space-y-2">
          {streamers.map((streamer) => (
            <StreamerRow key={streamer.twitchUserId} guildId={guild.guildId} streamer={streamer} csrfToken={csrfToken} />
          ))}
        </ul>
      )}
    </Card>
  );
}

function LockedAreasCard() {
  return (
    <Card eyebrow="Zentral verwaltet" title="Nicht in deinem Panel" icon={Lock} step={6}>
      <p className="mb-4 text-sm text-text-secondary">
        Diese Bereiche gelten für das ganze Netzwerk und werden zentral gepflegt. Sie sind hier gesperrt.
      </p>
      <ul className="flex flex-wrap gap-2">
        {LOCKED_AREAS.map((area) => (
          <li
            key={area}
            className="inline-flex items-center gap-1.5 rounded-full border border-border bg-background/60 px-3 py-1 text-xs font-semibold text-text-secondary"
          >
            <Ban className="h-3 w-3" />
            {area}
          </li>
        ))}
      </ul>
    </Card>
  );
}

function CenteredCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="internal-home-vibe relative flex min-h-screen items-center justify-center px-4 py-10">
      <Rise as="section" className="panel-card card-glow card-glow-accent hero-aura w-full max-w-xl rounded-2xl p-6 md:p-8">
        <div className={LABEL}>Server-Admin</div>
        <h1 className="display-font mt-1 text-2xl font-bold text-white md:text-[2rem]">{title}</h1>
        <div className="mt-3 space-y-4 text-sm text-text-secondary">{children}</div>
      </Rise>
    </div>
  );
}

function LoginCard() {
  return (
    <CenteredCard title="Server verwalten">
      <p>
        Melde dich mit Discord an. Du siehst danach die Server, auf denen du das Recht „Server verwalten“ hast und auf denen der Bot installiert ist.
      </p>
      <a href={SERVER_ADMIN_LOGIN_URL} className={GOLD_BUTTON}>
        <LogIn className="h-4 w-4" />
        Mit Discord anmelden
      </a>
    </CenteredCard>
  );
}

export function ServerAdminPage() {
  const section = useActiveSection();
  const [selectedGuildId, setSelectedGuildId] = useState<string | null>(null);
  const { data: me, error, isLoading, isFetching, refetch } = useQuery({
    queryKey: ME_KEY,
    queryFn: fetchServerAdminMe,
  });

  if (isLoading) {
    return (
      <CenteredCard title="Server verwalten">
        <span className="inline-flex items-center gap-2">
          <Loader2 className="h-4 w-4 animate-spin" /> Lade…
        </span>
      </CenteredCard>
    );
  }
  if (error instanceof ServerAdminLoginRequired || (!me && !error)) return <LoginCard />;
  if (error || !me) {
    return (
      <CenteredCard title="Server verwalten">
        <p className="text-danger">{errorText(error)}</p>
        <button type="button" className={OUTLINE_BUTTON} onClick={() => void refetch()}>
          Erneut versuchen
        </button>
      </CenteredCard>
    );
  }
  if (me.guilds.length === 0) {
    return (
      <CenteredCard title="Kein Server gefunden">
        <p>
          Auf keinem deiner Server mit „Server verwalten“ ist der Bot installiert. Lade den Bot auf deinen Server ein und melde dich erneut an.
        </p>
        <a href={SERVER_ADMIN_LOGIN_URL} className={OUTLINE_BUTTON}>
          <LogIn className="h-4 w-4" />
          Erneut anmelden
        </a>
      </CenteredCard>
    );
  }

  const guild = me.guilds.find((g) => g.guildId === selectedGuildId) ?? me.guilds[0];

  return (
    <DashboardShell
      sidebar={
        <ServerAdminSidebar
          displayName={me.displayName}
          guilds={me.guilds}
          activeGuildId={guild.guildId}
          onGuildChange={setSelectedGuildId}
          activeSection={section}
        />
      }
    >
      <Rise
        step={{ seconds: 0.04 }}
        as="section"
        id="overview"
        className="panel-card card-glow card-glow-accent hero-aura flex scroll-mt-5 flex-col gap-4 rounded-2xl px-5 py-4 md:flex-row md:items-center md:justify-between"
      >
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <div className="text-[11px] font-semibold uppercase tracking-[0.22em] text-text-secondary">Server-Admin</div>
            <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-background/60 px-2.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
              <span className="h-2 w-2 rounded-full bg-success" />
              Bot aktiv · {guild.language === 'de' ? 'Deutsch' : 'English'}
            </span>
          </div>
          <h1 className="display-font mt-1 text-2xl font-bold text-white md:text-[2rem]">{guild.name || `Server ${guild.guildId}`}</h1>
          <p className="mt-1 text-sm text-text-secondary">Streamer-Partner deines Servers auf einen Blick</p>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <button type="button" onClick={() => void refetch()} disabled={isFetching} className={OUTLINE_BUTTON}>
            {isFetching ? <Loader2 className="h-4 w-4 animate-spin" /> : <ArrowRight className="h-4 w-4" />}
            Neu laden
          </button>
          <a href="#streamers" className={GOLD_BUTTON}>
            Streamer verwalten
            <ArrowRight className="h-4 w-4" />
          </a>
        </div>
      </Rise>

      <Rise step={{ seconds: 0.08 }} as="section" className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <KpiCard title="Aktive Streamer" value={guild.activeStreamers} icon={UserCheck} color="green" />
        <KpiCard title="Deaktiviert" value={guild.inactiveStreamers} icon={UserX} color="yellow" />
        <KpiCard
          title="Auto-Partner"
          value={guild.autoPartnerDefault ? 'An' : 'Aus'}
          subValue="Standard für neue Streamer"
          icon={Server}
          color="blue"
        />
        <KpiCard
          title="Server-Raids"
          value={guild.crossServerRaids ? 'An' : 'Aus'}
          subValue="Raids über Servergrenzen"
          icon={Swords}
          color="purple"
        />
      </Rise>

      {/* key: Formulare beim Serverwechsel frisch aufsetzen. */}
      <div key={guild.guildId} className="space-y-4 md:space-y-5">
        <StreamersCard guild={guild} csrfToken={me.csrfToken} />
        <ChannelsCard guild={guild} csrfToken={me.csrfToken} />
        <LanguageCard guild={guild} csrfToken={me.csrfToken} />
        <RaidsCard guild={guild} csrfToken={me.csrfToken} />
        <LockedAreasCard />
      </div>
    </DashboardShell>
  );
}
