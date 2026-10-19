import { useEffect, useMemo, useRef, useState } from 'react';
import { useBlocker } from 'react-router';
import { PageHeader } from '@/components/layout/PageHeader';
import {
  createCasterRoleCamera,
  fetchCasterCameras,
  fetchCasterRoleMembers,
  fetchCasterOverlay,
  fetchCasterOverlayContext,
  revokeCasterCamera,
  saveCasterOverlay,
  type CasterCamera,
  type CasterContextTeam,
  type CasterDocument,
  type CasterLayout,
  type CasterRoleMember,
  type CasterScene,
} from '@/api/client';
import { CasterEventEditor, CasterTeamsEditor } from './CasterSceneSettings';
import {
  normalizedCountdown,
  normalizedEvent,
  overlayPages,
  previewPayload,
  PUBLIC_ORIGIN,
  type OverlayPageId,
} from './casterScene';

const button =
  'min-h-11 rounded-lg border border-white/20 px-3 py-2 text-sm hover:border-primary focus-visible:outline-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:opacity-40';
const input =
  'min-h-11 w-full rounded-lg border border-white/15 bg-black/50 px-3 text-white focus-visible:outline-2 focus-visible:outline-primary';

function initials(name: string) {
  const parts = name.replace(/[^\p{L}\p{N} ]/gu, ' ').trim().split(/\s+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : (parts[0] ?? '?').slice(0, 2)).toUpperCase();
}

function cameraStatus(member: CasterRoleMember) {
  const camera = member.camera;
  if (!camera) return { text: 'Noch kein Kamera-Link', tone: 'text-white/45' };
  if (camera.revoked) return { text: 'Link widerrufen, von dir oder vom Caster', tone: 'text-danger' };
  if (camera.online) return { text: 'Online · Live abrufbar', tone: 'text-success' };
  if (camera.consentEnabled) return { text: 'Freigegeben · aktuell offline', tone: 'text-primary-hover' };
  return { text: 'Link erstellt · noch nicht geöffnet', tone: 'text-white/60' };
}

function otherCameraTitle(camera: CasterCamera, roleListReliable: boolean) {
  if (!camera.ownerLogin.startsWith('discord_')) return `${camera.label} · @${camera.ownerLogin}`;
  return roleListReliable
    ? `Ehemalige Caster-Kamera · ${camera.label}`
    : `Caster-Kamera · ${camera.label} · Rolle gerade nicht prüfbar`;
}

function activeCameraId(member: CasterRoleMember | undefined) {
  return member?.camera && !member.camera.revoked ? member.camera.cameraId : null;
}
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
    event: normalizedEvent(scene.event),
    countdown: normalizedCountdown(scene.countdown),
    sidesSwapped: scene.sidesSwapped ?? false,
    roster: (scene.roster ?? []).map((person) => ({
      ...person,
      cameraUrl: person.cameraUrl ?? '',
      cameraId: person.cameraId ?? null,
      accountLogin: person.accountLogin ?? null,
      discordId: person.discordId ?? null,
      steamAccountId: person.steamAccountId ?? null,
      teamId: person.teamId ?? null,
    })),
  };
}

export default function CasterOverlayPage() {
  const [document, setDocument] = useState<CasterDocument | null>(null);
  const [saved, setSaved] = useState('');
  const [casters, setCasters] = useState<CasterRoleMember[]>([]);
  const [castersReason, setCastersReason] = useState('');
  const [castersIncomplete, setCastersIncomplete] = useState(false);
  const [invites, setInvites] = useState<Record<string, string>>({});
  const [cameras, setCameras] = useState<CasterCamera[]>([]);
  const [teams, setTeams] = useState<CasterContextTeam[]>([]);
  const [contextReason, setContextReason] = useState('');
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [busy, setBusy] = useState(false);
  const [dragged, setDragged] = useState<string | null>(null);
  const [newDiscordId, setNewDiscordId] = useState('');
  const [newName, setNewName] = useState('');
  const [newHandle, setNewHandle] = useState('');
  const [previewPage, setPreviewPage] = useState<OverlayPageId>('caster');
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
    iframe.current?.contentWindow?.postMessage(previewPayload(nextScene), PUBLIC_ORIGIN);
  }

  useEffect(() => preview(), [scene]);

  async function load() {
    setBusy(true);
    setError('');
    const [overlayResult, casterResult, cameraResult, contextResult] = await Promise.allSettled([
      fetchCasterOverlay(),
      fetchCasterRoleMembers(),
      fetchCasterCameras(),
      fetchCasterOverlayContext(),
    ]);
    try {
      if (overlayResult.status === 'rejected') throw overlayResult.reason;
      const normalized = { ...overlayResult.value, scene: normalizedScene(overlayResult.value.scene) };
      setDocument(normalized);
      setSaved(JSON.stringify(normalized.scene));

      if (casterResult.status === 'fulfilled') {
        setCasters(casterResult.value.items ?? []);
        setCastersReason(casterResult.value.available ? '' : casterResult.value.reason || 'nicht verfügbar');
        setCastersIncomplete(casterResult.value.available && casterResult.value.complete === false);
      } else {
        setCastersReason('Die Rolle Turnier Caster konnte nicht geladen werden.');
        setCastersIncomplete(false);
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

  function linkCaster(personId: string, discordId: string) {
    const member = casters.find((item) => item.discordId === discordId);
    edit((value) => ({
      ...value,
      roster: value.roster.map((item) =>
        item.id === personId
          ? member
            ? { ...item, discordId, name: member.displayName.slice(0, 60), cameraId: activeCameraId(member) }
            : { ...item, discordId: null }
          : item,
      ),
    }));
  }

  function chooseNewCaster(discordId: string) {
    setNewDiscordId(discordId);
    const member = casters.find((item) => item.discordId === discordId);
    if (member) setNewName(member.displayName.slice(0, 60));
  }

  function addPerson() {
    const name = newName.trim();
    if (!name) return;
    const member = casters.find((item) => item.discordId === newDiscordId);
    edit((value) => ({
      ...value,
      roster: [
        ...value.roster,
        {
          id: crypto.randomUUID(),
          name,
          handle: newHandle.trim().replace(/^@+/, ''),
          accountLogin: null,
          discordId: member ? member.discordId : null,
          cameraUrl: '',
          cameraId: activeCameraId(member),
          steamAccountId: null,
          teamId: null,
        },
      ],
    }));
    setNewDiscordId('');
    setNewName('');
    setNewHandle('');
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

  async function createRoleCamera(member: CasterRoleMember) {
    if (member.camera && !member.camera.revoked) {
      const ok = window.confirm(`Neuen Link für ${member.displayName} erstellen? Der bisherige Link funktioniert danach nicht mehr.`);
      if (!ok) return;
    }
    setBusy(true);
    setError('');
    try {
      const invite = await createCasterRoleCamera(member.discordId);
      setInvites((items) => ({ ...items, [member.discordId]: invite.inviteUrl }));
      setCasters((items) =>
        items.map((item) =>
          item.discordId === member.discordId
            ? { ...item, camera: { cameraId: invite.cameraId, consentEnabled: false, revoked: false, online: false, lastConnectedAt: item.camera?.lastConnectedAt ?? null } }
            : item,
        ),
      );
      if (scene?.roster.some((person) => person.discordId === member.discordId && person.cameraId !== invite.cameraId)) {
        edit((value) => ({
          ...value,
          roster: value.roster.map((person) =>
            person.discordId === member.discordId ? { ...person, cameraId: invite.cameraId } : person,
          ),
        }));
      }
      setStatus(`Kamera-Link für ${member.displayName} erstellt. Er wird nur jetzt angezeigt und gilt, bis du oder ${member.displayName} ihn widerruft.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Kamera-Link konnte nicht erstellt werden.');
    } finally {
      setBusy(false);
    }
  }

  async function copyInvite(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      setStatus('Kamera-Einladungslink kopiert.');
    } catch {
      setError('Link konnte nicht automatisch kopiert werden.');
    }
  }

  async function revokeCamera(camera: Pick<CasterCamera, 'cameraId' | 'label'>) {
    if (!window.confirm(`Kamera-Link ${camera.label} wirklich widerrufen?`)) return;
    setBusy(true);
    try {
      await revokeCasterCamera(camera.cameraId);
      setCameras((items) =>
        items.map((item) =>
          item.cameraId === camera.cameraId ? { ...item, consentEnabled: false, online: false, revoked: true } : item,
        ),
      );
      setCasters((items) =>
        items.map((item) =>
          item.camera?.cameraId === camera.cameraId
            ? { ...item, camera: { ...item.camera, revoked: true, consentEnabled: false, online: false } }
            : item,
        ),
      );
      setInvites((items) => Object.fromEntries(Object.entries(items).filter(([discordId]) =>
        casters.find((member) => member.discordId === discordId)?.camera?.cameraId !== camera.cameraId)));
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

  const notify = { status: setStatus, error: setError };
  const listedCasterCameraIds = new Set(casters.map((member) => member.camera?.cameraId).filter(Boolean));
  const legacyCameras = cameras.filter((camera) => !camera.ownerLogin.startsWith('discord_'));
  const otherCameras = cameras.filter((camera) => !camera.revoked && !listedCasterCameraIds.has(camera.cameraId));
  const rosterDiscordIds = new Set(scene?.roster.map((person) => person.discordId).filter(Boolean));
  const previewUrl = overlayPages.find((page) => page.id === previewPage)?.url ?? overlayPages[0].url;
  const observerCandidates = [scene?.matchContext.teamA, scene?.matchContext.teamB]
    .flatMap((team) => team?.players ?? [])
    .filter((player) => typeof player.accountId === 'number');

  return (
    <div className="space-y-6 pb-8">
      {blocker.state === 'blocked' && (
        <div role="alert" className="rounded-xl border border-primary/40 bg-bg p-4 text-white">
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
        <div role="alert" className="rounded-xl border border-danger/50 bg-danger/10 p-4 text-danger">
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
                <p className="mt-1 text-sm text-white/60">Die OBS-Adresse der Caster-Szene bleibt für alle Layouts identisch.</p>
              </div>
              <span className={`rounded-full px-3 py-1 text-sm ${dirty ? 'bg-primary/15 text-primary-hover' : 'bg-success/15 text-success'}`}>
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
                    className={`rounded-xl border p-4 text-left transition ${active ? 'border-primary bg-primary/10' : 'border-white/15 bg-black/30 hover:border-white/30'}`}
                    onClick={() => setLayout(layout)}
                    disabled={busy}
                  >
                    <strong className="block text-white">{meta.label}</strong>
                    <span className="mt-1 block text-sm text-white/55">{meta.description}</span>
                  </button>
                );
              })}
            </div>

            {overlayPages.length > 1 && (
              <div className="flex flex-wrap gap-2" role="tablist" aria-label="Vorschau-Seite">
                {overlayPages.map((page) => (
                  <button
                    key={page.id}
                    role="tab"
                    aria-selected={previewPage === page.id}
                    className={`${button} ${previewPage === page.id ? 'border-primary bg-primary/10 text-primary-hover' : ''}`}
                    onClick={() => setPreviewPage(page.id)}
                  >
                    {page.label}
                  </button>
                ))}
              </div>
            )}

            <div className="relative aspect-video overflow-hidden rounded-xl border border-white/15 bg-black">
              <iframe
                ref={iframe}
                title="Vorschau der Overlay-Seite"
                src={`${previewUrl}?preview=1`}
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
                  className={`rounded-xl border p-3 ${dragged ? 'border-primary/60 bg-primary/5' : 'border-white/10 bg-black/25'}`}
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
                Teams frei benennen, in der Bibliothek speichern oder aus der Turnierverwaltung übernehmen. Der Coach erscheint im Scoreboard, Versus-Bild und in der Caster-Szene. Bench-Spieler werden nicht als aktives Line-up übernommen.
              </p>
            </div>
            {contextReason && (
              <div className="rounded-lg border border-primary/30 bg-primary/5 p-3 text-sm text-primary-hover">
                Automatische Teamdaten gerade nicht verfügbar: {contextReason}
              </div>
            )}
            <CasterTeamsEditor scene={scene} teams={teams} edit={edit} busy={busy} notify={notify} />
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
              <h2 className="text-lg font-semibold text-white">Event, Marke & Countdown</h2>
              <p className="mt-1 text-sm text-white/60">Gilt für alle Overlay-Seiten.</p>
            </div>
            <CasterEventEditor scene={scene} edit={edit} busy={busy} notify={notify} />
          </section>

          <section className="panel-card space-y-4 rounded-2xl p-5">
            <div>
              <h2 className="text-lg font-semibold text-white">Caster & Kamera-Zuordnung</h2>
              <p className="mt-1 text-sm text-white/60">
                Zur Auswahl stehen alle Mitglieder der Discord-Rolle Turnier Caster. Ihre feste Kamera wird automatisch verknüpft, Name und Handle bleiben überschreibbar.
              </p>
            </div>
            {castersReason && (
              <div className="rounded-lg border border-primary/30 bg-primary/5 p-3 text-sm text-primary-hover">
                Caster-Liste gerade nicht verfügbar: {castersReason}
              </div>
            )}
            {castersIncomplete && (
              <div className="rounded-lg border border-primary/30 bg-primary/5 p-3 text-sm text-primary-hover">
                Die Caster-Liste ist eventuell unvollständig. Discord hat gerade nicht alle Mitglieder der Rolle geliefert.
              </div>
            )}
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
                  className="cursor-grab rounded-lg border border-primary/30 bg-black/40 px-3 py-2 text-sm text-primary-hover"
                >
                  ⠿ {person.name}
                </span>
              ))}
            </div>

            <div className="space-y-3">
              {scene.roster.map((person) => {
                const member = casters.find((item) => item.discordId === person.discordId);
                const ownCamera = activeCameraId(member);
                const legacyOptions = legacyCameras.filter((camera) => !person.accountLogin || camera.ownerLogin === person.accountLogin);
                return (
                  <div key={person.id} className="rounded-xl border border-white/10 bg-black/30 p-4">
                    <div className="grid gap-3 lg:grid-cols-2 xl:grid-cols-[1.3fr_1fr_1fr_1.5fr_auto]">
                      <label className="text-xs text-white/55">
                        Caster aus Discord
                        <select
                          className={`${input} mt-1`}
                          value={person.discordId ?? ''}
                          onChange={(event) => linkCaster(person.id, event.target.value)}
                          disabled={busy}
                        >
                          <option value="">Manuell</option>
                          {person.discordId && !member && (
                            <option value={person.discordId}>Nicht mehr in der Caster-Rolle</option>
                          )}
                          {casters
                            .filter((item) => item.discordId === person.discordId || !rosterDiscordIds.has(item.discordId))
                            .map((item) => (
                              <option key={item.discordId} value={item.discordId}>{item.displayName}</option>
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
                          {ownCamera && member && (
                            <option value={ownCamera}>Feste Kamera von {member.displayName} · {cameraStatus(member).text}</option>
                          )}
                          {person.cameraId && person.cameraId !== ownCamera && !legacyOptions.some((camera) => camera.cameraId === person.cameraId) && (
                            <option value={person.cameraId}>Bisherige Kamera</option>
                          )}
                          {legacyOptions.map((camera) => (
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
                Caster aus Discord
                <select className={`${input} mt-1`} value={newDiscordId} onChange={(event) => chooseNewCaster(event.target.value)}>
                  <option value="">Manuell</option>
                  {casters.filter((item) => !rosterDiscordIds.has(item.discordId)).map((item) => (
                    <option key={item.discordId} value={item.discordId}>{item.displayName}</option>
                  ))}
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
                Jede Person mit der Rolle Turnier Caster hat genau eine feste Kamera. Der Link gilt dauerhaft, bis du ihn widerrufst oder neu erstellst. Der Caster kann ihn auch selbst auf der Kamera-Seite widerrufen. Wir speichern nur Kamera-ID, Zuordnung und Freigabestatus, keine Videodatei.
              </p>
            </div>

            <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
              {casters.map((member) => {
                const state = cameraStatus(member);
                const invite = invites[member.discordId];
                const active = !!member.camera && !member.camera.revoked;
                return (
                  <div key={member.discordId} className="min-w-0 space-y-3 rounded-xl border border-white/10 bg-black/25 p-4">
                    <div className="flex flex-wrap items-center gap-3">
                      {member.avatarUrl ? (
                        <img src={member.avatarUrl} alt="" className="h-11 w-11 rounded-full object-cover" />
                      ) : (
                        <span aria-hidden="true" className="grid h-11 w-11 place-items-center rounded-full bg-primary/20 text-sm font-semibold text-primary-hover">
                          {initials(member.displayName)}
                        </span>
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="truncate font-semibold text-white">{member.displayName}</div>
                        <div className={`mt-1 text-sm ${state.tone}`}>{state.text}</div>
                      </div>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <button className={button} disabled={busy} onClick={() => void createRoleCamera(member)}>
                        {active ? 'Link neu erstellen' : 'Link erstellen'}
                      </button>
                      {active && member.camera && (
                        <button
                          className={button}
                          disabled={busy}
                          onClick={() => member.camera && void revokeCamera({ cameraId: member.camera.cameraId, label: member.displayName })}
                        >
                          Widerrufen
                        </button>
                      )}
                    </div>
                    {invite && (
                      <div className="min-w-0 rounded-lg border border-primary/35 bg-primary/5 p-3">
                        <p className="text-xs text-white/55">Nur jetzt sichtbar. Schick den Link direkt an {member.displayName}.</p>
                        <div className="mt-2 flex flex-col gap-2 md:flex-row">
                          <input className={input} readOnly aria-label={`Kamera-Link für ${member.displayName}`} value={invite} onFocus={(event) => event.target.select()} />
                          <button className={button} onClick={() => void copyInvite(invite)}>Kopieren</button>
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
              {casters.length === 0 && !castersReason && <p className="text-sm text-white/50">Niemand hat gerade die Rolle Turnier Caster.</p>}
            </div>

            {otherCameras.length > 0 && (
              <details className="rounded-xl border border-white/10 bg-black/20 p-4" open={otherCameras.some((camera) => camera.ownerLogin.startsWith('discord_'))}>
                <summary className="cursor-pointer text-sm text-white/70">Weitere aktive Kamera-Links ({otherCameras.length})</summary>
                <div className="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
                  {otherCameras.map((camera) => (
                    <div key={camera.cameraId} className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-white/10 bg-black/25 p-4">
                      <div className="min-w-0">
                        <div className="font-semibold text-white">
                          {otherCameraTitle(camera, !castersReason && !castersIncomplete)}
                        </div>
                        <div className={`mt-2 text-sm ${camera.online ? 'text-success' : camera.consentEnabled ? 'text-primary-hover' : 'text-white/45'}`}>
                          {camera.online ? 'Online · Live abrufbar' : camera.consentEnabled ? 'Freigegeben · aktuell offline' : 'Nicht freigegeben'}
                        </div>
                      </div>
                      <button className={button} disabled={busy} onClick={() => void revokeCamera(camera)}>Widerrufen</button>
                    </div>
                  ))}
                </div>
              </details>
            )}
          </section>

          <div className="sticky bottom-3 z-20 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-primary/40 bg-bg p-4 shadow-xl">
            <p role="status" className="text-sm text-white/75">{status || 'Bereit.'}</p>
            <button
              className={`${button} border-primary bg-primary/15 font-semibold text-primary-hover`}
              disabled={busy || !dirty || scene.roster.some((person) => !person.name.trim())}
              onClick={() => void publish()}
            >
              {busy ? 'Wird gespeichert …' : 'Live übernehmen'}
            </button>
          </div>

          <section className="panel-card space-y-3 rounded-2xl p-5">
            <h2 className="text-lg font-semibold text-white">OBS</h2>
            <p className="text-sm text-white/65">
              Jede Seite ist eine eigene Browser-Quelle mit 1920 × 1080. Alle Seiten lesen dieselbe Szene.
            </p>
            {overlayPages.map((page) => (
              <label key={page.id} className="block text-sm text-white/65">
                {page.label}
                <div className="mt-1 flex flex-col gap-2 md:flex-row">
                  <input className={input} readOnly value={page.url} onFocus={(event) => event.target.select()} />
                  <button
                    className={button}
                    onClick={async () => {
                      try {
                        await navigator.clipboard.writeText(page.url);
                        setStatus(`OBS-Adresse ${page.label} kopiert.`);
                      } catch {
                        setError('OBS-Adresse konnte nicht automatisch kopiert werden.');
                      }
                    }}
                  >
                    Kopieren
                  </button>
                </div>
              </label>
            ))}
            <p className="text-xs text-white/45">
              Unsere Kameras übertragen per WebRTC. Externe HTTPS-Browserquellen bleiben als Ersatz möglich. Bei strengen NAT-Einstellungen verbessert später ein eigenes TURN-Relay die Verbindung.
            </p>
          </section>
        </>
      )}
    </div>
  );
}
