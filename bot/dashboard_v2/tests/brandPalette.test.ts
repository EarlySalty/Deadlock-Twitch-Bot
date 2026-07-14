import { strict as assert } from 'node:assert';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';

/*
 * Markentreue der Dashboards ("Deadlock Industrial Gold").
 *
 * Warum es diesen Test gibt: Ein Rebrand faellt hier nicht durch den Build.
 * Eine Komponente mit einer Fremdfarbe kompiliert fehlerfrei und sieht nur
 * falsch aus — deshalb wurde das Admin-Dashboard beim Gold-Cutover 2026-07-13
 * schlicht uebersehen, und Petrol-Reste in rgba() ueberlebten das Farb-Mapping.
 * Dieser Test ist das Netz darunter.
 */

const APPS = [
  join(import.meta.dirname, '../src'),
  join(import.meta.dirname, '../../admin_dashboard/src'),
];

const ALLOWED_HEX = new Set([
  // Grund + Gusseisen (dashboard_v2 seit 2026-07-14 eine Stufe heller: der alte
  // Satz hob die Kachel nur um 0.54% Luminanz vom Grund ab, die Seite verschmolz
  // zu einem schwarzen Block. admin_dashboard + shared-theme stehen noch auf den
  // dunklen Toenen, deshalb bleiben beide Saetze erlaubt.)
  '#0d0806', '#140d0a', '#1a1210', '#16100d', '#1f1815', '#2a211b', '#3a2e25',
  '#1a1310', '#221a15', '#2a221c', '#362c23',
  // Gold + Messing. Messing ist der CHROME-Akzent (Buttons, Icon-Kacheln, Auren).
  // Vorher stand dort Plasma-Blau — Chrome und Status trugen dieselbe Farbe, und
  // das Neon brach neben dem Gold. Plasma ist jetzt ausschliesslich Status.
  '#c5a059', '#f1d299', '#9a7c42', '#e0be86', '#f3d9ae',
  // Tinte fuer Gold-/Messingflaechen (Weiss liegt dort bei 1.77:1, siehe Test unten)
  '#241a12',
  // Plasma (Status + Chart-Serien)
  '#00ff88', '#00c46a', '#00d9ff', '#5ce7ff', '#0093ad',
  // Schmiedefeuer / heisses Eisen
  '#e8a33d', '#d98a33', '#ff5a3c', '#ffc9b8',
  // Pergament + Tinten + Text
  '#e3d4b6', '#ede0c4', '#b5a488', '#7a6c57', '#6b5320', '#12545f', '#7e4c10',
  // neutral
  '#ffffff', '#000000',
]);

/* Tailwind-Standardpaletten. Sie tragen keine Hex-Werte im Code und rutschen
   deshalb an jeder Hex-Pruefung vorbei — hier separat abgefangen. */
const TAILWIND_PALETTES =
  'slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink';
const TAILWIND_UTILITY =
  'text|bg|border|from|to|via|shadow|ring|fill|stroke|divide|outline|placeholder|caret|decoration';

function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...sourceFiles(full));
    else if (/\.(css|ts|tsx)$/.test(entry)) out.push(full);
  }
  return out;
}

test('kein Hex-Wert ausserhalb der Industrial-Gold-Palette', () => {
  const strays: string[] = [];
  for (const app of APPS) {
    for (const file of sourceFiles(app)) {
      const src = readFileSync(file, 'utf8');
      for (const hex of src.match(/#[0-9a-fA-F]{6}\b/g) ?? []) {
        if (!ALLOWED_HEX.has(hex.toLowerCase())) strays.push(`${file}: ${hex}`);
      }
    }
  }
  assert.deepEqual(strays, [], `Fremdfarben gefunden:\n${strays.join('\n')}`);
});

test('keine Tailwind-Standardfarben (die ueberleben jeden Farb-Remap)', () => {
  const pattern = new RegExp(`\\b(${TAILWIND_UTILITY})-(${TAILWIND_PALETTES})-[0-9]{2,3}`, 'g');
  const strays: string[] = [];
  for (const app of APPS) {
    for (const file of sourceFiles(app)) {
      for (const hit of readFileSync(file, 'utf8').match(pattern) ?? []) {
        strays.push(`${file}: ${hit}`);
      }
    }
  }
  assert.deepEqual(strays, [], `Tailwind-Standardfarben gefunden:\n${strays.join('\n')}`);
});

test('kein weisser Text auf heller Markenflaeche', () => {
  /*
   * Das alte Teal (#55978f) war dunkel genug fuer weisse Schrift. Plasma-Blau
   * (#00D9FF) und Antik-Gold (#C5A059) sind es nicht: Weiss liegt dort bei
   * 1.70:1 bzw. 2.46:1 — die Billing-CTAs waren nach dem Rebrand faktisch
   * unlesbar. Ein Build faengt das nicht, dieser Test schon.
   *
   * Geprueft wird zeilenweise: Flaeche und Textfarbe eines Ternary-Zweigs
   * stehen immer zusammen, waehrend eine Datei durchaus daneben ein
   * legitimes `bg-white/15 text-white` (weisse Transparenz auf dunklem
   * Grund) enthalten darf.
   */
  const BRIGHT_SURFACE =
    /\b(gradient-accent|(bg|from|to)-(\[#(00d9ff|00ff88|c5a059|f1d299|5ce7ff|e0be86|f3d9ae)\]|accent|primary|success)(\/(100|[7-9]\d))?)(?=[\s"'`])/i;
  // bg-primary/60 ist nachgerechnet: zu 60% deckendes Gold auf dunklem Grund
  // ist selbst dunkel — Weiss 5.16:1 schlaegt dort Dunkel 3.73:1.
  const ALLOWED = /bg-(primary|accent|success)\/[1-6]\d?\b/;
  const WHITE_TEXT = /(?<!hover:)(?<!focus:)(?<!group-hover:)\btext-white\b/;

  /* Zwei Muster, zwei Regeln — und sie duerfen sich nicht vermischen:
   *
   * (a) Ternary-Zweig: Flaeche und Textfarbe stehen IMMER zusammen auf einer Zeile.
   *         ? 'bg-primary text-white'
   *         : 'bg-white/15 text-white'   <- legitim: weisse Transparenz auf dunklem Grund
   *     Ein Zeilenfenster wuerde Zweig A mit Zweig B verheiraten und Fehlalarm schlagen.
   *
   * (b) JSX-Eltern/Kind: Flaeche am oeffnenden Tag, weisser Text erst am Kind darunter.
   *         <div className="gradient-accent ...">
   *           <Heart className="h-4 w-4 text-white" />
   *     Rein zeilenweise bleibt das unsichtbar — so ueberlebte weisser Text auf Gold.
   *
   * Also: Zeile selbst immer pruefen; in die Folgezeilen nur schauen, wenn die
   * Flaeche ein oeffnendes Tag ist und die naechste Zeile ein echtes JSX-Kind. */
  const CHILD_WINDOW = 2;

  const strays: string[] = [];
  for (const app of APPS) {
    for (const file of sourceFiles(app)) {
      const lines = readFileSync(file, 'utf8').split('\n');
      lines.forEach((line, i) => {
        if (!BRIGHT_SURFACE.test(line) || ALLOWED.test(line)) return;

        if (WHITE_TEXT.test(line)) {
          strays.push(`${file}:${i + 1}: ${line.trim().slice(0, 90)}`);
          return;
        }

        const surface = line.trimEnd();
        if (!surface.endsWith('>') || surface.endsWith('/>')) return;

        for (let j = i + 1; j <= Math.min(i + CHILD_WINDOW, lines.length - 1); j += 1) {
          if (!lines[j].trim().startsWith('<')) break;
          if (WHITE_TEXT.test(lines[j]) && !ALLOWED.test(lines[j])) {
            strays.push(`${file}:${j + 1}: ${lines[j].trim().slice(0, 90)}`);
          }
        }
      });
    }
  }
  assert.deepEqual(
    strays,
    [],
    `Weisser Text auf heller Markenflaeche (unlesbar):\n${strays.join('\n')}`,
  );
});

test('Pergament-Tinten halten Kontrast >= 4.5:1 gegen das Papier', () => {
  // Auf hellem Pergament sind Antik-Gold und Plasma unlesbar (~2.5:1).
  // Die ink-Toene sind genau dafuer da — hier wird das nachgerechnet.
  const luminance = (hex: string): number => {
    const chan = [1, 3, 5].map((i) => {
      const c = parseInt(hex.slice(i, i + 2), 16) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * chan[0] + 0.7152 * chan[1] + 0.0722 * chan[2];
  };
  const ratio = (a: string, b: string): number => {
    const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  };

  const parchment = '#e3d4b6';
  for (const ink of ['#6b5320', '#12545f', '#7e4c10', '#2a211b']) {
    assert.ok(
      ratio(parchment, ink) >= 4.5,
      `${ink} auf Pergament: nur ${ratio(parchment, ink).toFixed(2)}:1`,
    );
  }
});
