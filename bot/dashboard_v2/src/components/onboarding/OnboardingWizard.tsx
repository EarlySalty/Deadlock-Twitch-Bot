import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import {
  ArrowLeft,
  ArrowRight,
  Bell,
  CheckCircle2,
  Circle,
  ExternalLink,
  Gamepad2,
  LayoutDashboard,
  Loader2,
  MessageSquare,
  Radio,
  RotateCcw,
  ShieldCheck,
  Sparkles,
  Zap,
} from 'lucide-react';
import {
  fetchOnboardingStatus,
  fetchTipSettings,
  saveOnboardingProgress,
  saveTipSettings,
  type OnboardingStatus,
} from '@/api/onboarding';

const MAX_STEP = 4;
const DISCORD_INVITE_URL = 'https://discord.gg/z5TfVHuQq2';

const overviewCards = [
  {
    title: 'Automatische Raids bei Deadlock',
    description: 'Wenn du offline gehst, leitet der Bot deine Viewer automatisch an aktive Partner weiter.',
    icon: Zap,
  },
  {
    title: 'Dein Stream-Dashboard',
    description: 'Viewer-Trends, Raid-Verlauf und Netzwerk-Analytics an einem Ort.',
    icon: LayoutDashboard,
  },
  {
    title: 'Discord beitreten',
    description: 'Mehr Sichtbarkeit, automatische Go-Live-Posts und schneller Kontakt zur Community.',
    icon: MessageSquare,
  },
];

const steamSections = [
  {
    title: 'Was bringt das?',
    items: [
      'Rang wird korrekt erkannt und auf dem Server zugeordnet',
      'Live-Status in den Voice Lanes funktioniert',
      'Spielersuche funktioniert richtig für dich',
    ],
  },
  {
    title: "So funktioniert's:",
    items: [
      'Du meldest dich kurz bei Steam an (OpenID - kein Passwort nötig)',
      'Du gibst deinen Steam-Freundescode ein',
      'Wir schicken dir eine Freundschaftsanfrage - einfach annehmen',
      'Fertig! Du bist verifiziert',
    ],
  },
  {
    title: 'Was wir NICHT machen:',
    items: [
      'Keine Passwörter oder Zugangsdaten',
      'Keine Steam-Freundschaftsliste auslesen',
      'Keine Spielstände oder Profile einsehen',
      'Keine Daten an Dritte weitergeben',
      'Keine Werbung oder Tracking',
    ],
  },
  {
    title: 'Was wir speichern:',
    items: [
      'Discord-ID (damit wir dich zuordnen können)',
      'SteamID64 (technische ID von Steam)',
      'Rang-Daten (nur zur Server-Zuordnung)',
    ],
  },
];

const stepItems = [
  {
    title: 'Tritt dem Netzwerk bei',
    description: 'Verbinde deinen Twitch-Kanal in Sekunden. Kein extra Konto - einfach mit Twitch einloggen.',
    icon: Sparkles,
  },
  {
    title: 'Discord verbinden',
    description: 'Mehr Sichtbarkeit, automatische Go-Live-Posts und schneller Kontakt zur Community.',
    icon: MessageSquare,
  },
  {
    title: 'Steam-Account verknüpfen',
    description: 'Verknüpfung notwendig für Rang, Live-Status und Spielersuche.',
    icon: Gamepad2,
  },
  {
    title: 'Go-Live-Posts',
    description: 'Go-Live-Posts werden automatisch gepostet, wenn du live gehst — ohne dass du etwas tun musst.',
    icon: Bell,
  },
  {
    title: 'Bereit zum Streamen',
    description: 'Checkliste: Kanal verbunden, Auto-Raid aktiv, Dashboard offen.',
    icon: ShieldCheck,
  },
];

interface OnboardingWizardProps {
  onNavigateOverview?: () => void;
}

function clampStep(step: number): number {
  if (!Number.isFinite(step)) return 0;
  return Math.max(0, Math.min(MAX_STEP, Math.trunc(step)));
}

function currentDashboardPath(): string {
  const url = new URL(window.location.href);
  url.searchParams.set('tab', 'onboarding');
  return `${url.pathname}${url.search}${url.hash}`;
}

function discordLinkUrl(): string {
  const next = encodeURIComponent(currentDashboardPath());
  return `/twitch/auth/discord/link?next=${next}`;
}

function linkedLabel(linked: boolean): string {
  return linked ? 'Verbunden' : 'Nicht verbunden';
}

function StatusPill({ linked }: { linked: boolean }) {
  return (
    <span
      className={`inline-flex items-center gap-2 rounded-full border px-3 py-1 text-xs font-semibold ${
        linked
          ? 'border-success/30 bg-success/10 text-success'
          : 'border-warning/30 bg-warning/10 text-warning'
      }`}
    >
      {linked ? <CheckCircle2 className="h-3.5 w-3.5" /> : <Circle className="h-3.5 w-3.5" />}
      {linkedLabel(linked)}
    </span>
  );
}

function WizardStepper({
  activeStep,
  status,
  onSelect,
}: {
  activeStep: number;
  status: OnboardingStatus;
  onSelect: (step: number) => void;
}) {
  return (
    <div className="grid gap-2 md:grid-cols-5">
      {stepItems.map((step, index) => {
        const Icon = step.icon;
        const linkedDone =
          (index === 1 && status.discord_linked) ||
          (index === 2 && status.steam_linked);
        const done = status.completed || index < activeStep || linkedDone;
        const active = index === activeStep;

        return (
          <button
            key={step.title}
            type="button"
            onClick={() => onSelect(index)}
            aria-current={active ? 'step' : undefined}
            className={`min-h-[7.25rem] rounded-lg border p-3 text-left transition-colors ${
              active
                ? 'border-primary/55 bg-primary/12 text-white'
                : 'border-border bg-background/55 text-text-secondary hover:border-border-hover hover:bg-card/80'
            }`}
          >
            <div className="mb-3 flex items-center justify-between gap-2">
              <span
                className={`flex h-8 w-8 items-center justify-center rounded-lg ${
                  done ? 'bg-success/15 text-success' : 'bg-card text-text-secondary'
                }`}
              >
                {done ? <CheckCircle2 className="h-4 w-4" /> : <Icon className="h-4 w-4" />}
              </span>
              <span className="text-xs font-semibold">{index + 1}</span>
            </div>
            <p className="text-sm font-bold leading-snug">{step.title}</p>
          </button>
        );
      })}
    </div>
  );
}

function WelcomeStep() {
  return (
    <div className="space-y-5">
      <div>
        <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">1. Verbinden</p>
        <h2 className="display-font text-2xl font-bold text-white">Tritt dem Netzwerk bei</h2>
      </div>
      <p className="text-sm leading-6 text-text-secondary">
        Der Bot kümmert sich um fünf Dinge: Er leitet beim Stream-Ende deine Zuschauer automatisch an einen live Deadlock-Partner weiter (Auto-Raid), hält automatisch nervige Werbe-Bots aus deinem Chat (die dir mehr Viewer oder Follower verkaufen wollen), trackt deine Stream-Zahlen für dein Dashboard, schickt bei Bedarf eine dezente Discord-Einladung in deinen Chat und bringt optionale Extras wie Lurker-Erinnerungen oder KI-Stream-Reports mit.
      </p>
      <div className="grid gap-3 md:grid-cols-3">
        {overviewCards.map(card => {
          const Icon = card.icon;
          return (
            <div key={card.title} className="rounded-lg border border-border bg-background/60 p-4">
              <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-lg bg-primary/15 text-primary">
                <Icon className="h-5 w-5" />
              </div>
              <h3 className="text-sm font-bold text-white">{card.title}</h3>
              <p className="mt-2 text-xs leading-5 text-text-secondary">{card.description}</p>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function DiscordStep({
  linked,
  refreshing,
  onRefresh,
}: {
  linked: boolean;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Discord</p>
          <h2 className="display-font text-2xl font-bold text-white">Discord verbinden</h2>
        </div>
        <StatusPill linked={linked} />
      </div>
      <p className="text-sm leading-6 text-text-secondary">
        Mehr Sichtbarkeit, automatische Go-Live-Posts und schneller Kontakt zur Community.
      </p>
      <div className="rounded-lg border border-border bg-background/60 p-4">
        <p className={`text-base font-bold ${linked ? 'text-success' : 'text-warning'}`}>
          {linkedLabel(linked)}
        </p>
        <p className="mt-1 text-xs text-text-secondary">
          {linked ? 'Discord-Verknüpfung erkannt.' : 'Noch kein Discord-Profil verknüpft.'}
        </p>
      </div>
      <div className="flex flex-wrap gap-3">
        <a
          href={discordLinkUrl()}
          className="inline-flex items-center gap-2 rounded-lg border border-accent/40 bg-accent/10 px-5 py-2.5 text-sm font-semibold text-accent transition-colors hover:border-accent/60 hover:bg-accent/20"
        >
          <MessageSquare className="h-4 w-4" />
          {linked ? 'Erneut verbinden' : 'Discord verknüpfen'}
          <ExternalLink className="h-4 w-4" />
        </a>
        <button
          type="button"
          onClick={onRefresh}
          disabled={refreshing}
          className="inline-flex items-center gap-2 rounded-lg border border-border bg-card px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover disabled:cursor-wait disabled:opacity-60"
        >
          <RotateCcw className={`h-4 w-4 ${refreshing ? 'animate-spin' : ''}`} />
          Erneut laden
        </button>
      </div>
    </div>
  );
}

function SteamStep({
  linked,
  refreshing,
  onRefresh,
}: {
  linked: boolean;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Steam</p>
          <h2 className="display-font text-2xl font-bold text-white">Steam-Account verknüpfen</h2>
        </div>
        <StatusPill linked={linked} />
      </div>
      <p className="text-sm leading-6 text-text-secondary">
        Verknüpfung notwendig für Rang, Live-Status und Spielersuche.
      </p>
      <p className="text-sm font-semibold text-accent">danach zeigt !rank deinen Rang</p>
      <div className="grid gap-3 md:grid-cols-2">
        {steamSections.map(section => (
          <div key={section.title} className="rounded-lg border border-border bg-background/60 p-4">
            <h3 className="text-sm font-bold text-white">{section.title}</h3>
            <ul className="mt-3 space-y-2 text-xs leading-5 text-text-secondary">
              {section.items.map(item => (
                <li key={item} className="flex gap-2">
                  <span className="mt-1 h-1.5 w-1.5 shrink-0 rounded-full bg-accent" />
                  <span>{item}</span>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
      <div className="flex flex-wrap gap-3">
        <a
          href={DISCORD_INVITE_URL}
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-2 rounded-lg border border-accent/40 bg-accent/10 px-5 py-2.5 text-sm font-semibold text-accent transition-colors hover:border-accent/60 hover:bg-accent/20"
        >
          <MessageSquare className="h-4 w-4" />
          Discord beitreten
          <ExternalLink className="h-4 w-4" />
        </a>
        <button
          type="button"
          onClick={onRefresh}
          disabled={refreshing}
          className="inline-flex items-center gap-2 rounded-lg border border-border bg-card px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover disabled:cursor-wait disabled:opacity-60"
        >
          <RotateCcw className={`h-4 w-4 ${refreshing ? 'animate-spin' : ''}`} />
          Erneut laden
        </button>
      </div>
    </div>
  );
}

function TipsStep() {
  const queryClient = useQueryClient();
  const { data, isLoading } = useQuery({
    queryKey: ['streamer-tip-settings'],
    queryFn: fetchTipSettings,
  });
  const mutation = useMutation({
    mutationFn: saveTipSettings,
    onSuccess: result => {
      queryClient.setQueryData(['streamer-tip-settings'], result);
    },
  });

  const optOut = data?.opt_out ?? false;
  const active = !optOut;

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Go-Live-Posts</p>
          <h2 className="display-font text-2xl font-bold text-white">Go-Live-Posts</h2>
        </div>
        <span
          className={`inline-flex items-center gap-2 rounded-full border px-3 py-1 text-xs font-semibold ${
            active
              ? 'border-success/30 bg-success/10 text-success'
              : 'border-border bg-background/70 text-text-secondary'
          }`}
        >
          <Radio className="h-3.5 w-3.5" />
          {active ? 'Aktiv' : 'Inaktiv'}
        </span>
      </div>
      <p className="text-sm leading-6 text-text-secondary">
        Sobald du auf Twitch live gehst, postet der Bot automatisch eine Benachrichtigung im Discord. Alle Community-Mitglieder sehen sofort, dass du streamst — ohne dass du selbst etwas tun musst.
      </p>
      <div className="rounded-lg border border-border bg-background/60 p-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <h3 className="text-sm font-bold text-white">Go-Live-Posts</h3>
            <p className="mt-1 text-xs text-text-secondary">
              Go-Live-Posts werden automatisch gepostet, wenn du live gehst — ohne dass du etwas tun musst.
            </p>
          </div>
          <button
            type="button"
            aria-pressed={active}
            onClick={() => mutation.mutate({ opt_out: active })}
            disabled={isLoading || mutation.isPending}
            className={`inline-flex min-w-28 items-center justify-center gap-2 rounded-lg border px-4 py-2 text-sm font-semibold transition-colors disabled:cursor-wait disabled:opacity-60 ${
              active
                ? 'border-success/35 bg-success/10 text-success hover:bg-success/15'
                : 'border-border bg-card text-text-secondary hover:border-border-hover hover:bg-card-hover'
            }`}
          >
            {mutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Radio className="h-4 w-4" />}
            {active ? 'Aktiv' : 'Inaktiv'}
          </button>
        </div>
        {mutation.isError && (
          <p className="mt-3 text-xs text-error">Speichern fehlgeschlagen</p>
        )}
      </div>
    </div>
  );
}

function FinishStep({
  completed,
  onComplete,
  completing,
  onNavigateOverview,
}: {
  completed: boolean;
  onComplete: () => void;
  completing: boolean;
  onNavigateOverview?: () => void;
}) {
  return (
    <div className="space-y-5">
      <div>
        <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">4. Start</p>
        <h2 className="display-font text-2xl font-bold text-white">Bereit zum Streamen</h2>
      </div>
      <p className="text-sm leading-6 text-text-secondary">
        Checkliste: Kanal verbunden, Auto-Raid aktiv, Dashboard offen.
      </p>
      <div className="rounded-lg border border-border bg-background/60 p-4">
        <div className="flex items-start gap-3">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-success/15 text-success">
            <CheckCircle2 className="h-5 w-5" />
          </div>
          <div>
            <h3 className="text-sm font-bold text-white">Bereit zum Streamen</h3>
            <p className="mt-1 text-xs leading-5 text-text-secondary">
              Auto-Raids sind für Deadlock gedacht und greifen nicht bei anderen Spielen.
            </p>
          </div>
        </div>
      </div>
      <div className="flex flex-wrap gap-3">
        {!completed && (
          <button
            type="button"
            onClick={onComplete}
            disabled={completing}
            className="inline-flex items-center gap-2 rounded-lg border border-success/40 bg-success/10 px-5 py-2.5 text-sm font-semibold text-success transition-colors hover:border-success/60 hover:bg-success/15 disabled:cursor-wait disabled:opacity-60"
          >
            {completing ? <Loader2 className="h-4 w-4 animate-spin" /> : <CheckCircle2 className="h-4 w-4" />}
            Fertig
          </button>
        )}
        {completed && onNavigateOverview && (
          <button
            type="button"
            onClick={onNavigateOverview}
            className="inline-flex items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-5 py-2.5 text-sm font-semibold text-primary transition-colors hover:border-primary/60 hover:bg-primary/15"
          >
            <LayoutDashboard className="h-4 w-4" />
            Zum Dashboard
          </button>
        )}
      </div>
    </div>
  );
}

export function OnboardingWizard({ onNavigateOverview }: OnboardingWizardProps) {
  const queryClient = useQueryClient();
  const [activeStep, setActiveStep] = useState(0);

  const {
    data: status,
    isLoading,
    isError,
    error,
    refetch,
    isFetching,
  } = useQuery({
    queryKey: ['streamer-onboarding'],
    queryFn: fetchOnboardingStatus,
  });

  const saveMutation = useMutation({
    mutationFn: saveOnboardingProgress,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['streamer-onboarding'] });
    },
  });

  useEffect(() => {
    if (status) {
      setActiveStep(clampStep(status.completed ? MAX_STEP : status.current_step));
    }
  }, [status?.completed, status?.current_step, status]);

  const resolvedStatus = useMemo<OnboardingStatus>(
    () => status ?? {
      current_step: 0,
      completed: false,
      discord_linked: false,
      steam_linked: false,
    },
    [status],
  );

  const selectStep = async (step: number) => {
    const next = clampStep(step);
    setActiveStep(next);
    await saveMutation.mutateAsync({ current_step: next });
  };

  const goNext = async () => {
    await selectStep(activeStep + 1);
  };

  const goBack = async () => {
    await selectStep(activeStep - 1);
  };

  const complete = async () => {
    await saveMutation.mutateAsync({ current_step: MAX_STEP, completed: true });
    setActiveStep(MAX_STEP);
  };

  if (isLoading) {
    return (
      <div className="panel-card rounded-2xl p-6 md:p-8">
        <div className="flex items-center gap-3 text-text-secondary">
          <Loader2 className="h-5 w-5 animate-spin text-primary" />
          <span>Einstellungen werden geladen ...</span>
        </div>
      </div>
    );
  }

  if (isError) {
    const message = error instanceof Error ? error.message : 'Speichern fehlgeschlagen';
    return (
      <div className="panel-card rounded-2xl p-6 md:p-8">
        <h2 className="text-xl font-bold text-white">Konto-Daten nicht verfügbar</h2>
        <p className="mt-1 text-sm text-text-secondary">{message}</p>
        <button
          type="button"
          onClick={() => void refetch()}
          className="mt-4 inline-flex items-center gap-2 rounded-lg border border-border bg-card px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover"
        >
          <ArrowRight className="h-4 w-4" />
          Erneut laden
        </button>
      </div>
    );
  }

  return (
    <div className="space-y-4 md:space-y-5">
      <WizardStepper
        activeStep={activeStep}
        status={resolvedStatus}
        onSelect={step => void selectStep(step)}
      />

      <motion.section
        key={activeStep}
        className="panel-card rounded-2xl p-5 md:p-6"
        initial={{ opacity: 0, y: 14 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.24 }}
      >
        {activeStep === 0 && <WelcomeStep />}
        {activeStep === 1 && (
          <DiscordStep
            linked={resolvedStatus.discord_linked}
            refreshing={isFetching}
            onRefresh={() => void refetch()}
          />
        )}
        {activeStep === 2 && (
          <SteamStep
            linked={resolvedStatus.steam_linked}
            refreshing={isFetching}
            onRefresh={() => void refetch()}
          />
        )}
        {activeStep === 3 && <TipsStep />}
        {activeStep === 4 && (
          <FinishStep
            completed={resolvedStatus.completed}
            onComplete={() => void complete()}
            completing={saveMutation.isPending}
            onNavigateOverview={onNavigateOverview}
          />
        )}
      </motion.section>

      <div className="flex items-center justify-between gap-3">
        <button
          type="button"
          onClick={() => void goBack()}
          disabled={activeStep === 0 || saveMutation.isPending}
          className="inline-flex items-center gap-2 rounded-lg border border-border bg-card px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover disabled:cursor-not-allowed disabled:opacity-45"
        >
          <ArrowLeft className="h-4 w-4" />
          Zurück
        </button>
        {activeStep < MAX_STEP && (
          <button
            type="button"
            onClick={() => void goNext()}
            disabled={saveMutation.isPending}
            className="inline-flex items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-5 py-2 text-sm font-semibold text-primary transition-colors hover:border-primary/60 hover:bg-primary/15 disabled:cursor-wait disabled:opacity-60"
          >
            {saveMutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <ArrowRight className="h-4 w-4" />}
            Weiter
          </button>
        )}
      </div>
    </div>
  );
}
