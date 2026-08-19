import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BarChart3,
  Copy,
  Film,
  Home,
  Loader2,
  Lock,
  Radio,
  Settings,
  MonitorPlay,
  Sparkles,
  FileText,
} from 'lucide-react';
import { Rise } from '../motion/Rise';
import { useAuthStatus } from '@/hooks/useAnalytics';
import {
  fetchUplinkMe,
  joinUplinkWaitlist,
  saveUplinkTwitchDestination,
} from '@/api/uplink';
import {
  PREVIEW_CHANGELOG_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_UPLINK_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
  analyticsTabHref,
} from '@/preview/routes';

function SidebarLink({
  href,
  label,
  icon: Icon,
  active,
}: {
  href: string;
  label: string;
  icon: typeof Home;
  active?: boolean;
}) {
  const activeClasses =
    'border border-primary/25 bg-primary/10 text-primary lg:rounded-l-none lg:border-y-0 lg:border-r-0 lg:border-t-0 lg:border-l-2 lg:border-primary lg:pl-2.5';
  const inactiveClasses =
    'border border-transparent text-text-secondary hover:bg-white/5 hover:text-white';
  return (
    <a
      href={href}
      className={`flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-semibold no-underline transition-colors whitespace-nowrap ${
        active ? activeClasses : inactiveClasses
      }`}
    >
      <Icon className="h-4 w-4 shrink-0" />
      <span>{label}</span>
    </a>
  );
}

function CopyField({ label, value }: { label: string; value: string }) {
  const [ok, setOk] = useState(false);
  if (!value) return null;
  return (
    <div className="space-y-1">
      <div className="text-[11px] font-semibold uppercase tracking-[0.16em] text-text-secondary">
        {label}
      </div>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded-xl border border-border bg-background/70 px-3 py-2 text-xs text-white">
          {value}
        </code>
        <button
          type="button"
          className="inline-flex items-center gap-1 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary hover:text-white"
          onClick={async () => {
            await navigator.clipboard.writeText(value);
            setOk(true);
            window.setTimeout(() => setOk(false), 1500);
          }}
        >
          <Copy className="h-3.5 w-3.5" />
          {ok ? 'Kopiert' : 'Kopieren'}
        </button>
      </div>
    </div>
  );
}

export function UplinkPage() {
  const queryClient = useQueryClient();
  const { data: authStatus } = useAuthStatus();
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ['uplink-me'],
    queryFn: fetchUplinkMe,
    retry: false,
  });
  const waitlist = useMutation({
    mutationFn: joinUplinkWaitlist,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['uplink-me'] }),
  });
  const [rtmpUrl, setRtmpUrl] = useState('rtmp://live.twitch.tv/app');
  const [streamKey, setStreamKey] = useState('');
  const saveDest = useMutation({
    mutationFn: () =>
      saveUplinkTwitchDestination({ rtmp_url: rtmpUrl, stream_key: streamKey }),
    onSuccess: () => {
      setStreamKey('');
      queryClient.invalidateQueries({ queryKey: ['uplink-me'] });
    },
  });

  const planName = authStatus?.plan?.displayName ?? 'Free';

  return (
    <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
      <div className="relative mx-auto max-w-[1440px]">
        <div className="grid gap-4 md:gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
          <Rise as="aside" className="panel-card card-glow self-start rounded-2xl p-4 lg:sticky lg:top-4">
            <div className="space-y-4">
              <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                Main
              </div>
              <nav className="lg:space-y-1">
                <SidebarLink href={PREVIEW_HOME_ROUTE} icon={Home} label="Home" />
                <SidebarLink href={analyticsTabHref('overview')} icon={BarChart3} label="Analyse" />
                <SidebarLink href="/social-media-admin" icon={Film} label="Social Media Dashboard" />
                <SidebarLink href={PREVIEW_UPLINK_ROUTE} icon={Radio} label="Uplink" active />
              </nav>
              <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                Tools
              </div>
              <div className="lg:space-y-1">
                <SidebarLink href={PREVIEW_VERWALTUNG_ROUTE} icon={Settings} label="Verwaltung" />
                <SidebarLink href={PREVIEW_OVERLAY_ROUTE} icon={MonitorPlay} label="Stream-Overlay" />
                <SidebarLink href={PREVIEW_PRICING_ROUTE} icon={Sparkles} label={`Plan: ${planName}`} />
                <SidebarLink href={PREVIEW_CHANGELOG_ROUTE} icon={FileText} label="Changelog" />
              </div>
            </div>
          </Rise>

          <div className="space-y-4">
            <Rise className="panel-card rounded-2xl p-6">
              <div className="mb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-primary">
                Eigenes Modul
              </div>
              <h1 className="display-font text-2xl font-extrabold text-white">Uplink</h1>
              <p className="mt-2 max-w-2xl text-sm text-text-secondary">
                Du sendest zu uns, wir legen die Plattform-Streams an. Start und Stop machst du in OBS.
              </p>
            </Rise>

            {isLoading && (
              <div className="panel-card flex items-center gap-2 rounded-2xl p-6 text-text-secondary">
                <Loader2 className="h-4 w-4 animate-spin" />
                Status wird geladen
              </div>
            )}

            {isError && (
              <div className="panel-card rounded-2xl p-6 text-sm text-warning">
                {error instanceof Error
                  ? error.message
                  : 'Uplink ist gerade nicht erreichbar.'}
              </div>
            )}

            {data && !data.enabled && (
              <div className="panel-card relative overflow-hidden rounded-2xl p-6">
                <div className="absolute inset-0 bg-black/20" />
                <div className="relative space-y-3">
                  <div className="inline-flex h-12 w-12 items-center justify-center rounded-full border border-white/10 bg-white/5">
                    <Lock className="h-5 w-5 text-white/40" />
                  </div>
                  <h2 className="text-lg font-bold text-white">Uplink ist ein bezahltes Add-on</h2>
                  <p className="max-w-xl text-sm text-text-secondary">
                    Ohne Freischaltung kannst du auf die Warteliste. Danach richtet EarlySalty den Slot ein.
                  </p>
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="button"
                      disabled={waitlist.isPending || data.waitlisted}
                      onClick={() => waitlist.mutate()}
                      className="rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
                    >
                      {data.waitlisted ? 'Stehst auf der Warteliste' : 'Auf die Warteliste'}
                    </button>
                    <a
                      href={PREVIEW_PRICING_ROUTE}
                      className="rounded-xl border border-border px-4 py-2 text-sm font-semibold text-white no-underline"
                    >
                      Zum Plan
                    </a>
                  </div>
                </div>
              </div>
            )}

            {data?.enabled && (
              <div className="space-y-4">
                <Rise className="panel-card space-y-4 rounded-2xl p-6">
                  <h2 className="text-lg font-bold text-white">OBS einrichten</h2>
                  <CopyField label="RTMP-Server" value={data.rtmp_url.replace(/\/[^/]+$/, '')} />
                  <CopyField label="Stream-Schlüssel" value={data.ingest_key} />
                  <CopyField label="Komplette RTMP-Adresse" value={data.rtmp_url} />
                  <p className="text-xs text-text-secondary">
                    In OBS: Dienst Benutzerdefiniert. Hardware-HEVC, VBR, Keyframe 2 s. Danach Stream starten.
                  </p>
                </Rise>

                <Rise className="panel-card space-y-3 rounded-2xl p-6">
                  <h2 className="text-lg font-bold text-white">Twitch-Ziel</h2>
                  <p className="text-sm text-text-secondary">
                    Stream-Key von Twitch, nicht unser Ingest-Key. Wird verschlüsselt gespeichert.
                  </p>
                  <input
                    value={rtmpUrl}
                    onChange={(e) => setRtmpUrl(e.target.value)}
                    className="w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
                    placeholder="rtmp://live.twitch.tv/app"
                  />
                  <input
                    value={streamKey}
                    onChange={(e) => setStreamKey(e.target.value)}
                    type="password"
                    className="w-full rounded-xl border border-border bg-background/70 px-3 py-2 text-sm text-white"
                    placeholder="Twitch Stream-Key"
                  />
                  <button
                    type="button"
                    disabled={saveDest.isPending || !streamKey}
                    onClick={() => saveDest.mutate()}
                    className="rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-60"
                  >
                    {saveDest.isSuccess ? 'Gespeichert' : 'Twitch-Ziel speichern'}
                  </button>
                </Rise>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
