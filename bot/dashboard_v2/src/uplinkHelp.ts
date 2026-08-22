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

/// Vite ersetzt `import.meta.env.BASE_URL` beim Bauen. Im nackten Node-Testlauf
/// gibt es das Objekt nicht, deshalb der Rueckfall auf die Wurzel.
function basisPfad(): string {
  return import.meta.env?.BASE_URL ?? '/';
}

export function uplinkHelpUrl(file: string): string {
  return `${basisPfad()}uplink/${file}`;
}

/**
 * Die Fragmente liegen unter <BASE_URL>uplink/, eingebettet werden sie auf der
 * Uplink-Seite des Dashboards. Relative Links wie `obs.html` zeigen von dort aus
 * auf einen Nachbarpfad der Seite, den es nicht gibt. Beim Einbetten werden sie
 * deshalb ueber uplinkHelpUrl auf ihre echte Adresse gezogen.
 */
function absoluteLinks(fragment: string): string {
  return fragment.replace(
    /href="(?!https?:|\/|#)([^"]+\.html)"/g,
    (_treffer, datei: string) => `href="${uplinkHelpUrl(datei)}"`,
  );
}

/**
 * Die Seite hat schon eine h1 (der Uplink-Titel). Drei eingebettete Fragmente mit
 * je eigener h1 zerlegen die Überschriftenstruktur für Screenreader, deshalb
 * rutscht jede Fragment-Überschrift eine Ebene tiefer.
 */
function ueberschriftenTieferlegen(fragment: string): string {
  return fragment.replace(/<(\/?)h([1-5])\b/g, (_t, schraeg: string, stufe: string) => {
    const neu = Math.min(Number(stufe) + 1, 6);
    return `<${schraeg}h${neu}`;
  });
}

/**
 * Zurueck-Link und Querverweise gehoeren zur Standalone-Seite. Eingebettet in
 * die Dashboard-Karte stuenden sonst drei davon mitten im Text und fuehrten aus
 * der Oberflaeche heraus auf die Rohseite, obwohl das Ziel zwei Kacheln tiefer
 * schon dasteht. Ganze Abschnitte, die dann leer waeren ("Weiterlesen"),
 * verschwinden mit.
 */
function nurStandaloneEntfernen(fragment: string): string {
  return fragment
    .replace(/<section class="nur-standalone"[\s\S]*?<\/section>\s*/g, '')
    .replace(/<p class="nur-standalone">[\s\S]*?<\/p>\s*/g, '');
}

/**
 * Nimmt die Titelueberschrift aus dem Fragment.
 *
 * Eingebettet steht der Kapitelname schon im Klapptitel. Bliebe die h1 im
 * Inhalt, stuende derselbe Satz zweimal untereinander, sobald jemand aufklappt.
 *
 * Bewusst vor `ueberschriftenTieferlegen`: dort wird aus h1 ein h2, und danach
 * waere der Titel von einer echten Zwischenueberschrift nicht mehr zu
 * unterscheiden. Nur der erste Treffer faellt weg, alles Weitere ist Inhalt.
 * Die eigenstaendige Hilfeseite laeuft nicht durch diese Funktion und behaelt
 * ihren Titel.
 */
export function titelUeberschriftEntfernen(fragment: string): string {
  return fragment.replace(/<h1\b[^>]*>[\s\S]*?<\/h1>\s*/, '');
}

export function extractUplinkMain(html: string): string {
  // Nicht gierig: sonst reicht der Treffer bis zum letzten </main> im Dokument
  // und zieht fremdes Markup in das dangerouslySetInnerHTML.
  const main = html.match(/<main class="uplink-doc" data-doc="[^"]+">[\s\S]*?<\/main>/);
  if (!main) {
    throw new Error('Uplink-Hilfe enthält kein main.uplink-doc.');
  }
  // main wird zu div: mehrere main-Landmarks in einem Dokument sind ungueltig
  // und fuer Screenreader genau das Problem, gegen das die Ueberschriften eine
  // Ebene tiefer rutschen.
  const alsAbschnitt = main[0]
    .replace(/^<main /, '<div ')
    .replace(/<\/main>$/, '</div>');
  return ueberschriftenTieferlegen(
    titelUeberschriftEntfernen(absoluteLinks(nurStandaloneEntfernen(alsAbschnitt))),
  );
}

export async function fetchUplinkHelp(): Promise<UplinkHelpPage[]> {
  // Bewusst kein Promise.all: eine einzelne 404 würde sonst alle drei Kacheln
  // kippen. Scheitern alle drei (Deploy ohne neues dist/), wirft die Funktion
  // weiterhin, und die Seite zeigt ihre Fehlerzeile.
  const ergebnisse: PromiseSettledResult<UplinkHelpPage>[] = await Promise.allSettled(
    UPLINK_HELP_PAGES.map(async (page): Promise<UplinkHelpPage> => {
      const response = await fetch(uplinkHelpUrl(page.file), { credentials: 'same-origin' });
      if (!response.ok) {
        throw new Error(`Uplink-Hilfe konnte nicht geladen werden: ${response.status}`);
      }
      return { ...page, html: extractUplinkMain(await response.text()) };
    }),
  );
  const geladen = ergebnisse
    .filter((e): e is PromiseFulfilledResult<UplinkHelpPage> => e.status === 'fulfilled')
    .map((e) => e.value);
  if (geladen.length === 0) {
    const erster = ergebnisse.find((e) => e.status === 'rejected');
    throw erster && erster.status === 'rejected'
      ? erster.reason
      : new Error('Uplink-Hilfe konnte nicht geladen werden.');
  }
  return geladen;
}
