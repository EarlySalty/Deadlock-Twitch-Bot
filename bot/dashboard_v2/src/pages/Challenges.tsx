import { useEffect, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Check,
  Copy,
  Link,
  Film,
  Search,
  Clock3,
  Crown,
  Flame,
  Medal,
  Snowflake,
  Sparkles,
  Trophy,
  UserPlus,
  Users,
  Zap,
} from 'lucide-react';
import {
  fetchChallengesMe,
  fetchNetworkAvatars,
  fetchChallengeViewers,
  fetchEffortLeaderboard,
  fetchViewerLeaderboard,
  type ChallengeAchievement,
  type ChallengeQuest,
} from '@/api/challenges';
import { ApiHttpError } from '@/api/httpError';

type LeaderboardTab = 'viewers' | 'effort';


const ACHIEVEMENT_NAMES: Record<string, string> = {
  recruiter: 'Werber', team_player: 'Teamspieler', duo: 'Duo',
  stamina: 'Ausdauer', talent_scout: 'Talentscout', clip_hunter: 'Clipjäger',
};
const ACHIEVEMENT_ICONS = {
  recruiter: UserPlus, team_player: Users, duo: Users,
  stamina: Flame, talent_scout: Search, clip_hunter: Film,
};

const ACHIEVEMENT_CONDITIONS: Record<string, string> = {
  recruiter: 'aktive Leute in den Discord bringen',
  team_player: 'Community-Matches spielen',
  duo: 'Stream-Together-Sessions schaffen',
  stamina: 'Wochen Streak erreichen',
  talent_scout: 'Partner empfehlen',
  clip_hunter: 'Top-3-Clips erreichen',
};

function progressPct(current: number, target: number) {
  if (!Number.isFinite(current) || !Number.isFinite(target) || target <= 0) return 0;
  return Math.max(0, Math.min(100, (current / target) * 100));
}

function ProgressBar({
  current,
  target,
  label,
}: {
  current: number;
  target: number;
  label: string;
}) {
  const pct = progressPct(current, target);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={target}
      aria-valuenow={Math.min(current, target)}
      className="h-2.5 overflow-hidden rounded-full border border-white/5 bg-black/35"
    >
      <div
        className="h-full rounded-full bg-gradient-to-r from-primary to-[#f1d299] transition-[width] duration-500"
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

function useMondayCountdown(nextResetAt?: string) {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  const remaining = nextResetAt ? Math.max(0, Date.parse(nextResetAt) - now) : 0;
  const totalMinutes = Math.floor(remaining / 60_000);
  const dayCount = Math.floor(totalMinutes / 1_440);
  const hours = Math.floor((totalMinutes % 1_440) / 60);
  const minutes = totalMinutes % 60;

  if (dayCount > 0) return `${dayCount} T ${hours} Std bis Montag`;
  if (hours > 0) return `${hours} Std ${minutes} Min bis Montag`;
  return `${minutes} Min bis Montag`;
}

function SectionTitle({ title, aside }: { title: string; aside?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <h2 className="display-font text-xl font-bold text-white md:text-2xl">{title}</h2>
      {aside}
    </div>
  );
}

function QuestCard({ quest }: { quest: ChallengeQuest }) {
  return (
    <article className="panel-card challenge-quest rounded-2xl border border-primary/25 p-5 md:p-6">
      <div className="flex items-start justify-between gap-3">
        <h3 className="font-semibold leading-snug text-white">{quest.text}</h3>
        <div
          className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full border ${
            quest.completed
              ? 'border-primary/30 bg-primary/10 text-primary'
              : 'border-primary/20 bg-primary/10 text-primary'
          }`}
        >
          {quest.completed ? <Check className="h-4 w-4" /> : <Sparkles className="h-4 w-4" />}
        </div>
      </div>
      <div className="mt-5">
        <div className="mb-2 flex items-center justify-between text-xs">
          <span className="text-text-secondary">{quest.data_complete === false ? 'Wertung ausgesetzt' : 'Fortschritt'}</span>
          <span className="font-semibold tabular-nums text-primary">
            {Math.min(quest.progress, quest.goal)} / {quest.goal}
          </span>
        </div>
        <ProgressBar current={quest.progress} target={quest.goal} label={quest.text} />
      </div>
    </article>
  );
}

function LeaderboardRow({
  rank,
  name,
  value,
  own,
  raidBoost,
  pinned = false,
  avatarUrl,
}: {
  rank: number;
  name: string;
  value: string;
  own: boolean;
  raidBoost?: boolean;
  pinned?: boolean;
  avatarUrl?: string | null;
}) {
  const first = rank === 1;
  const topThree = rank <= 3;
  const podium = rank === 2 ? 'border-[#e3d4b6]/20 bg-[#e3d4b6]/5' : 'border-[#d98a33]/20 bg-[#d98a33]/5';
  return (
    <div
      data-rank={rank}
      className={`grid grid-cols-[44px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl border px-3 py-3 md:px-4 ${
        own
          ? 'border-primary/50 bg-primary/15'
          : pinned
            ? 'border-primary/40 bg-primary/10'
            : first
              ? 'border-primary/20 bg-primary/5'
              : topThree ? podium : 'border-white/5 bg-black/15'
      }`}
    >
      <div className="flex items-center justify-center">
        {first ? (
          <Crown className="h-5 w-5 text-primary" aria-label="Platz 1" />
        ) : topThree ? (
          <Medal className={`h-5 w-5 ${rank === 2 ? 'text-[#e3d4b6]' : 'text-[#d98a33]'}`} aria-label={`Platz ${rank}`} />
        ) : (
          <span className={`text-sm font-bold tabular-nums ${own ? 'text-primary' : 'text-text-secondary'}`}>#{rank}</span>
        )}
      </div>
      <div className="flex min-w-0 items-center gap-3">
        <ChallengeAvatar name={name} url={avatarUrl} />
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <span className="truncate font-semibold text-white">{name}</span>
          {own ? (
            <span className="rounded-full border border-primary/40 bg-primary/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.14em] text-primary">
              DU
            </span>
          ) : null}
          {raidBoost ? (
            <span className="inline-flex items-center gap-1 rounded-full border border-primary/30 bg-primary/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-primary">
              <Zap className="h-3 w-3" />
              Raid Boost
            </span>
          ) : null}
        </div>
      </div>
      <div className={`text-right text-sm font-bold tabular-nums ${own ? 'text-primary' : 'text-white'}`}>{value}</div>
    </div>
  );
}

function ChallengeAvatar({ name, url }: { name: string; url?: string | null }) {
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  return url && url !== failedUrl ? (
    <img src={url} alt="" loading="lazy" onError={() => setFailedUrl(url)} className="h-10 w-10 shrink-0 rounded-full border border-white/15 object-cover" />
  ) : (
    <span aria-hidden="true" className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full border border-white/10 bg-white/5 text-sm font-bold text-text-secondary">{name[0]?.toLocaleUpperCase('de-DE') ?? '?'}</span>
  );
}

function AchievementCard({ achievement }: { achievement: ChallengeAchievement }) {
  const nextTier = achievement.tiers.findIndex(tier => !tier.unlocked);
  const targetTier = achievement.tiers[nextTier] ?? achievement.tiers.at(-1);
  const Icon = ACHIEVEMENT_ICONS[achievement.key as keyof typeof ACHIEVEMENT_ICONS] ?? Trophy;
  const name = ACHIEVEMENT_NAMES[achievement.key] ?? achievement.name;
  const complete = nextTier === -1;
  return (
    <article data-achievement={achievement.key} className="challenge-achievement rounded-2xl border border-white/10 bg-black/20 p-5">
      <div className="flex items-center gap-3">
        <span className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border ${complete ? 'border-primary/30 bg-primary/10 text-primary' : 'border-white/10 bg-white/5 text-text-secondary'}`}><Icon className="h-5 w-5" /></span>
        <div className="min-w-0"><h3 className="font-semibold text-white">{name}</h3><p className="mt-1 text-xs text-text-secondary">{ACHIEVEMENT_CONDITIONS[achievement.key]}</p></div>
      </div>
      <div className="mt-5 flex gap-2" aria-label={`Stufen für ${name}`}>
        {achievement.tiers.map((tier, index) => (
          <div key={tier.target} title={`Stufe ${index + 1}: ${tier.target}, ${tier.unlocked ? 'erreicht' : index === nextTier ? 'nächste Stufe' : 'noch gesperrt'}`} className={`flex flex-1 items-center justify-center gap-1.5 rounded-lg border py-2 text-xs font-semibold ${tier.unlocked ? 'border-primary/35 bg-primary/15 text-primary' : index === nextTier ? 'border-primary/35 text-primary' : 'border-white/5 bg-black/20 text-text-secondary/60'}`}>
            {tier.unlocked ? <Check className="h-3.5 w-3.5" /> : null}Stufe {index + 1}
          </div>
        ))}
      </div>
      <div className="mb-2 mt-4 flex justify-between gap-2 text-xs"><span className="text-text-secondary">{complete ? 'Alle Stufen erreicht' : `Nächste Stufe: ${nextTier + 1}`}</span><span className={`font-semibold tabular-nums ${complete ? 'text-primary' : 'text-white'}`}>{achievement.progress.toLocaleString('de-DE')} / {targetTier?.target.toLocaleString('de-DE') ?? 0}</span></div>
      <ProgressBar current={achievement.progress} target={targetTier?.target ?? 0} label={`Fortschritt für ${name}`} />
    </article>
  );
}

function LoadingState() {
  return (
    <div className="space-y-4">
      <div className="panel-card h-32 animate-pulse rounded-2xl border border-primary/20" />
      <div className="grid gap-4 md:grid-cols-3">
        {[0, 1, 2].map(item => (
          <div key={item} className="panel-card h-44 animate-pulse rounded-2xl border border-border" />
        ))}
      </div>
      <div className="panel-card h-72 animate-pulse rounded-2xl border border-border" />
    </div>
  );
}

export function Challenges() {
  const queryClient = useQueryClient();
  const [copyStatus, setCopyStatus] = useState<'idle' | 'copied' | 'error'>('idle');
  const [leaderboardTab, setLeaderboardTab] = useState<LeaderboardTab>('viewers');
  const me = useQuery({
    queryKey: ['challenges', 'me'],
    queryFn: fetchChallengesMe,
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const recruiters = useQuery({
    queryKey: ['challenges', 'viewers'],
    queryFn: fetchChallengeViewers,
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const viewers = useQuery({
    queryKey: ['leaderboard', 'viewers', 10],
    queryFn: () => fetchViewerLeaderboard(10),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const effort = useQuery({
    queryKey: ['leaderboard', 'effort'],
    queryFn: fetchEffortLeaderboard,
    staleTime: 60_000,
    refetchInterval: 60_000,
  });

  const network = useQuery({
    queryKey: ['public-network', 'avatars'], queryFn: fetchNetworkAvatars, staleTime: 300_000,
  });
  const avatarFor = (login: string, supplied?: string | null) => supplied ?? network.data?.streamers.find(streamer => streamer.login.toLowerCase() === login.toLowerCase())?.avatar_url;

  const countdown = useMondayCountdown(me.data?.next_reset_at);
  const quests = me.data?.quests ?? [];
  const isAssignmentPending = me.data?.quest_assignment_status === 'pending';
  const hasNoReachableQuests = me.data?.quest_assignment_status === 'no_reachable_quests';
  useEffect(() => {
    const resets = [me.data?.next_reset_at, me.data?.season.next_reset_at]
      .filter((value): value is string => Boolean(value))
      .map(Date.parse).filter(value => value > Date.now());
    if (!resets.length) return;
    const timer = window.setTimeout(() => {
      void queryClient.invalidateQueries({ queryKey: ['challenges'] });
      void queryClient.invalidateQueries({ queryKey: ['leaderboard'] });
    }, Math.min(2_147_483_647, Math.min(...resets) - Date.now() + 100));
    return () => window.clearTimeout(timer);
  }, [me.data?.next_reset_at, me.data?.season.next_reset_at, queryClient]);

  const trackedLeaderboard = viewers.data?.categories.find(category => category.key === 'tracked');
  const viewerRows = trackedLeaderboard?.entries.slice(0, 10) ?? [];
  const viewerOwn = trackedLeaderboard?.own_position ?? null;
  const viewerOwnVisible = viewerOwn
    ? viewerRows.some(
        row =>
          row.rank === viewerOwn.rank &&
          row.streamer.toLowerCase() === viewerOwn.streamer.toLowerCase(),
      )
    : false;

  const effortRows = effort.data?.entries.slice(0, 10) ?? [];
  const effortOwn = effort.data?.own_position ?? null;
  const effortOwnVisible = effortOwn
    ? effortRows.some(
        row => row.rank === effortOwn.rank && row.twitch_login === effortOwn.twitch_login,
      )
    : false;

  if (me.isLoading) return <LoadingState />;

  if (me.isError || !me.data) {
    const status = me.error instanceof ApiHttpError ? me.error.status : null;
    return (
      <section className="panel-card rounded-2xl border border-border p-6 md:p-8">
        <Trophy className="h-8 w-8 text-primary" />
        <h1 className="mt-4 text-2xl font-bold text-white">
          {status === 401
            ? 'Bitte mit deinem Partnerkonto einloggen'
            : 'Rangliste und Erfolge konnten nicht geladen werden'}
        </h1>
        <p className="mt-2 text-sm text-text-secondary">
          {status === 401
            ? 'Dieser Bereich gehört zu deinem Partner-Dashboard.'
            : 'Versuch es gleich noch einmal.'}
        </p>
      </section>
    );
  }

  const data = me.data;
  const copyReferral = async () => {
    if (!data.referral_url) return;
    try {
      await navigator.clipboard.writeText(data.referral_url);
      setCopyStatus('copied');
    } catch {
      setCopyStatus('error');
    }
  };
  const nextThreshold = data.level.next_threshold;
  const levelTarget =
    nextThreshold === null ? 1 : Math.max(1, nextThreshold - data.level.current_threshold);
  const levelCurrent =
    nextThreshold === null
      ? 1
      : Math.max(0, data.level.total_points - data.level.current_threshold);
  const nextGoalSentence =
    nextThreshold === null
      ? 'Du hast die aktuelle Maximalstufe erreicht.'
      : `${data.next_goal.fastest_route} bis Level ${data.level.level + 1}.`;


  return (
    <div className="challenges-page space-y-8 pb-32">
      <header className="flex items-center gap-4 py-3 md:py-5">
        <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded-2xl border border-primary/25 bg-primary/10 text-primary"><Trophy className="h-7 w-7" /></div>
        <div><h1 className="display-font text-3xl font-bold tracking-tight text-white md:text-4xl">Rangliste <span className="text-primary">&amp; Erfolge</span></h1><p className="mt-2 text-sm text-text-secondary">Deine Wochenaufgaben, dein Fortschritt und die Community im Vergleich.</p></div>
      </header>
      <section className="space-y-4">
        <SectionTitle
          title="Diese Woche"
          aside={
            <div className="flex flex-wrap items-center gap-2 text-xs">
              <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-black/20 px-3 py-1.5 text-text-secondary">
                <Clock3 className="h-3.5 w-3.5" />
                {countdown}
              </span>
              <span className="inline-flex items-center gap-1.5 rounded-full border border-primary/25 bg-primary/10 px-3 py-1.5 font-semibold text-primary">
                <Flame className="h-3.5 w-3.5" />
                {data.streak.current} Wochen{data.streak.data_complete === false ? ' (letzter bestätigter Stand)' : ''}
              </span>
              <span className="inline-flex items-center gap-1.5 rounded-full border border-white/10 bg-white/5 px-3 py-1.5 text-text-secondary">
                <Snowflake className="h-3.5 w-3.5" />
                {data.streak.freeze_used_this_month ? 'Schutz diesen Monat verbraucht' : 'Ein Schutz pro Monat verfügbar'}
              </span>
            </div>
          }
        />
        {isAssignmentPending ? (
          <div role="status" className="panel-card rounded-2xl border border-border p-4 md:p-5">
            <p className="font-semibold text-white">
              Deine Wochenaufgaben werden gerade vorbereitet.
            </p>
            <p className="mt-1 text-sm text-text-secondary">
              Sobald die aktuelle Woche ausgewertet ist, erscheinen sie hier.
            </p>
          </div>
        ) : hasNoReachableQuests ? (
          <div
            role="status"
            className="panel-card rounded-2xl border border-border p-4 md:p-5"
          >
            <p className="font-semibold text-white">
              Diese Woche ist gerade keine Aufgabe für dich erreichbar.
            </p>
            <p className="mt-1 text-sm text-text-secondary">
              Die Monatswertung läuft unabhängig davon weiter.
            </p>
          </div>
        ) : (
          <div className="grid gap-4 md:grid-cols-3">
            {quests.map(quest => (
              <QuestCard key={quest.key} quest={quest} />
            ))}
          </div>
        )}
      </section>

      <section className="space-y-4">
        <SectionTitle title="Mit uns erreicht" />
        <div className="grid gap-4 xl:grid-cols-[1.2fr_1fr]">
          <article className="panel-card challenge-level rounded-2xl border border-primary/30 p-5 md:p-6">
            <div className="flex items-start justify-between gap-4">
              <div>
                <div className="text-xs font-semibold uppercase tracking-[0.18em] text-primary">
                  Level {data.level.level}
                </div>
                <h3 className="mt-1 text-2xl font-bold text-white">
                  {data.level.total_points.toLocaleString('de-DE')} Punkte
                </h3>
              </div>
              <Medal className="h-8 w-8 text-primary" />
            </div>
            <p className="mt-3 text-sm font-medium text-white">{nextGoalSentence}</p>
            <div className="mt-4">
              <div className="mb-2 flex items-center justify-between text-xs">
                <span className="text-text-secondary">{nextThreshold === null ? 'Maximalstufe' : `Punkte für Level ${data.level.level + 1}`}</span>
                <span className="font-semibold text-white">
                  {Math.min(levelCurrent, levelTarget)} / {levelTarget} Punkte
                </span>
              </div>
              <ProgressBar current={levelCurrent} target={levelTarget} label="Fortschritt zum nächsten Level" />
            </div>
          </article>

          <div className="grid gap-2 sm:grid-cols-3 xl:grid-cols-1">
            <article className="panel-card flex h-auto items-center gap-3 rounded-2xl border border-border px-4 py-2">
              <UserPlus className="h-5 w-5 shrink-0 text-text-secondary" />
              <div className="min-w-0">
                <div className="text-xl font-bold tabular-nums text-white">
                  {data.with_us.people_brought_in_who_stayed}
                </div>
                <div className="mt-0.5 text-xs text-text-secondary">Leute, die geblieben sind</div>
              </div>
            </article>
            <article className="panel-card flex h-auto items-center gap-3 rounded-2xl border border-border px-4 py-2">
              <Users className="h-5 w-5 shrink-0 text-primary" />
              <div className="min-w-0">
                <div className="text-xl font-bold tabular-nums text-white">
                  {data.with_us.community_hours.toLocaleString('de-DE', { maximumFractionDigits: 1 })}
                </div>
                <div className="mt-0.5 text-xs text-text-secondary">Stunden mit der Community</div>
              </div>
            </article>
            <article className="panel-card flex h-auto items-center gap-3 rounded-2xl border border-border px-4 py-2">
              <Zap className="h-5 w-5 shrink-0 text-primary" />
              <div className="min-w-0">
                <div className="text-xl font-bold tabular-nums text-white">{data.with_us.received_raids}</div>
                <div className="mt-0.5 text-xs text-text-secondary">Raids erhalten</div>
              </div>
            </article>
          </div>
        </div>

        <div className="grid gap-4 md:grid-cols-2 2xl:grid-cols-3">
          {data.achievements.map(achievement => <AchievementCard key={achievement.key} achievement={achievement} />)}
        </div>
      </section>

      <section className="min-h-0 space-y-2">
        <SectionTitle title="Deine Werber" />
        <div className="panel-card min-h-0 self-start rounded-2xl border border-border p-4 md:p-5">
          {data.referral_url ? (
            <div className="mb-4">
              <label htmlFor="challenge-referral" className="mb-2 flex items-center gap-2 text-sm font-semibold text-white">
                <Link className="h-4 w-4 text-text-secondary" />Dein Empfehlungslink
              </label>
              <div className="flex gap-2">
                <input id="challenge-referral" readOnly value={data.referral_url}
                  onFocus={event => event.currentTarget.select()}
                  className="min-w-0 flex-1 rounded-xl border border-white/10 px-3 py-2.5 text-sm text-white" />
                <button type="button" onClick={() => void copyReferral()}
                  className="inline-flex shrink-0 items-center gap-2 rounded-xl border border-primary/30 bg-primary/10 px-4 py-2.5 text-sm font-semibold text-primary hover:bg-primary/20">
                  <Copy className="h-4 w-4" />{copyStatus === 'copied' ? 'Kopiert' : 'Kopieren'}
                </button>
              </div>
              <p className="mt-2 text-xs text-text-secondary">Teile den Link mit deinem Chat. Aktive Einladungen werden deinem Kanal zugeordnet.</p>
              <p role="status" className="mt-1 text-xs text-text-secondary">
                {copyStatus === 'error' ? 'Kopieren hat nicht geklappt. Markiere den Link und kopiere ihn selbst.'
                  : copyStatus === 'copied' ? 'Der Link ist in deiner Zwischenablage.' : ''}
              </p>
            </div>
          ) : <p className="mb-4 text-sm text-text-secondary">Dein Empfehlungslink ist gerade nicht verfügbar.</p>}
          {recruiters.data?.recruiters.length ? (
            <div className="space-y-2">
              {recruiters.data.recruiters.map((recruiter) => (
                <div
                  key={recruiter.twitch_user_id}
                  className="grid grid-cols-[44px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl border border-border bg-black/15 px-3 py-2.5"
                >
                  <ChallengeAvatar name={recruiter.display_name ?? 'Zuschauer'} url={recruiter.avatar_url} />
                  <div className="min-w-0">
                    <div className="truncate font-semibold text-white">
                      {recruiter.display_name ?? 'Twitch-Zuschauer'}
                    </div>
                  </div>
                  <div className="text-right">
                    <div className="font-bold text-white">{recruiter.qualified_invites}</div>
                    <div className="text-[11px] text-text-secondary">aktiv geblieben</div>
                  </div>
                </div>
              ))}
            </div>
          ) : recruiters.isError ? (
            <p className="px-2 py-1 text-sm text-text-secondary">Deine Werber konnten nicht geladen werden. Versuch es gleich noch einmal.</p>
          ) : recruiters.isLoading ? (
            <p className="px-2 py-1 text-sm text-text-secondary">Werber werden geladen.</p>
          ) : (
            <p className="px-2 py-1 text-sm text-text-secondary">
              Noch keine aktiven Einladungen über deinen Link.
            </p>
          )}
        </div>
      </section>

      <section className="space-y-4">
        <SectionTitle title="Rangliste" />
        <div className="panel-card rounded-2xl border border-border p-3 md:p-5">
          <div className="mb-4 flex w-full gap-2 rounded-full border border-white/10 bg-black/25 p-1.5 sm:w-fit">
            <button
              type="button"
              onClick={() => setLeaderboardTab('viewers')}
              aria-pressed={leaderboardTab === 'viewers'}
              title="Durchschnittliche Zuschauerzahl pro Stream in den letzten 30 Tagen."
              className={`flex-1 rounded-full px-4 py-2 text-sm font-semibold transition-colors sm:flex-none ${
                leaderboardTab === 'viewers'
                  ? 'bg-primary text-[#0D0806]'
                  : 'text-text-secondary hover:text-white'
              }`}
            >
              Zuschauer (30 Tage)
            </button>
            <button
              type="button"
              onClick={() => setLeaderboardTab('effort')}
              aria-pressed={leaderboardTab === 'effort'}
              title="Bestätigte Einsatzpunkte in diesem Kalendermonat."
              className={`flex-1 rounded-full px-4 py-2 text-sm font-semibold transition-colors sm:flex-none ${
                leaderboardTab === 'effort'
                  ? 'bg-primary text-[#0D0806]'
                  : 'text-text-secondary hover:text-white'
              }`}
            >
              Einsatz (Monat)
            </button>
          </div>

          <div className="mb-2 grid grid-cols-[44px_minmax(0,1fr)_auto] gap-3 border-b border-white/10 px-3 pb-3 text-xs font-semibold text-text-secondary md:px-4"><span className="text-center">#</span><span>Spieler</span><span title={leaderboardTab === 'viewers' ? 'Ø Zuschauer pro Stream' : 'Bestätigte Einsatzpunkte im Monat'} className="text-right">{leaderboardTab === 'viewers' ? 'Ø Zuschauer pro Stream' : 'Punkte'}</span></div>
          {leaderboardTab === 'viewers' ? (
            viewers.isLoading ? (
              <div className="py-8 text-center text-sm text-text-secondary">Rangliste wird geladen.</div>
            ) : viewers.isError ? (
              <div className="py-8 text-center text-sm text-text-secondary">Rangliste konnte nicht geladen werden.</div>
            ) : (
              <div className="space-y-2">
                {viewerRows.map(row => (
                  <LeaderboardRow
                    key={`${row.rank}-${row.streamer}`}
                    rank={row.rank}
                    name={row.streamer}
                    avatarUrl={avatarFor(row.streamer, row.avatar_url)}
                    value={`${row.avg_viewers.toLocaleString('de-DE', { maximumFractionDigits: 1 })} Ø`}
                    own={Boolean(
                      viewerOwn &&
                        row.rank === viewerOwn.rank &&
                        row.streamer.toLowerCase() === viewerOwn.streamer.toLowerCase(),
                    )}
                  />
                ))}
                {viewerOwn && !viewerOwnVisible ? (
                  <>
                    <div className="py-1 text-center text-xs text-text-secondary">Deine Position</div>
                    <LeaderboardRow
                      rank={viewerOwn.rank}
                      name={viewerOwn.streamer}
                      avatarUrl={avatarFor(viewerOwn.streamer, viewerOwn.avatar_url)}
                      value={`${viewerOwn.avg_viewers.toLocaleString('de-DE', { maximumFractionDigits: 1 })} Ø`}
                      own
                      pinned
                    />
                  </>
                ) : null}
              </div>
            )
          ) : effort.isLoading ? (
            <div className="py-8 text-center text-sm text-text-secondary">Rangliste wird geladen.</div>
          ) : effort.isError ? (
            <div className="py-8 text-center text-sm text-text-secondary">Rangliste konnte nicht geladen werden.</div>
          ) : (
            <div className="space-y-2">
              {effortRows.map(row => (
                <LeaderboardRow
                  key={`${row.rank}-${row.twitch_login}`}
                  rank={row.rank}
                  name={row.twitch_login}
                  avatarUrl={avatarFor(row.twitch_login, row.avatar_url)}
                  value={`${row.points.toLocaleString('de-DE')} Punkte`}
                  own={row.is_self}
                  raidBoost={row.raid_boost}
                />
              ))}
              {effortOwn && !effortOwnVisible ? (
                <>
                  <div className="py-1 text-center text-xs text-text-secondary">Deine Position</div>
                  <LeaderboardRow
                    rank={effortOwn.rank}
                    name={effortOwn.twitch_login}
                    avatarUrl={avatarFor(effortOwn.twitch_login, effortOwn.avatar_url)}
                    value={`${effortOwn.points.toLocaleString('de-DE')} Punkte`}
                    own
                    raidBoost={effortOwn.raid_boost}
                    pinned
                  />
                </>
              ) : null}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
