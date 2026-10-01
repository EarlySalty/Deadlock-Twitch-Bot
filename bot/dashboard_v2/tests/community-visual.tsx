// Standalone, synthetic browser fixture. Not a production entry point.
import React, { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { CommunityView } from '../src/pages/Community';
import type { CommunityData, CommunityProfile, CommunityRecommendation } from '../src/types/community';
import '../src/index.css';
const profile: CommunityProfile = { rank_name: 'Oracle', rank_tier: 8, subrank: 3, rank_updated_at: Math.floor(Date.now() / 1000), mode: 'normal', mode_source: 'steam_history', mode_samples: 18, history_updated_at: Math.floor(Date.now() / 1000), source_status: 'ok' };
const slots = Array.from({ length: 336 }, (_, i) => i % 48 >= 36 && i % 48 < 46 && i % 3 !== 0 ? 0.65 : 0);
const recommendation: CommunityRecommendation = { login: 'test_streamer_a', live_state: 'live', current_game: 'Deadlock', current_title: 'Gemeinsamer Abend', both_live_same_game: true, profile, schedule: { sessions: 24, slots, games: ['deadlock'] }, score: 84, schedule_overlap_pct: 76, observed_overlap_minutes: 720, shared_games: ['deadlock'], shared_windows: [{ weekday: 2, start_minute: 1140, end_minute: 1380, strength: 0.65 }, { weekday: 5, start_minute: 1080, end_minute: 1320, strength: 0.6 }], compatible: true, reasons: ['Ähnliche historische Streamzeiten', 'Ähnlicher bestätigter Steam-Rang', 'Gleicher erkannter Spielmodus'], warnings: [] };
const fixture: CommunityData = {
  generated_at: new Date().toISOString(), days: 56, timezone: 'Europe/Berlin', streamer: 'test_dein_kanal', own_profile: profile,
  own_schedule: { sessions: 28, slots: slots.map((value, i) => i % 5 === 0 ? 0.25 : value), games: ['deadlock'] }, candidate_count: 2, evaluated_count: 2,
  recommendations: [recommendation, { ...recommendation, login: 'test_streamer_b', live_state: 'offline', both_live_same_game: false, current_game: null, profile: { ...profile, mode: 'street_brawl', mode_source: 'current_title' }, score: 64, compatible: false, warnings: ['Unterschiedliche erkannte Spielmodi'], reasons: ['Ähnliche historische Streamzeiten'] }],
  discord: { status: 'ok', captured_at: Math.floor(Date.now() / 1000), lobbies: [
    { channel_id: '1326984426906714236', name: 'Streamer-VC', member_count: 2, user_limit: null, mode: null, intent: null, rank_average: 8.3, rank_samples: 2, requester_present: false, is_streamer_vc: true, joinable: true, slots_free: null },
    { channel_id: '1289721245281292291', name: 'Feierabendrunde · Standard', member_count: 3, user_limit: 6, mode: 'normal', intent: 'casual', rank_average: 8.1, rank_samples: 3, requester_present: false, is_streamer_vc: false, joinable: true, slots_free: 3 },
    { channel_id: '1289721245281292292', name: 'Volle Test-Lobby', member_count: 6, user_limit: 6, mode: 'street_brawl', intent: 'casual', rank_average: null, rank_samples: 0, requester_present: false, is_streamer_vc: false, joinable: false, slots_free: 0 },
  ] },
};
function Fixture() {
  const [now, setNow] = useState(Date.now());
  useEffect(() => { const timer = setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(timer); }, []);
  return <main className="mx-auto max-w-[1440px] space-y-5 p-4 sm:p-8"><p className="text-sm text-warning">SYNTHETISCHE TESTDATEN · keine echten Personen, keine Live-Lobbys</p><h1 className="text-3xl font-semibold text-white">Zusammen spielen & streamen</h1><CommunityView data={fixture} now={now} onRefresh={() => {}} /></main>;
}
createRoot(document.getElementById('root')!).render(<Fixture />);
