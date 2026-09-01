import type {
  ClipPreparation,
  ClipPreparationState,
  DiscardClipResult,
} from '../../types/socialMedia';

export const CLIP_VORBEREITUNG_POLL_MS = 2500;
export const MAX_MANUELLER_UPLOAD_BYTES = 200 * 1024 * 1024;

export type ManuellerUploadFehler = 'format' | 'zu_gross' | null;

export function pruefeManuellenUpload(datei: {
  name: string;
  type: string;
  size: number;
}): ManuellerUploadFehler {
  const istMp4 = datei.type === 'video/mp4' || datei.name.toLowerCase().endsWith('.mp4');
  if (!istMp4) return 'format';
  return datei.size > MAX_MANUELLER_UPLOAD_BYTES ? 'zu_gross' : null;
}

const AKTIVE_ZUSTAENDE = new Set<ClipPreparationState>([
  'pending',
  'materializing',
  'source_ready',
  'rendering',
]);

export function istVorbereitungAktiv(state: ClipPreparationState): boolean {
  return AKTIVE_ZUSTAENDE.has(state);
}

export type VorbereitungSchrittStatus = 'wartet' | 'aktiv' | 'fertig' | 'fehler';

export interface VorbereitungSchritt {
  id: 'quelle' | 'render' | 'vorschau';
  label: string;
  status: VorbereitungSchrittStatus;
}

export function vorbereitungSchritte(preparation: ClipPreparation): VorbereitungSchritt[] {
  const quelleFertig = preparation.source_ready || preparation.preview_ready;
  const renderAktiv = preparation.state === 'source_ready' || preparation.state === 'rendering';
  const fehlgeschlagen = preparation.state === 'failed';

  return [
    {
      id: 'quelle',
      label: 'Quelle',
      status: quelleFertig
        ? 'fertig'
        : fehlgeschlagen
          ? 'fehler'
          : preparation.state === 'materializing'
            ? 'aktiv'
            : 'wartet',
    },
    {
      id: 'render',
      label: 'Hochformat',
      status: preparation.preview_ready
        ? 'fertig'
        : fehlgeschlagen && preparation.source_ready
          ? 'fehler'
          : renderAktiv
            ? 'aktiv'
            : 'wartet',
    },
    {
      id: 'vorschau',
      label: 'Vorschau',
      status: preparation.preview_ready ? 'fertig' : 'wartet',
    },
  ];
}

export function freigabeAktion(
  releaseEnabled: boolean | null,
  zielAnzahl: number,
  pending: boolean,
  previewReady: boolean,
): { disabled: boolean; label: string; hinweis: string | null } {
  const planBekannt = releaseEnabled !== null;
  return {
    disabled: pending || !planBekannt || !previewReady || zielAnzahl === 0,
    label: !planBekannt
      ? 'Freigabemodus wird geladen…'
      : releaseEnabled === true
        ? 'Zur Veröffentlichung freigeben'
        : 'Für später freigeben',
    hinweis: !planBekannt
      ? 'Freigabemodus wird geladen.'
      : !previewReady
        ? 'Bereite den Clip zuerst auf und prüfe die Vorschau.'
        : zielAnzahl === 0
          ? 'Wähle mindestens eine Zielplattform.'
          : null,
  };
}

/**
 * Merkt sich nur eine weiterhin abspielbare Vorschau. Während der POST läuft,
 * meldet die Workbench dem Elternteil trotzdem `null`, damit nicht parallel
 * freigegeben werden kann.
 */
export function sichereVorbereitungVorMutation(
  preparation: ClipPreparation | null,
  previewAbspielbar: boolean,
): ClipPreparation | null {
  return previewAbspielbar ? preparation : null;
}

/** Stellt nach einem fehlgeschlagenen POST den letzten sicheren Stand wieder her. */
export function rueckmeldungNachVorbereitungsfehler(
  vorherigeVorbereitung: ClipPreparation | null,
): ClipPreparation | null {
  return vorherigeVorbereitung;
}

export function verwerfenWarnung(ergebnis: DiscardClipResult | null): string | null {
  return ergebnis && ergebnis.already_running > 0
    ? 'Mindestens ein laufender Upload konnte möglicherweise nicht mehr gestoppt werden. Prüfe die Zielplattformen.'
    : null;
}

const VORBEREITUNGS_FEHLER: Record<string, string> = {
  source_unavailable: 'Das Quellvideo ist nicht mehr erreichbar.',
  materialization_failed: 'Das Quellvideo konnte nicht vorbereitet werden.',
  download_failed: 'Das Quellvideo konnte nicht heruntergeladen werden.',
  render_failed: 'Die Hochformat-Vorschau konnte nicht gerendert werden.',
  preview_unavailable: 'Die gerenderte Vorschau ist nicht mehr erreichbar.',
};

export function vorbereitungFehler(preparation: ClipPreparation): string {
  if (preparation.error_code && VORBEREITUNGS_FEHLER[preparation.error_code]) {
    return VORBEREITUNGS_FEHLER[preparation.error_code];
  }
  return preparation.error_message || 'Die Clip-Aufbereitung ist fehlgeschlagen.';
}
