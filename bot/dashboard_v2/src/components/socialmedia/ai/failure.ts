export function failureExplanation(message: string | null): string {
  const fallback = 'Bitte prüfe den Auftrag und versuche die fehlgeschlagene Phase erneut, falls ein weiterer Versuch angeboten wird.';
  const text = message?.trim();
  if (!text || text.length > 240 || !/^(?:Der|Die|Das|Beim|Bitte|Es|Ein|Eine) [^\r\n<>\\/{}[\]:]{10,}[.!]$/u.test(text) || /\b(?:error|exception|stack|trace|failed|https?|null|undefined)\b/iu.test(text)) return fallback;
  return text;
}
