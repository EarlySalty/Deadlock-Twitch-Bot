import { QueryClient } from '@tanstack/react-query';
import type { PartnerProfileData } from '../../src/api/partnerProfile';
import { translate } from '../../src/i18n/dictionary';

export const surface = new URLSearchParams(location.search).get('surface') ?? 'profile';
export const client = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: Infinity } },
});
export const auth = (owner: string) => ({
  authenticated: true,
  twitchUserId: owner,
  twitchLogin: owner,
  isAdmin: true,
  isLocalhost: false,
  permissions: { viewAllStreamers: true },
  csrfToken: 'synthetic-csrf',
});
client.setQueryData(['auth-status'], auth('alice'));

function profile(owner: string): PartnerProfileData {
  return {
    login: owner,
    public_path: `/streamer/${owner}`,
    active: true,
    published: false,
    revision: 1,
    profile: {
      headline: `${owner} headline`,
      about: 'Grüße & Spaß',
      avatar_url: '',
      banner_url: '',
      accent: 'gold',
      socials: [],
      featured: [],
      main_heroes: [],
      rank: '',
      playstyles: [],
      preferred_times: [],
      show_history: true,
      sync_twitch_schedule: true,
      events: [],
    },
  };
}

export const fixture = {
  client,
  translate,
  votes: new Map<string, { own_vote: { winner: string; comment?: string }; totals: { compact: number; full: number; gleich: number } }>(),
  owner: 'alice',
  calls: [] as Array<{
    path: string;
    method: string;
    body: unknown;
    owner: string;
    language: string | null;
    csrf: string | null;
  }>,
  fail: new Set<string>(),
  failures: new Map<string, { status: number; body: unknown }>(),
  hold: new Set<string>(),
  held: [] as Array<() => void>,
  completed: 0,
  release() {
    for (const finish of this.held.splice(0)) finish();
  },
};
if (new URLSearchParams(location.search).get('initialFailure') === 'settings') {
  fixture.fail.add('/twitch/api/v2/streamer/moderation/settings');
  fixture.fail.add('/twitch/api/v2/streamer/scam-guard/settings');
} else if (new URLSearchParams(location.search).has('initialFailure')) {
  fixture.fail.add('/twitch/api/v2/streamer/profile');
}
Object.assign(window, { languageDraftFixture: fixture });
window.fetch = async (input, init = {}) => {
  const url = new URL(String(input), location.origin);
  const path = url.pathname;
  const method = init.method ?? 'GET';
  const body = init.body ? JSON.parse(String(init.body)) : undefined;
  const owner = url.searchParams.get('streamer') || fixture.owner;
  fixture.calls.push({
    path,
    method,
    body,
    owner,
    language: new Headers(init.headers).get('X-Dashboard-Language'),
    csrf: new Headers(init.headers).get('X-CSRF-Token'),
  });
  let data: unknown;
  if (path.endsWith('/auth-status')) data = auth(fixture.owner);
  else if (path.endsWith('/streamer/profile')) {
    data = method === 'GET'
      ? {
          ...profile(owner),
          ...(url.searchParams.has('refresh_twitch') ? {
            twitch: {
              available: true, display_name: owner, description: `${owner} imported Grüße`,
              profile_image_url: '', banner_url: '', live: null, clips: [], schedule: [],
            },
          } : {}),
        }
      : { ...body, login: owner, revision: 2, active: true, public_path: `/streamer/${owner}` };
  }
  else if (path.endsWith('/moderation/settings'))
    data =
      method === 'GET'
        ? {
            global_ban_enabled: owner === 'alice',
            scam_pitch_enabled: true,
            spam_autoban_enabled: true,
            sus_invite_enabled: true,
          }
        : body;
  else if (path.endsWith('/scam-guard/settings'))
    data =
      method === 'GET'
        ? { enabled: owner === 'alice', mode: 'auto_ban', threshold: 0.95, suggestion_floor: 0.7 }
        : body;
  else if (path.endsWith('/overview'))
    data = {
      sessions: [
        { id: 101, title: `${owner} stream`, date: '2026-10-13', avgViewers: 15, peakViewers: 30 },
      ],
    };
  else if (path.endsWith('/stream-report'))
    data = {
      status: 'done',
      session_id: 101,
      empty: false,
      report: { gut: [{ punkt: `${owner} content`, begruendung: 'Original content' }] },
      rating: { rating: 'neutral', comment: 'baseline' },
    };
  else if (path.endsWith('/stream-report/ab-vote')) {
    const current = fixture.votes.get(owner) ?? {
      own_vote: { winner: 'gleich', comment: 'baseline' },
      totals: { compact: 1, full: 1, gleich: 1 },
    };
    if (method === 'GET') data = current;
    else {
      const saved = {
        own_vote: { winner: body.winner, comment: body.comment },
        totals: { compact: 4, full: 2, gleich: 1 },
      };
      if (!fixture.failures.has(`${method} ${path}`)) fixture.votes.set(owner, saved);
      data = { ok: true, winner: body.winner, totals: saved.totals };
    }
  }
  else if (path.endsWith('/stream-report/rate'))
    data = { ok: true, rating: body.rating, comment: body.comment };
  else if (path.endsWith('/streamers'))
    data = [{ login: 'alice', twitchUserId: 'alice', isPartner: true }];
  else if (path.endsWith('/internal-home'))
    data = { profile: {
      twitch_login: fixture.owner,
      display_name: `Home ${new Headers(init.headers).get('X-Dashboard-Language') === 'en' ? 'EN' : 'DE'}`,
    } };
  else if (path.endsWith('/onboarding')) data = { step: 'completed', completed: true, steps: [] };
  else if (path.endsWith('/counts')) data = { unread: 0, total: 0 };
  else throw new Error(`Unexpected synthetic request: ${path}`);
  const failed = fixture.fail.has(path) && method === 'GET';
  const failure = fixture.failures.get(`${method} ${path}`);
  const result = new Response(JSON.stringify(failure?.body ?? (failed ? { error: 'Unbekannter Fehler' } : data)), {
    status: failure?.status ?? (failed ? 503 : 200),
    headers: { 'Content-Type': 'application/json' },
  });
  if (fixture.hold.has(path)) await new Promise<void>((resolve) => fixture.held.push(resolve));
  fixture.completed += 1;
  return result;
};
