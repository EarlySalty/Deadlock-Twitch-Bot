import { useState } from 'react';
import { ArchiveRestore, Ban, Eye, FolderArchive, Maximize2, Minimize2, Plus, SearchX, ShieldAlert, ShieldCheck, Trash2, X } from 'lucide-react';
import { Link } from 'react-router';
import { buildRaidAuthUrl, buildRaidRequirementsUrl } from '@/api/client';
import type { ScopeStatusRow, StreamerPartnerStatus, StreamerRow } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { SearchInput } from '@/components/shared/SearchInput';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { Toast } from '@/components/shared/Toast';
import { useAddStreamer, useArchiveStreamer, useBlockStreamer, useRemoveStreamer, useScopeStatus, useStreamers } from '@/hooks/useAdmin';
import { formatDateTime, formatNumber, formatRelativeTime } from '@/utils/formatters';

type PendingAction =
  | { type: 'remove'; row: StreamerRow }
  | { type: 'archive'; row: StreamerRow }
  | null;

const viewOptions: Array<{
  value: StreamerPartnerStatus | 'all';
  label: string;
  description: string;
}> = [
  { value: 'active', label: 'Aktiv', description: 'operative Partner ohne Admin-Archiv' },
  { value: 'archived', label: 'Archiv', description: 'im Admin ausgeblendete Partner' },
  { value: 'token_error', label: 'Token Error', description: 'OAuth defekt, 7 Tage Grace bis Opt-out' },
  { value: 'departnered', label: 'De-Partnered', description: 'operativ deaktivierte Ex-Partner' },
  { value: 'non_partner', label: 'Kein Partner', description: 'intern ausgeschlossene Logins' },
  { value: 'blocked', label: 'Blocked', description: 'harter globaler Ausschluss ohne Re-Auth-Rueckweg' },
  { value: 'all', label: 'Alle', description: 'gesamter Partnerbestand' },
];

function filterByView(rows: StreamerRow[], view: StreamerPartnerStatus | 'all') {
  if (view === 'all') {
    return rows;
  }
  return rows.filter((row) => row.partnerStatus === view);
}

function matchesStreamerSearch(row: StreamerRow, search: string) {
  const haystack = [
    row.login,
    row.displayName,
    row.discordDisplayName,
    row.discordUserId,
    row.planId,
    row.partnerStatus,
    row.status,
    row.oauthStatus,
  ];
  return haystack.some((value) =>
    String(value || '')
      .toLowerCase()
      .includes(search.toLowerCase()),
  );
}

function matchesScopeSearch(row: ScopeStatusRow, search: string) {
  const haystack = [row.login, row.displayName, row.partnerStatus, row.oauthStatus, ...(row.missingScopes ?? [])];
  return haystack.some((value) =>
    String(value || '')
      .toLowerCase()
      .includes(search.toLowerCase()),
  );
}

function scopeCellClass(enabled: boolean, critical = false) {
  if (enabled) {
    return critical
      ? 'border-success/35 bg-success/18 text-success'
      : 'border-accent/30 bg-accent/14 text-accent';
  }
  return critical
    ? 'border-warning/35 bg-warning/18 text-warning'
    : 'border-border bg-card-hover/30 text-secondary';
}

function formatPartnerStatus(status: StreamerPartnerStatus | undefined) {
  return status ?? 'active';
}

export function StreamerList() {
  const [search, setSearch] = useState('');
  const [view, setView] = useState<StreamerPartnerStatus | 'all'>('active');
  const [partnerSinceFrom, setPartnerSinceFrom] = useState('');
  const [partnerSinceTo, setPartnerSinceTo] = useState('');
  const [density, setDensity] = useState<'comfortable' | 'compact'>('comfortable');
  const [newLogin, setNewLogin] = useState('');
  const [newDiscordUserId, setNewDiscordUserId] = useState('');
  const [newDiscordDisplayName, setNewDiscordDisplayName] = useState('');
  const [newMemberFlag, setNewMemberFlag] = useState(false);
  const [pendingAction, setPendingAction] = useState<PendingAction>(null);
  const [toast, setToast] = useState<{ open: boolean; tone: 'success' | 'error'; message: string }>({
    open: false,
    tone: 'success',
    message: '',
  });

  const streamersQuery = useStreamers('all');
  const scopeStatusQuery = useScopeStatus();
  const addMutation = useAddStreamer();
  const removeMutation = useRemoveStreamer();
  const archiveMutation = useArchiveStreamer();
  const blockMutation = useBlockStreamer();
  const selectedView = viewOptions.find((option) => option.value === view);

  const allRows = streamersQuery.data ?? [];
  const counts = {
    active: allRows.filter((row) => row.partnerStatus === 'active').length,
    archived: allRows.filter((row) => row.partnerStatus === 'archived').length,
    token_error: allRows.filter((row) => row.partnerStatus === 'token_error').length,
    departnered: allRows.filter((row) => row.partnerStatus === 'departnered').length,
    non_partner: allRows.filter((row) => row.partnerStatus === 'non_partner').length,
    blocked: allRows.filter((row) => row.partnerStatus === 'blocked').length,
    all: allRows.length,
  };
  const rows = filterByView(allRows, view)
    .filter((row) => matchesStreamerSearch(row, search))
    .filter((row) => {
      if (!partnerSinceFrom && !partnerSinceTo) {
        return true;
      }
      if (!row.partnerSince) {
        return false;
      }
      const day = row.partnerSince.slice(0, 10);
      return (!partnerSinceFrom || day >= partnerSinceFrom) && (!partnerSinceTo || day <= partnerSinceTo);
    });
  const scopeRows = (scopeStatusQuery.data?.items ?? []).filter((row) => matchesScopeSearch(row, search));

  const header = (
    <PageHeader
      title="Streamer-Verwaltung"
      description="Bestand, Lifecycle-Status und OAuth-Scope-Lage in einer konsistenten Admin-Oberfläche bündeln."
      secondaryChips={
        <>
          <span className="stat-pill">{formatNumber(allRows.length)} gesamt</span>
          <span className="stat-pill">{formatNumber(allRows.filter((row) => row.isLive).length)} live</span>
          <span className="stat-pill">{formatNumber(allRows.filter((row) => row.oauthNeedsReauth).length)} Reauth</span>
          {selectedView && selectedView.value !== 'all' ? <span className="stat-pill">Filter: {selectedView.label}</span> : null}
        </>
      }
    />
  );

  function renderDensityToggle() {
    return (
      <div className="flex items-center gap-2">
        <button
          type="button"
          className={`admin-button admin-button-secondary !px-3 !py-2 text-xs ${density === 'comfortable' ? '!border-primary/40 !bg-primary/10 !text-white' : ''}`}
          onClick={() => setDensity('comfortable')}
        >
          <Maximize2 className="h-4 w-4" />
          Komfort
        </button>
        <button
          type="button"
          className={`admin-button admin-button-secondary !px-3 !py-2 text-xs ${density === 'compact' ? '!border-primary/40 !bg-primary/10 !text-white' : ''}`}
          onClick={() => setDensity('compact')}
        >
          <Minimize2 className="h-4 w-4" />
          Kompakt
        </button>
      </div>
    );
  }

  if (streamersQuery.isLoading && !streamersQuery.data) {
    return (
      <section className="space-y-6">
        {header}
        <div className="panel-card rounded-[1.8rem] p-8 text-white">Streamer werden geladen …</div>
      </section>
    );
  }

  if (streamersQuery.isError) {
    return (
      <section className="space-y-6">
        <PageHeader
          title="Streamer konnten nicht geladen werden"
          description={
            streamersQuery.error instanceof Error
              ? streamersQuery.error.message
              : 'Die Streamer-Liste konnte nicht geladen werden.'
          }
        />
      </section>
    );
  }

  const streamersColumns: TableColumn<StreamerRow>[] = [
    {
      key: 'login',
      title: 'Streamer',
      sortable: true,
      sortValue: (row) => row.login,
      render: (row) => (
        <div>
          <Link to={`/community/streamers/${encodeURIComponent(row.login)}`} className="font-semibold text-white transition hover:text-primary">
            {row.displayName || row.login}
          </Link>
          <p className="text-xs uppercase tracking-[0.16em] text-text-secondary">{row.login}</p>
        </div>
      ),
    },
    {
      key: 'status',
      title: 'Status',
      sortable: true,
      sortValue: (row) => `${row.partnerStatus}-${row.oauthStatus}-${row.isLive ? 1 : 0}`,
      render: (row) => (
        <div className="flex max-w-[300px] flex-wrap gap-2">
          <StatusBadge status={formatPartnerStatus(row.partnerStatus)} />
          <StatusBadge status={row.isLive ? 'live' : row.verified ? 'verified' : 'offline'} />
          <StatusBadge status={row.oauthStatus || 'missing'} />
          {row.planId ? <StatusBadge status={row.planId} /> : null}
        </div>
      ),
    },
    {
      key: 'discord',
      title: 'Discord',
      sortable: true,
      sortValue: (row) => row.discordDisplayName || row.discordUserId || row.login,
      render: (row) => (
        <div className="space-y-1">
          <div className="text-white">{row.discordDisplayName || 'Kein Anzeigename'}</div>
          <div className="text-xs text-text-secondary">{row.discordUserId || 'Keine Discord-ID'}</div>
          <StatusBadge status={row.isOnDiscord ? 'active' : 'inactive'} />
        </div>
      ),
    },
    {
      key: 'partner-since',
      title: 'Partner seit',
      sortable: true,
      sortValue: (row) => row.partnerSince || '',
      render: (row) => (
        <div className="space-y-1">
          <div className="font-medium text-white">{row.partnerSince ? formatDateTime(row.partnerSince) : 'Nie autorisiert'}</div>
          {row.partnerSince ? <div className="text-xs text-text-secondary">{formatRelativeTime(row.partnerSince)}</div> : null}
        </div>
      ),
    },
    {
      key: 'activity',
      title: 'Aktivität',
      sortable: true,
      sortValue: (row) => row.lastSeenAt || row.lastStreamAt || row.archivedAt || '',
      render: (row) => (
        <div className="space-y-1">
          <div className="font-medium text-white">{formatNumber(row.viewerCount ?? 0)} Viewer</div>
          <div className="text-xs text-text-secondary">
            Zuletzt gesehen {formatRelativeTime(row.lastSeenAt || row.lastStreamAt || row.archivedAt)}
          </div>
          {row.partnerStatus === 'archived' ? (
            <div className="text-xs text-text-secondary">Archiviert {formatDateTime(row.archivedAt)}</div>
          ) : row.partnerStatus === 'token_error' ? (
            <div className="text-xs text-text-secondary">OAuth-Fehler / eingeschraenkter Zugriff</div>
          ) : row.partnerStatus === 'blocked' ? (
            <div className="text-xs text-text-secondary">Hart blockiert, kein Dashboard-Zugriff</div>
          ) : row.partnerStatus === 'departnered' ? (
            <div className="text-xs text-text-secondary">Operativ deaktiviert</div>
          ) : null}
        </div>
      ),
    },
    {
      key: 'actions',
      title: 'Aktionen',
      className: 'min-w-[320px]',
      render: (row) => (
        <div className="flex flex-wrap gap-2">
          <Link to={`/community/streamers/${encodeURIComponent(row.login)}`} className="admin-button admin-button-secondary !px-3 !py-2">
            <Eye className="h-4 w-4" />
            Verwalten
          </Link>
          {row.partnerStatus !== 'non_partner' && row.partnerStatus !== 'departnered' ? (
            <button onClick={() => setPendingAction({ type: 'archive', row })} className="admin-button admin-button-secondary !px-3 !py-2">
              {row.partnerStatus === 'archived' ? <ArchiveRestore className="h-4 w-4" /> : <FolderArchive className="h-4 w-4" />}
              {row.partnerStatus === 'archived' ? 'Reaktivieren' : 'Archivieren'}
            </button>
          ) : null}
          {row.partnerStatus !== 'departnered' ? (
            <button onClick={() => setPendingAction({ type: 'remove', row })} className="admin-button admin-button-danger !px-3 !py-2">
              <Trash2 className="h-4 w-4" />
              {row.partnerStatus === 'non_partner' ? 'Entfernen' : 'Partner deaktivieren'}
            </button>
          ) : null}
          <a href={buildRaidAuthUrl(row.login)} target="_blank" rel="noreferrer" className="admin-button admin-button-secondary !px-3 !py-2">
            OAuth
          </a>
          <button
            onClick={async () => {
              try {
                const result = await blockMutation.mutateAsync({
                  login: row.login,
                  mode: row.partnerStatus === 'blocked' ? 'unblock' : 'block',
                });
                setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
              } catch (error) {
                setToast({ open: true, tone: 'error', message: error instanceof Error ? error.message : 'Block-Aktion fehlgeschlagen' });
              }
            }}
            className="admin-button admin-button-secondary !px-3 !py-2"
          >
            {row.partnerStatus === 'blocked' ? <ShieldCheck className="h-4 w-4" /> : <Ban className="h-4 w-4" />}
            {row.partnerStatus === 'blocked' ? 'Entsperren' : 'Blockieren'}
          </button>
          <a href={buildRaidRequirementsUrl(row.login)} className="admin-button admin-button-secondary !px-3 !py-2">
            Anforderungen
          </a>
        </div>
      ),
    },
  ];

  const requiredScopes = scopeStatusQuery.data?.requiredScopes ?? [];
  const criticalScopes = new Set(scopeStatusQuery.data?.criticalScopes ?? []);
  const scopeColumns: TableColumn<ScopeStatusRow>[] = [
    {
      key: 'scope-login',
      title: 'Streamer',
      sortable: true,
      sortValue: (row) => row.login,
      className: 'min-w-[200px]',
      render: (row) => (
        <div>
          <Link to={`/streamers/${encodeURIComponent(row.login)}`} className="font-semibold text-white hover:text-primary">
            {row.displayName || row.login}
          </Link>
          <p className="text-xs uppercase tracking-[0.16em] text-text-secondary">{row.login}</p>
        </div>
      ),
    },
    {
      key: 'scope-status',
      title: 'Status',
      sortable: true,
      sortValue: (row) => `${row.oauthStatus}-${row.partnerStatus}`,
      className: 'min-w-[180px]',
      render: (row) => (
        <div className="flex flex-wrap gap-2">
          <StatusBadge status={formatPartnerStatus(row.partnerStatus)} />
          <StatusBadge status={row.oauthStatus || 'missing'} />
        </div>
      ),
    },
    ...requiredScopes.map<TableColumn<ScopeStatusRow>>((scope) => ({
      key: scope,
      title: (scopeStatusQuery.data?.labels?.[scope] as string | undefined) || scope,
      sortable: true,
      sortValue: (row) => (row.grantedScopes.includes(scope) ? 1 : 0),
      className: 'min-w-[78px] text-center',
      render: (row) => {
        const enabled = row.grantedScopes.includes(scope);
        return (
          <span
            className={[
              'inline-flex min-w-[46px] items-center justify-center rounded-full border px-2 py-1 text-xs font-semibold uppercase tracking-[0.14em]',
              scopeCellClass(enabled, criticalScopes.has(scope)),
            ].join(' ')}
            title={scope}
          >
            {enabled ? 'Ja' : 'Nein'}
          </span>
        );
      },
    })),
  ];

  return (
    <section className="space-y-6">
      {header}

      <Section title="Streamer hinzufügen" hint="Neue Twitch-Logins samt optionaler Discord-Zuordnung in den verwalteten Bestand aufnehmen.">
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <label className="rounded-[1.6rem] border border-white/10 bg-white/[0.04] p-4 text-sm">
              <span className="text-xs uppercase tracking-[0.18em] text-text-secondary">Twitch Login</span>
              <input
                value={newLogin}
                onChange={(event) => setNewLogin(event.target.value)}
                placeholder="earlysalty"
                className="admin-input mt-3"
              />
          </label>
          <label className="rounded-[1.6rem] border border-white/10 bg-white/[0.04] p-4 text-sm">
              <span className="text-xs uppercase tracking-[0.18em] text-text-secondary">Discord User ID</span>
              <input
                value={newDiscordUserId}
                onChange={(event) => setNewDiscordUserId(event.target.value)}
                placeholder="123456789012345678"
                className="admin-input mt-3"
              />
          </label>
          <label className="rounded-[1.6rem] border border-white/10 bg-white/[0.04] p-4 text-sm">
              <span className="text-xs uppercase tracking-[0.18em] text-text-secondary">Discord Anzeigename</span>
              <input
                value={newDiscordDisplayName}
                onChange={(event) => setNewDiscordDisplayName(event.target.value)}
                placeholder="Discord-Name"
                className="admin-input mt-3"
              />
          </label>
          <div className="rounded-[1.6rem] border border-white/10 bg-white/[0.04] p-4 text-sm">
              <span className="text-xs uppercase tracking-[0.18em] text-text-secondary">Optionen</span>
              <label className="mt-4 flex items-center gap-3 text-text-secondary">
                <input
                  checked={newMemberFlag}
                  onChange={(event) => setNewMemberFlag(event.target.checked)}
                  type="checkbox"
                />
                Als Discord-Mitglied markieren
              </label>
              <button
                className="admin-button admin-button-primary mt-5 w-full"
                disabled={!newLogin.trim() || addMutation.isPending}
                onClick={async () => {
                  try {
                    const result = await addMutation.mutateAsync({
                      login: newLogin.trim(),
                      discordUserId: newDiscordUserId.trim() || undefined,
                      discordDisplayName: newDiscordDisplayName.trim() || undefined,
                      memberFlag: newMemberFlag,
                    });
                    setNewLogin('');
                    setNewDiscordUserId('');
                    setNewDiscordDisplayName('');
                    setNewMemberFlag(false);
                    setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
                  } catch (error) {
                    setToast({
                      open: true,
                      tone: 'error',
                      message: error instanceof Error ? error.message : 'Streamer konnte nicht hinzugefügt werden',
                    });
                  }
                }}
              >
                <Plus className="h-4 w-4" />
                Streamer hinzufügen
              </button>
          </div>
        </div>
      </Section>

      <Section title="Filter" hint="Statusansicht und Suche greifen auf Listen- und Scope-Matrix gleichzeitig.">
        <div className="grid gap-4 lg:grid-cols-[minmax(280px,420px)_1fr] lg:items-center">
          <SearchInput
            placeholder="Nach Login, Discord, Plan oder OAuth suchen"
            defaultValue={search}
            onDebouncedChange={setSearch}
          />
          <div className="flex flex-wrap gap-2">
            {viewOptions.map((option) => (
              <button
                key={option.value}
                onClick={() => setView(option.value)}
                className={[
                  'filter-chip',
                  view === option.value ? '!border-primary/45 !bg-primary/12 !text-white' : 'text-text-secondary',
                ].join(' ')}
              >
                {option.label}
                <span className="text-white">{counts[option.value]}</span>
              </button>
            ))}
          </div>
        </div>

        <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:max-w-2xl">
          <label className="text-sm text-text-secondary">
            <span className="mb-2 block text-xs font-semibold uppercase tracking-[0.16em]">Partner seit – von</span>
            <input
              type="date"
              value={partnerSinceFrom}
              max={partnerSinceTo || undefined}
              onChange={(event) => setPartnerSinceFrom(event.target.value)}
              className="admin-input"
            />
          </label>
          <label className="text-sm text-text-secondary">
            <span className="mb-2 block text-xs font-semibold uppercase tracking-[0.16em]">Partner seit – bis</span>
            <input
              type="date"
              value={partnerSinceTo}
              min={partnerSinceFrom || undefined}
              onChange={(event) => setPartnerSinceTo(event.target.value)}
              className="admin-input"
            />
          </label>
        </div>

        {view !== 'all' || search.trim() || partnerSinceFrom || partnerSinceTo ? (
          <div className="mt-5 flex flex-wrap gap-2">
            {view !== 'all' && selectedView ? (
              <div className="filter-chip">
                <span>{selectedView.label}</span>
                <button type="button" aria-label={`${selectedView.label} entfernen`} onClick={() => setView('all')}>
                  <X className="h-3.5 w-3.5" />
                </button>
              </div>
            ) : null}
            {search.trim() ? (
              <div className="filter-chip">
                <span>Suche: {search}</span>
                <button type="button" aria-label="Suche entfernen" onClick={() => setSearch('')}>
                  <X className="h-3.5 w-3.5" />
                </button>
              </div>
            ) : null}
            {partnerSinceFrom || partnerSinceTo ? (
              <div className="filter-chip">
                <span>Partner seit: {partnerSinceFrom || 'offen'} bis {partnerSinceTo || 'offen'}</span>
                <button
                  type="button"
                  aria-label="Partner-seit-Filter entfernen"
                  onClick={() => {
                    setPartnerSinceFrom('');
                    setPartnerSinceTo('');
                  }}
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              </div>
            ) : null}
          </div>
        ) : null}
      </Section>

      <div className="grid gap-4 xl:grid-cols-4">
        {viewOptions.map((option) => (
          <article key={option.value} className="panel-card rounded-[1.6rem] p-5">
            <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">{option.label}</p>
            <div className="mt-3 text-3xl font-semibold text-white">{formatNumber(counts[option.value])}</div>
            <p className="mt-2 text-sm leading-6 text-text-secondary">{option.description}</p>
          </article>
        ))}
      </div>

      <Section
        title="Bestand"
        hint="Sichtbarer Streamer-Bestand nach Status, Aktivität und Discord-/OAuth-Signalen."
        action={renderDensityToggle()}
      >
        <DataTable
          columns={streamersColumns}
          rows={rows}
          rowKey={(row) => row.login}
          density={density}
          emptyState={
            <EmptyState
              icon={SearchX}
              title="Keine Streamer gefunden"
              description={
                search.trim()
                  ? 'Die aktuelle Suche oder Statusansicht liefert keine Treffer.'
                  : 'Für den gewählten Status sind aktuell keine Streamer vorhanden.'
              }
              action={
                view !== 'all' || search.trim() || partnerSinceFrom || partnerSinceTo ? (
                  <button
                    type="button"
                    className="admin-button admin-button-secondary"
                    onClick={() => {
                      setView('all');
                      setSearch('');
                      setPartnerSinceFrom('');
                      setPartnerSinceTo('');
                    }}
                  >
                    Filter zurücksetzen
                  </button>
                ) : undefined
              }
            />
          }
        />
      </Section>

      <Section
        title="OAuth Token Scopes"
        hint="Dieselbe Logik wie im Legacy-Admin für autorisierte Twitch-Logins aus `twitch_raid_auth`."
        action={renderDensityToggle()}
      >
        {scopeStatusQuery.isError ? (
          <EmptyState
            icon={ShieldAlert}
            title="Scope-Status nicht verfügbar"
            description={
              scopeStatusQuery.error instanceof Error
                ? scopeStatusQuery.error.message
                : 'Die Scope-Matrix konnte nicht geladen werden.'
            }
          />
        ) : (
          <DataTable
            columns={scopeColumns}
            rows={scopeRows}
            rowKey={(row) => `scope-${row.login}`}
            density={density}
            emptyState={
              <EmptyState
                icon={SearchX}
                title="Keine Scope-Treffer"
                description="Für die aktuelle Suche wurden keine OAuth-Datensätze gefunden."
              />
            }
          />
        )}
      </Section>

      <ConfirmDialog
        open={Boolean(pendingAction)}
        title={
          pendingAction?.type === 'remove'
            ? pendingAction?.row.partnerStatus === 'non_partner'
              ? 'Streamer entfernen?'
              : 'Partner operativ deaktivieren?'
            : 'Archivstatus ändern?'
        }
        description={
          pendingAction?.type === 'remove'
            ? pendingAction?.row.partnerStatus === 'non_partner'
              ? `Der Streamer ${pendingAction?.row.login} wird vollständig aus dem verwalteten Bestand entfernt.`
              : `Der Streamer ${pendingAction?.row.login} bleibt im System, verliert aber operative Partnerfunktionen wie Auto-Raid und Raid-Targeting.`
            : pendingAction?.row.partnerStatus === 'archived'
              ? `Der Streamer ${pendingAction?.row.login} wird wieder als aktiver Partner geführt.`
              : `Der Streamer ${pendingAction?.row.login} wird archiviert.`
        }
        tone={pendingAction?.type === 'remove' ? 'danger' : 'default'}
        busy={removeMutation.isPending || archiveMutation.isPending}
        onCancel={() => setPendingAction(null)}
        onConfirm={async () => {
          if (!pendingAction) {
            return;
          }
          try {
            const result =
              pendingAction.type === 'remove'
                ? await removeMutation.mutateAsync(pendingAction.row.login)
                : await archiveMutation.mutateAsync({
                    login: pendingAction.row.login,
                    mode: pendingAction.row.partnerStatus === 'archived' ? 'unarchive' : 'archive',
                  });
            setToast({ open: true, tone: result.ok ? 'success' : 'error', message: result.message });
          } catch (error) {
            setToast({
              open: true,
              tone: 'error',
              message: error instanceof Error ? error.message : 'Aktion fehlgeschlagen',
            });
          } finally {
            setPendingAction(null);
          }
        }}
      />

      <Toast open={toast.open} tone={toast.tone} message={toast.message} onClose={() => setToast((current) => ({ ...current, open: false }))} />
    </section>
  );
}
