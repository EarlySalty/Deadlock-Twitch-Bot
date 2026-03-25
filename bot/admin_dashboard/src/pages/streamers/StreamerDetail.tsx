import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { ArrowLeft, BarChart3, Clock3, Radio, Save, Send, Trash2 } from 'lucide-react';
import { buildRaidAuthUrl, buildRaidRequirementsUrl } from '@/api/client';
import { KpiCard } from '@/components/shared/KpiCard';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { Toast } from '@/components/shared/Toast';
import {
  useArchiveStreamer,
  useClearManualPlanOverride,
  useManualPlanOverride,
  usePartnerChatAction,
  useRemoveStreamer,
  useStreamerDetail,
  useToggleStreamerDiscordFlag,
  useUpdateStreamerDiscordProfile,
  useVerifyStreamer,
} from '@/hooks/useAdmin';
import type { LegacyVerifyMode, PartnerChatAnnouncementColor, PartnerChatActionMode, SessionSummary } from '@/api/types';
import { coerceRecord, formatDateTime, formatNumber, formatRelativeTime } from '@/utils/formatters';

const PLAN_OPTIONS = [
  { value: 'raid_free', label: 'Raid Free' },
  { value: 'raid_boost', label: 'Raid Boost' },
  { value: 'analysis_dashboard', label: 'Analyse Dashboard' },
  { value: 'bundle_analysis_raid_boost', label: 'Bundle: Analyse + Raid Boost' },
];

const VERIFY_OPTIONS: Array<{ value: LegacyVerifyMode; label: string }> = [
  { value: 'permanent', label: 'Permanent verifizieren' },
  { value: 'temp', label: '30 Tage verifizieren' },
  { value: 'failed', label: 'Verifizierung fehlgeschlagen' },
  { value: 'clear', label: 'Kein Partner' },
];

const CHAT_MODES: Array<{ value: PartnerChatActionMode; label: string }> = [
  { value: 'message', label: 'Nachricht' },
  { value: 'action', label: '/me Action' },
  { value: 'announcement', label: 'Announcement' },
];

const CHAT_COLORS: Array<{ value: PartnerChatAnnouncementColor; label: string }> = [
  { value: 'purple', label: 'Purple' },
  { value: 'blue', label: 'Blue' },
  { value: 'green', label: 'Green' },
  { value: 'orange', label: 'Orange' },
  { value: 'primary', label: 'Primary' },
];

function metricFromStats(stats: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    if (typeof stats[key] === 'number' || typeof stats[key] === 'string') {
      return String(stats[key]);
    }
  }
  return '—';
}

function readBoolean(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'boolean') {
      return value;
    }
    if (typeof value === 'number') {
      return value !== 0;
    }
    if (typeof value === 'string') {
      const normalized = value.trim().toLowerCase();
      if (normalized) {
        return !['0', 'false', 'no', 'off'].includes(normalized);
      }
    }
  }
  return false;
}

function readString(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    if (typeof record[key] === 'string') {
      return String(record[key]);
    }
  }
  return '';
}

function readStringArray(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    if (Array.isArray(value)) {
      return value.map((entry) => String(entry ?? '').trim()).filter(Boolean);
    }
  }
  return [] as string[];
}

export function StreamerDetailPage() {
  const params = useParams();
  const navigate = useNavigate();
  const login = params.login;
  const detailQuery = useStreamerDetail(login);
  const detail = detailQuery.data;
  const stats = coerceRecord(detail?.stats);
  const settings = coerceRecord(detail?.settings);
  const grantedScopes = readStringArray(settings, 'grantedScopes', 'granted_scopes');
  const missingScopes = readStringArray(settings, 'missingScopes', 'missing_scopes');
  const oauthStatus = readString(settings, 'oauthStatus', 'oauth_status') || 'missing';
  const oauthConnected = readBoolean(settings, 'oauthConnected', 'oauth_connected');
  const oauthNeedsReauth = readBoolean(settings, 'oauthNeedsReauth', 'oauth_needs_reauth');
  const persistedMemberFlag = readBoolean(settings, 'isOnDiscord', 'is_on_discord');
  const canUseChatAction =
    detail?.partnerStatus === 'active' || detail?.partnerStatus === 'archived';

  const verifyMutation = useVerifyStreamer();
  const archiveMutation = useArchiveStreamer();
  const removeMutation = useRemoveStreamer();
  const discordProfileMutation = useUpdateStreamerDiscordProfile();
  const discordFlagMutation = useToggleStreamerDiscordFlag();
  const manualPlanMutation = useManualPlanOverride();
  const clearManualPlanMutation = useClearManualPlanOverride();
  const partnerChatMutation = usePartnerChatAction();

  const [verifyMode, setVerifyMode] = useState<LegacyVerifyMode>('permanent');
  const [discordUserId, setDiscordUserId] = useState('');
  const [discordDisplayName, setDiscordDisplayName] = useState('');
  const [memberFlag, setMemberFlag] = useState(false);
  const [manualPlanId, setManualPlanId] = useState('raid_free');
  const [manualPlanExpiresAt, setManualPlanExpiresAt] = useState('');
  const [manualPlanNotes, setManualPlanNotes] = useState('');
  const [chatMode, setChatMode] = useState<PartnerChatActionMode>('message');
  const [chatColor, setChatColor] = useState<PartnerChatAnnouncementColor>('purple');
  const [chatMessage, setChatMessage] = useState('');
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [toast, setToast] = useState<{ open: boolean; tone: 'success' | 'error'; message: string }>({
    open: false,
    tone: 'success',
    message: '',
  });

  useEffect(() => {
    if (!detail) {
      return;
    }
    setDiscordUserId(readString(settings, 'discordUserId', 'discord_user_id'));
    setDiscordDisplayName(readString(settings, 'discordDisplayName', 'discord_display_name'));
    setMemberFlag(readBoolean(settings, 'isOnDiscord', 'is_on_discord'));
    setManualPlanId(
      readString(settings, 'manualPlanId', 'manual_plan_id') ||
        detail.planId ||
        PLAN_OPTIONS[0].value,
    );
    setManualPlanExpiresAt(
      (readString(settings, 'manualPlanExpiresAt', 'manual_plan_expires_at') || '').slice(0, 10),
    );
    setManualPlanNotes(readString(settings, 'manualPlanNotes', 'manual_plan_notes'));
  }, [detail]);

  const sessionColumns: TableColumn<SessionSummary>[] = [
    {
      key: 'session',
      title: 'Session',
      sortable: true,
      sortValue: (row) => row.sessionId ?? 0,
      render: (row) => (
        <div>
          <p className="font-medium text-white">{row.title || `Session #${row.sessionId ?? '—'}`}</p>
          <p className="text-xs uppercase tracking-[0.16em] text-text-secondary">{row.category || 'Kategorie unbekannt'}</p>
        </div>
      ),
    },
    {
      key: 'started',
      title: 'Start',
      sortable: true,
      sortValue: (row) => row.startedAt || '',
      render: (row) => formatDateTime(row.startedAt),
    },
    {
      key: 'avg',
      title: 'Avg Viewer',
      sortable: true,
      sortValue: (row) => row.averageViewers ?? 0,
      render: (row) => formatNumber(row.averageViewers ?? 0),
    },
    {
      key: 'peak',
      title: 'Peak',
      sortable: true,
      sortValue: (row) => row.peakViewers ?? 0,
      render: (row) => formatNumber(row.peakViewers ?? 0),
    },
  ];

  if (detailQuery.isLoading) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Streamer wird geladen …</div>;
  }

  if (detailQuery.isError) {
    return (
      <section className="space-y-5">
        <div className="panel-card rounded-[1.8rem] p-6">
          <Link to="/streamers" className="inline-flex items-center gap-2 text-sm text-text-secondary hover:text-white">
            <ArrowLeft className="h-4 w-4" />
            Zurück zur Liste
          </Link>
        </div>
        <div className="panel-card rounded-[1.8rem] p-8 text-white">
          {detailQuery.error instanceof Error
            ? detailQuery.error.message
            : 'Streamer-Details konnten nicht geladen werden.'}
        </div>
      </section>
    );
  }

  if (!login) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Ungültiger Streamer-Link.</div>;
  }

  if (!detail) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Kein Datensatz für diesen Streamer.</div>;
  }

  return (
    <section className="space-y-5">
      <header className="panel-card rounded-[1.8rem] p-6">
        <Link to="/streamers" className="inline-flex items-center gap-2 text-sm text-text-secondary hover:text-white">
          <ArrowLeft className="h-4 w-4" />
          Zurück zur Liste
        </Link>
        <div className="mt-5 flex flex-col gap-5 lg:flex-row lg:items-end lg:justify-between">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.28em] text-text-secondary">Streamer Detail</p>
            <h1 className="mt-2 text-3xl font-semibold text-white">{detail.displayName || detail.login}</h1>
            <p className="mt-2 text-sm text-text-secondary">
              Zuletzt gesehen {formatRelativeTime(readString(stats, 'lastSeenAt', 'last_seen_at') || detail.archivedAt || detail.createdAt)}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <StatusBadge status={detail.partnerStatus || 'active'} />
            <StatusBadge status={detail.isLive ? 'live' : detail.verified ? 'verified' : 'offline'} />
            <StatusBadge status={oauthStatus} />
            {detail.planId ? <StatusBadge status={detail.planId} /> : null}
          </div>
        </div>
      </header>

      <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <KpiCard title="Plan" value={detail.planId || 'Unbekannt'} hint="effektiver Admin-Plan" icon={BarChart3} tone="primary" />
        <KpiCard title="Live Viewer" value={metricFromStats(stats, 'viewerCount', 'viewer_count')} hint={`OAuth ${oauthConnected ? 'verbunden' : 'fehlt'}`} icon={Radio} tone="accent" />
        <KpiCard title="Sessions" value={metricFromStats(stats, 'totalSessions', 'session_count', 'sessions')} hint="aggregiert aus Stream-Sessions" icon={Clock3} />
        <KpiCard title="Follower Delta" value={metricFromStats(stats, 'followerDelta', 'follower_delta_total', 'followers_delta_total')} hint="aus Session-Historie" />
      </section>

      <section className="grid gap-5 xl:grid-cols-2">
        <article className="panel-card rounded-[1.8rem] p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Verifizierung & Lifecycle</p>
          <div className="mt-4 grid gap-4 md:grid-cols-2">
            <label className="text-sm text-text-secondary">
              Verifizierungsmodus
              <select className="admin-input mt-2" value={verifyMode} onChange={(event) => setVerifyMode(event.target.value as LegacyVerifyMode)}>
                {VERIFY_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
            <div className="grid gap-2">
              <button
                className="admin-button admin-button-primary"
                disabled={verifyMutation.isPending}
                onClick={async () => {
                  try {
                    const result = await verifyMutation.mutateAsync({ login: detail.login, mode: verifyMode });
                    setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
                  } catch (error) {
                    setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Verifizierung fehlgeschlagen' });
                  }
                }}
              >
                <Save className="h-4 w-4" />
                Verifizierung anwenden
              </button>
              {detail.partnerStatus !== 'non_partner' && detail.partnerStatus !== 'departnered' ? (
                <button
                  className="admin-button admin-button-secondary"
                  disabled={archiveMutation.isPending}
                  onClick={async () => {
                    try {
                      const result = await archiveMutation.mutateAsync({
                        login: detail.login,
                        mode: detail.partnerStatus === 'archived' ? 'unarchive' : 'archive',
                      });
                      setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
                    } catch (error) {
                      setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Archiv-Aktion fehlgeschlagen' });
                    }
                  }}
                >
                  {detail.partnerStatus === 'archived' ? 'Reaktivieren' : 'Archivieren'}
                </button>
              ) : null}
              {detail.partnerStatus !== 'departnered' ? (
                <button className="admin-button admin-button-danger" onClick={() => setConfirmRemove(true)}>
                  <Trash2 className="h-4 w-4" />
                  {detail.partnerStatus === 'non_partner' ? 'Streamer entfernen' : 'Partner deaktivieren'}
                </button>
              ) : null}
            </div>
          </div>
          <div className="mt-4 flex flex-wrap gap-2">
            <StatusBadge status={persistedMemberFlag ? 'active' : 'inactive'} />
            <StatusBadge status={detail.partnerStatus || 'active'} />
            {detail.archivedAt ? <span className="stat-pill">Archiviert {formatDateTime(detail.archivedAt)}</span> : null}
            {detail.partnerStatus === 'departnered' ? <span className="stat-pill">Operativ deaktiviert</span> : null}
          </div>
        </article>

        <article className="panel-card rounded-[1.8rem] p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Discord Verwaltung</p>
          <div className="mt-4 grid gap-4">
            <div className="grid gap-4 md:grid-cols-2">
              <label className="text-sm text-text-secondary">
                Discord User ID
                <input value={discordUserId} onChange={(event) => setDiscordUserId(event.target.value)} className="admin-input mt-2" />
              </label>
              <label className="text-sm text-text-secondary">
                Discord Anzeigename
                <input value={discordDisplayName} onChange={(event) => setDiscordDisplayName(event.target.value)} className="admin-input mt-2" />
              </label>
            </div>
            <label className="flex items-center gap-3 text-sm text-text-secondary">
              <input checked={memberFlag} onChange={(event) => setMemberFlag(event.target.checked)} type="checkbox" />
              Als Discord-Mitglied markieren (wird mit Speichern übernommen)
            </label>
            <div className="flex flex-wrap gap-3">
              <button
                className="admin-button admin-button-primary"
                disabled={discordProfileMutation.isPending}
                onClick={async () => {
                  try {
                    const result = await discordProfileMutation.mutateAsync({
                      login: detail.login,
                      discordUserId: discordUserId.trim() || undefined,
                      discordDisplayName: discordDisplayName.trim() || undefined,
                      memberFlag,
                    });
                    setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
                  } catch (error) {
                    setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Discord-Profil konnte nicht gespeichert werden' });
                  }
                }}
              >
                <Save className="h-4 w-4" />
                Discord-Profil speichern
              </button>
              <button
                className="admin-button admin-button-secondary"
                disabled={discordFlagMutation.isPending}
                onClick={async () => {
                  try {
                    const result = await discordFlagMutation.mutateAsync({
                      login: detail.login,
                      mode: persistedMemberFlag ? 'unmark' : 'mark',
                    });
                    setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
                  } catch (error) {
                    setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Discord-Flag konnte nicht aktualisiert werden' });
                  }
                }}
              >
                {persistedMemberFlag ? 'Discord-Markierung entfernen' : 'Als Discord-Mitglied markieren'}
              </button>
            </div>
          </div>
        </article>
      </section>

      <section className="grid gap-5 xl:grid-cols-2">
        <article className="panel-card rounded-[1.8rem] p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Raid OAuth & Scopes</p>
          <div className="mt-4 flex flex-wrap gap-2">
            <StatusBadge status={oauthStatus} />
            {oauthNeedsReauth ? <StatusBadge status="warning" /> : null}
            <span className="stat-pill">{grantedScopes.length} Scopes vorhanden</span>
            <span className="stat-pill">{missingScopes.length} Scopes fehlen</span>
          </div>
          <div className="mt-4 flex flex-wrap gap-3">
            <a href={buildRaidAuthUrl(detail.login)} target="_blank" rel="noreferrer" className="admin-button admin-button-primary">
              OAuth-Link öffnen
            </a>
            <a href={buildRaidRequirementsUrl(detail.login)} className="admin-button admin-button-secondary">
              Anforderungen senden
            </a>
          </div>
          <div className="mt-5 grid gap-5 md:grid-cols-2">
            <div>
              <p className="text-sm font-semibold text-white">Vorhanden</p>
              <div className="mt-3 flex flex-wrap gap-2">
                {grantedScopes.length ? (
                  grantedScopes.map((scope) => (
                    <span key={scope} className="inline-flex items-center rounded-full border border-emerald-400/35 bg-emerald-500/14 px-2.5 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.18em] text-emerald-100">
                      {scope}
                    </span>
                  ))
                ) : (
                  <div className="text-sm text-text-secondary">Keine Scopes vorhanden.</div>
                )}
              </div>
            </div>
            <div>
              <p className="text-sm font-semibold text-white">Fehlend</p>
              <div className="mt-3 flex flex-wrap gap-2">
                {missingScopes.length ? (
                  missingScopes.map((scope) => (
                    <span key={scope} className="inline-flex items-center rounded-full border border-amber-400/35 bg-amber-500/14 px-2.5 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.18em] text-amber-100">
                      {scope}
                    </span>
                  ))
                ) : (
                  <div className="text-sm text-text-secondary">Alle Pflicht-Scopes vorhanden.</div>
                )}
              </div>
            </div>
          </div>
        </article>

        <article className="panel-card rounded-[1.8rem] p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Manuelle Planvergabe</p>
          <div className="mt-4 grid gap-4">
            <div className="grid gap-4 md:grid-cols-2">
              <label className="text-sm text-text-secondary">
                Plan
                <select className="admin-input mt-2" value={manualPlanId} onChange={(event) => setManualPlanId(event.target.value)}>
                  {PLAN_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="text-sm text-text-secondary">
                Ablaufdatum
                <input
                  className="admin-input mt-2"
                  placeholder="YYYY-MM-DD"
                  value={manualPlanExpiresAt}
                  onChange={(event) => setManualPlanExpiresAt(event.target.value)}
                />
              </label>
            </div>
            <label className="text-sm text-text-secondary">
              Notiz
              <input className="admin-input mt-2" value={manualPlanNotes} onChange={(event) => setManualPlanNotes(event.target.value)} />
            </label>
            <div className="flex flex-wrap gap-3">
              <button
                className="admin-button admin-button-primary"
                disabled={manualPlanMutation.isPending}
                onClick={async () => {
                  try {
                    const result = await manualPlanMutation.mutateAsync({
                      login: detail.login,
                      planId: manualPlanId,
                      expiresAt: manualPlanExpiresAt.trim() || undefined,
                      notes: manualPlanNotes.trim() || undefined,
                    });
                    setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
                  } catch (error) {
                    setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Plan-Override konnte nicht gespeichert werden' });
                  }
                }}
              >
                <Save className="h-4 w-4" />
                Override speichern
              </button>
              <button
                className="admin-button admin-button-secondary"
                disabled={clearManualPlanMutation.isPending}
                onClick={async () => {
                  try {
                    const result = await clearManualPlanMutation.mutateAsync(detail.login);
                    setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
                  } catch (error) {
                    setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Plan-Override konnte nicht entfernt werden' });
                  }
                }}
              >
                Override entfernen
              </button>
            </div>
          </div>
        </article>
      </section>

      <section className="grid gap-5 xl:grid-cols-[1.2fr_0.8fr]">
        <article className="panel-card rounded-[1.8rem] p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Partner Chat Aktion</p>
          <div className="mt-4 grid gap-4">
            <div className="grid gap-4 md:grid-cols-2">
              <label className="text-sm text-text-secondary">
                Modus
                <select className="admin-input mt-2" value={chatMode} onChange={(event) => setChatMode(event.target.value as PartnerChatActionMode)}>
                  {CHAT_MODES.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="text-sm text-text-secondary">
                Farbe
                <select className="admin-input mt-2" value={chatColor} onChange={(event) => setChatColor(event.target.value as PartnerChatAnnouncementColor)}>
                  {CHAT_COLORS.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <label className="text-sm text-text-secondary">
              Nachricht
              <textarea
                className="admin-input mt-2 min-h-[150px]"
                maxLength={450}
                value={chatMessage}
                onChange={(event) => setChatMessage(event.target.value)}
                placeholder="Nachricht für den Twitch-Chat"
              />
            </label>
            <button
              className="admin-button admin-button-primary"
              disabled={!canUseChatAction || !chatMessage.trim() || partnerChatMutation.isPending}
              onClick={async () => {
                try {
                  const result = await partnerChatMutation.mutateAsync({
                    login: detail.login,
                    mode: chatMode,
                    color: chatColor,
                    message: chatMessage.trim(),
                  });
                  setChatMessage('');
                  setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
                } catch (error) {
                  setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Chat-Aktion fehlgeschlagen' });
                }
              }}
            >
              <Send className="h-4 w-4" />
              Nachricht senden
            </button>
            <p className="text-sm text-text-secondary">
              {canUseChatAction
                ? 'Server prüft zusätzlich, ob dein Admin-Account Owner-Rechte für manuelle Chat-Aktionen hat.'
                : 'Chat-Aktionen sind nur für aktive oder admin-archivierte Partner-Streamer erlaubt.'}
            </p>
          </div>
        </article>

        <article className="panel-card rounded-[1.8rem] p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Settings Snapshot</p>
          <pre className="mt-4 overflow-auto rounded-[1.4rem] border border-white/10 bg-slate-950/55 p-4 text-xs leading-6 text-emerald-100">
            {JSON.stringify(settings, null, 2)}
          </pre>
        </article>
      </section>

      <article className="panel-card rounded-[1.8rem] p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Letzte Sessions</p>
        <div className="mt-4">
          <DataTable columns={sessionColumns} rows={detail.sessions ?? []} rowKey={(row, index) => `${row.sessionId ?? index}`} />
        </div>
      </article>

      <ConfirmDialog
        open={confirmRemove}
        title={detail.partnerStatus === 'non_partner' ? 'Streamer endgültig entfernen?' : 'Partner operativ deaktivieren?'}
        description={
          detail.partnerStatus === 'non_partner'
            ? `Der Streamer ${detail.login} wird vollständig entfernt. Diese Aktion sollte nur genutzt werden, wenn der Datensatz wirklich raus soll.`
            : `Der Streamer ${detail.login} bleibt im System, verliert aber operative Partnerfunktionen wie Auto-Raid und Raid-Targeting.`
        }
        tone="danger"
        busy={removeMutation.isPending}
        onCancel={() => setConfirmRemove(false)}
        onConfirm={async () => {
          try {
            const result = await removeMutation.mutateAsync(detail.login);
            setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
            setConfirmRemove(false);
            if (result.ok) {
              navigate('/streamers');
            }
          } catch (error) {
            setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Streamer konnte nicht entfernt werden' });
          }
        }}
      />

      <Toast open={toast.open} tone={toast.tone} message={toast.message} onClose={() => setToast((current) => ({ ...current, open: false }))} />
    </section>
  );
}
