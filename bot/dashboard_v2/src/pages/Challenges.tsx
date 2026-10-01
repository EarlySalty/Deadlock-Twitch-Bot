import { useEffect, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
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
      className={`grid grid-cols-[44px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl border px-3 py-2 md:px-4 ${
        own
          ? 'border-amber-500/50 bg-amber-500/15'
          : pinned
            ? 'border-primary/40 bg-primary/10'
            : first
              ? 'border-primary/20 bg-primary/5'
              : 'border-border bg-black/15'
      }`}
    >
      <div className="flex items-center justify-center">
        {first ? (
          <Crown className="h-5 w-5 text-primary" aria-label="Platz 1" />
        ) : (
          <span className={`text-sm font-bold tabular-nums ${own ? 'text-primary' : 'text-text-secondary'}`}>#{rank}</span>
        )}
      </div>
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate font-semibold text-white">{name}</span>
          {own ? (
            <span className="rounded-full border border-primary/40 bg-primary/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.14em] text-primary">
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
      <div className={`text-right text-sm font-bold tabular-nums ${own ? 'text-primary' : 'text-white'}`}>{value}</div>
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
  const queryClient = useQueryClient();
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
  const badges = achievementBadges(data.achievements);

  return (
    <div className="space-y-6 pb-10">
      {data.category_data_complete === false && (
        <div role="status" className="panel-card rounded-2xl border border-primary/30 p-4 text-sm text-text-secondary">
          Deine Punkte und Erfolge bleiben erhalten. Wegen einer Datenlücke sind betroffene Stream-Aufgaben und die Ausdauer-Wertung ausgesetzt. Die Rangliste zeigt die bisher bestätigten Punkte. Vollständig erfasste Zeiträume werden automatisch wieder gewertet.
        </div>
      )}
      <section className="panel-card rounded-2xl border border-primary/35 bg-black/25 px-4 py-3 md:px-5">
        <p className="display-font text-xl font-bold leading-snug text-white md:text-2xl">
          {nextGoalSentence}
        </p>
        <div className="mt-3">
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
                {data.streak.current} Wochen{data.streak.data_complete === false ? ' (letzter bestätigter Stand)' : ''}
              </span>
              <span className="inline-flex items-center gap-1.5 rounded-full border border-accent/25 bg-accent/10 px-3 py-1.5 text-accent">
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
          <article className="panel-card flex flex-col justify-between rounded-2xl border border-border p-4 md:p-5">
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

          <div className="grid gap-2 sm:grid-cols-3 xl:grid-cols-1">
            <article className="panel-card flex h-auto items-center gap-3 rounded-2xl border border-border px-4 py-2">
              <UserPlus className="h-5 w-5 shrink-0 text-primary" />
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

        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          {badges.map(badge => (
            <article
              key={badge.id}
              className={`rounded-2xl border p-4 ${
                badge.earned
                  ? 'border-primary/30 bg-primary/10'
                  : 'border-white/10 bg-black/20'
              }`}
            >
              <div className="flex items-center gap-3">
                <div
                  className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-full border ${
                    badge.earned
                      ? 'border-primary/25 bg-primary/10 text-primary'
                      : 'border-white/10 bg-white/5 text-zinc-300'
                  }`}
                >
                  {badge.earned ? (
                    <Trophy className="h-4 w-4" />
                  ) : (
                    <LockKeyhole className="h-4 w-4 text-zinc-500" />
                  )}
                </div>
                <div className="min-w-0">
                  <h3 className={`font-semibold ${badge.earned ? 'text-white' : 'text-zinc-300'}`}>
                    {badge.title}
                  </h3>
                  <p className={`mt-1 text-xs ${badge.earned ? 'text-text-secondary' : 'text-zinc-300'}`}>
                    {badge.earned ? 'Erreicht' : badge.condition}
                  </p>
                </div>
              </div>
            </article>
          ))}
        </div>
      </section>

      <section className="min-h-0 space-y-2">
        <SectionTitle title="Deine Werber" />
        <div className="panel-card min-h-0 self-start rounded-2xl border border-border p-2">
          {recruiters.data?.recruiters.length ? (
            <div className="space-y-2">
              {recruiters.data.recruiters.map((recruiter, index) => (
                <div
                  key={recruiter.twitch_user_id}
                  className="grid grid-cols-[44px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl border border-border bg-black/15 px-3 py-2.5"
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
          ) : recruiters.isError ? (
            <p className="px-2 py-1 text-sm text-text-secondary">Deine Werber konnten nicht geladen werden. Versuch es gleich noch einmal.</p>
          ) : recruiters.isLoading ? (
            <p className="px-2 py-1 text-sm text-text-secondary">Werber werden geladen.</p>
          ) : (
            <p className="px-2 py-1 text-sm text-text-secondary">
              Noch niemand hat über deinen Link aktive Leute gebracht.
            </p>
          )}
        </div>
      </section>

      <section className="space-y-4">
        <SectionTitle title="Rangliste" />
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
