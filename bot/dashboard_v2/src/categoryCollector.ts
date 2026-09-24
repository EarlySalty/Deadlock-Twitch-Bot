export const languageName = (code: string) => {
  if (code === 'und') return 'Unbekannt / nicht sicher erkannt';
  try { return new Intl.DisplayNames(['de'], { type: 'language' }).of(code) ?? code; } catch { return code; }
};
