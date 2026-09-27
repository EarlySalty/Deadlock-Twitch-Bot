import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  Check,
  Clock3,
  Crown,
  Flame,
  LockKeyhole,
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
  fetchChallengeViewers,
  fetchEffortLeaderboard,
  fetchViewerLeaderboard,
  type ChallengeAchievement,
  type ChallengeQuest,
} from '@/api/challenges';
import { ApiHttpError } from '@/api/httpError';

type LeaderboardTab = 'viewers' | 'effort';

const STARTER_QUESTS: ChallengeQuest[] = [
  {
    key: 'active_discord_invite',
    text: 'Bringe 1 neue aktive Person in den Discord',
    progress: 0,
    goal: 1,
    completed: false,
  },
  {
    key: 'stream_together',
    text: 'Streame mindestens 30 Minuten per Stream Together mit einem Partner',
    progress: 0,
    goal: 1,
    completed: false,
  },
  {
    key: 'community_match',
    text: 'Spiele ein Match mit jemandem aus der Community',
    progress: 0,
    goal: 1,
    completed: false,
  },
];

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
        className="h-full rounded-full bg-gradient-to-r from-primary to-accent transition-[width] duration-500"
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

function useMondayCountdown() {
  const [, tick] = useState(0);

  useEffect(() => {
    const timer = window.setInterval(() => tick(value => value + 1), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  const now = new Date();
  const target = new Date(now);
  const days = (8 - now.getDay()) % 7 || 7;
  target.setDate(now.getDate() + days);
  target.setHours(0, 0, 0, 0);

  const remaining = Math.max(0, target.getTime() - now.getTime());
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
    <article className="panel-card rounded-2xl border border-border p-4 md:p-5">
      <div className="flex items-start justify-between gap-3">
        <h3 className="font-semibold leading-snug text-white">{quest.text}</h3>
        <div
          className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full border ${
            quest.completed
              ? 'border-success/30 bg-success/10 text-success'
              : 'border-primary/20 bg-primary/10 text-primary'
          }`}
        >
          {quest.completed ? <Check className="h-4 w-4" /> : <Sparkles className="h-4 w-4" />}
        </div>
      </div>
      <div className="mt-5">
        <div className="mb-2 flex items-center justify-between text-xs">
          <span className="text-text-secondary">Fortschritt</span>
          <span className="font-semibold text-white">
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
}: {
  rank: number;
  name: string;
  value: string;
  own: boolean;
  raidBoost?: boolean;
  pinned?: boolean;
}) {
  const first = rank === 1;
  return (
    <div
      className={`grid grid-cols-[44px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl border px-3 py-3 md:px-4 ${
        first
          ? 'border-primary/40 bg-primary/10'
          : own || pinned
            ? 'border-accent/25 bg-accent/5'
            : 'border-border bg-black/15'
      }`}
    >
      <div className="flex items-center justify-center">
        {first ? (
          <Crown className="h-5 w-5 text-primary" aria-label="Platz 1" />
        ) : (
          <span className="text-sm font-bold text-text-secondary">#{rank}</span>
        )}
      </div>
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate font-semibold text-white">{name}</span>
          {own ? (
            <span className="rounded-full border border-accent/25 bg-accent/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.14em] text-accent">
              Du
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
      <div className="text-right text-sm font-bold text-white">{value}</div>
    </div>
  );
}

function achievementBadges(achievements: ChallengeAchievement[]) {
  return achievements.flatMap(achievement =>
    achievement.tiers.map((tier, index) => ({
      id: `${achievement.key}-${tier.target}`,
      title: `${achievement.name} ${index + 1}`,
      condition: `${tier.target} ${ACHIEVEMENT_CONDITIONS[achievement.key] ?? 'Fortschritt erreichen'}`,
      earned: tier.unlocked,
    })),
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
  const [leaderboardTab, setLeaderboardTab] = useState<LeaderboardTab>('viewers');
  const me = useQuery({
    queryKey: ['challenges', 'me'],
    queryFn: fetchChallengesMe,
    staleTime: 60_000,
  });
  const recruiters = useQuery({
    queryKey: ['challenges', 'viewers'],
    queryFn: fetchChallengeViewers,
    staleTime: 60_000,
  });
  const viewers = useQuery({
    queryKey: ['leaderboard', 'viewers', 10],
    queryFn: () => fetchViewerLeaderboard(10),
    staleTime: 60_000,
  });
  const effort = useQuery({
    queryKey: ['leaderboard', 'effort'],
    queryFn: fetchEffortLeaderboard,
    staleTime: 60_000,
  });

  const countdown = useMondayCountdown();
  const quests = useMemo(() => {
    const current = me.data?.quests ?? [];
    if (current.length >= 3) return current.slice(0, 3);
    const keys = new Set(current.map(quest => quest.key));
    return [...current, ...STARTER_QUESTS.filter(quest => !keys.has(quest.key))].slice(0, 3);
  }, [me.data?.quests]);

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
            : 'Challenges konnten nicht geladen werden'}
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
      : `Noch ${data.next_goal.fastest_route} bis Level ${data.level.level + 1}.`;
  const badges = achievementBadges(data.achievements);

  return (
    <div className="space-y-6 pb-10">
      <section className="panel-card min-h-[132px] rounded-2xl border border-primary/35 bg-black/25 p-5 md:min-h-[148px] md:p-7">
        <p className="display-font text-xl font-bold leading-snug text-white md:text-2xl">
          {nextGoalSentence}
        </p>
        <div className="mt-6">
          <ProgressBar current={levelCurrent} target={levelTarget} label="Fortschritt zum nächsten Ziel" />
        </div>
      </section>

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
                {data.streak.current} Wochen
              </span>
              <span className="inline-flex items-center gap-1.5 rounded-full border border-accent/25 bg-accent/10 px-3 py-1.5 text-accent">
                <Snowflake className="h-3.5 w-3.5" />
                Freeze automatisch
              </span>
            </div>
          }
        />
        <div className="grid gap-4 md:grid-cols-3">
          {quests.map(quest => (
            <QuestCard key={quest.key} quest={quest} />
          ))}
        </div>
      </section>

      <section className="space-y-4">
        <SectionTitle title="Mit uns erreicht" />
        <div className="grid gap-4 xl:grid-cols-[1.2fr_1fr]">
          <article className="panel-card rounded-2xl border border-border p-5 md:p-6">
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
            <div className="mt-6">
              <div className="mb-2 flex items-center justify-between text-xs">
                <span className="text-text-secondary">Nächstes Level</span>
                <span className="font-semibold text-white">
                  {Math.min(levelCurrent, levelTarget)} / {levelTarget} Punkte
                </span>
              </div>
              <ProgressBar current={levelCurrent} target={levelTarget} label="Fortschritt zum nächsten Level" />
            </div>
          </article>

          <div className="grid gap-3 sm:grid-cols-3 xl:grid-cols-1">
            <article className="panel-card rounded-2xl border border-border p-4">
              <UserPlus className="h-5 w-5 text-primary" />
              <div className="mt-3 text-2xl font-bold text-white">
                {data.with_us.people_brought_in_who_stayed}
              </div>
              <div className="mt-1 text-xs text-text-secondary">Leute, die geblieben sind</div>
            </article>
            <article className="panel-card rounded-2xl border border-border p-4">
              <Users className="h-5 w-5 text-primary" />
              <div className="mt-3 text-2xl font-bold text-white">
                {data.with_us.community_hours.toLocaleString('de-DE', { maximumFractionDigits: 1 })}
              </div>
              <div className="mt-1 text-xs text-text-secondary">Stunden mit der Community</div>
            </article>
            <article className="panel-card rounded-2xl border border-border p-4">
              <Zap className="h-5 w-5 text-primary" />
              <div className="mt-3 text-2xl font-bold text-white">{data.with_us.received_raids}</div>
              <div className="mt-1 text-xs text-text-secondary">Raids erhalten</div>
            </article>
          </div>
        </div>

        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          {badges.map(badge => (
            <article
              key={badge.id}
              className={`rounded-2xl border p-4 ${
                badge.earned
                  ? 'border-primary/30 bg-primary/10'
                  : 'border-border bg-black/20 grayscale'
              }`}
            >
              <div className="flex items-center gap-3">
                <div
                  className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-full border ${
                    badge.earned
                      ? 'border-primary/25 bg-primary/10 text-primary'
                      : 'border-border bg-white/5 text-text-secondary'
                  }`}
                >
                  {badge.earned ? <Trophy className="h-4 w-4" /> : <LockKeyhole className="h-4 w-4" />}
                </div>
                <div className="min-w-0">
                  <h3 className={`font-semibold ${badge.earned ? 'text-white' : 'text-text-secondary'}`}>
                    {badge.title}
                  </h3>
                  <p className="mt-1 text-xs text-text-secondary">
                    {badge.earned ? 'Erreicht' : badge.condition}
                  </p>
                </div>
              </div>
            </article>
          ))}
        </div>
      </section>

      <section className="space-y-4">
        <SectionTitle title="Deine Werber" />
        <div className="panel-card rounded-2xl border border-border p-3 md:p-4">
          {recruiters.data?.recruiters.length ? (
            <div className="space-y-2">
              {recruiters.data.recruiters.map((recruiter, index) => (
                <div
                  key={recruiter.twitch_user_id}
                  className="grid grid-cols-[44px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl border border-border bg-black/15 px-3 py-3"
                >
                  <span className="text-center text-sm font-bold text-text-secondary">#{index + 1}</span>
                  <div className="min-w-0">
                    <div className="truncate font-semibold text-white">
                      {recruiter.display_name ?? 'Twitch Viewer'}
                    </div>
                  </div>
                  <div className="text-right">
                    <div className="font-bold text-white">{recruiter.qualified_invites}</div>
                    <div className="text-[11px] text-text-secondary">aktiv geblieben</div>
                  </div>
                </div>
              ))}
            </div>
          ) : recruiters.isLoading ? (
            <p className="px-2 py-5 text-sm text-text-secondary">Werber werden geladen.</p>
          ) : (
            <p className="px-2 py-5 text-sm text-text-secondary">
              Noch niemand hat über deinen Link aktive Leute gebracht.
            </p>
          )}
        </div>
      </section>

      <section className="space-y-4">
        <SectionTitle title="Leaderboard" />
        <div className="panel-card rounded-2xl border border-border p-3 md:p-5">
          <div className="mb-4 flex w-full gap-2 rounded-xl border border-border bg-black/20 p-1.5 sm:w-fit">
            <button
              type="button"
              onClick={() => setLeaderboardTab('viewers')}
              className={`flex-1 rounded-lg px-4 py-2 text-sm font-semibold transition-colors sm:flex-none ${
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
              className={`flex-1 rounded-lg px-4 py-2 text-sm font-semibold transition-colors sm:flex-none ${
                leaderboardTab === 'effort'
                  ? 'bg-primary text-[#0D0806]'
                  : 'text-text-secondary hover:text-white'
              }`}
            >
              Einsatz (Monat)
            </button>
          </div>

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
