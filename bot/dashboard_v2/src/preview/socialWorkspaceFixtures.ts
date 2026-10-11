export type PreviewWorkspaceState = 'ready' | 'loading' | 'error' | 'empty';

export function previewWorkspaceState(
  search: string,
  workspace: 'automation' | 'library' | 'ai',
): PreviewWorkspaceState {
  const fixture = new URLSearchParams(search).get('fixture');
  for (const state of ['loading', 'error', 'empty'] as const) {
    if (fixture === `${workspace}-${state}`) return state;
  }
  return 'ready';
}

export interface PreviewAiVideoVersion {
  id: number;
  version_no: number;
  created_at: string;
  status: string;
  duration_ms: number | null;
  file_url: string | null;
  parent_version_id: number | null;
}

export interface PreviewAiJobView {
  id: number;
  clip_db_id: number;
  status:
    | 'queued'
    | 'preparing'
    | 'transcribing'
    | 'analyzing'
    | 'planning'
    | 'rendering'
    | 'validating'
    | 'review'
    | 'failed';
  stage: string;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
  provider: string | null;
  model: string | null;
  original_duration_ms: number | null;
  output_duration_ms: number | null;
  processing_ms: number | null;
  rendering_ms: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  estimated_provider_cost_usd: number | null;
  measured_provider_cost_usd: number | null;
  cost_basis: 'usage_priced_estimate' | 'provider_billed' | 'unknown';
  highlights: { start_ms: number; end_ms: number; description: string }[];
  decisions: { start_ms: number; end_ms: number; description: string }[];
  uncertainty: string[];
  review_reasons: string[];
  original_url: string | null;
  versions: PreviewAiVideoVersion[];
  failure_code: string | null;
  failure_message: string | null;
  can_retry: boolean;
}

export function previewAiJob(
  clipDbId: number,
  fixture: string | null,
): PreviewAiJobView {
  const rendering = fixture === 'ai-rendering';
  const failed = fixture === 'ai-last-version';
  const unknownCost = fixture === 'ai-unknown-cost';
  return {
    id: 9001,
    clip_db_id: clipDbId,
    status: rendering ? 'rendering' : failed ? 'failed' : 'review',
    stage: rendering || failed ? 'rendering' : 'complete',
    created_at: '2026-10-11T14:00:00Z',
    updated_at: '2026-10-11T14:03:08Z',
    completed_at: rendering || failed ? null : '2026-10-11T14:03:08Z',
    provider: 'Synthetischer Anbieter',
    model: 'Vorschaumodell, kein tatsächlicher Aufruf',
    original_duration_ms: 54800,
    output_duration_ms: 42400,
    processing_ms: rendering ? null : 188000,
    rendering_ms: rendering ? null : 46000,
    input_tokens: unknownCost ? null : 18240,
    output_tokens: unknownCost ? null : 2140,
    estimated_provider_cost_usd: unknownCost ? null : 0.084,
    measured_provider_cost_usd: null,
    cost_basis: unknownCost ? 'unknown' : 'usage_priced_estimate',
    highlights: [
      {
        start_ms: 8400,
        end_ms: 22600,
        description:
          'Der Einstieg in den Teamfight und die Reaktion bleiben zusammen.',
      },
      {
        start_ms: 31200,
        end_ms: 49800,
        description: 'Der letzte Push und sein Ausgang bleiben sichtbar.',
      },
    ],
    decisions: [
      {
        start_ms: 0,
        end_ms: 3600,
        description: 'Wartezeit vor dem Einstieg entfernt.',
      },
      {
        start_ms: 22600,
        end_ms: 27800,
        description: 'Ruhige Passage gekürzt, die Reaktion bleibt erhalten.',
      },
      {
        start_ms: 51200,
        end_ms: 54800,
        description: 'Leeren Auslauf nach der Szene entfernt.',
      },
    ],
    uncertainty: [
      'Die Fähigkeit im Hintergrund lässt sich nicht eindeutig zuordnen. Bitte die Szene ab 0:31 prüfen.',
    ],
    review_reasons: [
      'Der Übergang bei 0:23 muss auf erhaltenen Kontext geprüft werden.',
    ],
    original_url: null,
    versions: [
      {
        id: 9101,
        version_no: 1,
        created_at: '2026-10-11T13:50:00Z',
        status: 'review',
        duration_ms: 46800,
        file_url: null,
        parent_version_id: null,
      },
      {
        id: 9102,
        version_no: 2,
        created_at: '2026-10-11T14:03:08Z',
        status: rendering ? 'rendering' : failed ? 'failed' : 'review',
        duration_ms: rendering || failed ? null : 42400,
        file_url: null,
        parent_version_id: 9101,
      },
    ],
    failure_code: failed ? 'synthetic_render_failure' : null,
    failure_message: failed
      ? 'Die neue Fassung konnte in diesem Beispiel nicht gerendert werden.'
      : null,
    can_retry: failed,
  };
}

export const PREVIEW_SHARED_SLOTS = [
  { day: 'Montag', time: '18:00' },
  { day: 'Mittwoch', time: '19:00' },
  { day: 'Freitag', time: '17:30' },
];

export const PREVIEW_PRODUCTION_QUEUE = [
  {
    id: 1,
    title: 'Die letzte Sekunde: Lash hält den Patron',
    stage: 'Wartet auf Prüfung',
    detail: 'Beispiel: Die fertige Fassung ist noch nicht freigegeben.',
  },
  {
    id: 2,
    title: 'Bebop dreht den Teamfight am Markt',
    stage: 'Noch nicht gestartet',
    detail: 'Beispiel: Statische Bearbeitung zum nächsten gemeinsamen Termin.',
  },
  {
    id: 3,
    title: 'Paradox rettet den Rückzug',
    stage: 'Noch nicht gestartet',
    detail: 'Beispiel: Auswahl nach Quelldatum, nicht nach Aufrufen.',
  },
];
