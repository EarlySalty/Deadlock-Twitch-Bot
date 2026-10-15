import { globalTimezoneOwner } from '@/utils/socialMediaChannel';
import { useLanguage, useT } from '../context/LanguageContext';
import { GlobalTimezone } from '@/components/verwaltung/GlobalTimezone';
import { useEffect, useState, type ReactNode } from 'react';
import { motion } from 'framer-motion';
import { Rise } from '../motion/Rise';
import { useQuery } from '@tanstack/react-query';
import { fetchInternalHome } from '@/api/home';
import { OnboardingGuide } from '@/components/onboarding/OnboardingGuide';
import { useOnboarding } from '@/components/onboarding/onboardingState';
import { useAuthStatus } from '@/hooks/useAnalytics';
import { PartnerProfile } from './PartnerProfile';
import { PlanProvider } from '../context/PlanContext';
import { useOAuthLink } from '../context/LanguageContext';
import { PREVIEW_HOME_ROUTE, PREVIEW_OVERLAY_ROUTE, isPreviewModeEnabled } from '@/preview/routes';
import { AdManagerSection } from '@/components/verwaltung/AdManagerSection';
import { CommandNamesProvider } from '@/components/verwaltung/CommandNameSection';
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
import { PrivateVodSection } from '@/components/verwaltung/PrivateVodSection';
import { OtherCommandNames } from '@/components/verwaltung/OtherCommandsSection';
import { ConnectionCard, ScopeDisclosure, type ConnectionTone } from '@/components/verwaltung/ConnectionCard';
import { SectionCard } from '@/components/verwaltung/ui';
import { STAT_COMMANDS } from '@/api/statCommands';
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

const OTHER_COMMANDS_EXCLUDED = [...STAT_COMMANDS.map(item => item.command), 'clip', 'vodclip', 'title', 'lurk', 'sub', 'silentban', 'silentraid'];

function ProfileField({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="border-b border-border/60 py-2.5">
      <dt className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary">{label}</dt>
      <dd className={`mt-1 text-sm font-semibold text-white ${mono ? 'font-mono' : ''}`}>{value}</dd>
    </div>
  );
}

interface VerwaltungTabDef {
  id: VerwaltungTabId;
  label: string;
  icon: typeof User;
  render: () => ReactNode;
}

export function VerwaltungPage() {
  const oauthLink = useOAuthLink();
  const t = useT();
  const { locale } = useLanguage();
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();
  const timezoneOwner = globalTimezoneOwner(window.location.search, authStatus?.twitchUserId ?? undefined);
  const onboarding = useOnboarding();
  const [connectionNotice, setConnectionNotice] = useState<'checking' | { discord: string; steam: string } | null>(null);
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
          <span>{t("Konto wird geladen ...")}</span>
        </div>
      </div>
    );
  }

  if (isError && !data) {
    const errorMessage = error instanceof Error ? error.message : 'Unbekannter Fehler';
    return (
      <div className="panel-card rounded-2xl p-6 md:p-8">
        <h2 className="text-xl font-bold text-white">{t("Konto-Daten nicht verfügbar")}</h2>
        <p className="mt-1 text-sm text-text-secondary">{t(errorMessage)}</p>
        <button
          onClick={() => void refetch()}
          className="mt-4 inline-flex items-center gap-2 rounded-lg border border-border bg-card px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover"
        >
          <ArrowRight className="h-4 w-4" />
          {t("Erneut laden")}
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
  const reconnectUrl = oauthLink(home.oauth?.reconnectUrl || oauthFallbackUrl);
  const discordConnected = Boolean(home.discord?.connected);
  const discordError = isError || home.discord?.status === 'error';
  const steamError = isError || home.steam?.status === 'error';
  const checkConnections = async () => {
    setConnectionNotice('checking');
    onboarding.refresh();
    const result = await refetch();
    const status = (service: 'discord' | 'steam') => result.isError || result.data?.[service]?.status === 'error'
      ? 'gerade nicht prüfbar' : result.data?.[service]?.connected ? 'verbunden' : 'nicht verbunden';
    setConnectionNotice({ discord: status('discord'), steam: status('steam') });
  };
  const discordConnectUrl = home.discord?.connectUrl ? oauthLink(home.discord.connectUrl) : null;
  const steamConnected = Boolean(home.steam?.connected);
  const steamConnectUrl = home.steam?.connectUrl ? oauthLink(home.steam.connectUrl) : null;
  const userId = (authStatus as any)?.userId || (home as any)?.userId || '';
  const selfLogin = String(
    (authStatus as any)?.twitchLogin || (home as any)?.twitchLogin || '',
  ).trim();

  const oauthConnected = oauthStatus === 'connected' && !hasScopeIssue;
  const oauthStatusText = oauthConnected ? 'Verbunden' : needsReauth ? 'Re-Auth nötig' : 'Unvollständig';
  const oauthHintText = oauthConnected
    ? 'Twitch-OAuth ist aktiv und vollständig.'
    : needsReauth
      ? 'Das bestehende Twitch-OAuth ist zur Re-Auth markiert. Bitte neu autorisieren.'
      : missingScopeCount > 1
      ? t('{count} Scopes fehlen. Neu autorisieren, um alle Funktionen zu nutzen.', { count: missingScopeCount.toLocaleString(locale) })
      : '1 Scope fehlt. Bitte neu autorisieren.';
  const partnerStatus = String((authStatus as any)?.partnerStatus || '').trim().toLowerCase();
  const tokenErrorGraceExpiresAt = String((authStatus as any)?.tokenErrorGraceExpiresAt || '').trim();

  const discordStatus = isFetching ? 'Wird geprüft …' : discordError ? 'Gerade nicht prüfbar' : discordConnected ? 'Verbunden' : 'Nicht verbunden';
  const steamStatus = isFetching ? 'Wird geprüft …' : steamError ? 'Gerade nicht prüfbar' : steamConnected ? 'Verbunden' : 'Nicht verbunden';
  const serviceTone = (connected: boolean, failed: boolean): ConnectionTone => isFetching ? 'muted' : failed ? 'warning' : connected ? 'success' : 'warning';

  const kontoTab = (
    <>
      {timezoneOwner ? <GlobalTimezone owner={timezoneOwner} channelScoped={new URLSearchParams(window.location.search).has('twitch_user_id')} /> : <p role="alert">{t('Bitte einen gültigen Twitch-Kanal für die Zeitzone auswählen.')}</p>}
      <div className="grid gap-4 [grid-template-columns:repeat(auto-fit,minmax(300px,1fr))]">
        <ConnectionCard
          icon={oauthConnected ? <ShieldCheck className="h-5 w-5 text-primary" /> : <ShieldAlert className="h-5 w-5 text-warning" />}
          title={t("Twitch-Verbindung")}
          status={t(oauthStatusText)}
          tone={oauthConnected ? 'success' : needsReauth ? 'error' : 'warning'}
          sentence={t(oauthHintText)}
          footer={<a href={reconnectUrl} className="inline-flex items-center gap-2 rounded-lg border border-border px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover"><ShieldCheck className="h-4 w-4" />{t("Jetzt neu autorisieren")}</a>}
        >
          <ScopeDisclosure granted={grantedScopes} missing={missingScopes} />
        </ConnectionCard>
        <ConnectionCard
          tourId="onboarding-discord"
          icon={<MessageSquare className="h-5 w-5 text-primary" />}
          title="Discord"
          status={t(discordStatus)}
          tone={serviceTone(discordConnected, discordError)}
          sentence={discordError ? t("Der Status konnte nicht geprüft werden. Das bedeutet nicht, dass deine Verbindung fehlt.") : discordConnected ? t("Dein Discord-Konto ist mit Twitch verknüpft.") : t("Verbinde Discord zuerst, damit du danach Steam zuordnen kannst.")}
          footer={discordConnectUrl && !discordError ? (
            <a href={discordConnectUrl} className="inline-flex items-center gap-2 rounded-lg border border-border px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover">
              <MessageSquare className="h-4 w-4" />
              {discordConnected ? t("Erneut verbinden") : t("Discord verknüpfen")}
            </a>
          ) : (
            <div className="space-y-2">
              <button type="button" disabled className="inline-flex cursor-not-allowed items-center gap-2 rounded-lg border border-border px-4 py-2 text-sm font-semibold text-text-secondary">
                <MessageSquare className="h-4 w-4" />
                {discordError || isFetching ? t("Status prüfen") : discordConnected ? t("Discord verbunden") : t("Discord-Verbindung nicht verfügbar")}
              </button>
              {!discordConnected && !discordError && !isFetching && (
                <p className="text-xs text-text-secondary">{t("Öffne deine persönliche Partner-Ansicht, um Discord zu verbinden.")}</p>
              )}
            </div>
          )}
        />
        <ConnectionCard
          tourId="onboarding-steam"
          icon={<Gamepad2 className="h-5 w-5 text-primary" />}
          title="Steam"
          status={t(steamStatus)}
          tone={serviceTone(steamConnected, steamError)}
          sentence={steamError ? t("Der Status konnte nicht geprüft werden. Eine bestehende Verbindung kann weiterhin gültig sein.") : steamConnected ? t("Dein Steam-Account ist verknüpft.") : discordConnected ? t("Verknüpfe Steam für deinen Deadlock-Rang und deine Spielstatistiken.") : t("Verbinde zuerst Discord, danach Steam.")}
          footer={steamConnectUrl && !steamError ? (
            <a href={steamConnectUrl} className="inline-flex items-center gap-2 rounded-lg border border-border px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover">
              <Gamepad2 className="h-4 w-4" />
              {steamConnected ? t("Erneut verknüpfen") : t("Steam verknüpfen")}
            </a>
          ) : (
            <div className="space-y-2">
              <button type="button" disabled className="inline-flex cursor-not-allowed items-center gap-2 rounded-lg border border-border px-4 py-2 text-sm font-semibold text-text-secondary">
                <Gamepad2 className="h-4 w-4" />
                {t("Steam verknüpfen")}
              </button>
              <p className="text-xs text-text-secondary">
                {steamError || isFetching ? t("Bitte prüfe den Status erneut, bevor du ein Konto neu verknüpfst.") : discordConnected ? t("Der Verbindungslink ist gerade nicht verfügbar. Bitte prüfe den Status erneut.") : t("Verknüpfe zuerst deinen Discord-Account. Die Steam-Verknüpfung läuft darüber.")}
              </p>
            </div>
          )}
        />
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <button type="button" onClick={checkConnections} disabled={isFetching} className="min-h-11 w-fit rounded-lg border border-primary/50 px-4 py-2 text-sm font-semibold text-primary disabled:opacity-50">{t("Status prüfen")}</button>
        <p role="status" aria-atomic="true" className="text-sm text-text-secondary">{connectionNotice === 'checking' ? t('Verbindungen werden geprüft …') : connectionNotice ? t('Prüfung abgeschlossen. Discord {discord}. Steam {steam}.', { discord: t(connectionNotice.discord), steam: t(connectionNotice.steam) }) : ''}</p>
      </div>

      <SectionCard eyebrow={t("Profil")} title={t("Account-Details")}>
        <dl className="grid gap-x-6 sm:grid-cols-3">
          <ProfileField label={t("Twitch Login")} value={twitchLogin ? `@${twitchLogin}` : '–'} mono />
          <ProfileField label={t("Display Name")} value={displayName || '–'} />
          <ProfileField label={t("User-ID")} value={userId || '–'} mono />
        </dl>
        <p className="mt-3 text-xs text-text-secondary">
          {t("Profiländerungen direkt auf Twitch vornehmen. Daten werden beim nächsten Login synchronisiert.")}
        </p>
      </SectionCard>
    </>
  );

  const chatTab = (
    <CommandNamesProvider>
      <p className="text-xs text-text-secondary">{t("Namen ändern · speichert automatisch · nur Buchstaben und Zahlen")}</p>
      <SectionCard eyebrow={t("Chat-Interaktion & Clips")}>
        <GreetingSection />
        <LurkCommandSection />
        <TitleCommandSection />
        <ClipCommandSection />
        <PrivateVodSection />
      </SectionCard>
      <StatCommandSection />
      <SectionCard eyebrow={t("Community, Raids & System")}>
        <OtherCommandNames excluded={OTHER_COMMANDS_EXCLUDED} />
      </SectionCard>
      <SubReminderSection />
      <LurkerTaxSection />
    </CommandNamesProvider>
  );

  const botTab = (
    <CommandNamesProvider>
      <ModerationSection />
      <ScamGuardSection />
      <SilentNotificationsSection />
      <DisconnectBotSection login={selfLogin} />
    </CommandNamesProvider>
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
          {t("Stream-Overlay")}
        </p>
        <h2 className="display-font mb-1 text-2xl font-bold text-white">
          {t("Overlay für OBS")}
        </h2>
        <p className="text-sm text-text-secondary">
          {t("Stell dir dein Stream-Overlay zusammen — Rang, Winrate, Serie und Live-Match als einblendbare Karte für OBS.")}
        </p>
      </div>

      <a
        href={PREVIEW_OVERLAY_ROUTE}
        className="inline-flex items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-5 py-2.5 text-sm font-semibold text-primary transition-colors hover:border-primary/60 hover:bg-primary/20"
      >
        <ArrowRight className="h-4 w-4" />
        {t("Overlay-Baukasten öffnen")}
      </a>
    </motion.section>
  );

  const werbungTab = <AdManagerSection reconnectUrl={home.oauth?.reconnectUrl || oauthFallbackUrl} />;

  const tabs: VerwaltungTabDef[] = [
    { id: 'konto', label: 'Konto & Verbindungen', icon: User, render: () => kontoTab },
    { id: 'profil', label: 'Mein Profil', icon: User, render: () => <PlanProvider plan={authStatus?.plan ?? null} isAdmin={authStatus?.isAdmin ?? false} isLocalhost={authStatus?.isLocalhost ?? false} isDemoMode={Boolean(authStatus?.demoMode)}><PartnerProfile twitchAvatarUrl={home.avatarUrl} /></PlanProvider> },
    { id: 'chat', label: 'Chat-Befehle', icon: Terminal, render: () => chatTab },
    { id: 'bot', label: 'Bot & Schutz', icon: Bot, render: () => botTab },
    { id: 'overlay', label: 'Overlay', icon: Monitor, render: () => overlayTab },
    { id: 'werbung', label: 'Werbung (Experimentell)', icon: FlaskConical, render: () => werbungTab },
  ];
  const activeTab = tabs.find((item) => item.id === tab) ?? tabs[0];

  return (
    <div className="w-full max-w-[1440px] space-y-4 md:space-y-5">
        {partnerStatus && partnerStatus !== 'active' && partnerStatus !== 'blocked' ? (
          <Rise
            as="section"
            className="panel-card rounded-2xl border border-warning/30 bg-warning/10 p-5 md:p-6"
          >
            <div className="space-y-3">
              <h2 className="text-lg font-bold text-white">
                {partnerStatus === 'token_error'
                  ? t("Twitch OAuth braucht Re-Auth")
                  : partnerStatus === 'departnered'
                  ? t("Du bist aktuell kein aktiver Partner")
                  : partnerStatus === 'archived'
                  ? t("Dein Account ist im Admin-Archiv")
                  : t("Partner-Status inaktiv")}
              </h2>
              <p className="text-sm text-text-secondary">
                {t("Verwaltung, Pläne und Affiliate bleiben offen. Analyse, Social Media und Title-Generator sind gesperrt. Eine erfolgreiche Twitch-OAuth setzt den Status automatisch wieder auf aktiv (außer Bot-Bann oder permanenter Block).")}
              </p>
              {tokenErrorGraceExpiresAt ? (
                <p className="text-xs text-text-secondary">
                  {t("Grace endet:")} {new Date(tokenErrorGraceExpiresAt).toLocaleString(locale)}
                </p>
              ) : null}
              <a
                href={reconnectUrl}
                className="inline-flex items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-4 py-2 text-sm font-semibold text-primary transition-colors hover:border-primary/60 hover:bg-primary/20"
              >
                <ShieldCheck className="h-4 w-4" />
                {t("Jetzt neu autorisieren")}
              </a>
            </div>
          </Rise>
        ) : null}

        {returnMessage && <p role="status" className={`rounded-xl border p-4 text-sm ${returnMessage.failed ? 'border-warning/40 text-warning' : 'border-success/40 text-success'}`}>{t(returnMessage.text)}</p>}
        {isError && data && <p role="alert" className="text-sm text-warning">{t("Der aktuelle Kontostatus konnte nicht geladen werden.")} <button onClick={checkConnections} className="underline">{t("Status prüfen")}</button></p>}
        <header>
          <h1 className="display-font text-2xl font-extrabold text-white">{t("Dein Bot, deine Einstellungen")}</h1>
          <p className="mt-1 max-w-2xl text-sm text-text-secondary">
            {t("Hier verbindest du deine Konten und schaltest Funktionen an oder aus. Wähle einen Bereich und passe den Bot an deinen Kanal an.")}
          </p>
        </header>

        <nav aria-label={t("Verwaltungsbereiche")} className="sticky top-0 z-20 flex flex-wrap gap-1.5 rounded-xl border border-border bg-card/90 p-1.5 backdrop-blur">
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
                {t(item.label)}
              </a>
            );
          })}
        </nav>

        {onboarding.status?.paused && onboarding.error && <p role="alert" className="rounded-lg border border-warning/40 p-3 text-sm text-warning">{t(onboarding.error)} <button type="button" onClick={() => void onboarding.save({paused: true})} className="underline">{t("Pause speichern")}</button></p>}
        <OnboardingGuide tab={activeTab.id} />
        <div className="space-y-4 md:space-y-5">{activeTab.render()}</div>
    </div>
  );
}
