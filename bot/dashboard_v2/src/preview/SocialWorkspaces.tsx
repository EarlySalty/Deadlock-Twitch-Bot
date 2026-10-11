import { useEffect, useState, type ReactNode } from 'react';
import {
  AlertCircle,
  Calendar,
  Clock3,
  Film,
  Layers3,
  ShieldCheck,
} from 'lucide-react';
import { SocialMedia, type SocialMediaProps } from '@/pages/SocialMedia';
import type { AiEditorMountProps } from '@/components/socialmedia/studioNavigation';
import {
  PREVIEW_PRODUCTION_QUEUE,
  PREVIEW_SHARED_SLOTS,
  previewAiJob,
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

function SyntheticImport() {
  const state = previewWorkspaceState(window.location.search, 'library');
  const [range, setRange] = useState('quarter');
  if (state !== 'ready')
    return <FixtureState state={state} subject="Der historische Clip-Import" />;
  return (
    <PreviewDraft>
      {(dirty, reset) => (
        <section className="studio-workspace-surface studio-preview-import">
          <div className="studio-section-heading">
            <Film aria-hidden="true" />
            <h2>Historische Twitch-Clips</h2>
            <span>Nur Deadlock · eigener Kanal</span>
          </div>
          <p className="studio-workspace-note">
            Ein Import füllt die Bibliothek. Er startet keine Analyse,
            Videoproduktion oder Veröffentlichung.
          </p>
          <div className="studio-import-fields">
            <label>
              Zeitraum
              <select
                aria-label="Zeitraum"
                className="studio-select"
                value={range}
                onChange={(event) => setRange(event.target.value)}
              >
                <option value="all">Alle verfügbaren Clips</option>
                <option value="year">Jahr</option>
                <option value="quarter">Quartal</option>
                <option value="custom">Eigener Zeitraum</option>
                <option value="before">Vor einem Datum</option>
                <option value="after">Nach einem Datum</option>
              </select>
            </label>
            {(range === 'year' || range === 'quarter') && (
              <label>
                Jahr
                <input
                  className="studio-input"
                  aria-label="Jahr"
                  type="number"
                  min="2020"
                  defaultValue="2025"
                />
              </label>
            )}
            {range === 'quarter' && (
              <label>
                Quartal
                <select
                  aria-label="Quartal"
                  className="studio-select"
                  defaultValue="1"
                >
                  <option value="1">1. Quartal</option>
                  <option value="2">2. Quartal</option>
                  <option value="3">3. Quartal</option>
                  <option value="4">4. Quartal</option>
                </select>
              </label>
            )}
            {(range === 'custom' ||
              range === 'before' ||
              range === 'after') && (
              <label>
                {range === 'custom' ? 'Von' : 'Datum'}
                <input
                  type="date"
                  aria-label={range === 'custom' ? 'Von' : 'Datum'}
                  className="studio-input"
                  defaultValue="2025-01-01"
                />
              </label>
            )}
            {range === 'custom' && (
              <label>
                Bis
                <input
                  type="date"
                  aria-label="Bis"
                  className="studio-input"
                  defaultValue="2025-03-31"
                />
              </label>
            )}
            <label>
              Auswahlreihenfolge
              <select
                aria-label="Auswahlreihenfolge"
                className="studio-select"
                defaultValue="oldest"
              >
                <option value="oldest">Älteste zuerst</option>
                <option value="newest">Neueste zuerst</option>
              </select>
            </label>
          </div>
          <div className="studio-preview-actions">
            <button type="button" className="studio-primary" disabled>
              Import starten
            </button>
            <button type="button" className="studio-button" disabled>
              Filter speichern
            </button>
            <button
              type="button"
              className="studio-button"
              disabled={!dirty}
              onClick={() => {
                setRange('quarter');
                reset();
              }}
            >
              Vorschau zurücksetzen
            </button>
          </div>
          <p role="status" className="studio-workspace-note">
            {dirty
              ? 'Nur lokal geändert. Import und Speicherung bleiben gesperrt.'
              : 'Zeitraum und Reihenfolge sind Beispieldaten, kein aktiver Import.'}
          </p>
          <details className="studio-preview-import-state">
            <summary>Beispiel: unterbrochener Import</summary>
            <p>
              Die Erfassung ist unvollständig. Bereits erfasste Clips bleiben
              erhalten; ein vollständiges Archiv ist nicht bestätigt.
            </p>
            <button type="button" disabled className="studio-button">
              Import fortsetzen
            </button>
          </details>
        </section>
      )}
    </PreviewDraft>
  );
}

function duration(ms: number | null): string {
  if (ms === null) return 'Noch nicht bekannt';
  const seconds = ms / 1000;
  return `${Math.floor(seconds / 60)}:${(seconds % 60).toLocaleString('de-DE', { minimumIntegerDigits: 2, maximumFractionDigits: 1 })}`;
}

function cost(value: number | null): string {
  return value === null
    ? 'Nicht bekannt'
    : `${value.toLocaleString('de-DE', { minimumFractionDigits: 3, maximumFractionDigits: 3 })} USD`;
}

function SyntheticAiReview({
  twitchUserId,
  streamer,
  clipDbId,
}: AiEditorMountProps) {
  const params = new URLSearchParams(window.location.search);
  const job = previewAiJob(clipDbId ?? 1, params.get('fixture'));
  const latest = job.versions.at(-1)!;
  const [versionId, setVersionId] = useState(
    job.status === 'rendering' || job.status === 'failed'
      ? job.versions[0].id
      : latest.id,
  );
  const version = job.versions.find((item) => item.id === versionId) ?? latest;
  const state = previewWorkspaceState(window.location.search, 'ai');
  if (state !== 'ready')
    return <FixtureState state={state} subject="Die KI-Aufträge" />;
  const status =
    job.status === 'rendering'
      ? 'Neue Fassung wird gerendert'
      : job.status === 'failed'
        ? 'Rendern fehlgeschlagen'
        : 'Ergebnis wartet auf Prüfung';
  const facts = [
    ['Originaldauer', duration(job.original_duration_ms)],
    ['Ausgewählte Fassung', duration(version.duration_ms)],
    ['Verarbeitung gesamt', duration(job.processing_ms)],
    ['Renderzeit (FFmpeg)', duration(job.rendering_ms)],
    ['KI-Modell', job.model ?? 'Nicht bekannt'],
    ['Anbieter', job.provider ?? 'Nicht bekannt'],
    ['Geschätzte Anbieter-Kosten', cost(job.estimated_provider_cost_usd)],
    [
      'Gemessene Anbieter-Kosten',
      job.measured_provider_cost_usd === null
        ? 'Nicht abgeglichen'
        : cost(job.measured_provider_cost_usd),
    ],
  ];
  return (
    <div
      className="studio-preview-ai"
      data-owner={twitchUserId}
      data-clip-db-id={job.clip_db_id}
    >
      <section className="studio-workspace-surface">
        <div className="studio-section-heading">
          <Film aria-hidden="true" />
          <h2>Original & bearbeitete Fassung</h2>
          <span
            className={
              job.status === 'failed'
                ? 'studio-tone-danger'
                : 'studio-tone-warning'
            }
            role="status"
          >
            {status}
          </span>
        </div>
        <div className="studio-preview-review-toolbar">
          <p>
            {streamer} · Clip {job.clip_db_id} · Beispielauftrag {job.id}
          </p>
          <label>
            Videofassung
            <select
              aria-label="Videofassung"
              className="studio-select"
              value={versionId}
              onChange={(event) => setVersionId(Number(event.target.value))}
            >
              {job.versions.map((item) => (
                <option
                  key={item.id}
                  value={item.id}
                  disabled={item.status !== 'review'}
                >
                  Fassung {item.version_no} ·{' '}
                  {item.status === 'review'
                    ? 'Prüfbar'
                    : item.status === 'failed'
                      ? 'Fehlgeschlagen'
                      : 'Wird gerendert'}
                </option>
              ))}
            </select>
          </label>
        </div>
        {job.status !== 'review' && (
          <p
            className="studio-preview-review-warning"
            role={job.status === 'failed' ? 'alert' : 'status'}
          >
            {job.failure_message ?? 'Die neue Fassung ist noch nicht fertig.'}{' '}
            Die vorherige Fassung bleibt zum Vergleich ausgewählt.
          </p>
        )}
        <div className="social-ai-video-grid">
          {[
            {
              title: 'Originalclip',
              time: duration(job.original_duration_ms),
              vertical: false,
            },
            {
              title: `Bearbeitete Fassung ${version.version_no}`,
              time: duration(version.duration_ms),
              vertical: true,
            },
          ].map((media) => (
            <figure key={media.title}>
              <figcaption>
                <span>{media.title}</span>
                <strong>{media.time}</strong>
              </figcaption>
              <div
                className={`studio-preview-media ${media.vertical ? 'studio-preview-media-vertical' : ''}`}
              >
                <video controls preload="none" aria-label={media.title} />
                <div>
                  <Film aria-hidden="true" />
                  <span>Synthetischer Medienplatzhalter</span>
                  <small>Keine abspielbare Datei</small>
                </div>
              </div>
            </figure>
          ))}
        </div>
        <div className="social-ai-explanations">
          <section>
            <h3>Erkannte Highlights</h3>
            <ol>
              {job.highlights.map((item) => (
                <li key={item.start_ms}>
                  <time>
                    {duration(item.start_ms)} bis {duration(item.end_ms)}
                  </time>
                  <p>{item.description}</p>
                </li>
              ))}
            </ol>
          </section>
          <section>
            <h3>Entfernte Abschnitte & Schnittentscheidungen</h3>
            <ol>
              {job.decisions.map((item) => (
                <li key={item.start_ms}>
                  <time>
                    {duration(item.start_ms)} bis {duration(item.end_ms)}
                  </time>
                  <p>{item.description}</p>
                </li>
              ))}
            </ol>
          </section>
        </div>
        <div className="studio-preview-review-notes">
          <section>
            <h3>Unsicherheit im Gameplay</h3>
            {job.uncertainty.map((text) => (
              <p key={text}>{text}</p>
            ))}
          </section>
          <section>
            <h3>Bei der Prüfung beachten</h3>
            {job.review_reasons.map((text) => (
              <p key={text}>{text}</p>
            ))}
          </section>
        </div>
      </section>
      <section className="studio-workspace-surface">
        <div className="studio-section-heading">
          <Clock3 aria-hidden="true" />
          <h2>Zeit, Kosten & Fassungen</h2>
        </div>
        <dl className="social-ai-facts">
          {facts.map(([label, value]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
        <p className="studio-workspace-note">
          {job.cost_basis === 'unknown'
            ? 'Kostenstand unbekannt, kein Nullbetrag.'
            : 'Die Schätzung basiert auf Beispiel-Tokenzahlen, nicht auf einer Anbieterrechnung.'}{' '}
          FFmpeg-Laufzeit ist keine KI-Anbietergebühr.
        </p>
        <ol className="studio-preview-version-history">
          {job.versions.map((item) => (
            <li key={item.id}>
              <strong>Fassung {item.version_no}</strong>
              <span>
                {item.status === 'review'
                  ? 'Wartet auf Prüfung'
                  : item.status === 'failed'
                    ? 'Fehlgeschlagen'
                    : 'Wird gerendert'}
              </span>
              <span>{duration(item.duration_ms)}</span>
              <span>
                {item.parent_version_id === null
                  ? 'Erste Fassung'
                  : 'Überarbeitung der ersten Fassung'}
              </span>
            </li>
          ))}
        </ol>
        <div className="studio-preview-actions">
          <button type="button" disabled className="studio-primary">
            KI-Verarbeitung starten
          </button>
          <button type="button" disabled className="studio-button">
            Fassung freigeben
          </button>
          <button type="button" disabled className="studio-button">
            Für Veröffentlichung einplanen
          </button>
        </div>
        <p className="studio-workspace-note">
          Manuelle KI-Bearbeitung veröffentlicht nichts automatisch. Feedback,
          Freigabe und Übergabe sind hier nicht angebunden.
        </p>
      </section>
    </div>
  );
}

export default function SocialWorkspaces(props: SocialMediaProps) {
  const legacy =
    new URLSearchParams(window.location.search)
      .get('fixture')
      ?.startsWith('legacy') ?? false;
  return (
    <>
      <p
        className="studio-preview-notice"
        data-preview="p3-social-workspace-synthetic-v1"
      >
        Lokale Layoutvorschau mit synthetischen Daten. Kein Speichern, Import,
        KI-Aufruf oder Veröffentlichen.
      </p>
      <SocialMedia
        {...props}
        aiEditor={legacy ? props.aiEditor : SyntheticAiReview}
        workspaceSlots={
          legacy
            ? props.workspaceSlots
            : {
                automation: <SyntheticAutomation />,
                libraryImport: <SyntheticImport />,
              }
        }
      />
    </>
  );
}
