import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router';
import { ArrowLeft, Ban, BarChart3, CctvOff, Clock3, PlugZap, Radio, Save, Send, ShieldCheck, ShieldOff, Trash2 } from 'lucide-react';
import { buildRaidAuthUrl, buildRaidRequirementsUrl } from '@/api/client';
import { PageHeader } from '@/components/layout/PageHeader';
import { KpiCard } from '@/components/shared/KpiCard';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { ConfirmTypedDialog } from '@/components/shared/ConfirmTypedDialog';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { Toast } from '@/components/shared/Toast';
import {
  useArchiveStreamer,
  useBlockStreamer,
  useClearManualPlanOverride,
  useDisconnectBot,
  useEngagementSettings,
  useEngagementToggle,
  useManualPlanOverride,
  usePartnerAccess,
  usePartnerChatAction,
  useSetPartnerAccess,
  useRemoveStreamer,
  useStreamerDetail,
  useToggleStreamerDiscordFlag,
  useUpdateStreamerDiscordProfile,
  useVerifyStreamer,
} from '@/hooks/useAdmin';
import type {
  DisconnectBotResult,
  LegacyVerifyMode,
  PartnerChatAnnouncementColor,
  PartnerChatActionMode,
  SessionSummary,
} from '@/api/types';
import { coerceRecord, formatDateTime, formatNumber, formatRelativeTime } from '@/utils/formatters';
import { findPartnerAccessEntry } from '@/utils/partnerAccess';

const PLAN_OPTIONS = [
  { value: 'raid_free', label: 'Raid Free' },
  { value: 'chat_quiet', label: 'Werbefrei' },
  { value: 'raid_boost', label: 'Raid Boost' },
  { value: 'analysis_dashboard', label: 'Analyse Dashboard' },
  { value: 'bundle_chat_quiet_raid_boost', label: 'Bundle: Werbefrei + Raid Boost' },
  { value: 'bundle_werbefrei_analyse', label: 'Bundle: Werbefrei + Analyse' },
  { value: 'bundle_komplett', label: 'Bundle Komplett' },
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

/** Klartext für jeden Ausgang des Unmod-Schritts — auch die, die nichts bewirkt haben. */
const UNMOD_LABELS: Record<DisconnectBotResult['unmod'], string> = {
  removed: 'Moderator-Rechte entzogen',
  not_moderator: 'war ohnehin kein Moderator',
  no_token: 'keine gültige Twitch-Autorisierung — Moderator-Rechte bleiben bestehen',
  unknown_channel: 'Kanal-ID nicht auflösbar — Moderator-Rechte bleiben bestehen',
  unavailable: 'Bot nicht erreichbar — Moderator-Rechte bleiben bestehen',
  failed: 'Entzug fehlgeschlagen — Moderator-Rechte bleiben bestehen',
};

/**
 * Klartext für den Rollen-Entzug. Nur `revoked` und „keine Verknüpfung" sind
 * erledigt; jeder andere Ausgang heißt, die Rolle liegt noch beim Streamer —
 * inklusive des Grundes, sonst ist es nicht nachvollziehbar.
 */
function discordRoleLabel(role: string): string {
  if (role === 'revoked') return 'Streamer-Rolle entzogen';
  if (role === 'skipped:no_discord_link') return 'keine Discord-Verknüpfung — nichts zu entziehen';
  if (role.startsWith('skipped:')) {
    return `Streamer-Rolle bleibt bestehen — Entzug übersprungen (${role.slice('skipped:'.length)})`;
  }
  if (role.startsWith('failed:')) {
    return `Streamer-Rolle konnte nicht entzogen werden (${role.slice('failed:'.length)})`;
  }
  return `Streamer-Rolle: unklarer Ausgang (${role})`;
}

function discordRoleDone(role: string): boolean {
  return role === 'revoked' || role === 'skipped:no_discord_link';
}

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
  const blockMutation = useBlockStreamer();
  const removeMutation = useRemoveStreamer();
  const discordProfileMutation = useUpdateStreamerDiscordProfile();
  const discordFlagMutation = useToggleStreamerDiscordFlag();
  const manualPlanMutation = useManualPlanOverride();
  const clearManualPlanMutation = useClearManualPlanOverride();
  const partnerChatMutation = usePartnerChatAction();
  const disconnectMutation = useDisconnectBot();
  const engagementQuery = useEngagementSettings(login);
  const engagementToggle = useEngagementToggle();
  const partnerAccessQuery = usePartnerAccess();
  const partnerAccessMutation = useSetPartnerAccess();
  const partnerAccessEntry = findPartnerAccessEntry(partnerAccessQuery.data, login);
  const partnerAccessGranted = Boolean(partnerAccessEntry?.granted);

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
  const [disconnectStage, setDisconnectStage] = useState<'idle' | 'warn' | 'type'>('idle');
  const [disconnectReport, setDisconnectReport] = useState<DisconnectBotResult | null>(null);
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
    return (
      <section className="space-y-5">
        <PageHeader title="Streamer wird geladen" description="Die Detaildaten werden gerade aus dem Admin-Backend geladen." />
        <div className="panel-card rounded-[1.8rem] p-8 text-white">Streamer wird geladen …</div>
      </section>
    );
  }

  if (detailQuery.isError) {
    return (
      <section className="space-y-5">
        <div className="panel-card rounded-[1.8rem] p-6">
          <Link to="/community/streamers" className="inline-flex items-center gap-2 text-sm text-text-secondary transition hover:text-white">
            <ArrowLeft className="h-4 w-4" />
            Zurück zur Liste
          </Link>
        </div>
        <PageHeader
          title="Streamer-Details nicht verfügbar"
          description={
            detailQuery.error instanceof Error
              ? detailQuery.error.message
              : 'Streamer-Details konnten nicht geladen werden.'
          }
        />
      </section>
    );
  }

  if (!login) {
    return (
      <section className="space-y-5">
        <PageHeader title="Ungültiger Streamer-Link" description="Die Detailseite benötigt einen gültigen Login im Pfad." />
      </section>
    );
  }

  if (!detail) {
    return (
      <section className="space-y-5">
        <PageHeader title={login} description="Für diesen Login ist aktuell kein verwalteter Streamer-Datensatz vorhanden." />
      </section>
    );
  }

  return (
    <section className="space-y-5">
      <div className="panel-card rounded-[1.8rem] p-6">
        <Link to="/community/streamers" className="inline-flex items-center gap-2 text-sm text-text-secondary transition hover:text-white">
          <ArrowLeft className="h-4 w-4" />
          Zurück zur Liste
        </Link>
      </div>

      <PageHeader
        title={detail.displayName || detail.login}
        description={`Zuletzt gesehen ${formatRelativeTime(readString(stats, 'lastSeenAt', 'last_seen_at') || detail.archivedAt || detail.createdAt)}`}
        primaryAction={
          <div className="flex flex-wrap justify-end gap-2">
            <span className="stat-pill">Plan: {detail.planId || 'Unbekannt'}</span>
            <span className="stat-pill">Sessions: {metricFromStats(stats, 'totalSessions', 'session_count', 'sessions')}</span>
          </div>
        }
        secondaryChips={
          <>
            <StatusBadge status={detail.partnerStatus || 'active'} />
            <StatusBadge status={detail.isLive ? 'live' : detail.verified ? 'verified' : 'offline'} />
            <StatusBadge status={oauthStatus} />
            {oauthNeedsReauth ? <StatusBadge status="reauth-needed" /> : null}
            {detail.planId ? <StatusBadge status={detail.planId} /> : null}
          </>
        }
      />

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
              <button
                className="admin-button admin-button-secondary"
                disabled={blockMutation.isPending}
                onClick={async () => {
                  try {
                    const result = await blockMutation.mutateAsync({
                      login: detail.login,
                      mode: detail.partnerStatus === 'blocked' ? 'unblock' : 'block',
                    });
                    setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
                  } catch (error) {
                    setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Block-Aktion fehlgeschlagen' });
                  }
                }}
              >
                {detail.partnerStatus === 'blocked' ? <ShieldCheck className="h-4 w-4" /> : <Ban className="h-4 w-4" />}
                {detail.partnerStatus === 'blocked' ? 'Entsperren' : 'Blockieren'}
              </button>
            </div>
          </div>
          <div className="mt-4 flex flex-wrap gap-2">
            <StatusBadge status={persistedMemberFlag ? 'active' : 'inactive'} />
            <StatusBadge status={detail.partnerStatus || 'active'} />
            {detail.archivedAt ? <span className="stat-pill">Archiviert {formatDateTime(detail.archivedAt)}</span> : null}
            {detail.partnerStatus === 'departnered' ? <span className="stat-pill">Operativ deaktiviert</span> : null}
            {detail.partnerStatus === 'blocked' ? <span className="stat-pill">Hart blockiert</span> : null}
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
            {oauthNeedsReauth ? <StatusBadge status="reauth-needed" /> : null}
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
                    <span key={scope} className="inline-flex items-center rounded-full border border-success/35 bg-success/14 px-2.5 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.18em] text-success">
                      {scope}
                    </span>
                  ))
                ) : (
                  <EmptyState
                    icon={ShieldOff}
                    title="Keine Scopes vorhanden"
                    description="Für diesen Streamer liegen aktuell keine granteten OAuth-Scopes vor."
                    className="w-full"
                  />
                )}
              </div>
            </div>
            <div>
              <p className="text-sm font-semibold text-white">Fehlend</p>
              <div className="mt-3 flex flex-wrap gap-2">
                {missingScopes.length ? (
                  missingScopes.map((scope) => (
                    <span key={scope} className="inline-flex items-center rounded-full border border-warning/35 bg-warning/14 px-2.5 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.18em] text-warning">
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
                  type="date"
                  className="admin-input mt-2"
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

      <article className="panel-card rounded-[1.8rem] p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Bot vom Kanal trennen</p>
        <p className="mt-3 text-sm leading-6 text-text-secondary">
          Für Streamer, die den Bot nicht mehr wollen: entzieht die Moderator-Rechte auf Twitch, beendet die
          Partnerschaft und setzt den Opt-out, damit kein automatischer Sweep den Kanal zurückholt. Die
          Twitch-Autorisierung bleibt liegen — verbindet sich der Streamer später erneut, moddet sich der Bot wieder
          selbst.
        </p>
        <div className="mt-4 flex flex-wrap items-center gap-3">
          <button
            className="admin-button admin-button-danger"
            disabled={disconnectMutation.isPending}
            onClick={() => {
              setDisconnectReport(null);
              setDisconnectStage('warn');
            }}
          >
            <PlugZap className="h-4 w-4" />
            Bot bewusst trennen
          </button>
          <span className="text-xs text-text-secondary">Zwei Bestätigungen, die zweite mit Login-Eingabe.</span>
        </div>
        {disconnectReport ? (
          <ul className="mt-5 space-y-2 rounded-2xl border border-white/10 bg-bg/55 p-4 text-sm leading-6">
            <li className={disconnectReport.unmod === 'removed' ? 'text-success' : 'text-warning'}>
              Twitch: {UNMOD_LABELS[disconnectReport.unmod]}
              {disconnectReport.unmodDetail ? ` (${disconnectReport.unmodDetail})` : ''}
            </li>
            <li className={disconnectReport.departnered ? 'text-success' : 'text-warning'}>
              Partnerschaft: {disconnectReport.departnered ? 'beendet' : 'war kein aktiver Partner'}
            </li>
            <li className={disconnectReport.optOut ? 'text-success' : 'text-warning'}>
              Opt-out: {disconnectReport.optOut ? 'gesetzt' : 'nicht gesetzt'}
            </li>
            <li
              className={
                disconnectReport.discordRole === 'revoked'
                  ? 'text-success'
                  : discordRoleDone(disconnectReport.discordRole)
                    ? 'text-text-secondary'
                    : 'text-warning'
              }
            >
              Discord: {discordRoleLabel(disconnectReport.discordRole)}
            </li>
          </ul>
        ) : null}
      </article>

      <article className="panel-card rounded-[1.8rem] p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Partner-Freigabe (Social Media)</p>
        <div className="mt-4 flex items-center justify-between gap-4">
          <div>
            <p className="text-sm text-white">Social-Media-Posts für diesen Streamer erlaubt</p>
            <p className="mt-1 text-xs text-text-secondary">
              {partnerAccessQuery.isLoading
                ? 'Wird geladen …'
                : partnerAccessQuery.isError
                  ? 'Freigabestatus konnte nicht geladen werden — Toggle bleibt gesperrt.'
                  : partnerAccessGranted
                    ? `Freigegeben${partnerAccessEntry?.granted_by ? ` · zuletzt von ${partnerAccessEntry.granted_by}` : ''}${partnerAccessEntry?.granted_at ? ` · ${formatDateTime(partnerAccessEntry.granted_at)}` : ''}`
                    : 'Nicht freigegeben — jeder Schreibpfad (Post, Draft, Caption, Upload) wird serverseitig mit 403 abgelehnt.'}
            </p>
          </div>
          <button
            className={`relative inline-flex h-7 w-12 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 focus:outline-none ${
              partnerAccessGranted ? 'bg-success' : 'bg-white/20'
            } ${partnerAccessMutation.isPending || partnerAccessQuery.isError ? 'opacity-50 cursor-not-allowed' : ''}`}
            disabled={partnerAccessMutation.isPending || partnerAccessQuery.isError}
            role="switch"
            aria-checked={partnerAccessGranted}
            aria-label={partnerAccessGranted ? 'Partner-Freigabe entfernen' : 'Partner-Freigabe erteilen'}
            onClick={async () => {
              if (!login) return;
              const result = await partnerAccessMutation.mutateAsync({
                login,
                granted: !partnerAccessGranted,
              });
              setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
            }}
          >
            <span
              className={`inline-block h-6 w-6 transform rounded-full bg-white shadow transition duration-200 ${
                partnerAccessGranted ? 'translate-x-5' : 'translate-x-0'
              }`}
            />
          </button>
        </div>
      </article>

      <article className="panel-card rounded-[1.8rem] p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">AI-Engagement (Chatter)</p>
        <div className="mt-4 flex items-center justify-between gap-4">
          <div>
            <p className="text-sm text-white">AI-Stammgast im Chat</p>
            <p className="mt-1 text-xs text-text-secondary">
              {engagementQuery.isLoading
                ? 'Wird geladen …'
                : engagementQuery.data
                  ? `${engagementQuery.data.enabled ? 'Aktiv' : 'Inaktiv'}${engagementQuery.data.enabledBy ? ` · zuletzt von ${engagementQuery.data.enabledBy}` : ''}`
                  : 'Noch nicht konfiguriert — Standard: inaktiv'}
            </p>
          </div>
          <button
            className={`relative inline-flex h-7 w-12 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 focus:outline-none ${
              engagementQuery.data?.enabled ? 'bg-success' : 'bg-white/20'
            } ${engagementToggle.isPending ? 'opacity-50 cursor-not-allowed' : ''}`}
            disabled={engagementToggle.isPending}
            role="switch"
            aria-checked={engagementQuery.data?.enabled ?? false}
            onClick={async () => {
              if (!login) return;
              const next = !(engagementQuery.data?.enabled ?? false);
              const result = await engagementToggle.mutateAsync({ login, enabled: next });
              setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
            }}
          >
            <span
              className={`inline-block h-6 w-6 transform rounded-full bg-white shadow transition duration-200 ${
                engagementQuery.data?.enabled ? 'translate-x-5' : 'translate-x-0'
              }`}
            />
          </button>
        </div>
      </article>

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
          <pre className="mt-4 overflow-auto rounded-[1.4rem] border border-white/10 bg-bg/55 p-4 text-xs leading-6 text-success">
            {JSON.stringify(settings, null, 2)}
          </pre>
        </article>
      </section>

      <article className="panel-card rounded-[1.8rem] p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Letzte Sessions</p>
        <div className="mt-4">
          <DataTable
            columns={sessionColumns}
            rows={detail.sessions ?? []}
            rowKey={(row, index) => `${row.sessionId ?? index}`}
            emptyState={
              <EmptyState
                icon={CctvOff}
                title="Keine Sessions sichtbar"
                description="Für diesen Streamer wurden im aktuellen Snapshot noch keine Sessions gefunden."
              />
            }
          />
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
              navigate('/community/streamers');
            }
          } catch (error) {
            setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Streamer konnte nicht entfernt werden' });
          }
        }}
      />

      <ConfirmDialog
        open={disconnectStage === 'warn'}
        title={`Bot von ${detail.login} trennen?`}
        description={`Der Bot verlässt ${detail.login} operativ: Moderator-Rechte weg, Partnerschaft beendet, Opt-out gesetzt. Im nächsten Schritt musst du den Login abtippen.`}
        confirmLabel="Weiter"
        tone="danger"
        onCancel={() => setDisconnectStage('idle')}
        onConfirm={() => setDisconnectStage('type')}
      />

      <ConfirmTypedDialog
        open={disconnectStage === 'type'}
        title={`${detail.login} wirklich trennen?`}
        description="Das passiert der Reihe nach, sobald du bestätigst:"
        expected={detail.login}
        steps={[
          'Der Bot verliert seine Moderator-Rechte im Twitch-Kanal.',
          'Die Partnerschaft wird beendet: kein Auto-Raid, kein Raid-Ziel, keine Chat-Funktionen.',
          'Der Opt-out wird gesetzt, damit kein automatischer Sweep den Kanal zurückholt.',
          'Die Discord-Streamer-Rolle wird entzogen, falls verknüpft.',
          'Die Twitch-Autorisierung bleibt bestehen — eine erneute Freigabe holt den Bot zurück.',
        ]}
        confirmLabel="Jetzt trennen"
        busy={disconnectMutation.isPending}
        onCancel={() => setDisconnectStage('idle')}
        onConfirm={async () => {
          try {
            const result = await disconnectMutation.mutateAsync({
              login: detail.login,
              confirmLogin: detail.login,
            });
            setDisconnectReport(result);
            setDisconnectStage('idle');
            setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
          } catch (error) {
            setToast({
              open: true,
              tone: 'error',
              message: error instanceof Error ? error.message : 'Trennung fehlgeschlagen',
            });
          }
        }}
      />

      <Toast open={toast.open} tone={toast.tone} message={toast.message} onClose={() => setToast((current) => ({ ...current, open: false }))} />
    </section>
  );
}
