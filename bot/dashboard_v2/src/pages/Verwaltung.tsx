import { motion } from 'framer-motion';
import { useQuery } from '@tanstack/react-query';
import { fetchInternalHome } from '@/api/client';
import { useAuthStatus } from '@/hooks/useAnalytics';
import {
  ArrowLeft,
  ArrowRight,
  Loader2,
  MessageSquare,
  ShieldAlert,
  ShieldCheck,
  User,
} from 'lucide-react';

export function VerwaltungPage() {
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();

  const { data, isLoading, isError, error, refetch } = useQuery({
    queryKey: ['internal-home', null],
    queryFn: () => fetchInternalHome(null),
    staleTime: Number.POSITIVE_INFINITY,
    enabled: !loadingAuth,
  });

  if (isLoading || loadingAuth) {
    return (
      <div className="min-h-screen relative px-3 py-4 md:px-7 md:py-8">
        <div className="relative max-w-[900px] mx-auto">
          <div className="panel-card rounded-2xl p-6 md:p-8">
            <div className="flex items-center gap-3 text-text-secondary">
              <Loader2 className="h-5 w-5 animate-spin text-primary" />
              <span>Konto wird geladen ...</span>
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (isError) {
    const errorMessage = error instanceof Error ? error.message : 'Unbekannter Fehler';
    return (
      <div className="min-h-screen relative px-3 py-4 md:px-7 md:py-8">
        <div className="relative max-w-[900px] mx-auto">
          <div className="panel-card rounded-2xl p-6 md:p-8">
            <h2 className="text-xl font-bold text-white">Konto-Daten nicht verfügbar</h2>
            <p className="mt-1 text-sm text-text-secondary">{errorMessage}</p>
            <button
              onClick={() => void refetch()}
              className="mt-4 inline-flex items-center gap-2 rounded-lg border border-border bg-card px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover"
            >
              <ArrowRight className="h-4 w-4" />
              Erneut laden
            </button>
          </div>
        </div>
      </div>
    );
  }

  const home = data ?? {};
  const twitchLogin = home.twitchLogin?.trim() || '';
  const displayName = home.displayName?.trim() || twitchLogin || 'Creator';
  const grantedScopes = home.oauth?.grantedScopes ?? [];
  const missingScopes = home.oauth?.missingScopes ?? [];
  const needsReauth = Boolean(home.oauth?.needsReauth) || home.oauth?.status === 'reauth';
  const missingScopeCount = missingScopes.length;
  const hasScopeIssue = needsReauth || missingScopeCount > 0 || home.oauth?.status === 'partial' || home.oauth?.status === 'missing';
  const oauthStatus = home.oauth?.status || (home.oauth?.connected ? 'connected' : hasScopeIssue ? 'missing' : 'partial');
  const oauthFallbackUrl = '/twitch/auth/login?next=%2Ftwitch%2Fdashboard';
  const discordConnectFallbackUrl = '/twitch/auth/discord/login?next=%2Ftwitch%2Fdashboard';
  const reconnectUrl = home.oauth?.reconnectUrl || oauthFallbackUrl;
  const discordConnected = Boolean(home.discord?.connected);
  const discordConnectUrl = home.discord?.connectUrl || discordConnectFallbackUrl;
  const userId = (authStatus as any)?.userId || (home as any)?.userId || '';

  const oauthConnected = oauthStatus === 'connected' && !hasScopeIssue;
  const oauthStatusText = oauthConnected ? 'Verbunden' : needsReauth ? 'Re-Auth nötig' : 'Unvollständig';
  const oauthStatusClass = oauthConnected ? 'text-success' : needsReauth ? 'text-error' : 'text-warning';
  const oauthHintText = oauthConnected
    ? 'Twitch-OAuth ist aktiv und vollständig.'
    : needsReauth
      ? 'Das bestehende Twitch-OAuth ist zur Re-Auth markiert. Bitte neu autorisieren.'
      : missingScopeCount > 1
      ? `${missingScopeCount} Scopes fehlen. Neu autorisieren, um alle Funktionen zu nutzen.`
      : '1 Scope fehlt. Bitte neu autorisieren.';

  return (
    <div className="internal-home-vibe min-h-screen relative px-3 py-4 md:px-7 md:py-8">
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -top-32 right-[-8rem] h-[28rem] w-[28rem] rounded-full bg-primary/22 blur-3xl" />
        <div className="absolute top-[30%] -left-28 h-[20rem] w-[20rem] rounded-full bg-accent/20 blur-3xl" />
      </div>

      <div className="relative max-w-[900px] mx-auto space-y-4 md:space-y-5">

        {/* Hero */}
        <motion.section
          className="panel-card rounded-2xl p-5 md:p-8"
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.32 }}
        >
          <div className="space-y-4">
            <div className="inline-flex items-center gap-2 rounded-full border border-border bg-card px-4 py-1.5 text-sm font-medium text-text-secondary">
              <User className="h-3.5 w-3.5 text-primary" />
              Konto
            </div>
            <div className="space-y-2">
              <h1 className="display-font text-4xl font-bold leading-tight md:text-5xl">
                Dein{' '}
                <span className="bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
                  Konto
                </span>{' '}
                verwalten
              </h1>
              <p className="max-w-2xl text-sm text-text-secondary md:text-base">
                OAuth-Status, Discord-Verbindung und Profil-Details auf einen Blick.
              </p>
            </div>
            <a
              href="/twitch/dashboard"
              className="inline-flex items-center gap-2 text-sm text-text-secondary transition-colors hover:text-white"
            >
              <ArrowLeft className="h-4 w-4" />
              Zurück zur Startseite
            </a>
          </div>
        </motion.section>

        {/* Twitch OAuth Section */}
        <motion.section
          className="panel-card rounded-2xl p-5 md:p-6"
          initial={{ opacity: 0, y: 16 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.32, delay: 0.04 }}
        >
          <div className="mb-5">
            <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">OAuth</p>
            <h2 className="display-font text-2xl font-bold text-white mb-1">Twitch-Verbindung</h2>
          </div>

          <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4 mb-4">
            <div className="flex items-start gap-3">
              <div className="w-10 h-10 rounded-lg gradient-accent flex items-center justify-center shrink-0">
                {oauthConnected
                  ? <ShieldCheck className="h-5 w-5 text-white" />
                  : <ShieldAlert className="h-5 w-5 text-white" />}
              </div>
              <div className="min-w-0 flex-1">
                <p className={`text-base font-bold ${oauthStatusClass}`}>{oauthStatusText}</p>
                <p className="mt-0.5 text-xs text-text-secondary">{oauthHintText}</p>
              </div>
            </div>
          </div>

          {/* Scope chips */}
          {(grantedScopes.length > 0 || missingScopes.length > 0) && (
            <div className="mb-5 space-y-2">
              {grantedScopes.length > 0 && (
                <div>
                  <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-text-secondary">Aktive Scopes ({grantedScopes.length})</p>
                  <div className="flex flex-wrap gap-1.5">
                    {grantedScopes.map((scope: string) => (
                      <span key={scope} className="rounded-full border border-success/30 bg-success/10 px-2.5 py-0.5 text-[11px] font-medium text-success">
                        {scope}
                      </span>
                    ))}
                  </div>
                </div>
              )}
              {missingScopes.length > 0 && (
                <div>
                  <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-text-secondary">Fehlende Scopes ({missingScopes.length})</p>
                  <div className="flex flex-wrap gap-1.5">
                    {missingScopes.map((scope: string) => (
                      <span key={scope} className="rounded-full border border-error/40 bg-error/10 px-2.5 py-0.5 text-[11px] font-medium text-error">
                        {scope}
                      </span>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}

          <a
            href={reconnectUrl}
            className="inline-flex items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-5 py-2.5 text-sm font-semibold text-primary transition-colors hover:border-primary/60 hover:bg-primary/20"
          >
            <ShieldCheck className="h-4 w-4" />
            Jetzt neu autorisieren
          </a>
        </motion.section>

        {/* Discord Section */}
        <motion.section
          className="panel-card rounded-2xl p-5 md:p-6"
          initial={{ opacity: 0, y: 16 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.32, delay: 0.08 }}
        >
          <div className="mb-5">
            <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Discord</p>
            <h2 className="display-font text-2xl font-bold text-white mb-1">Discord verbinden</h2>
          </div>

          <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4 mb-5">
            <div className="flex items-start gap-3">
              <div className="w-10 h-10 rounded-lg gradient-accent flex items-center justify-center shrink-0">
                <MessageSquare className="h-5 w-5 text-white" />
              </div>
              <div className="min-w-0 flex-1">
                <p className={`text-base font-bold ${discordConnected ? 'text-success' : 'text-warning'}`}>
                  {discordConnected ? 'Verbunden' : 'Nicht verbunden'}
                </p>
                <p className="mt-0.5 text-xs text-text-secondary">
                  {discordConnected ? 'Discord-Verknüpfung erkannt.' : 'Noch kein Discord-Profil verknüpft.'}
                </p>
              </div>
            </div>
          </div>

          <a
            href={discordConnectUrl}
            className="inline-flex items-center gap-2 rounded-lg border border-accent/40 bg-accent/10 px-5 py-2.5 text-sm font-semibold text-accent transition-colors hover:border-accent/60 hover:bg-accent/20"
          >
            <MessageSquare className="h-4 w-4" />
            {discordConnected ? 'Erneut verbinden' : 'Discord verknüpfen'}
          </a>
        </motion.section>

        {/* Profile Section */}
        <motion.section
          className="panel-card rounded-2xl p-5 md:p-6"
          initial={{ opacity: 0, y: 16 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.32, delay: 0.12 }}
        >
          <div className="mb-5">
            <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Profil</p>
            <h2 className="display-font text-2xl font-bold text-white mb-1">Account-Details</h2>
          </div>

          <div className="grid gap-3 sm:grid-cols-3 mb-4">
            <div className="soft-elevate rounded-xl border border-border bg-background/60 p-3.5">
              <p className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary mb-1.5">Twitch Login</p>
              <p className="text-sm font-semibold text-white font-mono">{twitchLogin ? `@${twitchLogin}` : '–'}</p>
            </div>
            <div className="soft-elevate rounded-xl border border-border bg-background/60 p-3.5">
              <p className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary mb-1.5">Display Name</p>
              <p className="text-sm font-semibold text-white">{displayName || '–'}</p>
            </div>
            <div className="soft-elevate rounded-xl border border-border bg-background/60 p-3.5">
              <p className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary mb-1.5">User-ID</p>
              <p className="text-sm font-semibold text-white font-mono">{userId || '–'}</p>
            </div>
          </div>

          <div className="rounded-xl border border-border/50 bg-background/40 px-4 py-3 text-xs text-text-secondary">
            Profiländerungen direkt auf Twitch vornehmen. Daten werden beim nächsten Login synchronisiert.
          </div>
        </motion.section>

      </div>
    </div>
  );
}
