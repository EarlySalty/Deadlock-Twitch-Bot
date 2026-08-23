/**
 * Vertragstest der Social-Media-Oberflaeche.
 *
 * Die drei vorhandenen Tests pruefen Geometrie, Uebersetzungsmechanik und den
 * Provider. Keiner davon haette gefunden, dass ein Clip-Status und ein
 * Admin-Knopf denselben deutschen Schluessel benutzen (englisch stand am
 * freigegebenen Clip dann "ACCESS GRANTED"), und keiner haette gemerkt, dass im
 * Metadaten-Panel ein englischer Schluessel ohne Eintrag steht.
 *
 * Deshalb prueft dieser Test die Verdrahtung statt der Mechanik:
 *  1. jeder in der Oberflaeche benutzte Schluessel hat eine englische Fassung,
 *  2. kein deutscher Text traegt in zwei Tabellen zwei Bedeutungen,
 *  3. jede gebaute URL gibt es als Route, und jede Route hat einen Aufrufer
 *     oder steht bewusst ohne Oberflaeche in einer Liste.
 *
 * Node-Test-Mechanik ohne Vite, wie in languageProvider.test.tsx.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { dictionaryFor } from '../src/i18n/dictionary';
import {
  ALLE_LABEL,
  APPROVAL_MODE_TEXTE,
  APPROVAL_STATE_LABELS,
  FEHLER_TEXTE,
  FELD_FEHLER,
  KATEGORIE_LABELS,
  SOCIAL_MEDIA_TABS,
  STATUS_LABELS,
  STATUS_META,
  ZUGRIFF_LABELS,
} from '../src/components/socialmedia/labels';

const HIER = path.dirname(fileURLToPath(import.meta.url));
const WURZEL = path.resolve(HIER, '..');
/** Vom Dashboard aus zum Repo-Wurzelverzeichnis: bot/dashboard_v2 -> Repo. */
const REPO = path.resolve(WURZEL, '..', '..');

function lies(relativ: string): string {
  return fs.readFileSync(path.join(WURZEL, relativ), 'utf8');
}

// ── 1. Uebersetzungsabdeckung ────────────────────────────────────────────────

/** Dateien, deren Texte ein englischsprachiger Nutzer zu sehen bekommt. */
const OBERFLAECHE = [
  'src/pages/SocialMedia.tsx',
  'src/pages/SocialMediaAdmin.tsx',
  'src/components/socialmedia/AnalyticsTab.tsx',
  'src/components/socialmedia/EnrichmentPanel.tsx',
  'src/components/socialmedia/LayoutEditor.tsx',
  'src/components/layout/Header.tsx',
];

/** `t('...')` und `t("...")`, auch ueber einen Zeilenumbruch hinweg. */
const T_AUFRUF = /\bt\(\s*(['"])((?:\\.|(?!\1)[^\\])*)\1/g;

function schluesselAusDateien(dateien: string[]): Map<string, string> {
  const treffer = new Map<string, string>();
  for (const datei of dateien) {
    const quelle = lies(datei);
    let m: RegExpExecArray | null;
    T_AUFRUF.lastIndex = 0;
    while ((m = T_AUFRUF.exec(quelle))) {
      const schluessel = m[2].replace(/\\'/g, "'").replace(/\\"/g, '"').replace(/\\\\/g, '\\');
      if (!treffer.has(schluessel)) treffer.set(schluessel, datei);
    }
  }
  return treffer;
}

/** Die Konstantentabellen, aus denen zur Laufzeit `t()` gefuettert wird. */
const TABELLEN: Record<string, string[]> = {
  STATUS_LABELS: Object.values(STATUS_LABELS).map((eintrag) => eintrag.label),
  APPROVAL_STATE_LABELS: Object.values(APPROVAL_STATE_LABELS),
  APPROVAL_MODE_TEXTE: Object.values(APPROVAL_MODE_TEXTE).flatMap((eintrag) => [
    eintrag.label,
    eintrag.hinweis,
  ]),
  STATUS_META: Object.values(STATUS_META).map((eintrag) => eintrag.label),
  SOCIAL_MEDIA_TABS: SOCIAL_MEDIA_TABS.map((eintrag) => eintrag.label),
  STATUS_FILTER: [ALLE_LABEL],
  ZUGRIFF_LABELS: Object.values(ZUGRIFF_LABELS),
  KATEGORIE_LABELS: Object.values(KATEGORIE_LABELS),
  FELD_FEHLER: Object.values(FELD_FEHLER),
  FEHLER_TEXTE: Object.values(FEHLER_TEXTE),
};

test('jeder Schluessel der Oberflaeche hat eine englische Fassung', () => {
  const en = dictionaryFor('en');
  const fehlend: string[] = [];

  for (const [schluessel, datei] of schluesselAusDateien(OBERFLAECHE)) {
    if (!(schluessel in en)) fehlend.push(`${datei}: ${JSON.stringify(schluessel)}`);
  }
  for (const [tabelle, werte] of Object.entries(TABELLEN)) {
    for (const wert of werte) {
      if (!(wert in en)) fehlend.push(`${tabelle}: ${JSON.stringify(wert)}`);
    }
  }

  assert.deepEqual(
    fehlend,
    [],
    `Ohne englischen Eintrag steht der deutsche Text im englischen Dashboard:\n${fehlend.join('\n')}`,
  );
});

test('der Regex findet ueberhaupt Schluessel', () => {
  // Schutz gegen den stillen Ausfall: ein kaputter Regex laesst den Test oben
  // durchlaufen, ohne irgendetwas zu pruefen.
  assert.ok(schluesselAusDateien(OBERFLAECHE).size > 100);
});

// ── 2. Ein Text, eine Bedeutung ──────────────────────────────────────────────

/**
 * Texte, die bewusst in mehreren Tabellen stehen, weil sie dasselbe meinen und
 * deshalb dieselbe Uebersetzung bekommen. Alles andere ist ein Befund.
 */
const BEWUSST_GETEILT: Record<string, string> = {
  Fehler: 'Clip-Status und Enrichment-Status meinen beide "fehlgeschlagen".',
  'Clip freigegeben': 'Clip-Status und Approval-Zustand meinen dieselbe Entscheidung.',
  Übersprungen: 'Clip-Status und Approval-Zustand meinen dasselbe Ueberspringen.',
  Veröffentlicht: 'Der Reiter zeigt genau die Clips mit diesem Status.',
};

test('kein deutscher Text traegt in zwei Tabellen zwei Bedeutungen', () => {
  const herkunft = new Map<string, string[]>();
  for (const [tabelle, werte] of Object.entries(TABELLEN)) {
    for (const wert of werte) {
      const liste = herkunft.get(wert) ?? [];
      if (!liste.includes(tabelle)) liste.push(tabelle);
      herkunft.set(wert, liste);
    }
  }

  const kollisionen = [...herkunft.entries()]
    .filter(([wert, tabellen]) => tabellen.length > 1 && !(wert in BEWUSST_GETEILT))
    .map(([wert, tabellen]) => `${JSON.stringify(wert)} in ${tabellen.join(' + ')}`);

  assert.deepEqual(
    kollisionen,
    [],
    `Derselbe deutsche Text mit zwei Bedeutungen bekommt eine falsche Uebersetzung:\n${kollisionen.join('\n')}`,
  );

  // Der konkrete Befund von damals: der Admin-Knopf und der Clip-Status.
  assert.notEqual(STATUS_LABELS.approved.label, ZUGRIFF_LABELS.granted);

  // Die Ausnahmeliste darf nicht zur Muellhalde werden.
  for (const wert of Object.keys(BEWUSST_GETEILT)) {
    assert.ok(
      (herkunft.get(wert)?.length ?? 0) > 1,
      `${JSON.stringify(wert)} steht ohne Grund in der Ausnahmeliste.`,
    );
  }
});

// ── 3. Gebaute URLs gegen die Routen des Backends ────────────────────────────

const API_MODUL = 'src/api/socialMedia.ts';
const RUST_ROUTEN = path.join(REPO, 'rust/crates/tb-dashboard-api/src/lib.rs');

/** `:clip_db_id` und `${clipDbId}` sind derselbe Platzhalter. */
function normalisiere(pfad: string): string {
  return pfad.replace(/:[A-Za-z_][A-Za-z0-9_]*/g, ':p');
}

/**
 * Alle Pfade, die das API-Modul zusammenbaut. Konstanten werden vorher
 * eingesetzt, Query-Anhaenge fallen weg, Ausdruecke werden Platzhalter.
 */
function gebautePfade(): string[] {
  const quelle = lies(API_MODUL);

  const konstanten = new Map<string, string>();
  const konstantenMuster = /const\s+([A-Z_]+)\s*=\s*'([^']+)';/g;
  let k: RegExpExecArray | null;
  while ((k = konstantenMuster.exec(quelle))) konstanten.set(k[1], k[2]);
  // Die Deklaration selbst ist keine aufgerufene URL.
  const rumpf = quelle.replace(konstantenMuster, '');

  const roh = new Set<string>();
  // Sowohl '/social-media/...' als auch `${ADMIN_PREFIX}/...`.
  const pfadMuster = /['"`](\/social-media[^'"`]*)['"`]|`\$\{([A-Z_]+)\}([^`]*)`/g;
  let m: RegExpExecArray | null;
  while ((m = pfadMuster.exec(rumpf))) {
    if (m[1]) roh.add(m[1]);
    else if (m[2] && konstanten.has(m[2])) roh.add(`${konstanten.get(m[2])}${m[3]}`);
  }
  // Eine Konstante, die direkt an fetch() geht, ist selbst die URL.
  for (const [name, wert] of konstanten) {
    const direkt = new RegExp(`(^|[^{])\\b${name}\\b`, 'm');
    if (direkt.test(rumpf)) roh.add(wert);
  }

  const pfade = new Set<string>();
  for (const eintrag of roh) {
    let pfad = eintrag
      // `${qs}` ist der zusammengebaute Query-String, kein Pfadsegment.
      .replace(/\$\{qs\}/g, '')
      .replace(/\$\{[^}]*\}/g, ':p');
    const fragezeichen = pfad.indexOf('?');
    if (fragezeichen >= 0) pfad = pfad.slice(0, fragezeichen);
    if (pfad.startsWith('/social-media')) pfade.add(normalisiere(pfad));
  }
  return [...pfade].sort();
}

/** Alle `/social-media`-Routen aus der Axum-Registrierung. Nur lesen. */
function registrierteRouten(): string[] {
  const quelle = fs.readFileSync(RUST_ROUTEN, 'utf8');
  const routen = new Set<string>();
  const muster = /\.route\(\s*"(\/social-media[^"]*)"/g;
  let m: RegExpExecArray | null;
  while ((m = muster.exec(quelle))) routen.add(normalisiere(m[1]));
  return [...routen].sort();
}

/**
 * Routen, die es im Backend gibt und die die Oberflaeche bewusst nicht ruft.
 * Das sind durchweg Vorgaenger des Admin-Pfades aus der Python-Zeit.
 */
const BEWUSST_OHNE_UI = new Set([
  '/social-media/api/stats',
  '/social-media/api/clips',
  '/social-media/api/last-hashtags',
  '/social-media/api/analytics',
  '/social-media/api/upload',
  '/social-media/api/mark-uploaded',
  '/social-media/api/batch-upload',
  '/social-media/api/templates/global',
  '/social-media/api/templates/streamer',
  '/social-media/api/templates/apply',
]);

test('jede gebaute URL gibt es als Route', () => {
  const routen = new Set(registrierteRouten());
  const gebaut = gebautePfade();
  assert.ok(gebaut.length > 15, 'Die Pfad-Erkennung findet nichts mehr.');
  const ohneRoute = gebaut.filter((pfad) => !routen.has(pfad));
  assert.deepEqual(ohneRoute, [], `URL ohne Route im Backend:\n${ohneRoute.join('\n')}`);
});

test('jede /social-media/api-Route hat einen Aufrufer oder einen Grund', () => {
  const gebaut = new Set(gebautePfade());
  const verwaist = registrierteRouten()
    .filter((route) => route.startsWith('/social-media/api/'))
    .filter((route) => !gebaut.has(route) && !BEWUSST_OHNE_UI.has(route));
  assert.deepEqual(
    verwaist,
    [],
    `Route ohne Aufrufer und ohne Eintrag in BEWUSST_OHNE_UI:\n${verwaist.join('\n')}`,
  );
});

test('die Liste "bewusst ohne UI" bleibt ehrlich', () => {
  const routen = new Set(registrierteRouten());
  const gebaut = new Set(gebautePfade());
  for (const route of BEWUSST_OHNE_UI) {
    assert.ok(routen.has(route), `${route} steht in BEWUSST_OHNE_UI, gibt es aber nicht mehr.`);
    assert.ok(!gebaut.has(route), `${route} wird inzwischen aufgerufen und gehoert aus der Liste.`);
  }
});
