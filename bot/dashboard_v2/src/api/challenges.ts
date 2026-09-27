import { fetchApi } from './core';

export interface ChallengeQuest {
  key: string;
  text: string;
  progress: number;
  goal: number;
  completed: boolean;
}

export interface ChallengeStreak {
  current: number;
  longest: number;
}

export interface ChallengeLevel {
  level: number;
  total_points: number;
  current_threshold: number;
  next_threshold: number | null;
}

export interface ChallengeNextGoal {
  missing_points: number;
  fastest_route: string;
}

export interface AchievementTier {
  target: number;
  unlocked: boolean;
}

export interface ChallengeAchievement {
  key: string;
  name: string;
  progress: number;
  tiers: AchievementTier[];
}

export interface WithUsStats {
  people_brought_in_who_stayed: number;
  community_hours: number;
  received_raids: number;
}

export interface ChallengeSeason {
  month: string;
  points: number;
  rank: number;
  active_partners: number;
}

export interface ChallengesMe {
  generated_at: string;
  timezone: string;
  streamer: string;
  quests: ChallengeQuest[];
  streak: ChallengeStreak;
  level: ChallengeLevel;
  next_goal: ChallengeNextGoal;
  achievements: ChallengeAchievement[];
  with_us: WithUsStats;
  season: ChallengeSeason;
}

export interface ViewerRecruiter {
  twitch_user_id: string;
  display_name: string | null;
  qualified_invites: number;
}

export interface ChallengeViewers {
  generated_at: string;
  streamer: string;
  recruiters: ViewerRecruiter[];
}

export interface EffortLeaderboardEntry {
  rank: number;
  twitch_login: string;
  points: number;
  raid_boost: boolean;
  is_self: boolean;
}

export interface EffortLeaderboard {
  month: string;
  entries: EffortLeaderboardEntry[];
  own_position: EffortLeaderboardEntry | null;
}

export interface ViewerLeaderboardEntry {
  rank: number;
  streamer: string;
  avg_viewers: number;
  max_viewers: number;
  samples: number;
}

export interface ViewerLeaderboardCategory {
  key: string;
  title: string;
  count: number;
  entries: ViewerLeaderboardEntry[];
  own_position?: ViewerLeaderboardEntry | null;
}

export interface ViewerLeaderboard {
  window: { days: number };
  categories: ViewerLeaderboardCategory[];
}

export function fetchChallengesMe(): Promise<ChallengesMe> {
  return fetchApi<ChallengesMe>('/challenges/me');
}

export function fetchChallengeViewers(): Promise<ChallengeViewers> {
  return fetchApi<ChallengeViewers>('/challenges/viewers');
}

export function fetchEffortLeaderboard(): Promise<EffortLeaderboard> {
  return fetchApi<EffortLeaderboard>('/leaderboard/effort');
}

export function fetchViewerLeaderboard(limit = 10): Promise<ViewerLeaderboard> {
  return fetchApi<ViewerLeaderboard>('/leaderboard', { limit });
}
