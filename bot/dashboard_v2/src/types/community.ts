export type CommunityMode = 'normal' | 'street_brawl' | 'sandbox' | 'custom';
export interface CommunityProfile {
  rank_name: string | null;
  rank_tier: number | null;
  subrank: number | null;
  rank_updated_at: number | null;
  mode: CommunityMode | null;
  mode_source: 'steam_history' | 'current_title' | 'historical_title' | null;
  mode_samples: number;
  history_updated_at: number | null;
  source_status: string;
}
export interface CommunitySchedule {
  /** Monday through Sunday, 48 half-hour slots each, Europe/Berlin. */
  slots: number[];
  sessions: number;
  games: string[];
}
export interface SharedWindow {
  weekday: number;
  start_minute: number;
  end_minute: number;
  strength: number;
}
export interface CommunityRecommendation {
  login: string;
  live_state: 'live' | 'offline' | 'unknown';
  current_game: string | null;
  current_title: string | null;
  both_live_same_game: boolean;
  profile: CommunityProfile;
  schedule: CommunitySchedule;
  score: number | null;
  schedule_overlap_pct: number | null;
  observed_overlap_minutes: number;
  shared_games: string[];
  shared_windows: SharedWindow[];
  compatible: boolean;
  reasons: string[];
  warnings: string[];
}
export interface CommunityLobby {
  channel_id: string;
  name: string;
  member_count: number;
  user_limit: number | null;
  mode: CommunityMode | null;
  intent: string | null;
  rank_average: number | null;
  rank_samples: number;
  requester_present: boolean;
  is_streamer_vc: boolean;
  joinable: boolean;
  slots_free: number | null;
}
export interface CommunityData {
  generated_at: string;
  days: number;
  timezone: string;
  streamer: string;
  own_profile: CommunityProfile;
  own_schedule: CommunitySchedule;
  recommendations: CommunityRecommendation[];
  discord: {
    status: 'ok' | 'stale' | 'unavailable' | 'link_required' | 'membership_unconfirmed';
    captured_at: number | null;
    lobbies: CommunityLobby[];
  };
  candidate_count: number;
  evaluated_count: number;
}
