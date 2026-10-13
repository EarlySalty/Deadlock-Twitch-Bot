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
import { tr } from '../i18n/dictionary';

function scopeCatalog(): ScopeDefinition[] {
  return [
    {
      id: 'channel:manage:raids',
      label: tr('Auto-Raids'),
      description: tr('Raids direkt aus Deadlock vorbereiten und auslösen.'),
      why: tr('Ohne diesen Scope können wir keine Raid-Links oder Auto-Raid-Workflows starten.'),
      importance: 'required',
      addedAt: '2026-02-01',
    },
    {
      id: 'channel:manage:moderators',
      label: tr('Mods verwalten'),
      description: tr('Mod-Status prüfen oder setzen (z. B. für den Bot).'),
      why: tr('Self-Service Onboarding: Bot kann sich selbst als Mod hinzufügen/prüfen.'),
      importance: 'optional',
    },
    {
      id: 'channel:bot',
      label: tr('Channel Bot'),
      description: tr('Neuer Twitch-Bot-Scope für Channel-spezifische Funktionen.'),
      why: tr('Aktiviert Deadlock-spezifische Automationen direkt im Channel.'),
      importance: 'required',
      addedAt: '2026-02-01',
    },
    {
      id: 'clips:edit',
      label: tr('Clips'),
      description: tr('Clips direkt aus dem Dashboard anlegen.'),
      why: tr('Für Highlight-Empfehlungen und Coaching-To-Dos.'),
      importance: 'optional',
    },
    {
      id: 'channel:read:ads',
      label: tr('Ads'),
      description: tr('Ad-Breaks und Restzeiten lesen.'),
      why: tr('Zeigt Ad-Snooze-Fenster und sorgt für saubere Revenue-Metriken.'),
      importance: 'required',
      addedAt: '2026-02-15',
    },
    {
      id: 'bits:read',
      label: tr('Bits'),
      description: tr('Bits-Events und Cheermotes erfassen.'),
      why: tr('Kritisch für Umsatz-Tracking und Hype-Highlights.'),
      importance: 'critical',
      addedAt: '2026-02-15',
    },
    {
      id: 'channel:read:redemptions',
      label: tr('Channel Points'),
      description: tr('Channel-Point-Redemptions abrufen.'),
      why: tr('Trigger für Aktionen und Analytics auf Punktebene.'),
      importance: 'critical',
      addedAt: '2026-02-15',
    },
  ];
}

const CRITICAL_SCOPE_IDS = new Set<string>([
  'channel:read:redemptions',
  'bits:read',
]);

function scopeChangelog(): ScopeChangelogEntry[] {
  return [
    {
      date: '2026-03-18',
      title: tr('Basis-Auth entschlackt'),
      items: [
        tr('Neu-Auth fragt channel:read:subscriptions und channel:read:hype_train nicht mehr im ersten Consent ab.'),
        tr('Sub-Ereignisse koennen initial ueber channel.chat.notification als Fallback mitlaufen.'),
        tr('Monetization-Upgrade-Scopes werden spaeter gezielt fuer Dashboard-Erweiterungen nachgezogen.'),
      ],
      tags: [tr('Auth Split'), tr('Consent')],
    },
    {
      date: '2026-02-20',
      title: tr('Channel Points, Bits & Hype-Train Tracking'),
      items: [
        tr('Neu: channel:read:redemptions für Punkt-Redemptions in Analytics & Triggers.'),
        tr('Neu: bits:read und channel:read:hype_train für Umsatz- und Hype-Auswertungen.'),
        tr('Hinweis: Backend-Hooks folgen – aktuell rein visuelles Preview.'),
      ],
      tags: [tr('Beispiel'), tr('Backend folgt')],
    },
    {
      date: '2026-03-16',
      title: tr('Bot- vs. Streamer-Scopes getrennt'),
      items: [
        tr('Streamer-Matrix bleibt bei Broadcaster-Scopes wie channel:bot und channel:manage:raids.'),
        tr('Bot-Scopes wie user:read:chat laufen zentral auf dem Bot-Account und sind keine Streamer-Pflicht.'),
        tr('user:bot bleibt für den aktuellen User-Token-Chat-Flow optional und wird erst bei App-Token-EventSub Pflicht.'),
      ],
      tags: [tr('Scope Modell')],
    },
  ];
}

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

  const catalog = useMemo(() => scopeCatalog(), []);
  const changelog = useMemo(() => scopeChangelog(), []);
  const grantedSet = useMemo(() => parseScopes(input), [input]);

  const statuses = useMemo<ScopeStatus[]>(
    () =>
      catalog.map(scope => ({
        ...scope,
        status: grantedSet.has(scope.id) ? 'granted' : 'missing',
      })),
    [catalog, grantedSet],
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
      lastChange: changelog[0]?.date,
    }),
    [statuses.length, grantedCount, missing, criticalMissing, coverage, changelog],
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
      scopeCatalog: catalog,
      changelog,
      setFromList,
      reset,
    }),
    [input, statuses, summary, catalog, changelog, setFromList, reset],
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
