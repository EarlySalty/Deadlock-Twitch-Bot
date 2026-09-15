import { useEffect, useRef, useState } from 'react';
import { useBlocker } from 'react-router';
import { PageHeader } from '@/components/layout/PageHeader';
import {
  fetchAdminStreamers,
  fetchCasterOverlay,
  saveCasterOverlay,
  type CasterDocument,
  type CasterPerson,
  type CasterScene,
} from '@/api/client';

const OBS_URL = 'https://deutsche-deadlock-community.de/twitch/caster-overlay';
const OBS_ORIGIN = new URL(OBS_URL).origin;
const button =
  'min-h-11 rounded-lg border border-white/20 px-3 py-2 text-sm hover:border-amber-400 focus-visible:outline-2 focus-visible:outline-amber-400 disabled:opacity-40';
const input =
  'min-h-11 w-full rounded-lg border border-white/15 bg-black/50 px-3 text-white focus-visible:outline-2 focus-visible:outline-amber-400';

type AccountOption = { login: string; displayName: string };

export default function CasterOverlayPage() {
  const [document, setDocument] = useState<CasterDocument | null>(null);
  const [accounts, setAccounts] = useState<AccountOption[]>([]);
  const [saved, setSaved] = useState('');
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [busy, setBusy] = useState(false);
  const [newAccountLogin, setNewAccountLogin] = useState('');
  const [name, setName] = useState('');
  const [handle, setHandle] = useState('');
  const [cameraUrl, setCameraUrl] = useState('');
  const [dragged, setDragged] = useState<string | null>(null);
  const iframe = useRef<HTMLIFrameElement>(null);
  const scene = document?.scene;
  const dirty = !!scene && JSON.stringify(scene) !== saved;
  const blocker = useBlocker(dirty);
  const slots =
    scene?.slots.map((id) => scene.roster.find((person) => person.id === id) ?? null) ?? [null, null];

  function preview() {
    iframe.current?.contentWindow?.postMessage(
      {
        type: 'ddc-caster-preview',
        slots: slots.map(
          (person) =>
            person && {
              name: person.name,
              handle: person.handle,
              cameraUrl: person.cameraUrl ?? '',
            },
        ),
      },
      OBS_ORIGIN,
    );
  }

  useEffect(preview, [scene]);

  async function load() {
    setBusy(true);
    setError('');
    const [casterResult, accountResult] = await Promise.allSettled([
      fetchCasterOverlay(),
      fetchAdminStreamers('all'),
    ]);
    try {
      if (casterResult.status === 'rejected') {
        throw casterResult.reason;
      }
      setDocument(casterResult.value);
      setSaved(JSON.stringify(casterResult.value.scene));
      if (accountResult.status === 'fulfilled') {
        const nextAccounts = accountResult.value
          .filter((row) => row.login)
          .map((row) => ({ login: row.login.toLowerCase(), displayName: row.displayName || row.login }))
          .sort((a, b) => a.displayName.localeCompare(b.displayName, 'de'));
        setAccounts(nextAccounts);
        setStatus('Gespeicherte Szene und Konten geladen.');
      } else {
        setStatus('Szene geladen. Konten konnten gerade nicht geladen werden; manuelle Namen funktionieren weiter.');
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Szene konnte nicht geladen werden.');
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
    setDocument((current) => current && { ...current, scene: change(current.scene) });
    setStatus('Entwurf geändert – noch nicht in OBS.');
  }

  function assign(index: number, id: string | null) {
    edit((value) => ({
      ...value,
      slots: value.slots.map((current, slot) => (slot === index ? id : current)) as CasterScene['slots'],
    }));
  }

  function accountDefaults(login: string): { name: string; handle: string } | null {
    const account = accounts.find((entry) => entry.login === login);
    return account ? { name: account.displayName || account.login, handle: account.login } : null;
  }

  function linkAccount(person: CasterPerson, login: string) {
    const defaults = accountDefaults(login);
    edit((value) => ({
      ...value,
      roster: value.roster.map((item) =>
        item.id === person.id
          ? {
              ...item,
              accountLogin: login || null,
              ...(defaults ? defaults : {}),
            }
          : item,
      ),
    }));
  }

  function chooseNewAccount(login: string) {
    setNewAccountLogin(login);
    const defaults = accountDefaults(login);
    if (defaults) {
      setName(defaults.name);
      setHandle(defaults.handle);
    }
  }

  async function publish() {
    if (!document) return;
    setBusy(true);
    setError('');
    try {
      const result = await saveCasterOverlay(document);
      setDocument(result);
      setSaved(JSON.stringify(result.scene));
      setStatus('Live übernommen. OBS aktualisiert sich innerhalb von zwei Sekunden.');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Speichern fehlgeschlagen. Deine Änderungen bleiben im Entwurf.');
    } finally {
      setBusy(false);
    }
  }

  function addPerson() {
    const trimmed = name.trim();
    if (!trimmed) return;
    edit((value) => ({
      ...value,
      roster: [
        ...value.roster,
        {
          id: crypto.randomUUID(),
          name: trimmed,
          handle: handle.trim().replace(/^@+/, ''),
          accountLogin: newAccountLogin || null,
          cameraUrl: cameraUrl.trim(),
        },
      ],
    }));
    setNewAccountLogin('');
    setName('');
    setHandle('');
    setCameraUrl('');
  }

  return (
    <div className="space-y-6 pb-8">
      {blocker.state === 'blocked' && (
        <div role="alert" className="rounded-xl border border-amber-400/40 bg-zinc-950 p-4 text-white">
          Deine Änderungen sind noch nicht live. Seite wirklich verlassen?
          <div className="mt-3 flex gap-3">
            <button className={button} onClick={() => blocker.reset()}>
              Weiter bearbeiten
            </button>
            <button className={button} onClick={() => blocker.proceed()}>
              Entwurf verwerfen
            </button>
          </div>
        </div>
      )}

      <PageHeader
        title="Caster-Overlay"
        description="Caster-Konten und Kameras verknüpfen, auf die beiden Plätze ziehen und direkt als OBS-Browserquelle ausspielen."
      />

      {error && (
        <div role="alert" className="rounded-xl border border-red-400/50 bg-red-950/40 p-4 text-red-100">
          {error}
          <button
            className={`${button} ml-3`}
            disabled={busy}
            onClick={() => {
              if (!dirty || window.confirm('Ungespeicherte Änderungen verwerfen und die aktuelle Szene laden?')) void load();
            }}
          >
            Neu laden
          </button>
        </div>
      )}

      {!scene ? (
        <p role="status">{busy ? 'Caster-Szene wird geladen …' : 'Die Szene ist noch nicht verfügbar.'}</p>
      ) : (
        <>
          <section className="panel-card space-y-4 rounded-2xl p-5">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <h2 className="text-lg font-semibold text-white">DACH-Caster · zwei Kameras</h2>
                <p className="text-sm text-white/60">
                  1920 × 1080 · {dirty ? 'Vorschau deines Entwurfs' : 'Gespeicherte Szene'}
                </p>
              </div>
              <span
                className={`rounded-full px-3 py-1 text-sm ${
                  dirty ? 'bg-amber-400/15 text-amber-200' : 'bg-emerald-400/15 text-emerald-200'
                }`}
              >
                {dirty ? 'Noch nicht live' : 'Gespeichert'}
              </span>
            </div>

            <div className="flex max-h-28 flex-wrap gap-2 overflow-y-auto" aria-label="Caster zum Ziehen">
              {scene.roster.map((person) => (
                <span
                  key={person.id}
                  data-caster-drag={person.id}
                  draggable={!busy}
                  onDragStart={(event) => {
                    event.dataTransfer.setData('text/plain', person.id);
                    event.dataTransfer.effectAllowed = 'copy';
                    setDragged(person.id);
                  }}
                  onDragEnd={() => setDragged(null)}
                  className="cursor-grab rounded-lg border border-amber-400/30 bg-black/40 px-3 py-2 text-sm text-amber-100"
                  title={`${person.name} auf einen Kameraplatz ziehen`}
                >
                  ⠿ {person.name}{person.cameraUrl ? ' · Cam' : ''}
                </span>
              ))}
            </div>

            <div
              className="relative aspect-video overflow-hidden rounded-xl border border-white/15"
              style={{ background: 'repeating-conic-gradient(#232326 0% 25%, #171719 0% 50%) 0 / 24px 24px' }}
            >
              <iframe
                ref={iframe}
                title="Vorschau der Caster-Szene"
                src={`${OBS_URL}?preview=1`}
                onLoad={preview}
                className="pointer-events-none absolute inset-0 h-full w-full border-0"
              />
              {[0, 1].map((index) => (
                <button
                  key={index}
                  aria-label={`${index === 0 ? 'Linker' : 'Rechter'} Kameraplatz${
                    slots[index] ? `: ${slots[index]?.name}` : ': frei'
                  }`}
                  disabled={busy}
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
                  className={`absolute flex items-center justify-center border-2 border-dashed text-sm text-white/70 focus-visible:outline-2 focus-visible:outline-amber-400 ${
                    dragged ? 'border-amber-300 bg-amber-400/10' : 'border-white/20'
                  }`}
                  style={{
                    left: `${(index === 0 ? 185 : 1043) / 19.2}%`,
                    top: `${282 / 10.8}%`,
                    width: `${694 / 19.2}%`,
                    height: `${429 / 10.8}%`,
                  }}
                  onClick={() => globalThis.document.getElementById(`caster-slot-${index}`)?.focus()}
                >
                  {dragged ? 'Hier ablegen' : slots[index]?.cameraUrl ? 'Kamera aktiv' : 'Keine Kameraquelle'}
                </button>
              ))}
            </div>

            <div className="grid gap-3 sm:grid-cols-[1fr_auto_1fr]">
              {[0, 1].map((index) => (
                <div key={index} className={index ? 'sm:col-start-3 sm:row-start-1' : ''}>
                  <label className="mb-1 block text-sm text-white/70" htmlFor={`caster-slot-${index}`}>
                    {index === 0 ? 'Links' : 'Rechts'}
                  </label>
                  <select
                    id={`caster-slot-${index}`}
                    disabled={busy}
                    className={input}
                    value={scene.slots[index] ?? ''}
                    onChange={(event) => assign(index, event.target.value || null)}
                  >
                    <option value="">Platz leeren</option>
                    {scene.roster.map((person) => (
                      <option key={person.id} value={person.id}>
                        {person.name}{person.cameraUrl ? ' · Cam' : ''}
                      </option>
                    ))}
                  </select>
                </div>
              ))}
              <button
                disabled={busy}
                className={`${button} self-end sm:col-start-2 sm:row-start-1`}
                onClick={() => edit((value) => ({ ...value, slots: [value.slots[1], value.slots[0]] }))}
              >
                ⇄ Tauschen
              </button>
            </div>
          </section>

          <section className="panel-card space-y-4 rounded-2xl p-5">
            <div>
              <h2 className="text-lg font-semibold text-white">Caster-Liste</h2>
              <p className="mt-1 text-sm text-white/60">
                Twitch-Konto auswählen = Name und Handle werden direkt als Default gesetzt. Pro Caster kann zusätzlich eine HTTPS-Browserquelle für die Kamera hinterlegt werden, z. B. eine VDO.Ninja-View-URL.
              </p>
            </div>

            {scene.roster.length === 0 && (
              <p className="text-white/60">Wähle unten ein vorhandenes Konto oder lege einen Caster manuell an.</p>
            )}

            <div className="space-y-3">
              {scene.roster.map((person) => (
                <div key={person.id} className="rounded-xl border border-white/10 bg-black/30 p-3">
                  <div className="grid items-end gap-2 xl:grid-cols-[auto_1fr_1fr_1fr_auto]">
                    <span
                      draggable={!busy}
                      onDragStart={(event) => {
                        event.dataTransfer.setData('text/plain', person.id);
                        event.dataTransfer.effectAllowed = 'copy';
                        setDragged(person.id);
                      }}
                      onDragEnd={() => setDragged(null)}
                      className="cursor-grab px-2 py-3 text-xl text-amber-300"
                      title={`${person.name} auf einen Kameraplatz ziehen`}
                      aria-hidden="true"
                    >
                      ⠿
                    </span>
                    <label className="text-xs text-white/55">
                      Twitch-Konto
                      <select
                        aria-label={`Twitch-Konto von ${person.name}`}
                        disabled={busy}
                        className={`${input} mt-1`}
                        value={person.accountLogin ?? ''}
                        onChange={(event) => linkAccount(person, event.target.value)}
                      >
                        <option value="">Nicht verknüpft</option>
                        {person.accountLogin && !accounts.some((account) => account.login === person.accountLogin) && (
                          <option value={person.accountLogin}>{person.accountLogin}</option>
                        )}
                        {accounts.map((account) => (
                          <option key={account.login} value={account.login}>
                            {account.displayName} (@{account.login})
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="text-xs text-white/55">
                      Anzeigename
                      <input
                        aria-label={`Name von ${person.name}`}
                        disabled={busy}
                        maxLength={60}
                        className={`${input} mt-1`}
                        value={person.name}
                        onChange={(event) =>
                          edit((value) => ({
                            ...value,
                            roster: value.roster.map((item) =>
                              item.id === person.id ? { ...item, name: event.target.value } : item,
                            ),
                          }))
                        }
                      />
                    </label>
                    <label className="text-xs text-white/55">
                      Handle
                      <input
                        aria-label={`Handle von ${person.name}`}
                        disabled={busy}
                        maxLength={60}
                        placeholder="optional"
                        className={`${input} mt-1`}
                        value={person.handle}
                        onChange={(event) =>
                          edit((value) => ({
                            ...value,
                            roster: value.roster.map((item) =>
                              item.id === person.id
                                ? { ...item, handle: event.target.value.replace(/^@+/, '') }
                                : item,
                            ),
                          }))
                        }
                      />
                    </label>
                    <div className="flex flex-wrap gap-2">
                      <button className={button} disabled={busy} onClick={() => assign(0, person.id)}>
                        Links
                      </button>
                      <button className={button} disabled={busy} onClick={() => assign(1, person.id)}>
                        Rechts
                      </button>
                      <button
                        aria-label={`${person.name} entfernen`}
                        className={button}
                        disabled={busy}
                        onClick={() =>
                          edit((value) => ({
                            ...value,
                            roster: value.roster.filter((item) => item.id !== person.id),
                            slots: value.slots.map((id) => (id === person.id ? null : id)) as CasterScene['slots'],
                          }))
                        }
                      >
                        Entfernen
                      </button>
                    </div>
                  </div>
                  <label className="mt-3 block text-xs text-white/55">
                    Kamera-/Browserquellen-URL
                    <input
                      aria-label={`Kameraquelle von ${person.name}`}
                      disabled={busy}
                      maxLength={2048}
                      inputMode="url"
                      placeholder="https://vdo.ninja/?view=…"
                      className={`${input} mt-1`}
                      value={person.cameraUrl ?? ''}
                      onChange={(event) =>
                        edit((value) => ({
                          ...value,
                          roster: value.roster.map((item) =>
                            item.id === person.id ? { ...item, cameraUrl: event.target.value } : item,
                          ),
                        }))
                      }
                    />
                  </label>
                </div>
              ))}
            </div>

            <form
              className="grid gap-3 border-t border-white/10 pt-4 lg:grid-cols-2 xl:grid-cols-[1.2fr_1fr_1fr_2fr_auto]"
              onSubmit={(event) => {
                event.preventDefault();
                addPerson();
              }}
            >
              <label className="text-sm text-white/70">
                Twitch-Konto (optional)
                <select
                  className={`${input} mt-1`}
                  value={newAccountLogin}
                  onChange={(event) => chooseNewAccount(event.target.value)}
                  disabled={busy}
                >
                  <option value="">Manuell</option>
                  {accounts.map((account) => (
                    <option key={account.login} value={account.login}>
                      {account.displayName} (@{account.login})
                    </option>
                  ))}
                </select>
              </label>
              <label className="text-sm text-white/70">
                Name
                <input
                  className={`${input} mt-1`}
                  maxLength={60}
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  disabled={busy}
                  placeholder="z. B. Nimo"
                  required
                />
              </label>
              <label className="text-sm text-white/70">
                Handle
                <input
                  className={`${input} mt-1`}
                  maxLength={60}
                  value={handle}
                  onChange={(event) => setHandle(event.target.value)}
                  disabled={busy}
                  placeholder="z. B. zro_dl"
                />
              </label>
              <label className="text-sm text-white/70">
                Kamera (optional)
                <input
                  className={`${input} mt-1`}
                  maxLength={2048}
                  inputMode="url"
                  value={cameraUrl}
                  onChange={(event) => setCameraUrl(event.target.value)}
                  disabled={busy}
                  placeholder="https://vdo.ninja/?view=…"
                />
              </label>
              <button
                className={`${button} self-end`}
                disabled={busy || !name.trim() || scene.roster.length >= 100}
              >
                Person hinzufügen
              </button>
            </form>
          </section>

          <div className="sticky bottom-3 z-10 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-400/40 bg-zinc-950 p-4 shadow-xl">
            <p role="status" className="text-sm text-white/75">
              {status || 'Bereit. Änderungen erscheinen nach „Live übernehmen“ in OBS.'}
            </p>
            <button
              className={`${button} border-amber-400 bg-amber-400/15 font-semibold text-amber-100`}
              disabled={busy || !dirty || scene.roster.some((person) => !person.name.trim())}
              onClick={() => void publish()}
            >
              {busy ? 'Wird gespeichert …' : 'Live übernehmen'}
            </button>
          </div>

          <section className="panel-card space-y-3 rounded-2xl p-5">
            <h2 className="text-lg font-semibold text-white">Einmal in OBS einrichten</h2>
            <p className="text-sm text-white/70">
              Browser-Quelle mit 1920 × 1080 anlegen. Kameras und Namensleisten kommen jetzt gemeinsam aus dieser einen Quelle; separate Kameraquellen unter dem Overlay sind nicht mehr nötig, sobald pro Caster eine Kamera-URL hinterlegt ist.
            </p>
            <label className="block text-sm text-white/70" htmlFor="caster-obs-url">
              Feste Browser-Adresse
            </label>
            <div className="flex flex-wrap gap-2">
              <input
                id="caster-obs-url"
                readOnly
                className={input}
                value={OBS_URL}
                onFocus={(event) => event.target.select()}
              />
              <button
                className={button}
                onClick={async () => {
                  try {
                    await navigator.clipboard.writeText(OBS_URL);
                    setStatus('OBS-Adresse kopiert.');
                  } catch {
                    setError('Kopieren nicht möglich. Bitte die Adresse markieren und kopieren.');
                  }
                }}
              >
                Kopieren
              </button>
            </div>
            <p className="text-sm text-white/55">
              Die Kamera-URL wird nur für den jeweils zugewiesenen Caster geladen. Ohne Kamera-URL bleibt das Kamerafenster transparent und kann weiterhin mit einer klassischen OBS-Kamera darunter benutzt werden.
            </p>
            <p className="text-xs text-white/45">
              Kamerapositionen: links X 185 / Y 282, rechts X 1043 / Y 282 · jeweils 694 × 429 Pixel.
            </p>
          </section>
        </>
      )}
    </div>
  );
}
