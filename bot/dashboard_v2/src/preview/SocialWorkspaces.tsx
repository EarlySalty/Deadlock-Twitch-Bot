import { useEffect, useState, type ReactNode } from 'react';
import { AlertCircle, Calendar, Layers3, ShieldCheck } from 'lucide-react';
import { SocialMedia, type SocialMediaProps } from '@/pages/SocialMedia';
import { ImportPoolPanel } from '@/components/socialmedia/autopilot/import-pool';
import { createImportPoolFixture } from '@/components/socialmedia/autopilot/import-pool/fixtures';
import {
  PREVIEW_PRODUCTION_QUEUE,
  PREVIEW_SHARED_SLOTS,
  previewWorkspaceState,
  type PreviewWorkspaceState,
} from './socialWorkspaceFixtures';

function PreviewDraft({
  children,
}: {
  children: (dirty: boolean, reset: () => void) => ReactNode;
}) {
  const [dirty, setDirty] = useState(false);
  const [version, setVersion] = useState(0);
  useEffect(() => {
    const warn = (event: BeforeUnloadEvent) => {
      if (!dirty) return;
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);
  const reset = () => {
    setDirty(false);
    setVersion((value) => value + 1);
  };
  return (
    <div
      key={version}
      data-unsaved={dirty ? 'true' : undefined}
      onInput={() => setDirty(true)}
    >
      {children(dirty, reset)}
    </div>
  );
}

function FixtureState({
  state,
  subject,
}: {
  state: PreviewWorkspaceState;
  subject: string;
}) {
  if (state === 'loading')
    return (
      <div className="studio-preview-loading" role="status" aria-busy="true">
        <p>{subject} wird geladen…</p>
        <div />
        <div />
        <div />
      </div>
    );
  if (state === 'error')
    return (
      <div className="studio-preview-error" role="alert">
        <AlertCircle aria-hidden="true" />
        <div>
          <h2>Beispiel eines Ladefehlers</h2>
          <p>
            {subject} ist nicht abrufbar. Einstellungen bleiben unbekannt,
            Änderungen sind gesperrt.
          </p>
          <p>
            Die lokale Vorschau kann keine Verbindung zum echten Dienst
            herstellen.
          </p>
        </div>
      </div>
    );
  return (
    <div className="studio-preview-empty">
      <Layers3 aria-hidden="true" />
      <h2>Noch keine Einträge</h2>
      <p>
        {subject} ist in diesem Beispiel leer. Es wird keine Bearbeitung oder
        Veröffentlichung gestartet.
      </p>
    </div>
  );
}

function SyntheticAutomation() {
  const state = previewWorkspaceState(window.location.search, 'automation');
  if (state !== 'ready')
    return <FixtureState state={state} subject="Der gemeinsame Zeitplan" />;
  return (
    <PreviewDraft>
      {(dirty, reset) => (
        <div className="studio-workspace-grid">
          <section className="studio-workspace-surface">
            <div className="studio-section-heading">
              <Calendar aria-hidden="true" />
              <h2>Gemeinsamer Wochenplan</h2>
              <span className="studio-tone-neutral">
                Auto-Pilot ausgeschaltet
              </span>
            </div>
            <p className="studio-workspace-note">
              Ein Termin nutzt eine vorbereitete Videofassung für die gewählten
              Plattformen.
            </p>
            <div className="studio-preview-slot-list">
              {PREVIEW_SHARED_SLOTS.map(({ day, time }) => (
                <label key={day}>
                  <span>{day}</span>
                  <input
                    type="time"
                    aria-label={`${day}, Uhrzeit`}
                    defaultValue={time}
                    className="studio-input"
                  />
                  <span>YouTube · TikTok</span>
                </label>
              ))}
            </div>
            <div className="studio-field-grid">
              <label>
                Zeitzone
                <select
                  aria-label="Zeitzone"
                  defaultValue="Europe/Berlin"
                  className="studio-select"
                >
                  <option>Europe/Berlin</option>
                  <option>Europe/London</option>
                  <option>America/New_York</option>
                </select>
              </label>
              <label>
                Vorlauf
                <select
                  aria-label="Vorlauf"
                  defaultValue="24"
                  className="studio-select"
                >
                  <option value="12">12 Stunden</option>
                  <option value="24">24 Stunden</option>
                  <option value="48">48 Stunden</option>
                  <option value="72">72 Stunden</option>
                </select>
              </label>
            </div>
            <p className="studio-workspace-note">
              Vorlauf ist ein Ziel, keine Mindestzeit. Verpasste Termine sammeln
              sich nicht an.
            </p>
            <div className="studio-preview-setting-row">
              <div>
                <h3>KI-Bearbeitung</h3>
                <p>
                  Statische Layouts bleiben der Standard. Ein manueller KI-Test
                  ist davon unabhängig.
                </p>
              </div>
              <label>
                <input type="checkbox" disabled /> KI verwenden
              </label>
            </div>
            <div className="studio-preview-setting-row">
              <div>
                <h3>Nur nach Freigabe</h3>
                <p>
                  Bei zwei wartenden Prüfungen beginnt keine weitere
                  automatische Produktion.
                </p>
              </div>
              <ShieldCheck aria-hidden="true" />
            </div>
            <div className="studio-preview-actions">
              <button type="button" className="studio-primary" disabled>
                Auto-Pilot starten
              </button>
              <button type="button" className="studio-button" disabled>
                Speichern
              </button>
              <button
                type="button"
                className="studio-button"
                disabled={!dirty}
                onClick={reset}
              >
                Vorschau zurücksetzen
              </button>
            </div>
            <p role="status" className="studio-workspace-note">
              {dirty
                ? 'Nur lokal geändert, nicht gespeichert.'
                : 'Beispieleinstellungen, kein gespeicherter Kanalzustand.'}
            </p>
          </section>
          <section className="studio-workspace-surface">
            <div className="studio-section-heading">
              <Layers3 aria-hidden="true" />
              <h2>Produktion & Prüfung</h2>
            </div>
            <p className="studio-workspace-note">
              Ein Video nach dem anderen. Laufende Arbeit bleibt bei einer Pause
              erhalten.
            </p>
            <ol className="studio-preview-queue">
              {PREVIEW_PRODUCTION_QUEUE.map((item) => (
                <li key={item.id}>
                  <span className="studio-status studio-tone-neutral">
                    {item.stage}
                  </span>
                  <h3>{item.title}</h3>
                  <p>{item.detail}</p>
                  {item.id > 1 && (
                    <button type="button" className="studio-button" disabled>
                      Als Nächstes
                    </button>
                  )}
                </li>
              ))}
            </ol>
            <p className="studio-workspace-note">
              Reihenfolge, Pause und Entfernen werden erst mit dem echten
              Produktionsdienst bedienbar.
            </p>
          </section>
        </div>
      )}
    </PreviewDraft>
  );
}

function PreviewImport({
  twitchUserId,
}: Pick<SocialMediaProps, 'twitchUserId'>) {
  const [fixture] = useState(() => {
    const state = previewWorkspaceState(window.location.search, 'library');
    const value = createImportPoolFixture({
      owner: twitchUserId,
      preferences: {
        clip_date_filter: { kind: 'quarter', year: 2025, quarter: 1 },
        clip_sort: 'oldest_first',
        timezone: 'Europe/Berlin',
      },
      ...(state === 'empty' ? { clips: [] } : {}),
    });
    if (state === 'loading') {
      value.holdNext('getPreferences');
      value.holdNext('getLatestImport');
    } else if (state === 'error') {
      value.failNext('getPreferences');
      value.failNext('getLatestImport');
    }
    return value;
  });
  return <ImportPoolPanel twitchUserId={twitchUserId} api={fixture.api} />;
}

export default function SocialWorkspaces(props: SocialMediaProps) {
  const legacy =
    new URLSearchParams(window.location.search)
      .get('fixture')
      ?.startsWith('legacy') ?? false;
  const workspaceSlots = {
    ...props.workspaceSlots,
    libraryImport: props.workspaceSlots?.libraryImport ?? (
      <PreviewImport
        key={props.twitchUserId ?? ''}
        twitchUserId={props.twitchUserId}
      />
    ),
  };
  return (
    <>
      <p
        className="studio-preview-notice"
        data-preview="p3-social-workspace-synthetic-v1"
      >
        Lokale Layoutvorschau mit synthetischen Daten. Änderungen und
        Importaufträge bleiben lokal. Keine KI-Aufrufe oder Veröffentlichungen.
      </p>
      <SocialMedia
        {...props}
        workspaceSlots={
          legacy
            ? workspaceSlots
            : {
                ...workspaceSlots,
                automation: props.workspaceSlots?.automation ?? (
                  <SyntheticAutomation />
                ),
                publishReview: props.workspaceSlots?.publishReview ?? (
                  <section
                    className="studio-workspace-surface"
                    data-preview="p3-publish-review-reservation-v1"
                  >
                    <div className="studio-section-heading">
                      <h2>Veröffentlichung prüfen</h2>
                    </div>
                    <p className="studio-workspace-note">
                      Die Veröffentlichungsprüfung ist noch nicht verfügbar.
                    </p>
                  </section>
                ),
              }
        }
      />
    </>
  );
}
