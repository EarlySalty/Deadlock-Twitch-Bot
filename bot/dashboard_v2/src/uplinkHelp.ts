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

export function uplinkHelpUrl(file: string): string {
  const baseUrl =
    typeof import.meta.env?.BASE_URL === 'string' && import.meta.env.BASE_URL
      ? import.meta.env.BASE_URL
      : '/twitch/dashboard-v2/';
  return `${baseUrl}uplink/${file}`;
}

function helpLink(href: string): string {
  if (href.startsWith('/') || href.startsWith('#') || /^[a-z][a-z\d+.-]*:/i.test(href)) {
    return href;
  }
  return uplinkHelpUrl(href.replace(/^\.\//, ''));
}

export function extractUplinkMain(html: string): string {
  const main = html.match(/<main class="uplink-doc" data-doc="[^"]+">[\s\S]*<\/main>/);
  if (!main) {
    throw new Error('Uplink-Hilfe enthält kein main.uplink-doc.');
  }
  return main[0].replace(
    /(<a\b[^>]*\bhref=["'])([^"']+)(["'])/gi,
    (_whole, prefix: string, href: string, suffix: string) =>
      `${prefix}${helpLink(href)}${suffix}`,
  );
}

export async function fetchUplinkHelp(): Promise<UplinkHelpPage[]> {
  return Promise.all(
    UPLINK_HELP_PAGES.map(async (page) => {
      const response = await fetch(uplinkHelpUrl(page.file), { credentials: 'same-origin' });
      if (!response.ok) {
        throw new Error(`Uplink-Hilfe konnte nicht geladen werden: ${response.status}`);
      }
      return { ...page, html: extractUplinkMain(await response.text()) };
    }),
  );
}
