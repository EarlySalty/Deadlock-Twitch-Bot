import { useEffect, useMemo, useRef, useState } from 'react';
import { useBlocker } from 'react-router';
import { PageHeader } from '@/components/layout/PageHeader';
import {
  createCasterCamera,
  fetchAdminStreamers,
  fetchCasterCameras,
  fetchCasterOverlay,
  fetchCasterOverlayContext,
  revokeCasterCamera,
  saveCasterOverlay,
  type CasterCamera,
  type CasterContextTeam,
  type CasterDocument,
  type CasterLayout,
  type CasterPerson,
  type CasterScene,
  type CasterSceneTeam,
  type CreatedCasterCamera,
} from '@/api/client';

const OBS_URL = 'https://deutsche-deadlock-community.de/twitch/caster-overlay';
const OBS_ORIGIN = new URL(OBS_URL).origin;
const button =
  'min-h-11 rounded-lg border border-white/20 px-3 py-2 text-sm hover:border-amber-400 focus-visible:outline-2 focus-visible:outline-amber-400 disabled:cursor-not-allowed disabled:opacity-40';
const input =
  'min-h-11 w-full rounded-lg border border-white/15 bg-black/50 px-3 text-white focus-visible:outline-2 focus-visible:outline-amber-400';

type AccountOption = { login: string; displayName: string };
const layoutMeta: Record<CasterLayout, { label: string; slots: number; description: string }> = {
  solo: { label: 'Solo-Cam', slots: 1, description: 'Eine große Kamera' },
  duo: { label: 'Duo-Cam', slots: 2, description: 'Zwei gleich große Kameras' },
  trio: { label: 'Trio-Cam', slots: 3, description: 'Drei Kameras nebeneinander' },
};

function normalizedScene(scene: CasterScene): CasterScene {
  const layout = scene.layout || 'duo';
  const slotCount = layoutMeta[layout]?.slots ?? 2;
  const slots = [...(scene.slots ?? [])];
  while (slots.length < slotCount) slots.push(null);
  slots.length = slotCount;
  return {
    ...scene,
    layout,
    slots,
    matchContext: scene.matchContext ?? {},
    roster: (scene.roster ?? []).map((person) => ({
      ...person,
      cameraUrl: person.cameraUrl ?? '',
      cameraId: person.cameraId ?? null,
      accountLogin: person.accountLogin ?? null,
      steamAccountId: person.steamAccountId ?? null,
      teamId: person.teamId ?? null,
    })),
  };
}

function toSceneTeam(team: CasterContextTeam | undefined): CasterSceneTeam | null {
  if (!team) return null;
  const activePlayers = team.players.filter((player) => !player.isBench);
  return {
    id: team.id,
    name: team.name,
    players: activePlayers.map((player) => ({
      displayName: player.displayName,
      steamId64: player.steamId64 ?? null,
      accountId: player.accountId ?? null,
    })),
  };
}

function teamLabel(team: CasterContextTeam) {
  const active = team.players.filter((player) => !player.isBench).length;
  const bench = team.players.filter((player) => player.isBench).length;
  return `${team.name} · ${active} aktiv${bench ? ` · ${bench} Bench` : ''}`;
}

export default function CasterOverlayPage() {
  const [document, setDocument] = useState<CasterDocument | null>(null);
  const [saved, setSaved] = useState('');
  const [accounts, setAccounts] = useState<AccountOption[]>([]);
  const [cameras, setCameras] = useState<CasterCamera[]>([]);
  const [teams, setTeams] = useState<CasterContextTeam[]>([]);
  const [contextReason, setContextReason] = useState('');
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [busy, setBusy] = useState(false);
  const [dragged, setDragged] = useState<string | null>(null);
  const [newAccountLogin, setNewAccountLogin] = useState('');
  const [newName, setNewName] = useState('');
  const [newHandle, setNewHandle] = useState('');
  const [cameraOwner, setCameraOwner] = useState('');
  const [cameraLabel, setCameraLabel] = useState('Caster-Cam');
  const [createdCamera, setCreatedCamera] = useState<CreatedCasterCamera | null>(null);
  const iframe = useRef<HTMLIFrameElement>(null);

  const scene = document?.scene;
  const dirty = !!scene && JSON.stringify(scene) !== saved;
  const blocker = useBlocker(dirty);
  const slotCount = scene ? layoutMeta[scene.layout]?.slots ?? 2 : 2;
  const slots = useMemo(
    () => scene?.slots.map((id) => scene.roster.find((person) => person.id === id) ?? null) ?? [],
    [scene],
  );

  function preview(nextScene = scene) {
    if (!nextScene) return;
    iframe.current?.contentWindow?.postMessage(
      {
        type: 'ddc-caster-preview',
        layout: nextScene.layout,
        slots: nextScene.slots.map((id) => nextScene.roster.find((person) => person.id === id) ?? null),
        matchContext: nextScene.matchContext,
      },
      OBS_ORIGIN,
    );
  }

  useEffect(() => preview(), [scene]);

  async function load() {
    setBusy(true);
    setError('');
    const [overlayResult, accountResult, cameraResult, contextResult] = await Promise.allSettled([
      fetchCasterOverlay(),
      fetchAdminStreamers('all'),
      fetchCasterCameras(),
      fetchCasterOverlayContext(),
    ]);
    try {
      if (overlayResult.status === 'rejected') throw overlayResult.reason;
      const normalized = { ...overlayResult.value, scene: normalizedScene(overlayResult.value.scene) };
      setDocument(normalized);
      setSaved(JSON.stringify(normalized.scene));

      if (accountResult.status === 'fulfilled') {
        const options = accountResult.value
          .filter((row) => row.login)
          .map((row) => ({ login: row.login.toLowerCase(), displayName: row.displayName || row.login }))
          .sort((a, b) => a.displayName.localeCompare(b.displayName, 'de'));
        setAccounts(options);
        if (!cameraOwner && options[0]) setCameraOwner(options[0].login);
      }
      if (cameraResult.status === 'fulfilled') setCameras(cameraResult.value);
      if (contextResult.status === 'fulfilled') {
        setTeams(contextResult.value.teams ?? []);
        setContextReason(contextResult.value.available ? '' : contextResult.value.reason || 'nicht verfügbar');
      } else {
        setContextReason('Turnier-/Steam-Kontext konnte nicht geladen werden.');
      }
      setStatus('Caster-Szene geladen.');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Caster-Szene konnte nicht geladen werden.');
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void load();
  }, []);

  useEffect(() => {
    if (!dirty) return;
    const guard = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload', guard);
    return () => window.removeEventListener('beforeunload', guard);
  }, [dirty]);

  function edit(change: (value: CasterScene) => CasterScene) {
    setDocument((current) => {
      if (!current) return current;
      const next = normalizedScene(change(current.scene));
      return { ...current, scene: next };
    });
    setStatus('Entwurf geändert – noch nicht live.');
  }

  function setLayout(layout: CasterLayout) {
    edit((value) => ({ ...value, layout }));
  }

  function assign(slot: number, id: string | null) {
    edit((value) => {
      const slots = [...value.slots];
      slots[slot] = id;
      return { ...value, slots };
    });
  }

  function accountDefaults(login: string) {
    const account = accounts.find((item) => item.login === login);
    return account ? { name: account.displayName || account.login, handle: account.login } : null;
  }

  function linkAccount(person: CasterPerson, login: string) {
    const defaults = accountDefaults(login);
    edit((value) => ({
      ...value,
      roster: value.roster.map((item) =>
        item.id === person.id
          ? { ...item, accountLogin: login || null, ...(defaults ? defaults : {}) }
          : item,
      ),
    }));
  }

  function chooseNewAccount(login: string) {
    setNewAccountLogin(login);
    const defaults = accountDefaults(login);
    if (defaults) {
      setNewName(defaults.name);
      setNewHandle(defaults.handle);
      setCameraOwner(login);
    }
  }

  function addPerson() {
    const name = newName.trim();
    if (!name) return;
    edit((value) => ({
      ...value,
      roster: [
        ...value.roster,
        {
          id: crypto.randomUUID(),
          name,
          handle: newHandle.trim().replace(/^@+/, ''),
          accountLogin: newAccountLogin || null,
          cameraUrl: '',
          cameraId: null,
          steamAccountId: null,
          teamId: null,
        },
      ],
    }));
    setNewAccountLogin('');
    setNewName('');
    setNewHandle('');
  }

  function setTeam(side: 'teamA' | 'teamB', teamId: string) {
    const selected = teams.find((team) => team.id === teamId);
    edit((value) => ({
      ...value,
      matchContext: { ...value.matchContext, [side]: toSceneTeam(selected) },
    }));
  }

  async function publish() {
    if (!document) return;
    setBusy(true);
    setError('');
    try {
      const result = await saveCasterOverlay(document);
      const normalized = { ...result, scene: normalizedScene(result.scene) };
      setDocument(normalized);
      setSaved(JSON.stringify(normalized.scene));
      setStatus('Live übernommen. OBS aktualisiert sich innerhalb von zwei Sekunden.');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Speichern fehlgeschlagen. Der Entwurf bleibt erhalten.');
    } finally {
      setBusy(false);
    }
  }

  async function createCamera() {
    if (!cameraOwner || !cameraLabel.trim()) return;
    setBusy(true);
    setError('');
    try {
      const camera = await createCasterCamera(cameraOwner, cameraLabel.trim());
      setCreatedCamera(camera);
      setCameras((items) => [...items, camera].sort((a, b) => a.ownerLogin.localeCompare(b.ownerLogin)));
      setStatus('Kamera-Link erstellt. Das Publish-Geheimnis wird nur jetzt in diesem Link angezeigt.');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Kamera-Link konnte nicht erstellt werden.');
    } finally {
      setBusy(false);
    }
  }

  async function revokeCamera(camera: CasterCamera) {
    if (!window.confirm(`${camera.label} für @${camera.ownerLogin} wirklich widerrufen?`)) return;
    setBusy(true);
    try {
      await revokeCasterCamera(camera.cameraId);
      setCameras((items) =>
        items.map((item) =>
          item.cameraId === camera.cameraId ? { ...item, consentEnabled: false, online: false } : item,
        ),
      );
      edit((value) => ({
        ...value,
        roster: value.roster.map((person) =>
          person.cameraId === camera.cameraId ? { ...person, cameraId: null } : person,
        ),
      }));
      setStatus('Kamera-Freigabe widerrufen. Laufende Verbindungen wurden beendet.');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Kamera konnte nicht widerrufen werden.');
    } finally {
      setBusy(false);
    }
  }

  const teamAId = scene?.matchContext.teamA?.id ?? '';
  const teamBId = scene?.matchContext.teamB?.id ?? '';
  const observerCandidates = [scene?.matchContext.teamA, scene?.matchContext.teamB]
    .flatMap((team) => team?.players ?? [])
    .filter((player) => typeof player.accountId === 'number');

  return (
    <div className="space-y-6 pb-8">
      {blocker.state === 'blocked' && (
        <div role="alert" className="rounded-xl border border-amber-400/40 bg-zinc-950 p-4 text-white">
          Deine Änderungen sind noch nicht live. Seite wirklich verlassen?
          <div className="mt-3 flex gap-3">
            <button className={button} onClick={() => blocker.reset()}>Weiter bearbeiten</button>
            <button className={button} onClick={() => blocker.proceed()}>Entwurf verwerfen</button>
          </div>
        </div>
      )}

      <PageHeader
        title="Caster-Overlay"
        description="Solo-, Duo- oder Trio-Cams, gespeicherte Kamera-Freigaben, Caster-Konten und Teamdaten aus Turniere/Steam in einer OBS-Szene."
      />

      {error && (
        <div role="alert" className="rounded-xl border border-red-400/50 bg-red-950/40 p-4 text-red-100">
          {error}
          <button
            className={`${button} ml-3`}
            disabled={busy}
            onClick={() => {
              if (!dirty || window.confirm('Ungespeicherte Änderungen verwerfen?')) void load();
            }}
          >
            Neu laden
          </button>
        </div>
      )}

      {!scene ? (
        <p role="status">{busy ? 'Caster-Szene wird geladen …' : 'Caster-Szene ist nicht verfügbar.'}</p>
      ) : (
        <>
          <section className="panel-card space-y-4 rounded-2xl p-5">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div>
                <h2 className="text-lg font-semibold text-white">Layout & Live-Vorschau</h2>
                <p className="mt-1 text-sm text-white/60">Die OBS-Adresse bleibt für alle Layouts identisch.</p>
              </div>
              <span className={`rounded-full px-3 py-1 text-sm ${dirty ? 'bg-amber-400/15 text-amber-200' : 'bg-emerald-400/15 text-emerald-200'}`}>
                {dirty ? 'Noch nicht live' : 'Gespeichert'}
              </span>
            </div>

            <div className="grid gap-3 sm:grid-cols-3">
              {(Object.keys(layoutMeta) as CasterLayout[]).map((layout) => {
                const meta = layoutMeta[layout];
                const active = scene.layout === layout;
                return (
                  <button
                    key={layout}
                    className={`rounded-xl border p-4 text-left transition ${active ? 'border-amber-400 bg-amber-400/10' : 'border-white/15 bg-black/30 hover:border-white/30'}`}
                    onClick={() => setLayout(layout)}
                    disabled={busy}
                  >
                    <strong className="block text-white">{meta.label}</strong>
                    <span className="mt-1 block text-sm text-white/55">{meta.description}</span>
                  </button>
                );
              })}
            </div>

            <div className="relative aspect-video overflow-hidden rounded-xl border border-white/15 bg-black">
              <iframe
                ref={iframe}
                title="Vorschau der Caster-Szene"
                src={`${OBS_URL}?preview=1`}
                onLoad={() => preview()}
                className="absolute inset-0 h-full w-full border-0"
              />
            </div>

            <div className={`grid gap-3 ${slotCount === 1 ? 'grid-cols-1' : slotCount === 2 ? 'md:grid-cols-2' : 'md:grid-cols-3'}`}>
              {Array.from({ length: slotCount }, (_, index) => (
                <div
                  key={index}
                  onDragOver={(event) => {
                    event.preventDefault();
                    event.dataTransfer.dropEffect = 'copy';
                  }}
                  onDrop={(event) => {
                    event.preventDefault();
                    const id = event.dataTransfer.getData('text/plain');
                    if (scene.roster.some((person) => person.id === id)) assign(index, id);
                    setDragged(null);
                  }}
                  className={`rounded-xl border p-3 ${dragged ? 'border-amber-400/60 bg-amber-400/5' : 'border-white/10 bg-black/25'}`}
                >
                  <label className="text-sm text-white/65" htmlFor={`caster-slot-${index}`}>Kamera {index + 1}</label>
                  <select
                    id={`caster-slot-${index}`}
                    className={`${input} mt-1`}
                    value={scene.slots[index] ?? ''}
                    disabled={busy}
                    onChange={(event) => assign(index, event.target.value || null)}
                  >
                    <option value="">Platz leer</option>
                    {scene.roster.map((person) => (
                      <option key={person.id} value={person.id}>
                        {person.name}{person.cameraId ? ' · Community-Kamera' : person.cameraUrl ? ' · URL Cam' : ' · ohne Cam'}
                      </option>
                    ))}
                  </select>
                  <p className="mt-2 text-xs text-white/45">
                    {slots[index]?.cameraId ? 'Eigene Kamera über WebRTC' : slots[index]?.cameraUrl ? 'Externe Browserquelle' : 'Kein Kamerabild – Fenster bleibt frei'}
                  </p>
                </div>
              ))}
            </div>
          </section>

          <section className="panel-card space-y-4 rounded-2xl p-5">
            <div>
              <h2 className="text-lg font-semibold text-white">Teams & Player</h2>
              <p className="mt-1 text-sm text-white/60">
                Teamnamen und aktive Spieler kommen aus der Turnierverwaltung; Steam-IDs werden über die vorhandenen verknüpften Steam-Konten ergänzt. Bench-Spieler werden im Admin angezeigt, aber nicht als aktives Line-up ins Overlay übernommen.
              </p>
            </div>
            {contextReason && (
              <div className="rounded-lg border border-amber-400/30 bg-amber-400/5 p-3 text-sm text-amber-100">
                Automatische Teamdaten gerade nicht verfügbar: {contextReason}
              </div>
            )}
            <div className="grid gap-4 lg:grid-cols-2">
              {(['teamA', 'teamB'] as const).map((side, index) => {
                const selectedId = side === 'teamA' ? teamAId : teamBId;
                const selected = teams.find((team) => team.id === selectedId);
                return (
                  <div key={side} className="rounded-xl border border-white/10 bg-black/25 p-4">
                    <label className="text-sm text-white/65">Team {index + 1}</label>
                    <select className={`${input} mt-1`} value={selectedId} onChange={(event) => setTeam(side, event.target.value)}>
                      <option value="">Kein Team</option>
                      {teams.map((team) => <option key={team.id} value={team.id}>{teamLabel(team)}</option>)}
                    </select>
                    {selected && (
                      <div className="mt-3 space-y-1 text-sm">
                        {selected.players.map((player) => (
                          <div key={`${selected.id}-${player.discordId || player.displayName}`} className="flex items-center justify-between gap-3 rounded-lg bg-white/[0.03] px-3 py-2">
                            <span className={player.isBench ? 'text-white/45' : 'text-white/85'}>{player.displayName}{player.isBench ? ' · Bench' : ''}</span>
                            <span className="font-mono text-xs text-white/35">{player.accountId ? `AID ${player.accountId}` : 'Steam nicht verknüpft'}</span>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
            <label className="block max-w-xl text-sm text-white/65">
              Observer-POV / Steam Account-ID
              <select
                className={`${input} mt-1`}
                value={scene.matchContext.observerAccountId ?? ''}
                onChange={(event) =>
                  edit((value) => ({
                    ...value,
                    matchContext: {
                      ...value.matchContext,
                      observerAccountId: event.target.value ? Number(event.target.value) : null,
                    },
                  }))
                }
              >
                <option value="">Kein POV markieren</option>
                {observerCandidates.map((player) => (
                  <option key={`${player.accountId}-${player.displayName}`} value={player.accountId ?? ''}>
                    {player.displayName} · {player.accountId}
                  </option>
                ))}
              </select>
              <span className="mt-1 block text-xs text-white/45">
                Das ist dieselbe Steam-account_id, die der Observer bereits für current/recommended POV verwendet. Damit gibt es keine zweite Namenszuordnung.
              </span>
            </label>
          </section>

          <section className="panel-card space-y-4 rounded-2xl p-5">
            <div>
              <h2 className="text-lg font-semibold text-white">Caster & Kamera-Zuordnung</h2>
              <p className="mt-1 text-sm text-white/60">
                Ein verknüpftes Twitch-Konto setzt Anzeigename und Handle als Default. Die Werte bleiben danach überschreibbar.
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              {scene.roster.map((person) => (
                <span
                  key={person.id}
                  draggable={!busy}
                  onDragStart={(event) => {
                    event.dataTransfer.setData('text/plain', person.id);
                    event.dataTransfer.effectAllowed = 'copy';
                    setDragged(person.id);
                  }}
                  onDragEnd={() => setDragged(null)}
                  className="cursor-grab rounded-lg border border-amber-400/30 bg-black/40 px-3 py-2 text-sm text-amber-100"
                >
                  ⠿ {person.name}
                </span>
              ))}
            </div>

            <div className="space-y-3">
              {scene.roster.map((person) => {
                const ownerCameras = cameras.filter((camera) => !person.accountLogin || camera.ownerLogin === person.accountLogin);
                return (
                  <div key={person.id} className="rounded-xl border border-white/10 bg-black/30 p-4">
                    <div className="grid gap-3 lg:grid-cols-2 xl:grid-cols-[1.3fr_1fr_1fr_1.5fr_auto]">
                      <label className="text-xs text-white/55">
                        Twitch-Konto
                        <select
                          className={`${input} mt-1`}
                          value={person.accountLogin ?? ''}
                          onChange={(event) => linkAccount(person, event.target.value)}
                          disabled={busy}
                        >
                          <option value="">Manuell</option>
                          {person.accountLogin && !accounts.some((account) => account.login === person.accountLogin) && (
                            <option value={person.accountLogin}>@{person.accountLogin}</option>
                          )}
                          {accounts.map((account) => (
                            <option key={account.login} value={account.login}>{account.displayName} (@{account.login})</option>
                          ))}
                        </select>
                      </label>
                      <label className="text-xs text-white/55">
                        Anzeigename
                        <input
                          className={`${input} mt-1`}
                          value={person.name}
                          maxLength={60}
                          onChange={(event) => edit((value) => ({
                            ...value,
                            roster: value.roster.map((item) => item.id === person.id ? { ...item, name: event.target.value } : item),
                          }))}
                        />
                      </label>
                      <label className="text-xs text-white/55">
                        Handle
                        <input
                          className={`${input} mt-1`}
                          value={person.handle}
                          maxLength={60}
                          onChange={(event) => edit((value) => ({
                            ...value,
                            roster: value.roster.map((item) => item.id === person.id ? { ...item, handle: event.target.value.replace(/^@+/, '') } : item),
                          }))}
                        />
                      </label>
                      <label className="text-xs text-white/55">
                        Community-Kamera
                        <select
                          className={`${input} mt-1`}
                          value={person.cameraId ?? ''}
                          onChange={(event) => edit((value) => ({
                            ...value,
                            roster: value.roster.map((item) => item.id === person.id ? { ...item, cameraId: event.target.value || null } : item),
                          }))}
                        >
                          <option value="">Keine eigene Cam</option>
                          {ownerCameras.map((camera) => (
                            <option key={camera.cameraId} value={camera.cameraId}>
                              {camera.label} · {camera.online ? 'online' : camera.consentEnabled ? 'freigegeben/offline' : 'noch nicht freigegeben'}
                            </option>
                          ))}
                        </select>
                      </label>
                      <button
                        className={button}
                        onClick={() => edit((value) => ({
                          ...value,
                          roster: value.roster.filter((item) => item.id !== person.id),
                          slots: value.slots.map((id) => id === person.id ? null : id),
                        }))}
                      >
                        Entfernen
                      </button>
                    </div>
                    <label className="mt-3 block text-xs text-white/55">
                      Externe Kamera-URL als Fallback (optional)
                      <input
                        className={`${input} mt-1`}
                        inputMode="url"
                        maxLength={2048}
                        value={person.cameraUrl ?? ''}
                        placeholder="https://vdo.ninja/?view=…"
                        onChange={(event) => edit((value) => ({
                          ...value,
                          roster: value.roster.map((item) => item.id === person.id ? { ...item, cameraUrl: event.target.value } : item),
                        }))}
                      />
                    </label>
                  </div>
                );
              })}
            </div>

            <form
              className="grid gap-3 border-t border-white/10 pt-4 md:grid-cols-2 xl:grid-cols-[1.4fr_1fr_1fr_auto]"
              onSubmit={(event) => { event.preventDefault(); addPerson(); }}
            >
              <label className="text-sm text-white/65">
                Twitch-Konto
                <select className={`${input} mt-1`} value={newAccountLogin} onChange={(event) => chooseNewAccount(event.target.value)}>
                  <option value="">Manuell</option>
                  {accounts.map((account) => <option key={account.login} value={account.login}>{account.displayName} (@{account.login})</option>)}
                </select>
              </label>
              <label className="text-sm text-white/65">
                Name
                <input className={`${input} mt-1`} value={newName} maxLength={60} required onChange={(event) => setNewName(event.target.value)} />
              </label>
              <label className="text-sm text-white/65">
                Handle
                <input className={`${input} mt-1`} value={newHandle} maxLength={60} onChange={(event) => setNewHandle(event.target.value)} />
              </label>
              <button className={`${button} self-end`} disabled={busy || !newName.trim() || scene.roster.length >= 100}>Caster hinzufügen</button>
            </form>
          </section>

          <section className="panel-card space-y-4 rounded-2xl p-5">
            <div>
              <h2 className="text-lg font-semibold text-white">Community-Kamera-Portal</h2>
              <p className="mt-1 text-sm text-white/60">
                Wir speichern Kamera-ID, Zuordnung und Freigabestatus – keine Videodatei. Die eingeladene Person öffnet den Link, gibt die Kamera ausdrücklich frei und lässt die Seite während der Live-Übertragung geöffnet.
              </p>
            </div>
            <div className="grid gap-3 md:grid-cols-[1fr_1fr_auto]">
              <label className="text-sm text-white/65">
                Konto
                <select className={`${input} mt-1`} value={cameraOwner} onChange={(event) => setCameraOwner(event.target.value)}>
                  <option value="">Konto wählen</option>
                  {accounts.map((account) => <option key={account.login} value={account.login}>{account.displayName} (@{account.login})</option>)}
                </select>
              </label>
              <label className="text-sm text-white/65">
                Kamera-Name
                <input className={`${input} mt-1`} value={cameraLabel} maxLength={80} onChange={(event) => setCameraLabel(event.target.value)} />
              </label>
              <button className={`${button} self-end`} disabled={busy || !cameraOwner || !cameraLabel.trim()} onClick={() => void createCamera()}>
                Einladungs-Link erstellen
              </button>
            </div>

            {createdCamera && (
              <div className="rounded-xl border border-amber-400/35 bg-amber-400/5 p-4">
                <strong className="text-amber-100">Link jetzt an @{createdCamera.ownerLogin} schicken</strong>
                <p className="mt-1 text-xs text-white/50">Das Publish-Geheimnis steckt nur im Fragment dieses Links und wird nicht erneut aus der Datenbank gelesen.</p>
                <div className="mt-3 flex flex-col gap-2 md:flex-row">
                  <input className={input} readOnly value={createdCamera.inviteUrl} onFocus={(event) => event.target.select()} />
                  <button
                    className={button}
                    onClick={async () => {
                      try {
                        await navigator.clipboard.writeText(createdCamera.inviteUrl);
                        setStatus('Kamera-Einladungslink kopiert.');
                      } catch {
                        setError('Link konnte nicht automatisch kopiert werden.');
                      }
                    }}
                  >
                    Kopieren
                  </button>
                </div>
              </div>
            )}

            <div className="grid gap-3 lg:grid-cols-2">
              {cameras.map((camera) => (
                <div key={camera.cameraId} className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-white/10 bg-black/25 p-4">
                  <div>
                    <div className="font-semibold text-white">{camera.label} · @{camera.ownerLogin}</div>
                    <div className="mt-1 text-xs text-white/45">{camera.cameraId}</div>
                    <div className={`mt-2 text-sm ${camera.online ? 'text-emerald-300' : camera.consentEnabled ? 'text-amber-200' : 'text-white/45'}`}>
                      {camera.online ? 'Online · Live abrufbar' : camera.consentEnabled ? 'Freigegeben · aktuell offline' : 'Noch nicht freigegeben'}
                    </div>
                  </div>
                  <button className={button} disabled={busy} onClick={() => void revokeCamera(camera)}>Widerrufen</button>
                </div>
              ))}
              {cameras.length === 0 && <p className="text-sm text-white/50">Noch keine Community-Kamera angelegt.</p>}
            </div>
          </section>

          <div className="sticky bottom-3 z-20 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-400/40 bg-zinc-950 p-4 shadow-xl">
            <p role="status" className="text-sm text-white/75">{status || 'Bereit.'}</p>
            <button
              className={`${button} border-amber-400 bg-amber-400/15 font-semibold text-amber-100`}
              disabled={busy || !dirty || scene.roster.some((person) => !person.name.trim())}
              onClick={() => void publish()}
            >
              {busy ? 'Wird gespeichert …' : 'Live übernehmen'}
            </button>
          </div>

          <section className="panel-card space-y-3 rounded-2xl p-5">
            <h2 className="text-lg font-semibold text-white">OBS</h2>
            <p className="text-sm text-white/65">
              Eine Browser-Quelle mit 1920 × 1080 reicht. Layout, Kameras, Namensleisten und Teamkopf kommen gemeinsam aus derselben URL.
            </p>
            <div className="flex flex-col gap-2 md:flex-row">
              <input className={input} readOnly value={OBS_URL} onFocus={(event) => event.target.select()} />
              <button
                className={button}
                onClick={async () => {
                  try {
                    await navigator.clipboard.writeText(OBS_URL);
                    setStatus('OBS-Adresse kopiert.');
                  } catch {
                    setError('OBS-Adresse konnte nicht automatisch kopiert werden.');
                  }
                }}
              >
                Kopieren
              </button>
            </div>
            <p className="text-xs text-white/45">
              Unsere Kameras übertragen per WebRTC. Externe HTTPS-Browserquellen bleiben als Ersatz möglich. Bei strengen NAT-Einstellungen verbessert später ein eigener TURN-Relay die Verbindung.
            </p>
          </section>
        </>
      )}
    </div>
  );
}
