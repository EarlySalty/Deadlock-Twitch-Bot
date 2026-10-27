import { useEffect, useState } from 'react';
import {
  deleteCasterAsset,
  fetchCasterAssets,
  uploadCasterAsset,
  type CasterAsset,
  type CasterAssetKind,
  type CasterChat,
  type CasterContextTeam,
  type CasterEventLogo,
  type CasterScene,
  type CasterSceneTeam,
} from '@/api/client';
import { builtinEventLogos, eventLogoUrl, fromLocalInput, publicUrl, toLocalInput } from './casterScene';

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

function emptyTeam(): CasterSceneTeam {
  return { id: null, name: '', short: null, logoAssetId: null, players: [] };
}

function fromContext(team: CasterContextTeam): CasterSceneTeam {
  return {
    id: team.id,
    name: team.name,
    short: null,
    logoAssetId: null,
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
};

function TeamCard({ side, index, scene, teams, edit, busy, logos }: TeamCardProps) {
  const team = scene.matchContext[side] ?? null;
  const update = (change: (value: CasterSceneTeam) => CasterSceneTeam | null) =>
    edit((value) => ({
      ...value,
      matchContext: { ...value.matchContext, [side]: change(value.matchContext[side] ?? emptyTeam()) },
    }));
  const selectedLogo = logos.assets.find((asset) => asset.id === team?.logoAssetId) ?? null;
  const fillFrom = (teamId: string) => {
    const selected = teams.find((item) => item.id === teamId);
    update(() => (selected ? fromContext(selected) : null));
  };

  return (
    <div className="space-y-3 rounded-xl border border-white/10 bg-black/25 p-4">
      <div className="flex items-center gap-3">
        <LogoPreview url={publicUrl(selectedLogo?.url ?? null)} alt={`Logo ${team?.name || `Team ${index + 1}`}`} />
        <strong className="text-white">{index === 0 ? 'Team links' : 'Team rechts'} (vor dem Tausch)</strong>
      </div>
      <label className="block text-sm text-white/65">
        Aus Turnierverwaltung übernehmen
        <select className={`${input} mt-1`} value={team?.id ?? ''} disabled={busy} onChange={(event) => fillFrom(event.target.value)}>
          <option value="">Freies Team / kein Team</option>
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
            onChange={(event) => update((value) => ({ ...value, short: event.target.value.toUpperCase() }))}
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
      {team && team.players.length > 0 && (
        <div className="space-y-1 text-sm">
          {team.players.map((player) => (
            <div key={`${player.displayName}-${player.accountId ?? ''}`} className="flex items-center justify-between gap-3 rounded-lg bg-white/[0.03] px-3 py-2">
              <span className="text-white/85">{player.displayName}</span>
              <span className="font-mono text-xs text-white/35">{player.accountId ? `AID ${player.accountId}` : 'Steam nicht verknüpft'}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

type TeamsProps = { scene: CasterScene; teams: CasterContextTeam[]; edit: Edit; busy: boolean; notify: Notify };

export function CasterTeamsEditor({ scene, teams, edit, busy, notify }: TeamsProps) {
  const logos = useAssets('team_logo', notify);
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
          <TeamCard key={side} side={side} index={index} scene={scene} teams={teams} edit={edit} busy={busy} logos={logos} />
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

export function CasterChatEditor({ scene, edit, busy }: { scene: CasterScene; edit: Edit; busy: boolean }) {
  const update = (change: Partial<CasterChat>) => edit((value) => ({ ...value, chat: { ...value.chat, ...change } }));
  return (
    <div className="grid gap-4 rounded-xl border border-white/10 bg-black/25 p-4 sm:grid-cols-[auto_1fr_auto] sm:items-end">
      <label className="flex min-h-11 items-center gap-3 text-sm text-white/80">
        <input
          type="checkbox"
          className="h-5 w-5 accent-primary"
          checked={scene.chat.enabled}
          disabled={busy}
          onChange={(event) => update({ enabled: event.target.checked })}
        />
        Chat in der Szene anzeigen
      </label>
      <label className="block text-sm text-white/65">
        Twitch-Kanal
        <input
          className={`${input} mt-1`}
          value={scene.chat.channel}
          maxLength={25}
          disabled={busy}
          onChange={(event) => update({ channel: event.target.value })}
        />
      </label>
      <div className="flex gap-2" role="group" aria-label="Position des Chats">
        {(['left', 'right'] as const).map((position) => (
          <button
            key={position}
            className={`${button} ${scene.chat.position === position ? 'border-primary text-white' : 'text-white/70'}`}
            aria-pressed={scene.chat.position === position}
            disabled={busy}
            onClick={() => update({ position })}
          >
            {position === 'left' ? 'Links' : 'Rechts'}
          </button>
        ))}
      </div>
    </div>
  );
}
