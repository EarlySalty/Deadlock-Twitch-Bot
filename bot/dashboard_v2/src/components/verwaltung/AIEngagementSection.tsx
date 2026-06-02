import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { Loader2, Power, PowerOff } from 'lucide-react';
import {
  fetchEngagementLog,
  fetchEngagementSettings,
  toggleEngagement,
  type EngagementLogEntry,
  type EngagementSettings,
} from '@/api/engagement';

const DECISION_LABELS: Record<string, string> = {
  spoke: 'geantwortet',
  silent: 'mitgelesen, geschwiegen',
  anti_burst: 'kurz pausiert (zu viele Antworten in Folge)',
  flood_guard: 'kurz pausiert (zu schnell hintereinander)',
  disabled: 'inaktiv',
  optout: 'User-Opt-Out',
  provider_error: 'AI-Fehler',
};

const DECISION_COLORS: Record<string, string> = {
  spoke: 'text-success',
  silent: 'text-text-secondary',
  anti_burst: 'text-warning',
  flood_guard: 'text-warning',
  disabled: 'text-text-secondary',
  optout: 'text-text-secondary',
  provider_error: 'text-danger',
};

export function AIEngagementSection() {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState<EngagementSettings | null>(null);
  const [channelLogin, setChannelLogin] = useState<string | null>(null);
  const [logEntries, setLogEntries] = useState<EngagementLogEntry[]>([]);
  const [pending, setPending] = useState(false);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchEngagementSettings();
      const own = data.settings[0] ?? null;
      const channel = own?.channelLogin ?? data.actorLogin ?? null;
      setSettings(own);
      setChannelLogin(channel);
      if (channel) {
        try {
          const logData = await fetchEngagementLog(channel, 10);
          setLogEntries(logData.entries);
        } catch {
          setLogEntries([]);
        }
      } else {
        setLogEntries([]);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Unbekannter Fehler');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onToggle = async () => {
    if (!channelLogin) return;
    const want = !settings?.enabled;
    setPending(true);
    setError(null);
    try {
      await toggleEngagement(channelLogin, want);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Toggle fehlgeschlagen');
    } finally {
      setPending(false);
    }
  };

  const enabled = Boolean(settings?.enabled);

  return (
    <motion.section
      className="panel-card rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32, delay: 0.16 }}
    >
      <div className="mb-5">
        <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">
          AI-Engagement (Beta)
        </p>
        <h2 className="display-font text-2xl font-bold text-white mb-1">
          Stammgast-AI im Chat
        </h2>
        <p className="text-sm text-text-secondary">
          MiniMax-M3 liest deinen Chat mit und mischt sich situativ ein — kennt Deadlock,
          merkt sich Konversationen mit deinen Chattern, deaktiviert sich automatisch bei
          Stream-Ende.
        </p>
      </div>

      {error && (
        <div className="mb-4 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
          {error}
        </div>
      )}

      {loading ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          AI-Status wird geladen ...
        </div>
      ) : !channelLogin ? (
        <div className="rounded-xl border border-border bg-background/40 px-4 py-6 text-sm text-text-secondary text-center">
          Dein Twitch-Account konnte nicht aus der Session gelesen werden. Bitte
          erneut einloggen.
        </div>
      ) : (
        <>
          <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4 mb-5">
            <div className="flex items-center justify-between gap-4 flex-wrap">
              <div className="min-w-0">
                <p
                  className={`text-base font-bold ${
                    enabled ? 'text-success' : 'text-text-secondary'
                  }`}
                >
                  {enabled ? 'AI ist AKTIV' : 'AI ist aus'}
                </p>
                <p className="text-xs text-text-secondary mt-0.5">
                  {enabled
                    ? 'Schaltet sich am Stream-Ende automatisch wieder aus.'
                    : 'Aktiviere die AI, damit sie deinen Stream-Chat begleitet.'}
                </p>
              </div>
              <button
                type="button"
                disabled={pending}
                onClick={() => void onToggle()}
                className={`inline-flex items-center gap-2 rounded-lg px-5 py-2.5 text-sm font-semibold transition-colors ${
                  enabled
                    ? 'border border-danger/40 bg-danger/10 text-danger hover:bg-danger/20'
                    : 'border border-primary/40 bg-primary/10 text-primary hover:bg-primary/20'
                } disabled:opacity-50 disabled:cursor-not-allowed`}
              >
                {pending ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : enabled ? (
                  <PowerOff className="h-4 w-4" />
                ) : (
                  <Power className="h-4 w-4" />
                )}
                {enabled ? 'AI deaktivieren' : 'AI aktivieren'}
              </button>
            </div>
          </div>

          {logEntries.length > 0 && (
            <div>
              <h3 className="text-xs uppercase tracking-wider font-semibold text-text-secondary mb-2">
                Letzte Aktionen
              </h3>
              <div className="space-y-1">
                {logEntries.map((entry, i) => {
                  const colorClass =
                    DECISION_COLORS[entry.decision] ?? 'text-text-secondary';
                  const label = DECISION_LABELS[entry.decision] ?? entry.decision;
                  return (
                    <div
                      key={i}
                      className="rounded-md border border-border/60 bg-background/40 px-3 py-2 text-xs"
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className={`font-semibold ${colorClass}`}>{label}</span>
                        <span className="text-text-secondary">
                          {entry.ts ? new Date(entry.ts).toLocaleString('de-DE') : '–'}
                        </span>
                      </div>
                      {entry.responseText && (
                        <div className="text-text-secondary italic mt-1">
                          „{entry.responseText.slice(0, 180)}
                          {entry.responseText.length > 180 ? '…' : ''}"
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </>
      )}
    </motion.section>
  );
}
