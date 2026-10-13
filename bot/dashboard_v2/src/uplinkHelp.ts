import { DEFAULT_LANGUAGE, translate, type Language } from './i18n/dictionary';

export const UPLINK_HELP_PAGES = [
  { file: 'was-ist.html', label: 'Was ist Uplink' },
  { file: 'obs.html', label: 'OBS einrichten' },
  { file: 'stoerungen.html', label: 'Störungen' },
] as const;

export interface UplinkHelpPage {
  file: (typeof UPLINK_HELP_PAGES)[number]['file'];
  label: (typeof UPLINK_HELP_PAGES)[number]['label'];
  html: string;
}

function basisPfad(): string {
  return import.meta.env?.BASE_URL ?? '/';
}

export function uplinkHelpUrl(file: string, language: Language = DEFAULT_LANGUAGE): string {
  return `${basisPfad()}uplink/${language === 'en' ? 'en/' : ''}${file}`;
}

function absoluteLinks(fragment: string, language: Language): string {
  return fragment.replace(
    /href="(?!https?:|\/|#)([^"]+\.html(?:[?#][^"]*)?)"/g,
    (_treffer, datei: string) => `href="${uplinkHelpUrl(datei, language)}"`,
  );
}

function absoluteBilder(fragment: string, language: Language): string {
  return fragment.replace(
    /src="(?!https?:|data:|\/)([^"]+)"/g,
    (_treffer, pfad: string) => `src="${uplinkHelpUrl(pfad, language)}"`,
  );
}

function ueberschriftenTieferlegen(fragment: string): string {
  return fragment.replace(/<(\/?)h([1-5])\b/g, (_t, schraeg: string, stufe: string) => {
    const neu = Math.min(Number(stufe) + 1, 6);
    return `<${schraeg}h${neu}`;
  });
}

function nurStandaloneEntfernen(fragment: string): string {
  return fragment
    .replace(/<section class="nur-standalone"[\s\S]*?<\/section>\s*/g, '')
    .replace(/<p class="nur-standalone">[\s\S]*?<\/p>\s*/g, '');
}

export function titelUeberschriftEntfernen(fragment: string): string {
  return fragment.replace(/<h1\b[^>]*>[\s\S]*?<\/h1>\s*/, '');
}

export function extractUplinkMain(html: string, language: Language = DEFAULT_LANGUAGE): string {
  const t = translate.bind(null, language);
  const main = html.match(/<main class="uplink-doc" data-doc="[^"]+">[\s\S]*?<\/main>/);
  if (!main) {
    throw new Error(t('Uplink-Hilfe enthält kein main.uplink-doc.'));
  }
  const alsAbschnitt = main[0]
    .replace(/^<main /, '<div ')
    .replace(/<\/main>$/, '</div>');
  return ueberschriftenTieferlegen(
    titelUeberschriftEntfernen(absoluteBilder(absoluteLinks(nurStandaloneEntfernen(alsAbschnitt), language), language)),
  );
}

export async function fetchUplinkHelp(language: Language = DEFAULT_LANGUAGE): Promise<UplinkHelpPage[]> {
  const t = translate.bind(null, language);
  const ergebnisse: PromiseSettledResult<UplinkHelpPage>[] = await Promise.allSettled(
    UPLINK_HELP_PAGES.map(async (page): Promise<UplinkHelpPage> => {
      const response = await fetch(uplinkHelpUrl(page.file, language), { credentials: 'same-origin' });
      if (!response.ok) {
        throw new Error(t('Uplink-Hilfe konnte nicht geladen werden: {status}', { status: response.status }));
      }
      return { ...page, html: extractUplinkMain(await response.text(), language) };
    }),
  );
  const geladen = ergebnisse
    .filter((e): e is PromiseFulfilledResult<UplinkHelpPage> => e.status === 'fulfilled')
    .map((e) => e.value);
  if (geladen.length === 0) {
    const erster = ergebnisse.find((e) => e.status === 'rejected');
    throw erster && erster.status === 'rejected'
      ? erster.reason
      : new Error(t('Uplink-Hilfe konnte nicht geladen werden.'));
  }
  return geladen;
}
