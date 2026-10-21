export function currentPathname(): string {
  const win = (globalThis as { window?: { location?: { pathname?: string } } }).window;
  return win?.location?.pathname ?? '';
}

export function onPublicTour(): boolean {
  return isPublicTourPath(currentPathname());
}

function normalizeDashboardPath(pathname: string): string {
  return pathname.replace(/\/+$/, '') || '/';
}

export function isAnalyticsDemoPath(pathname: string): boolean {
  const normalized = normalizeDashboardPath(pathname);
  return (
    normalized === '/twitch/demo' ||
    normalized.startsWith('/twitch/demo/') ||
    normalized === '/demo/twitch/demo' ||
    normalized.startsWith('/demo/twitch/demo/')
  );
}

export function isPublicTourPath(pathname: string): boolean {
  const normalized = normalizeDashboardPath(pathname);
  return (
    normalized === '/twitch/rundgang' ||
    normalized.startsWith('/twitch/rundgang/') ||
    normalized === '/demo/twitch/rundgang' ||
    normalized.startsWith('/demo/twitch/rundgang/')
  );
}

export function tourBasePath(pathname: string): string {
  const normalized = normalizeDashboardPath(pathname);
  if (normalized === '/demo/twitch/rundgang' || normalized.startsWith('/demo/twitch/rundgang/')) {
    return '/demo/twitch/rundgang';
  }
  return '/twitch/rundgang';
}

export function livePathFromTourPath(pathname: string): string | null {
  const normalized = normalizeDashboardPath(pathname);
  for (const prefix of ['/demo/twitch/rundgang', '/twitch/rundgang'] as const) {
    if (normalized === prefix) {
      return '/twitch/dashboard';
    }
    if (normalized.startsWith(`${prefix}/`)) {
      const rest = normalized.slice(prefix.length);
      if (rest.startsWith('/dashboard-v2')) {
        return null;
      }
      if (rest === '/dashboard') {
        return '/twitch/dashboard';
      }
      return `/twitch${rest}`;
    }
  }
  return null;
}

export function tourHref(livePath: string, pathname: string): string {
  if (!isPublicTourPath(pathname)) {
    return livePath;
  }
  const url = new URL(livePath, 'https://deutsche-deadlock-community.de');
  const live = normalizeDashboardPath(url.pathname);
  let rest: string;
  if (live === '/twitch/dashboard' || live === '/twitch') {
    rest = '';
  } else if (live.startsWith('/twitch/')) {
    rest = live.slice('/twitch'.length);
  } else {
    rest = live;
  }
  const href = rest ? `${tourBasePath(pathname)}${rest}` : tourBasePath(pathname);
  return `${href}${url.search}${url.hash}`;
}
