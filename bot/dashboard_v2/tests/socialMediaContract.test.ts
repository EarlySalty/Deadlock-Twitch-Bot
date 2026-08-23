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
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// Die Node-Tests laufen ohne Vite und damit ohne den automatischen
// JSX-Transform, wie in languageProvider.test.tsx.
(globalThis as { React?: typeof React }).React = React;

import { dictionaryFor, translate } from '../src/i18n/dictionary';
import { LanguageProvider } from '../src/context/LanguageContext';
import {
  clipFehler,
  istGesperrt,
  istStandUnbekannt,
} from '../src/components/socialmedia/kartenZustand';
import { LadeFehlerHinweis } from '../src/components/socialmedia/LadeFehlerHinweis';
import {
  ALLE_LABEL,
  APPROVAL_MODE_TEXTE,
  APPROVAL_STATE_LABELS,
  FEHLER_TEXTE,
  FELD_FEHLER,
  fehlerText,
  KATEGORIE_LABELS,
  REPORT_KIND_LABELS,
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
  'src/components/socialmedia/LadeFehlerHinweis.tsx',
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
  // Stand vorher als Funktion `kindLabel()` in AnalyticsTab und fiel damit
  // durch dieses Netz: ein fehlender englischer Eintrag waere unbemerkt
  // geblieben.
  REPORT_KIND_LABELS: Object.values(REPORT_KIND_LABELS),
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

// ── 4. Karten ohne geladenen Stand bleiben gesperrt ──────────────────────────

/**
 * Der rote Faden hinter den Befunden 4 bis 7: eine Karte, deren GET
 * gescheitert ist, kennt den gespeicherten Stand nicht. Sie faellt auf ihre
 * Vorgabewerte zurueck ("aus", "Privat", "Nur nach Freigabe", "nicht
 * verbunden") und sieht dabei aus wie eine Karte mit echten Daten. Ein Klick
 * schreibt den erfundenen Wert fest, und der Serverschutz "fehlendes Feld
 * bleibt beim bisherigen Wert" hilft nicht, weil die Oberflaeche das Feld
 * ausdruecklich mitschickt.
 *
 * Geprueft wird deshalb beides: die Sperrmechanik selbst und die Verdrahtung
 * jeder betroffenen Karte.
 */

const SEITE = 'src/pages/SocialMedia.tsx';

/** Rumpf einer Komponente aus der Seite, von `function X(` bis zur schliessenden Klammer in Spalte 0. */
function komponentenRumpf(quelle: string, name: string): string {
  const start = quelle.indexOf(`function ${name}(`);
  assert.ok(start >= 0, `${name} gibt es in ${SEITE} nicht mehr.`);
  const ende = quelle.indexOf('\n}\n', start);
  assert.ok(ende > start, `${name} laesst sich nicht abgrenzen.`);
  return quelle.slice(start, ende);
}

/**
 * Die Attribute genau eines JSX-Elements, von `<Name` bis zum ersten `/>`.
 * Ohne diese Begrenzung wuerde ein `[\s\S]*?` bis in die naechste Karte
 * laufen und deren Attribut als Treffer verkaufen.
 */
function elementAttribute(quelle: string, name: string): string {
  const start = quelle.indexOf(`<${name}`);
  assert.ok(start >= 0, `<${name}> steht nicht mehr in ${SEITE}.`);
  const ende = quelle.indexOf('/>', start);
  assert.ok(ende > start, `<${name}> ist nicht selbstschliessend.`);
  return quelle.slice(start, ende);
}

/** Alle `disabled={...}`-Ausdruecke eines Komponentenrumpfs. */
function disabledAusdruecke(rumpf: string): string[] {
  return [...rumpf.matchAll(/disabled=\{([^}]*)\}/g)].map((m) => m[1].trim());
}

test('istGesperrt sperrt weiter, wenn der Ladevorgang vorbei und gescheitert ist', () => {
  assert.equal(istGesperrt({ isLoading: false, isSaving: false, ladeFehler: null }), false);
  assert.equal(istGesperrt({ isLoading: true, isSaving: false, ladeFehler: null }), true);
  assert.equal(istGesperrt({ isLoading: false, isSaving: true, ladeFehler: null }), true);
  // Der eigentliche Befund: nach dem gescheiterten GET steht isLoading wieder
  // auf false, und ohne den Ladefehler waere die Karte wieder bedienbar.
  assert.equal(
    istGesperrt({ isLoading: false, isSaving: false, ladeFehler: new Error('boom') }),
    true,
  );
  assert.equal(istStandUnbekannt(new Error('boom')), true);
  assert.equal(istStandUnbekannt(null), false);
  assert.equal(istStandUnbekannt(undefined), false);
});

test('der Ladefehler wird sichtbar und nennt die Sperre', () => {
  const html = renderToStaticMarkup(
    React.createElement(
      LanguageProvider,
      null,
      React.createElement(LadeFehlerHinweis, { fehler: { code: 'save_failed' } }),
    ),
  );
  assert.match(html, /Gespeicherter Stand nicht abrufbar/);
  assert.match(html, /Das Speichern hat nicht geklappt/);
  assert.match(html, /gesperrt/);
  assert.match(html, /role="alert"/);

  // Ohne Fehler bleibt die Karte still.
  const leer = renderToStaticMarkup(
    React.createElement(
      LanguageProvider,
      null,
      React.createElement(LadeFehlerHinweis, { fehler: null }),
    ),
  );
  assert.equal(leer, '');
});

test('Befund 4: die VOD-Archiv-Karte sperrt nach gescheitertem GET', () => {
  const quelle = lies(SEITE);
  assert.match(
    elementAttribute(quelle, 'VodArchiveCard'),
    /ladeFehler=\{vodArchiveQuery\.error\}/,
    'Die Karte bekommt den Fehler von GET /settings/vod-archive nicht.',
  );
  const rumpf = komponentenRumpf(quelle, 'VodArchiveCard');
  assert.match(rumpf, /LadeFehlerHinweis fehler=\{ladeFehler\}/);
  assert.match(rumpf, /const gesperrt = istGesperrt\(\{ isLoading, isSaving, ladeFehler \}\)/);
  const ausdruecke = disabledAusdruecke(rumpf);
  assert.ok(ausdruecke.length >= 2, 'Der Schalter und die Sichtbarkeits-Knoepfe fehlen.');
  for (const ausdruck of ausdruecke) {
    assert.ok(
      ausdruck.includes('gesperrt'),
      `disabled={${ausdruck}} ignoriert die Sperre: der Schalter schickt sonst privacy: 'private' mit.`,
    );
  }
});

test('Befund 5: die Verbindungskarte erfindet keinen Verbindungszustand', () => {
  const quelle = lies(SEITE);
  assert.match(
    elementAttribute(quelle, 'PlatformConnectionsCard'),
    /ladeFehler=\{platformStatusQuery\.error\}/,
    'Die Karte bekommt den Fehler des Status-Abrufs nicht.',
  );
  const rumpf = komponentenRumpf(quelle, 'PlatformConnectionsCard');
  assert.match(rumpf, /LadeFehlerHinweis fehler=\{ladeFehler\}/);
  assert.match(rumpf, /const standUnbekannt = istStandUnbekannt\(ladeFehler\)/);

  // Ohne Status darf keine Zeile "nicht verbunden" behaupten.
  const unbekannt = rumpf.indexOf('if (standUnbekannt) {');
  const nichtVerbunden = rumpf.indexOf("t('nicht verbunden')");
  assert.ok(unbekannt >= 0 && unbekannt < nichtVerbunden, 'Der unbekannte Stand wird nicht zuerst geprueft.');

  // Und es darf kein Knopf dastehen, der einen ueberfluessigen OAuth-Flow startet.
  const sperre = rumpf.indexOf('{standUnbekannt ? null :');
  const oauth = rumpf.indexOf('oauthStartUrl');
  assert.ok(sperre >= 0 && sperre < oauth, 'Der Verbinden-Knopf steht auch ohne Status noch da.');
  for (const ausdruck of disabledAusdruecke(rumpf)) {
    assert.ok(ausdruck.includes('gesperrt'), `disabled={${ausdruck}} ignoriert die Sperre.`);
  }
});

test('Befund 6: die drei Zeitplan-Karten sperren nach gescheitertem GET', () => {
  const quelle = lies(SEITE);
  for (const karte of ['ApprovalModeCard', 'CategoryCard', 'PostingScheduleCard']) {
    assert.match(
      elementAttribute(quelle, karte),
      /ladeFehler=\{postingPlanQuery\.error\}/,
      `${karte} bekommt den Fehler von GET posting-plan nicht.`,
    );
    const rumpf = komponentenRumpf(quelle, karte);
    assert.match(rumpf, /LadeFehlerHinweis fehler=\{ladeFehler\}/, `${karte} zeigt den Fehler nicht.`);
    const ausdruecke = disabledAusdruecke(rumpf);
    assert.ok(ausdruecke.length > 0, `${karte} hat kein einziges gesperrtes Bedienelement.`);
    for (const ausdruck of ausdruecke) {
      assert.ok(
        ausdruck.includes('gesperrt') || ausdruck.includes('Gesperrt'),
        `${karte}: disabled={${ausdruck}} ignoriert die Sperre.`,
      );
    }
  }

  // Ein Kanal auf full_auto darf nicht "Nur nach Freigabe" markiert sehen.
  const approval = komponentenRumpf(quelle, 'ApprovalModeCard');
  assert.match(
    approval,
    /istStandUnbekannt\(ladeFehler\) \? null : 'manual'/,
    'Ohne Plan wird weiterhin manual als aktiver Modus markiert.',
  );
});

test('Befund 7: ein gescheitertes Verwerfen landet an der Clip-Karte', () => {
  const clipDbId = 42;
  const verwerfen = new Error('discard');
  // Genau der Fall aus dem Befund: nur das Verwerfen ist gescheitert.
  assert.equal(
    clipFehler(clipDbId, [
      { clipDbId: undefined, error: null },
      { clipDbId, error: verwerfen },
    ]),
    verwerfen,
  );
  // Eine erfolgreiche Freigabe am selben Clip darf den Fehler nicht verdecken.
  assert.equal(
    clipFehler(clipDbId, [
      { clipDbId, error: null },
      { clipDbId, error: verwerfen },
    ]),
    verwerfen,
  );
  // Ein Fehler an einem anderen Clip gehoert nicht auf diese Karte.
  assert.equal(clipFehler(clipDbId, [{ clipDbId: 7, error: verwerfen }]), null);
  assert.equal(clipFehler(clipDbId, []), null);

  // Und die Seite reicht das Verwerfen ueberhaupt durch.
  const quelle = lies(SEITE);
  assert.match(
    quelle,
    /fehler=\{clipFehler\(clip\.clip_db_id, \[[\s\S]*?discardMutation\.error[\s\S]*?\]\)\}/,
    'discardMutation.error kommt nicht an der Clip-Karte an.',
  );
});

/**
 * Ein Backend-Fehler darf keinen fertigen deutschen Satz mitschicken: der
 * laeuft nie durch `translate` und stuende im englischen Dashboard auf Deutsch
 * da. `only_paused_platforms` traegt deshalb nur einen stabilen Code, die
 * betroffenen Plattformnamen kommen als `message` und werden hier in den
 * uebersetzten Satz eingesetzt.
 */
test('only_paused_platforms wird uebersetzt und behaelt die Plattformnamen', () => {
  const fehler = { code: 'only_paused_platforms', message: 'youtube, tiktok' };

  const deutsch = fehlerText(fehler, (text, params) => translate('de', text, params));
  assert.ok(deutsch);
  assert.match(deutsch, /youtube, tiktok/);
  assert.doesNotMatch(deutsch, /\{details\}/);

  const englisch = fehlerText(fehler, (text, params) => translate('en', text, params));
  assert.ok(englisch);
  assert.match(englisch, /youtube, tiktok/);
  assert.match(englisch, /zero posts/, 'im englischen Dashboard steht der englische Satz');
  assert.doesNotMatch(englisch, /Plattformen stehen auf null/);
});

/**
 * Ohne `message` faellt `SocialMediaApiError` auf den Code zurueck. Der ist
 * keine Liste und darf nicht als Platzhalterinhalt im Satz landen.
 */
test('ein Code ohne Meldung landet nicht als Platzhalterinhalt im Satz', () => {
  const satz = fehlerText(
    { code: 'approval_decision_failed', message: 'approval_decision_failed' },
    (text, params) => translate('en', text, params),
  );
  assert.equal(satz, 'The decision could not be saved.');
});
