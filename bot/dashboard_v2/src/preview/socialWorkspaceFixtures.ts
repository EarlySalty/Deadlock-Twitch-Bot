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
