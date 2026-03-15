import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import type {
  ScopeChangelogEntry,
  ScopeDefinition,
  ScopeStatus,
  ScopeSummary,
} from '@/types/scopes';

const scopeCatalog: ScopeDefinition[] = [
  {
    id: 'channel:manage:raids',
    label: 'Auto-Raids',
    description: 'Raids direkt aus Deadlock vorbereiten und auslösen.',
    why: 'Ohne diesen Scope können wir keine Raid-Links oder Auto-Raid-Workflows starten.',
    importance: 'required',
    addedAt: '2026-02-01',
  },
  {
    id: 'channel:read:subscriptions',
    label: 'Subs lesen',
    description: 'Sub-Events inkl. Tiers, Gifts und Prime.',
    why: 'Kritisch für Revenue-KPIs, Alerts und Loyalty-Analysen.',
    importance: 'critical',
  },
  {
    id: 'channel:manage:moderators',
    label: 'Mods verwalten',
    description: 'Mod-Status prüfen oder setzen (z. B. für den Bot).',
    why: 'Self-Service Onboarding: Bot kann sich selbst als Mod hinzufügen/prüfen.',
    importance: 'optional',
  },
  {
    id: 'channel:bot',
    label: 'Channel Bot',
    description: 'Neuer Twitch-Bot-Scope für Channel-spezifische Funktionen.',
    why: 'Aktiviert Deadlock-spezifische Automationen direkt im Channel.',
    importance: 'required',
    addedAt: '2026-02-01',
  },
  {
    id: 'clips:edit',
    label: 'Clips',
    description: 'Clips direkt aus dem Dashboard anlegen.',
    why: 'Für Highlight-Empfehlungen und Coaching-To-Dos.',
    importance: 'optional',
  },
  {
    id: 'channel:read:ads',
    label: 'Ads',
    description: 'Ad-Breaks und Restzeiten lesen.',
    why: 'Zeigt Ad-Snooze-Fenster und sorgt für saubere Revenue-Metriken.',
    importance: 'required',
    addedAt: '2026-02-15',
  },
  {
    id: 'bits:read',
    label: 'Bits',
    description: 'Bits-Events und Cheermotes erfassen.',
    why: 'Kritisch für Umsatz-Tracking und Hype-Highlights.',
    importance: 'critical',
    addedAt: '2026-02-15',
  },
  {
    id: 'channel:read:hype_train',
    label: 'Hype Train',
    description: 'Hype-Train-Events live lesen.',
    why: 'Für Alerts, KPIs und Funnel-Messung bei Hype-Trains.',
    importance: 'critical',
    addedAt: '2026-02-15',
  },
  {
    id: 'channel:read:redemptions',
    label: 'Channel Points',
    description: 'Channel-Point-Redemptions abrufen.',
    why: 'Trigger für Aktionen und Analytics auf Punktebene.',
    importance: 'critical',
    addedAt: '2026-02-15',
  },
];

const CRITICAL_SCOPE_IDS = new Set<string>([
  'channel:read:redemptions',
  'bits:read',
  'channel:read:hype_train',
  'channel:read:subscriptions',
]);

const scopeChangelog: ScopeChangelogEntry[] = [
  {
    date: '2026-02-20',
    title: 'Channel Points, Bits & Hype-Train Tracking',
    items: [
      'Neu: channel:read:redemptions für Punkt-Redemptions in Analytics & Triggers.',
      'Neu: bits:read und channel:read:hype_train für Umsatz- und Hype-Auswertungen.',
      'Hinweis: Backend-Hooks folgen – aktuell rein visuelles Preview.',
    ],
    tags: ['Beispiel', 'Backend folgt'],
  },
  {
    date: '2026-02-01',
    title: 'Raid-Bot Refresh',
    items: [
      'Erforderlich: channel:bot + channel:manage:raids für den neuen Auto-Raid-Flow.',
      'Empfohlen: channel:manage:moderators für Self-Service Bot-Mod im Onboarding.',
    ],
    tags: ['Raid Bot'],
  },
];

const STORAGE_KEY = 'deadlock.scopeCheck.input';

function parseScopes(raw: string): Set<string> {
  if (!raw) return new Set<string>();
  return new Set(
    raw
      .split(/[\s,]+/)
      .map(part => part.trim())
      .filter(Boolean),
  );
}

function getInitialInput(): string {
  if (typeof window === 'undefined') return '';
  const params = new URLSearchParams(window.location.search);
  const fromUrl = params.get('scopes') || params.get('scope') || params.get('granted');
  if (fromUrl) return fromUrl;
  const stored = window.localStorage.getItem(STORAGE_KEY);
  return stored || '';
}

interface ScopeContextValue {
  input: string;
  setInput: (value: string) => void;
  statuses: ScopeStatus[];
  summary: ScopeSummary;
  scopeCatalog: ScopeDefinition[];
  changelog: ScopeChangelogEntry[];
  setFromList: (values: Iterable<string>) => void;
  reset: () => void;
}

const ScopeContext = createContext<ScopeContextValue | null>(null);

export function ScopeProvider({ children }: { children: ReactNode }) {
  const [input, setInput] = useState<string>(() => getInitialInput());

  useEffect(() => {
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(STORAGE_KEY, input);
    }
  }, [input]);

  const grantedSet = useMemo(() => parseScopes(input), [input]);

  const statuses = useMemo<ScopeStatus[]>(
    () =>
      scopeCatalog.map(scope => ({
        ...scope,
        status: grantedSet.has(scope.id) ? 'granted' : 'missing',
      })),
    [grantedSet],
  );

  const missing = useMemo(() => statuses.filter(s => s.status === 'missing'), [statuses]);
  const criticalMissing = useMemo(
    () => missing.filter(s => CRITICAL_SCOPE_IDS.has(s.id)),
    [missing],
  );

  const grantedCount = statuses.length - missing.length;
  const coverage = statuses.length ? Math.round((grantedCount / statuses.length) * 100) : 0;

  const summary = useMemo<ScopeSummary>(
    () => ({
      total: statuses.length,
      granted: grantedCount,
      missing,
      criticalMissing,
      coverage,
      lastChange: scopeChangelog[0]?.date,
    }),
    [statuses.length, grantedCount, missing, criticalMissing],
  );

  const setFromList = useCallback(
    (values: Iterable<string>) => setInput(Array.from(values).join(' ')),
    [],
  );

  const reset = useCallback(() => setInput(''), []);

  const value = useMemo<ScopeContextValue>(
    () => ({
      input,
      setInput,
      statuses,
      summary,
      scopeCatalog,
      changelog: scopeChangelog,
      setFromList,
      reset,
    }),
    [input, statuses, summary, setFromList, reset],
  );

  return <ScopeContext.Provider value={value}>{children}</ScopeContext.Provider>;
}

export function useScopes(): ScopeContextValue {
  const ctx = useContext(ScopeContext);
  if (!ctx) {
    throw new Error('useScopes must be used within a ScopeProvider');
  }
  return ctx;
}

export { scopeCatalog, scopeChangelog, CRITICAL_SCOPE_IDS };
