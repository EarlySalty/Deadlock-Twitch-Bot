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
 * Die Fragmente liegen unter /uplink/, eingebettet werden sie auf /twitch/uplink.
 * Relative Links wie `obs.html` zeigen dort auf /twitch/obs.html, also ins Leere.
 * Beim Einbetten werden sie deshalb auf ihre echte Adresse gezogen.
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
 * Der Zurueck-Link gehoert zur Standalone-Seite. Eingebettet in die
 * Dashboard-Karte stuenden sonst drei davon mitten im Text und fuehrten aus der
 * Oberflaeche heraus auf die Rohseite.
 */
function nurStandaloneEntfernen(fragment: string): string {
  return fragment.replace(/<p class="nur-standalone">[\s\S]*?<\/p>\s*/g, '');
}

export function extractUplinkMain(html: string): string {
  // Nicht gierig: sonst reicht der Treffer bis zum letzten </main> im Dokument
  // und zieht fremdes Markup in das dangerouslySetInnerHTML.
  const main = html.match(/<main class="uplink-doc" data-doc="[^"]+">[\s\S]*?<\/main>/);
  if (!main) {
    throw new Error('Uplink-Hilfe enthält kein main.uplink-doc.');
  }
  return ueberschriftenTieferlegen(absoluteLinks(nurStandaloneEntfernen(main[0])));
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
