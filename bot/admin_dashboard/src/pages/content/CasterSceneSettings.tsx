import { useEffect, useRef, useState } from 'react';
import {
  createCasterLibraryTeam,
  deleteCasterAsset,
  deleteCasterLibraryTeam,
  fetchCasterAssets,
  fetchCasterTeamLibrary,
  searchCasterCoachMembers,
  updateCasterLibraryTeam,
  uploadCasterAsset,
  type CasterAsset,
  type CasterAssetKind,
  type CasterCoachMember,
  type CasterContextTeam,
  type CasterEventLogo,
  type CasterScene,
  type CasterSceneCoach,
  type CasterSceneTeam,
} from '@/api/client';
import { assetUrl, builtinEventLogos, eventLogoUrl, fromLocalInput, publicUrl, toLocalInput } from './casterScene';

type Side = 'teamA' | 'teamB';
type Edit = (change: (value: CasterScene) => CasterScene) => void;
type Notify = { status: (message: string) => void; error: (message: string) => void };

const button =
  'min-h-11 rounded-lg border border-white/20 px-3 py-2 text-sm hover:border-primary focus-visible:outline-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:opacity-40';
const input =
  'min-h-11 w-full rounded-lg border border-white/15 bg-black/50 px-3 text-white focus-visible:outline-2 focus-visible:outline-primary';
const ACCEPTED = 'image/png,image/webp,image/jpeg,image/svg+xml';
const MAX_BYTES = 2 * 1024 * 1024;

function errorText(cause: unknown, fallback: string) {
  return cause instanceof Error ? cause.message : fallback;
}

function useAssets(kind: CasterAssetKind, notify: Notify) {
  const [assets, setAssets] = useState<CasterAsset[]>([]);
  useEffect(() => {
    fetchCasterAssets(kind)
      .then(setAssets)
      .catch((cause) => notify.error(errorText(cause, 'Hochgeladene Bilder konnten nicht geladen werden.')));
  }, [kind]);

  async function upload(file: File): Promise<CasterAsset | null> {
    if (file.size > MAX_BYTES) {
      notify.error('Das Bild ist größer als 2 MB.');
      return null;
    }
    try {
      const asset = await uploadCasterAsset(kind, file.name.slice(0, 80) || 'Logo', file);
      setAssets((items) => [asset, ...items]);
      notify.status(`${asset.name} hochgeladen.`);
      return asset;
    } catch (cause) {
      notify.error(errorText(cause, 'Upload fehlgeschlagen.'));
      return null;
    }
  }

  async function remove(asset: CasterAsset) {
    if (!window.confirm(`${asset.name} wirklich löschen?`)) return false;
    try {
      await deleteCasterAsset(asset.id);
      setAssets((items) => items.filter((item) => item.id !== asset.id));
      notify.status(`${asset.name} gelöscht.`);
      return true;
    } catch (cause) {
      notify.error(errorText(cause, 'Löschen fehlgeschlagen.'));
      return false;
    }
  }

  return { assets, upload, remove };
}

function useTeamLibrary(notify: Notify) {
  const [teams, setTeams] = useState<CasterSceneTeam[]>([]);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    fetchCasterTeamLibrary()
      .then(setTeams)
      .catch((cause) => notify.error(errorText(cause, 'Teambibliothek konnte nicht geladen werden.')));
  }, []);

  async function save(team: CasterSceneTeam): Promise<CasterSceneTeam | null> {
    if (!team.name.trim()) {
      notify.error('Für die Teambibliothek braucht das Team einen Namen.');
      return null;
    }
    setSaving(true);
    try {
      const known = team.libraryId ? teams.some((item) => item.libraryId === team.libraryId) : false;
      const saved = known && team.libraryId ? await updateCasterLibraryTeam(team.libraryId, team) : await createCasterLibraryTeam(team);
      setTeams((items) =>
        [...items.filter((item) => item.libraryId !== saved.libraryId), saved].sort((a, b) => a.name.localeCompare(b.name, 'de')),
      );
      notify.status(`${saved.name} in der Teambibliothek gespeichert.`);
      return saved;
    } catch (cause) {
      notify.error(errorText(cause, 'Team konnte nicht gespeichert werden.'));
      return null;
    } finally {
      setSaving(false);
    }
  }

  async function remove(team: CasterSceneTeam) {
    if (!team.libraryId || !window.confirm(`${team.name} aus der Teambibliothek löschen? Das laufende Overlay bleibt unverändert.`)) return false;
    setSaving(true);
    try {
      await deleteCasterLibraryTeam(team.libraryId);
      setTeams((items) => items.filter((item) => item.libraryId !== team.libraryId));
      notify.status(`${team.name} aus der Teambibliothek gelöscht.`);
      return true;
    } catch (cause) {
      notify.error(errorText(cause, 'Team konnte nicht gelöscht werden.'));
      return false;
    } finally {
      setSaving(false);
    }
  }

  return { teams, saving, save, remove };
}

function useCoachSearch(query: string, enabled: boolean) {
  const [hits, setHits] = useState<CasterCoachMember[]>([]);
  const [unavailable, setUnavailable] = useState(false);
  const latest = useRef(0);
  useEffect(() => {
    const term = query.trim();
    if (!enabled || term.length < 2) {
      setHits([]);
      return;
    }
    const ticket = latest.current + 1;
    latest.current = ticket;
    const timer = window.setTimeout(() => {
      searchCasterCoachMembers(term)
        .then((result) => {
          if (latest.current !== ticket) return;
          setUnavailable(!result.available);
          setHits(result.items.slice(0, 6));
        })
        .catch(() => {
          if (latest.current === ticket) setUnavailable(true);
        });
    }, 300);
    return () => window.clearTimeout(timer);
  }, [query, enabled]);
  return { hits, unavailable, clear: () => setHits([]) };
}

function UploadButton({ label, disabled, onFile }: { label: string; disabled: boolean; onFile: (file: File) => void }) {
  return (
    <label className={`${button} inline-flex cursor-pointer items-center justify-center ${disabled ? 'pointer-events-none opacity-40' : ''}`}>
      {label}
      <input
        type="file"
        accept={ACCEPTED}
        className="sr-only"
        disabled={disabled}
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = '';
          if (file) onFile(file);
        }}
      />
    </label>
  );
}

function LogoPreview({ url, alt }: { url: string | null; alt: string }) {
  return (
    <div className="flex h-16 w-16 shrink-0 items-center justify-center overflow-hidden rounded-full border border-white/15 bg-black/50">
      {url ? <img src={url} alt={alt} className="h-full w-full object-cover" /> : <span className="text-xs text-white/35">kein Logo</span>}
    </div>
  );
}

const MAX_PLAYERS = 12;

function emptyTeam(): CasterSceneTeam {
  return { id: null, libraryId: null, name: '', short: null, logoAssetId: null, players: [], coach: null };
}

function CoachField({ coach, busy, onChange }: { coach: CasterSceneCoach | null; busy: boolean; onChange: (coach: CasterSceneCoach | null) => void }) {
  const [focused, setFocused] = useState(false);
  const name = coach?.name ?? '';
  const search = useCoachSearch(name, focused && !coach?.discordId);
  return (
    <div className="space-y-2">
      <label className="block text-sm text-white/65">
        Coach
        <input
          className={`${input} mt-1`}
          value={name}
          maxLength={80}
          disabled={busy}
          placeholder="Discord-Name suchen oder frei eintragen"
          onFocus={() => setFocused(true)}
          onBlur={() => window.setTimeout(() => setFocused(false), 200)}
          onChange={(event) => onChange(event.target.value ? { name: event.target.value, discordId: null } : null)}
        />
      </label>
      {coach?.discordId && (
        <p className="flex flex-wrap items-center gap-2 text-xs text-white/55">
          <span className="rounded-full border border-primary/50 px-2 py-1 text-primary">Discord-Mitglied verknüpft</span>
          <button className="underline hover:text-white" disabled={busy} onClick={() => onChange({ name, discordId: null })}>
            Verknüpfung lösen
          </button>
        </p>
      )}
      {search.unavailable && focused && <p className="text-xs text-white/45">Discord-Suche gerade nicht erreichbar. Der Name wird als freier Text gespeichert.</p>}
      {search.hits.length > 0 && (
        <ul className="space-y-1" aria-label="Discord-Treffer">
          {search.hits.map((hit) => (
            <li key={hit.discordId}>
              <button
                className="flex min-h-11 w-full min-w-0 items-center justify-between gap-3 rounded-lg bg-white/[0.05] px-3 py-2 text-left text-sm hover:bg-white/[0.1]"
                disabled={busy}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  onChange({ name: hit.displayName.slice(0, 80), discordId: hit.discordId });
                  search.clear();
                }}
              >
                <span className="truncate text-white">{hit.displayName}</span>
                {hit.username && <span className="truncate text-xs text-white/40">@{hit.username}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function PlayersEditor({ team, busy, update }: { team: CasterSceneTeam | null; busy: boolean; update: (change: (value: CasterSceneTeam) => CasterSceneTeam) => void }) {
  const players = team?.players ?? [];
  return (
    <div className="space-y-2">
      <strong className="block text-sm text-white/80">Spieler</strong>
      {players.map((player, playerIndex) => (
        <div key={playerIndex} className="flex min-w-0 items-center gap-2">
          <input
            className={`${input} min-w-0 flex-1`}
            aria-label={`Spieler ${playerIndex + 1}`}
            value={player.displayName}
            maxLength={80}
            disabled={busy}
            onChange={(event) =>
              update((value) => ({
                ...value,
                players: value.players.map((item, index) => (index === playerIndex ? { ...item, displayName: event.target.value } : item)),
              }))
            }
          />
          <span className="hidden w-28 shrink-0 font-mono text-xs text-white/35 sm:block">
            {player.accountId ? `AID ${player.accountId}` : 'Steam nicht verknüpft'}
          </span>
          <button
            className={`${button} shrink-0`}
            disabled={busy}
            aria-label={`${player.displayName || `Spieler ${playerIndex + 1}`} entfernen`}
            onClick={() => update((value) => ({ ...value, players: value.players.filter((_, index) => index !== playerIndex) }))}
          >
            Entfernen
          </button>
        </div>
      ))}
      <button
        className={button}
        disabled={busy || players.length >= MAX_PLAYERS}
        onClick={() => update((value) => ({ ...value, players: [...value.players, { displayName: '', steamId64: null, accountId: null }] }))}
      >
        Spieler hinzufügen
      </button>
      {players.length >= MAX_PLAYERS && <p className="text-xs text-white/45">Höchstens zwölf Spieler pro Team.</p>}
    </div>
  );
}

function fromContext(team: CasterContextTeam): CasterSceneTeam {
  return {
    id: team.id,
    libraryId: null,
    name: team.name,
    short: null,
    logoAssetId: null,
    coach: null,
    players: team.players
      .filter((player) => !player.isBench)
      .map((player) => ({ displayName: player.displayName, steamId64: player.steamId64 ?? null, accountId: player.accountId ?? null })),
  };
}

function teamLabel(team: CasterContextTeam) {
  const active = team.players.filter((player) => !player.isBench).length;
  const bench = team.players.filter((player) => player.isBench).length;
  return `${team.name} · ${active} aktiv${bench ? ` · ${bench} Bench` : ''}`;
}

type TeamCardProps = {
  side: Side;
  index: number;
  scene: CasterScene;
  teams: CasterContextTeam[];
  edit: Edit;
  busy: boolean;
  logos: ReturnType<typeof useAssets>;
  library: ReturnType<typeof useTeamLibrary>;
  notify: Notify;
};

function TeamCard({ side, index, scene, teams, edit, busy, logos, library, notify }: TeamCardProps) {
  const team = scene.matchContext[side] ?? null;
  const update = (change: (value: CasterSceneTeam) => CasterSceneTeam | null) =>
    edit((value) => ({
      ...value,
      matchContext: { ...value.matchContext, [side]: change(value.matchContext[side] ?? emptyTeam()) },
    }));
  const selectedLogo = logos.assets.find((asset) => asset.id === team?.logoAssetId) ?? null;
  const fillFrom = (teamId: string) => {
    const selected = teams.find((item) => item.id === teamId);
    if (selected) update(() => fromContext(selected));
    else if (team) update((value) => ({ ...value, id: null }));
  };
  const pickLibrary = (libraryId: string) => {
    const selected = library.teams.find((item) => item.libraryId === libraryId);
    if (selected) update(() => ({ ...selected, id: null }));
    else if (team) update((value) => ({ ...value, libraryId: null }));
  };
  const missingContext = team?.id && !teams.some((item) => item.id === team.id) ? team : null;
  const missingLibrary = team?.libraryId && !library.teams.some((item) => item.libraryId === team.libraryId) ? team : null;
  const libraryBusy = busy || library.saving;

  return (
    <div className="space-y-3 rounded-xl border border-white/10 bg-black/25 p-4">
      <div className="flex items-center gap-3">
        <LogoPreview
          url={publicUrl(selectedLogo?.url ?? (team?.logoAssetId ? assetUrl(team.logoAssetId) : null))}
          alt={`Logo ${team?.name || `Team ${index + 1}`}`}
        />
        <strong className="text-white">{index === 0 ? 'Team links' : 'Team rechts'} (vor dem Tausch)</strong>
      </div>
      <label className="block text-sm text-white/65">
        Aus Teambibliothek
        <select className={`${input} mt-1`} value={team?.libraryId ?? ''} disabled={libraryBusy} onChange={(event) => pickLibrary(event.target.value)}>
          <option value="">Nicht aus der Bibliothek</option>
          {missingLibrary && <option value={missingLibrary.libraryId ?? ''}>{missingLibrary.name || 'Team'} (gelöscht)</option>}
          {library.teams.map((item) => <option key={item.libraryId} value={item.libraryId ?? ''}>{item.name}{item.short ? ` · ${item.short}` : ''}</option>)}
        </select>
      </label>
      <label className="block text-sm text-white/65">
        Aus Turnierverwaltung übernehmen
        <select className={`${input} mt-1`} value={team?.id ?? ''} disabled={busy} onChange={(event) => fillFrom(event.target.value)}>
          <option value="">Freies Team (nicht verknüpft)</option>
          {missingContext && <option value={missingContext.id ?? ''}>{missingContext.name || 'Team'} (nicht mehr in der Turnierverwaltung)</option>}
          {teams.map((item) => <option key={item.id} value={item.id}>{teamLabel(item)}</option>)}
        </select>
      </label>
      <div className="grid gap-3 sm:grid-cols-[1fr_8rem]">
        <label className="text-sm text-white/65">
          Teamname
          <input
            className={`${input} mt-1`}
            value={team?.name ?? ''}
            maxLength={80}
            disabled={busy}
            onChange={(event) => update((value) => ({ ...value, name: event.target.value }))}
          />
        </label>
        <label className="text-sm text-white/65">
          Kürzel
          <input
            className={`${input} mt-1 uppercase`}
            value={team?.short ?? ''}
            maxLength={8}
            disabled={busy}
            onChange={(event) =>
              update((value) => ({
                ...value,
                short: event.target.value.trim() ? event.target.value.toUpperCase() : null,
              }))
            }
          />
        </label>
      </div>
      <label className="block text-sm text-white/65">
        Teamlogo
        <select
          className={`${input} mt-1`}
          value={team?.logoAssetId ?? ''}
          disabled={busy}
          onChange={(event) => update((value) => ({ ...value, logoAssetId: event.target.value || null }))}
        >
          <option value="">Kein Logo</option>
          {logos.assets.map((asset) => <option key={asset.id} value={asset.id}>{asset.name}</option>)}
        </select>
      </label>
      <div className="flex flex-wrap gap-2">
        <UploadButton
          label="Logo hochladen"
          disabled={busy}
          onFile={async (file) => {
            const asset = await logos.upload(file);
            if (asset) update((value) => ({ ...value, logoAssetId: asset.id }));
          }}
        />
        {selectedLogo && (
          <button
            className={button}
            disabled={busy}
            onClick={async () => {
              if (await logos.remove(selectedLogo)) update((value) => ({ ...value, logoAssetId: null }));
            }}
          >
            Logo löschen
          </button>
        )}
        {team && (
          <button className={button} disabled={busy} onClick={() => update(() => null)}>Team leeren</button>
        )}
      </div>
      <CoachField coach={team?.coach ?? null} busy={busy} onChange={(coach) => update((value) => ({ ...value, coach }))} />
      <PlayersEditor team={team} busy={busy} update={update} />
      <div className="flex flex-wrap gap-2 border-t border-white/10 pt-3">
        <button
          className={button}
          disabled={libraryBusy || !team?.name.trim()}
          onClick={async () => {
            if (!team) return;
            const saved = await library.save(team);
            if (saved) {
              update((value) => ({ ...value, libraryId: saved.libraryId, logoAssetId: saved.logoAssetId }));
              notify.status(`${saved.name} in der Teambibliothek gespeichert.`);
            }
          }}
        >
          {team?.libraryId && !missingLibrary ? 'Bibliothek aktualisieren' : 'In Teambibliothek speichern'}
        </button>
        {team?.libraryId && !missingLibrary && (
          <button
            className={button}
            disabled={libraryBusy}
            onClick={async () => {
              const stored = library.teams.find((item) => item.libraryId === team.libraryId);
              if (stored && (await library.remove(stored))) update((value) => ({ ...value, libraryId: null }));
            }}
          >
            Aus Bibliothek löschen
          </button>
        )}
      </div>
    </div>
  );
}

type TeamsProps = { scene: CasterScene; teams: CasterContextTeam[]; edit: Edit; busy: boolean; notify: Notify };

export function CasterTeamsEditor({ scene, teams, edit, busy, notify }: TeamsProps) {
  const logos = useAssets('team_logo', notify);
  const library = useTeamLibrary(notify);
  const left = scene.sidesSwapped ? scene.matchContext.teamB : scene.matchContext.teamA;
  const right = scene.sidesSwapped ? scene.matchContext.teamA : scene.matchContext.teamB;
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-white/10 bg-black/30 p-3">
        <p className="text-sm text-white/70">
          Im Stream links: <strong className="text-white">{left?.name || '–'}</strong> · rechts: <strong className="text-white">{right?.name || '–'}</strong>
        </p>
        <button
          className={button}
          disabled={busy}
          onClick={() => edit((value) => ({ ...value, sidesSwapped: !value.sidesSwapped }))}
        >
          Seiten tauschen
        </button>
      </div>
      <div className="grid gap-4 lg:grid-cols-2">
        {(['teamA', 'teamB'] as const).map((side, index) => (
          <TeamCard key={side} side={side} index={index} scene={scene} teams={teams} edit={edit} busy={busy} logos={logos} library={library} notify={notify} />
        ))}
      </div>
    </div>
  );
}

function logoValue(logo: CasterEventLogo) {
  if (logo.kind === 'builtin') return `builtin:${logo.id}`;
  if (logo.kind === 'asset') return `asset:${logo.assetId}`;
  return 'none';
}

function parseLogoValue(value: string): CasterEventLogo {
  const builtin = builtinEventLogos.find((item) => `builtin:${item.id}` === value);
  if (builtin) return { kind: 'builtin', id: builtin.id };
  if (value.startsWith('asset:')) return { kind: 'asset', assetId: value.slice(6) };
  return { kind: 'none' };
}

export function CasterEventEditor({ scene, edit, busy, notify }: Omit<TeamsProps, 'teams'>) {
  const eventLogos = useAssets('event_logo', notify);
  const setLogo = (logo: CasterEventLogo) => edit((value) => ({ ...value, event: { ...value.event, logo } }));
  const selectedAsset = scene.event.logo.kind === 'asset'
    ? eventLogos.assets.find((asset) => scene.event.logo.kind === 'asset' && asset.id === scene.event.logo.assetId) ?? null
    : null;
  const previewUrl = publicUrl(eventLogoUrl(scene.event.logo));

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <div className="space-y-3 rounded-xl border border-white/10 bg-black/25 p-4">
        <strong className="block text-white">Event-Logo & Marke</strong>
        <div className="flex h-24 items-center justify-center rounded-lg border border-white/10 bg-black/50">
          {previewUrl ? <img src={previewUrl} alt="Event-Logo" className="max-h-20 max-w-full object-contain" /> : <span className="text-sm text-white/40">Kein Event-Logo</span>}
        </div>
        <label className="block text-sm text-white/65">
          Event-Logo
          <select
            className={`${input} mt-1`}
            value={logoValue(scene.event.logo)}
            disabled={busy}
            onChange={(event) => setLogo(parseLogoValue(event.target.value))}
          >
            {builtinEventLogos.map((item) => <option key={item.id} value={`builtin:${item.id}`}>{item.label}</option>)}
            {eventLogos.assets.map((asset) => <option key={asset.id} value={`asset:${asset.id}`}>Eigenes Bild: {asset.name}</option>)}
            <option value="none">Keins</option>
          </select>
        </label>
        <div className="flex flex-wrap gap-2">
          <UploadButton
            label="Eigenes Bild hochladen"
            disabled={busy}
            onFile={async (file) => {
              const asset = await eventLogos.upload(file);
              if (asset) setLogo({ kind: 'asset', assetId: asset.id });
            }}
          />
          {selectedAsset && (
            <button
              className={button}
              disabled={busy}
              onClick={async () => {
                if (await eventLogos.remove(selectedAsset)) setLogo({ kind: 'builtin', id: 'wochenende' });
              }}
            >
              Bild löschen
            </button>
          )}
        </div>
        <label className="flex min-h-11 items-center gap-3 text-sm text-white/80">
          <input
            type="checkbox"
            className="h-5 w-5 accent-primary"
            checked={scene.event.showBrand}
            disabled={busy}
            onChange={(event) => edit((value) => ({ ...value, event: { ...value.event, showBrand: event.target.checked } }))}
          />
          DACHLOCK-Logo anzeigen
        </label>
      </div>
      <div className="space-y-3 rounded-xl border border-white/10 bg-black/25 p-4">
        <strong className="block text-white">Countdown</strong>
        <label className="block text-sm text-white/65">
          Titel
          <input
            className={`${input} mt-1`}
            value={scene.countdown.title}
            maxLength={60}
            disabled={busy}
            onChange={(event) => edit((value) => ({ ...value, countdown: { ...value.countdown, title: event.target.value } }))}
          />
        </label>
        <label className="block text-sm text-white/65">
          Startzeit
          <input
            type="datetime-local"
            className={`${input} mt-1`}
            value={toLocalInput(scene.countdown.target)}
            disabled={busy}
            onChange={(event) => edit((value) => ({ ...value, countdown: { ...value.countdown, target: fromLocalInput(event.target.value) } }))}
          />
        </label>
        <div className="flex flex-wrap gap-2">
          {[5, 10, 15].map((minutes) => (
            <button
              key={minutes}
              className={button}
              disabled={busy}
              onClick={() => edit((value) => ({
                ...value,
                countdown: { ...value.countdown, target: new Date(Date.now() + minutes * 60000).toISOString() },
              }))}
            >
              in {minutes} Min.
            </button>
          ))}
        </div>
        <p className="text-xs text-white/45">Ohne Startzeit zeigt der Countdown 00:00. Die Zeit läuft erst nach „Live übernehmen“ im Stream.</p>
      </div>
    </div>
  );
}
