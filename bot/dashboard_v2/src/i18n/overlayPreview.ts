export const OVERLAY_PREVIEW_TEXT = [
  'Held',
  'Sieg',
  'Siege',
  'Niederlage',
  'Niederlagen',
  'S',
  'N',
  'RANG',
  'HEUTE',
  'SERIE',
  'LETZTES SPIEL',
  'LIEBLINGSHELD',
  'Letzte Spiele · neuestes links',
  'Match-Verlauf',
  'Spielername',
  'Laufendes Spiel',
] as const;

export function overlayPreviewTranslations(t: (text: string) => string): Record<string, string> {
  return Object.fromEntries(OVERLAY_PREVIEW_TEXT.map(text => [text, t(text)]));
}
