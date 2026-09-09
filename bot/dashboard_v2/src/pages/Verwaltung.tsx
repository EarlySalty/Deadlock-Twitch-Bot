import { useEffect, useState, type ReactNode } from 'react';
import { motion } from 'framer-motion';
import { Rise } from '../motion/Rise';
import { useQuery } from '@tanstack/react-query';
import { fetchInternalHome } from '@/api/home';
import { OnboardingGuide } from '@/components/onboarding/OnboardingGuide';
import { useOnboarding } from '@/components/onboarding/onboardingState';
import { FeedbackBox } from '@/components/feedback/FeedbackBox';
import { useAuthStatus } from '@/hooks/useAnalytics';
import { PREVIEW_HOME_ROUTE, PREVIEW_OVERLAY_ROUTE, isPreviewModeEnabled } from '@/preview/routes';
import { AIEngagementSection } from '@/components/verwaltung/AIEngagementSection';
import { AdManagerSection } from '@/components/verwaltung/AdManagerSection';
import { StatCommandSection } from '@/components/verwaltung/StatCommandSection';
import { SubReminderSection } from '@/components/verwaltung/SubReminderSection';
import { TitleCommandSection } from '@/components/verwaltung/TitleCommandSection';
import { ClipCommandSection } from '@/components/verwaltung/ClipCommandSection';
import { DisconnectBotSection } from '@/components/verwaltung/DisconnectBotSection';
import { GreetingSection } from '@/components/verwaltung/GreetingSection';
import { LurkCommandSection } from '@/components/verwaltung/LurkCommandSection';
import { LurkerTaxSection } from '@/components/verwaltung/LurkerTaxSection';
import { SilentNotificationsSection } from '@/components/verwaltung/SilentNotificationsSection';
import { ModerationSection } from '@/components/verwaltung/ModerationSection';
import { ScamGuardSection } from '@/components/verwaltung/ScamGuardSection';
import { resolveVerwaltungTab, type VerwaltungTabId } from '@/pages/verwaltungTabs';
import {
  ArrowRight,
  Bot,
  FlaskConical,
  Gamepad2,
  Loader2,
  MessageSquare,
  Monitor,
  ShieldAlert,
  ShieldCheck,
  Terminal,
  User,
} from 'lucide-react';

interface VerwaltungTabDef {
  id: VerwaltungTabId;
  label: string;
  icon: typeof User;
  render: () => ReactNode;
}

export function VerwaltungPage() {
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();
  const onboarding = useOnboarding();
  const [connectionNotice, setConnectionNotice] = useState('');
  const [returnMessage] = useState(() => {
    const params = new URLSearchParams(window.location.search);
    const failed = params.has('err');
    return params.has('ok') || failed ? {failed, text: failed ? 'Die Discord-Verknüpfung wurde nicht abgeschlossen. Du kannst sie hier erneut starten.' : 'Discord wurde verknüpft. Der aktuelle Status wird geprüft.'} : null;
  });

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    if (!params.has('ok') && !params.has('err')) return;
    params.delete('ok'); params.delete('err');
    const query = params.toString();
    window.history.replaceState(null, '', window.location.pathname + (query ? '?' + query : '') + window.location.hash);
  }, []);

  // Tab im Hash halten: Reload und geteilte Links landen wieder im selben Bereich.
  const [tab, setTab] = useState<VerwaltungTabId>(() =>
    resolveVerwaltungTab(typeof window === 'undefined' ? '' : window.location.hash),
  );

  useEffect(() => {
    const onHashChange = () => setTab(resolveVerwaltungTab(window.location.hash));
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  }, []);

  const { data, isLoading, isError, error, refetch, isFetching } = useQuery({
    queryKey: ['internal-home', null, authStatus?.twitchUserId],
    queryFn: () => fetchInternalHome(null),
    staleTime: 30_000,
    refetchOnWindowFocus: 'always',
    enabled: !loadingAuth,
  });

  if (isLoading || loadingAuth) {
    return (
      <div className="panel-card rounded-2xl p-6 md:p-8">
        <div className="flex items-center gap-3 text-text-secondary">
          <Loader2 className="h-5 w-5 animate-spin text-primary" />
          <span>Konto wird geladen ...</span>
        </div>
      </div>
    );
  }

  if (isError && !data) {
    const errorMessage = error instanceof Error ? error.message : 'Unbekannter Fehler';
    return (
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
  const oauthFallbackUrl = isPreviewModeEnabled()
    ? PREVIEW_HOME_ROUTE
    : '/twitch/auth/login?next=%2Ftwitch%2Fdashboard';
  const reconnectUrl = home.oauth?.reconnectUrl || oauthFallbackUrl;
  const discordConnected = Boolean(home.discord?.connected);
  const discordError = isError || home.discord?.status === 'error';
  const steamError = isError || home.steam?.status === 'error';
  const checkConnections = async () => {
    setConnectionNotice('Verbindungen werden geprüft …');
    onboarding.refresh();
    const result = await refetch();
    const status = (service: 'discord' | 'steam') => result.isError || result.data?.[service]?.status === 'error'
      ? 'gerade nicht prüfbar' : result.data?.[service]?.connected ? 'verbunden' : 'nicht verbunden';
    setConnectionNotice(`Prüfung abgeschlossen. Discord ${status('discord')}. Steam ${status('steam')}.`);
  };
  const discordConnectUrl = home.discord?.connectUrl || null;
  const steamConnected = Boolean(home.steam?.connected);
  const steamConnectUrl = home.steam?.connectUrl || null;
  const userId = (authStatus as any)?.userId || (home as any)?.userId || '';
  // Eigener Kanal-Login für die Trenn-Aktion. Leer heißt: Aktion bleibt
  // gesperrt, statt gegen einen geratenen Login zu bestätigen.
  const selfLogin = String(
    (authStatus as any)?.twitchLogin || (home as any)?.twitchLogin || '',
  ).trim();

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
  const partnerStatus = String((authStatus as any)?.partnerStatus || '').trim().toLowerCase();
  const tokenErrorGraceExpiresAt = String((authStatus as any)?.tokenErrorGraceExpiresAt || '').trim();

  const kontoTab = (
    <>
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
                ? <ShieldCheck className="h-5 w-5 text-on-gold" />
                : <ShieldAlert className="h-5 w-5 text-on-gold" />}
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
        tabIndex={-1}
        data-tour-id="onboarding-discord"
        data-tour-ready="true"
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
              <MessageSquare className="h-5 w-5 text-on-gold" />
            </div>
            <div className="min-w-0 flex-1">
              <p className={`text-base font-bold ${discordConnected ? 'text-success' : 'text-warning'}`}>
                {isFetching ? 'Wird geprüft …' : discordError ? 'Gerade nicht prüfbar' : discordConnected ? 'Verbunden' : 'Nicht verbunden'}
              </p>
              <p className="mt-0.5 text-xs text-text-secondary">
                {discordError ? 'Der Status konnte nicht geprüft werden. Das bedeutet nicht, dass deine Verbindung fehlt.' : discordConnected ? 'Dein Discord-Konto ist mit Twitch verknüpft.' : 'Verbinde Discord zuerst, damit du danach Steam zuordnen kannst.'}
              </p>
            </div>
          </div>
        </div>

        {discordConnectUrl && !discordError ? (
          <a
            href={discordConnectUrl}
            className="inline-flex items-center gap-2 rounded-lg border border-accent/40 bg-accent/10 px-5 py-2.5 text-sm font-semibold text-accent transition-colors hover:border-accent/60 hover:bg-accent/20"
          >
            <MessageSquare className="h-4 w-4" />
            {discordConnected ? 'Erneut verbinden' : 'Discord verknüpfen'}
          </a>
        ) : (
          <div className="space-y-2">
            <button
              type="button"
              disabled
              className="inline-flex cursor-not-allowed items-center gap-2 rounded-lg border border-border bg-background/70 px-5 py-2.5 text-sm font-semibold text-text-secondary"
            >
              <MessageSquare className="h-4 w-4" />
              {discordError || isFetching ? 'Status prüfen' : discordConnected ? 'Discord verbunden' : 'Discord-Verbindung nicht verfügbar'}
            </button>
            {!discordConnected && !discordError && !isFetching && (
              <p className="text-xs text-text-secondary">
                Öffne deine persönliche Partner-Ansicht, um Discord zu verbinden.
              </p>
            )}
          </div>
        )}
      </motion.section>

      {/* Steam Section */}
      <motion.section
        tabIndex={-1}
        data-tour-id="onboarding-steam"
        data-tour-ready="true"
        className="panel-card rounded-2xl p-5 md:p-6"
        initial={{ opacity: 0, y: 16 }}
        whileInView={{ opacity: 1, y: 0 }}
        viewport={{ once: true }}
        transition={{ duration: 0.32, delay: 0.1 }}
      >
        <div className="mb-5">
          <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Steam</p>
          <h2 className="display-font text-2xl font-bold text-white mb-1">Steam verbinden</h2>
        </div>

        <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4 mb-5">
          <div className="flex items-start gap-3">
            <div className="w-10 h-10 rounded-lg gradient-accent flex items-center justify-center shrink-0">
              <Gamepad2 className="h-5 w-5 text-on-gold" />
            </div>
            <div className="min-w-0 flex-1">
              <p className={`text-base font-bold ${steamConnected ? 'text-success' : 'text-warning'}`}>
                {isFetching ? 'Wird geprüft …' : steamError ? 'Gerade nicht prüfbar' : steamConnected ? 'Verbunden' : 'Nicht verbunden'}
              </p>
              <p className="mt-0.5 text-xs text-text-secondary">
                {steamError ? 'Der Status konnte nicht geprüft werden. Eine bestehende Verbindung kann weiterhin gültig sein.' : steamConnected ? 'Dein Steam-Account ist verknüpft.' : discordConnected ? 'Verknüpfe Steam für deinen Deadlock-Rang und deine Spielstatistiken.' : 'Verbinde zuerst Discord, danach Steam.'}
              </p>
            </div>
          </div>
        </div>

        {steamConnectUrl && !steamError ? (
          <a
            href={steamConnectUrl}
            className="inline-flex items-center gap-2 rounded-lg border border-accent/40 bg-accent/10 px-5 py-2.5 text-sm font-semibold text-accent transition-colors hover:border-accent/60 hover:bg-accent/20"
          >
            <Gamepad2 className="h-4 w-4" />
            {steamConnected ? 'Erneut verknüpfen' : 'Steam verknüpfen'}
          </a>
        ) : (
          <div className="space-y-2">
            <button
              type="button"
              disabled
              className="inline-flex cursor-not-allowed items-center gap-2 rounded-lg border border-border bg-background/70 px-5 py-2.5 text-sm font-semibold text-text-secondary"
            >
              <Gamepad2 className="h-4 w-4" />
              Steam verknüpfen
            </button>
            <p className="text-xs text-text-secondary">
              {steamError || isFetching ? 'Bitte prüfe den Status erneut, bevor du ein Konto neu verknüpfst.' : discordConnected ? 'Der Verbindungslink ist gerade nicht verfügbar. Bitte prüfe den Status erneut.' : 'Verknüpfe zuerst deinen Discord-Account. Die Steam-Verknüpfung läuft darüber.'}
            </p>
          </div>
        )}
      </motion.section>

      <button type="button" onClick={checkConnections} disabled={isFetching} className="min-h-11 w-fit rounded-lg border border-primary/50 px-4 py-2 text-sm font-semibold text-primary disabled:opacity-50">Status prüfen</button>

      <p role="status" aria-atomic="true" className="text-sm text-text-secondary">{connectionNotice}</p>

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
    </>
  );

  const chatTab = (
    <>
      <GreetingSection />
      <LurkCommandSection />
      <ClipCommandSection />
      <TitleCommandSection />
      <StatCommandSection />
      <SubReminderSection />
      <LurkerTaxSection />
    </>
  );

  const botTab = (
    <>
      <AIEngagementSection />
      <ModerationSection />
      <ScamGuardSection />
      <SilentNotificationsSection />
      <DisconnectBotSection login={selfLogin} />
    </>
  );

  const overlayTab = (
    <motion.section
      tabIndex={-1}
      data-tour-id="onboarding-overlay"
      data-tour-ready="true"
      className="panel-card rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32, delay: 0.14 }}
    >
      <div className="mb-5">
        <p className="mb-1 text-sm font-medium uppercase tracking-wider text-primary">
          Stream-Overlay
        </p>
        <h2 className="display-font mb-1 text-2xl font-bold text-white">
          Overlay für OBS
        </h2>
        <p className="text-sm text-text-secondary">
          Stell dir dein Stream-Overlay zusammen — Rang, Winrate, Serie und Live-Match als einblendbare Karte für OBS.
        </p>
      </div>

      <a
        href={PREVIEW_OVERLAY_ROUTE}
        className="inline-flex items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-5 py-2.5 text-sm font-semibold text-primary transition-colors hover:border-primary/60 hover:bg-primary/20"
      >
        <ArrowRight className="h-4 w-4" />
        Overlay-Baukasten öffnen
      </a>
    </motion.section>
  );

  const werbungTab = <AdManagerSection reconnectUrl={reconnectUrl} />;

  const tabs: VerwaltungTabDef[] = [
    { id: 'konto', label: 'Konto & Verbindungen', icon: User, render: () => kontoTab },
    { id: 'chat', label: 'Chat-Befehle', icon: Terminal, render: () => chatTab },
    { id: 'bot', label: 'Bot & Schutz', icon: Bot, render: () => botTab },
    { id: 'overlay', label: 'Overlay', icon: Monitor, render: () => overlayTab },
    { id: 'werbung', label: 'Werbung (Experimentell)', icon: FlaskConical, render: () => werbungTab },
  ];
  const activeTab = tabs.find((item) => item.id === tab) ?? tabs[0];

  return (
    <>
        {partnerStatus && partnerStatus !== 'active' && partnerStatus !== 'blocked' ? (
          <Rise
            as="section"
            className="panel-card rounded-2xl border border-warning/30 bg-warning/10 p-5 md:p-6"
          >
            <div className="space-y-3">
              <h2 className="text-lg font-bold text-white">
                {partnerStatus === 'token_error'
                  ? 'Twitch OAuth braucht Re-Auth'
                  : partnerStatus === 'departnered'
                  ? 'Du bist aktuell kein aktiver Partner'
                  : partnerStatus === 'archived'
                  ? 'Dein Account ist im Admin-Archiv'
                  : 'Partner-Status inaktiv'}
              </h2>
              <p className="text-sm text-text-secondary">
                Verwaltung, Pläne und Affiliate bleiben offen. Analyse, Social Media und
                Title-Generator sind gesperrt. Eine erfolgreiche Twitch-OAuth setzt den Status
                automatisch wieder auf aktiv (außer Bot-Bann oder permanenter Block).
              </p>
              {tokenErrorGraceExpiresAt ? (
                <p className="text-xs text-text-secondary">
                  Grace endet: {tokenErrorGraceExpiresAt}
                </p>
              ) : null}
              <a
                href={reconnectUrl}
                className="inline-flex items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-4 py-2 text-sm font-semibold text-primary transition-colors hover:border-primary/60 hover:bg-primary/20"
              >
                <ShieldCheck className="h-4 w-4" />
                Jetzt neu autorisieren
              </a>
            </div>
          </Rise>
        ) : null}

        {returnMessage && <p role="status" className={`rounded-xl border p-4 text-sm ${returnMessage.failed ? 'border-warning/40 text-warning' : 'border-success/40 text-success'}`}>{returnMessage.text}</p>}
        {isError && data && <p role="alert" className="text-sm text-warning">Der aktuelle Kontostatus konnte nicht geladen werden. <button onClick={checkConnections} className="underline">Status prüfen</button></p>}
        <Rise as="section" className="panel-card rounded-2xl p-5 md:p-6">
          <div className="mb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-primary">
            Konto
          </div>
          <h1 className="display-font text-2xl font-extrabold text-white">Dein Bot, deine Einstellungen</h1>
          <p className="mt-2 max-w-2xl text-sm text-text-secondary">
            Hier verbindest du deine Konten und schaltest Funktionen an oder aus. Wähle einen Bereich und passe den Bot an deinen Kanal an.
          </p>
        </Rise>

        <nav aria-label="Verwaltungsbereiche" className="sticky top-2 z-20 flex flex-wrap gap-1.5 rounded-xl border border-border bg-card/90 p-1.5 backdrop-blur">
          {tabs.map((item) => {
            const Icon = item.icon;
            const isActive = item.id === activeTab.id;
            return (
              <a
                key={item.id}
                href={`#${item.id}`}
                aria-current={isActive ? 'page' : undefined}
                className={`flex min-h-11 items-center gap-1.5 rounded-lg px-3.5 py-2 text-sm font-semibold transition-colors ${
                  isActive ? 'bg-primary/85 text-bg' : 'text-text-secondary hover:text-white'
                }`}
              >
                <Icon className="h-4 w-4" />
                {item.label}
              </a>
            );
          })}
        </nav>

        {onboarding.status?.paused && onboarding.error && <p role="alert" className="rounded-lg border border-warning/40 p-3 text-sm text-warning">{onboarding.error} <button type="button" onClick={() => void onboarding.save({paused: true})} className="underline">Pause speichern</button></p>}
        <OnboardingGuide tab={activeTab.id} />
        <div className="space-y-4 md:space-y-5">{activeTab.render()}</div>
        <FeedbackBox area={`Verwaltung: ${activeTab.label}`} />
    </>
  );
}
