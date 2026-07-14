import { useQuery } from '@tanstack/react-query';
import { ChevronRight, LogOut, Search, ShieldCheck, Wifi, X } from 'lucide-react';
import { useDeferredValue, useEffect, useRef, useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import { fetchAdminStreamers } from '@/api/client';
import type { AdminAuthStatus, StreamerRow } from '@/api/types';

interface TopBarProps {
  auth?: AdminAuthStatus;
}

const BREADCRUMB_LABELS: Record<string, string> = {
  operations: 'Operations',
  system: 'System Health',
  scopes: 'Scopes & OAuth',
  eventsub: 'EventSub',
  database: 'Database',
  errors: 'Error Logs',
  bot: 'Bot Control',
  community: 'Community',
  streamers: 'Streamer',
  raids: 'Raids',
  engagement: 'Engagement AI',
  chat: 'Chat Actions',
  content: 'Content & Comms',
  announcements: 'Announcements',
  roadmap: 'Roadmap',
  changelog: 'Changelog',
  legal: 'Legal Pages',
  money: 'Money & Compliance',
  subscriptions: 'Subscriptions',
  affiliates: 'Affiliates',
  gutschriften: 'Gutschriften',
  audit: 'Audit Log',
  config: 'Konfiguration',
  billing: 'Billing',
  monitoring: 'Monitoring',
};

function buildBreadcrumbs(pathname: string) {
  const trimmed = pathname.replace(/^\/+|\/+$/g, '');
  if (!trimmed) {
    return [{ label: 'Home', to: '/' }];
  }
  const parts = trimmed.split('/');
  return parts.map((part, index) => ({
    label: BREADCRUMB_LABELS[part] || decodeURIComponent(part),
    to: `/${parts.slice(0, index + 1).join('/')}`,
  }));
}

function matchesStreamer(row: StreamerRow, query: string) {
  const normalizedQuery = query.toLowerCase();
  return [row.login, row.displayName, row.discordDisplayName]
    .filter(Boolean)
    .some((value) => String(value).toLowerCase().includes(normalizedQuery));
}

function rankStreamer(row: StreamerRow, query: string) {
  const normalizedQuery = query.toLowerCase();
  const login = row.login.toLowerCase();
  const displayName = String(row.displayName || '').toLowerCase();
  if (login === normalizedQuery) {
    return 0;
  }
  if (login.startsWith(normalizedQuery)) {
    return 1;
  }
  if (displayName.startsWith(normalizedQuery)) {
    return 2;
  }
  return 3;
}

export function TopBar({ auth }: TopBarProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const breadcrumbs = buildBreadcrumbs(location.pathname);
  const logoutHref = auth?.user?.authType === 'discord_admin' ? '/twitch/auth/discord/logout' : '/twitch/auth/logout';
  const [query, setQuery] = useState('');
  const [isOpen, setIsOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const searchRef = useRef<HTMLDivElement | null>(null);
  const deferredQuery = useDeferredValue(query);

  const streamersQuery = useQuery({
    queryKey: ['admin-streamers', 'all'],
    queryFn: () => fetchAdminStreamers('all'),
    staleTime: 60_000,
  });

  const trimmedQuery = deferredQuery.trim().toLowerCase();
  const matches = trimmedQuery
    ? [...(streamersQuery.data ?? [])]
        .filter((row) => matchesStreamer(row, trimmedQuery))
        .sort((left, right) => {
          const rankDiff = rankStreamer(left, trimmedQuery) - rankStreamer(right, trimmedQuery);
          if (rankDiff !== 0) {
            return rankDiff;
          }
          return left.login.localeCompare(right.login, 'de');
        })
        .slice(0, 8)
    : [];

  useEffect(() => {
    setIsOpen(false);
    setActiveIndex(-1);
    setQuery('');
  }, [location.pathname]);

  useEffect(() => {
    function handlePointerDown(event: MouseEvent) {
      if (!searchRef.current?.contains(event.target as Node)) {
        setIsOpen(false);
        setActiveIndex(-1);
      }
    }

    document.addEventListener('mousedown', handlePointerDown);
    return () => document.removeEventListener('mousedown', handlePointerDown);
  }, []);

  useEffect(() => {
    setActiveIndex(-1);
    if (!trimmedQuery) {
      setIsOpen(false);
      return;
    }
    setIsOpen(true);
  }, [trimmedQuery]);

  function navigateToStreamer(login: string) {
    setIsOpen(false);
    setActiveIndex(-1);
    setQuery('');
    navigate(`/community/streamers/${encodeURIComponent(login)}`);
  }

  return (
    <header className="glass sticky top-4 z-20 rounded-[1.8rem] px-5 py-4">
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,32rem)_auto] lg:items-center">
        <div className="flex flex-wrap items-center gap-2 text-sm text-text-secondary">
          {breadcrumbs.map((crumb, index) => (
            <div key={crumb.to} className="flex items-center gap-2">
              {index > 0 ? <ChevronRight className="h-4 w-4" /> : null}
              <Link to={crumb.to} className={index === breadcrumbs.length - 1 ? 'text-white' : 'hover:text-white'}>
                {crumb.label}
              </Link>
            </div>
          ))}
        </div>

        <div ref={searchRef} className="relative w-full md:max-w-md lg:justify-self-center">
          <label className="relative block">
            <Search className="pointer-events-none absolute left-4 top-1/2 h-4 w-4 -translate-y-1/2 text-text-secondary" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onFocus={() => {
                if (trimmedQuery) {
                  setIsOpen(true);
                }
              }}
              onKeyDown={(event) => {
                if (event.key === 'Escape') {
                  setIsOpen(false);
                  setActiveIndex(-1);
                  return;
                }
                if (!matches.length) {
                  return;
                }
                if (event.key === 'ArrowDown') {
                  event.preventDefault();
                  setIsOpen(true);
                  setActiveIndex((current) => (current >= matches.length - 1 ? 0 : current + 1));
                  return;
                }
                if (event.key === 'ArrowUp') {
                  event.preventDefault();
                  setIsOpen(true);
                  setActiveIndex((current) => (current <= 0 ? matches.length - 1 : current - 1));
                  return;
                }
                if (event.key === 'Enter') {
                  event.preventDefault();
                  const target = matches[activeIndex >= 0 ? activeIndex : 0];
                  if (target) {
                    navigateToStreamer(target.login);
                  }
                }
              }}
              placeholder="Streamer suchen …"
              className="admin-input rounded-full border-white/10 bg-[rgba(20, 13, 10,0.76)] py-3 pl-11 pr-11 text-sm"
            />
            {query ? (
              <button
                type="button"
                className="absolute right-3 top-1/2 -translate-y-1/2 rounded-full p-1 text-text-secondary transition hover:text-white"
                onClick={() => {
                  setQuery('');
                  setIsOpen(false);
                  setActiveIndex(-1);
                }}
              >
                <X className="h-4 w-4" />
              </button>
            ) : null}
          </label>

          {isOpen ? (
            <div className="panel-card absolute left-0 right-0 top-[calc(100%+0.65rem)] z-30 overflow-hidden rounded-[1.35rem] border border-white/10 p-2">
              {streamersQuery.isLoading && !streamersQuery.data ? (
                <div className="px-3 py-3 text-sm text-text-secondary">Streamer werden geladen …</div>
              ) : streamersQuery.isError ? (
                <div className="px-3 py-3 text-sm text-text-secondary">Suche ist gerade nicht verfügbar.</div>
              ) : matches.length ? (
                <div className="space-y-1">
                  {matches.map((row, index) => (
                    <button
                      key={row.login}
                      type="button"
                      className={[
                        'interactive-surface flex w-full items-center justify-between rounded-[1rem] px-3 py-3 text-left',
                        index === activeIndex ? 'bg-white/10 text-white' : 'bg-transparent text-text-secondary hover:bg-white/6 hover:text-white',
                      ].join(' ')}
                      onMouseEnter={() => setActiveIndex(index)}
                      onClick={() => navigateToStreamer(row.login)}
                    >
                      <div className="min-w-0">
                        <div className="truncate font-semibold text-white">{row.displayName || row.login}</div>
                        <div className="truncate text-xs uppercase tracking-[0.16em] text-text-secondary">{row.login}</div>
                      </div>
                      <ChevronRight className="h-4 w-4 shrink-0 text-text-secondary" />
                    </button>
                  ))}
                </div>
              ) : trimmedQuery ? (
                <div className="px-3 py-3 text-sm text-text-secondary">Keine Treffer.</div>
              ) : null}
            </div>
          ) : null}
        </div>

        <div className="flex flex-wrap items-center gap-3 lg:justify-self-end">
          <span className="stat-pill">
            {auth?.isLocalhost ? <Wifi className="h-4 w-4" /> : <ShieldCheck className="h-4 w-4" />}
            {auth?.isLocalhost ? 'Localhost Admin' : 'Discord Admin'}
          </span>
          <div className="rounded-full border border-white/10 bg-white/5 px-4 py-2 text-sm">
            <span className="text-text-secondary">Angemeldet als </span>
            <span className="font-semibold text-white">
              {auth?.user?.displayName || auth?.user?.login || 'Admin'}
            </span>
          </div>
          <a className="admin-button admin-button-secondary" href={logoutHref}>
            <LogOut className="h-4 w-4" />
            Logout
          </a>
        </div>
      </div>
    </header>
  );
}
