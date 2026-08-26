/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const PAGES_ROOT = import.meta.dirname;
const UPLINK = readFileSync(join(PAGES_ROOT, 'Uplink.tsx'), 'utf8');
const ZIEL = readFileSync(join(PAGES_ROOT, 'UplinkZiel.tsx'), 'utf8');
const FIXTURES = readFileSync(join(PAGES_ROOT, '../preview/fixtures.ts'), 'utf8');
const UPLINK_API = readFileSync(join(PAGES_ROOT, '../api/uplink.ts'), 'utf8');

test('der Kopf zeigt nur den Streamstatus und dupliziert keine Plattformzustände', () => {
  assert.doesNotMatch(UPLINK, /data-section="uplink-status"/);
  assert.match(UPLINK, /role="status"[\s\S]{0,500}\{streamStatus\.text\}/);
  assert.match(UPLINK, /Stream offline/);
  assert.doesNotMatch(UPLINK, />OBS verbunden</);
});

test('OBS ist eine geordnete Liste aus vier nativen Disclosures', () => {
  assert.match(UPLINK, /<ol[^>]+aria-label="OBS einrichten"/);
  assert.equal((UPLINK.match(/data-obs-step=/g) ?? []).length, 1);
  assert.match(UPLINK, /function ObsSchritt[\s\S]+<details/);
  assert.match(UPLINK, /function ObsSchritt[\s\S]+<summary/);
});

test('der private OBS-Schlüsselhinweis steht in einer eigenen Warnbox', () => {
  assert.match(
    UPLINK,
    /data-uplink-private-warning[\s\S]{0,180}rounded-xl[\s\S]{0,180}border-warning[\s\S]{0,180}bg-warning/,
  );
});

test('die Warteliste erscheint ausschließlich im aktiven Admin-Modus rechts', () => {
  assert.match(UPLINK, /useAuthStatus/);
  assert.match(UPLINK, /authStatus\?\.adminMode/);
  assert.match(UPLINK, /authStatus\?\.adminMode[\s\S]{0,300}<AdminUplinkWarteliste/);
  assert.match(UPLINK, /data-section="uplink-admin-waitlist"/);
  assert.match(UPLINK, /data-section="uplink-right-column"/);
});

test('Admin-Wartelistenaufrufe senden das Session-CSRF-Token', () => {
  assert.match(UPLINK_API, /\/twitch\/api\/v2\/uplink\/admin\/waitlist/);
  assert.match(UPLINK_API, /\/twitch\/api\/v2\/uplink\/admin\/users/);
  assert.match(UPLINK_API, /'X-CSRF-Token': csrfToken/);
});

test('Disclosure-Zustände werden mit geschlossenen Startwerten gespeichert', () => {
  assert.match(UPLINK, /useUplinkDisclosure\('obs-docks', false\)/);
  assert.match(UPLINK, /useUplinkDisclosure\('uplink-hilfe', false\)/);
  assert.match(UPLINK, /useUplinkDisclosure\(`obs-\$\{nummer\}`, offenStart\)/);
  assert.match(ZIEL, /useUplinkDisclosure\(`plattform-\$\{platform\}`, offenStart\)/);
  assert.match(UPLINK, /data-section="obs-docks"[\s\S]{0,100}open=\{docksOffen\}/);
  assert.match(UPLINK, /data-section="uplink-help"[\s\S]{0,100}open=\{hilfeOffen\}/);
});

test('jede Plattformkarte exponiert Plattform und ausgeschriebenen Zustand', () => {
  assert.match(ZIEL, /data-platform=\{platform\}/);
  assert.match(ZIEL, /data-state=\{/);
  assert.match(ZIEL, /aria-label=\{`\$\{label\}-Einstellungen`\}/);
});

test('alle vier Plattformkarten verwenden echte lokale Logos', () => {
  const markenfarben = {
    twitch: '9146' + 'FF',
    youtube: 'FF00' + '00',
    kick: '53FC' + '18',
    tiktok: '25F4' + 'EE',
  } as const;
  for (const [platform, farbe] of Object.entries(markenfarben)) {
    assert.match(ZIEL, new RegExp(`import ${platform}Logo from '@/assets/platforms/${platform}\\.svg'`));
    const logo = readFileSync(join(PAGES_ROOT, `../assets/platforms/${platform}.svg`), 'utf8');
    assert.match(logo, new RegExp(`fill="#${farbe}"`, 'i'));
  }
  assert.match(ZIEL, /src=\{PLATTFORM_LOGOS\[platform\]\}/);
  assert.doesNotMatch(ZIEL, /const kuerzel/);
});

test('die parallel gelieferte Reconnect-Wartezeit bleibt im neuen Layout erhalten', () => {
  assert.match(UPLINK, /function ReconnectWaitKarte/);
  assert.match(UPLINK, /saveUplinkReconnectWait/);
  assert.match(UPLINK, /<ReconnectWaitKarte/);
  assert.match(FIXTURES, /reconnect_wait_s:\s*90/);
  assert.match(FIXTURES, /reconnect_wait_max_s:\s*300/);
});

test('Laden und Fehler erfinden keine leeren Plattformziele', () => {
  assert.match(UPLINK, /zieleFehler\s*\|\|\s*zieleLaden\s*\?\s*'hidden'/);
  assert.match(UPLINK, /gespeicherteZiele\.length === 0 && !zieleFehler && !zieleLaden/);
  assert.match(UPLINK, /Ziele werden geladen/);
  assert.match(UPLINK, /Status unbekannt/);
});

test('Clipboard-Fehler hinterlassen ein fokussiertes, auswählbares Feld', () => {
  assert.match(UPLINK, /feldRef\.current\?\.focus\(\)/);
  assert.match(UPLINK, /feldRef\.current\?\.select\(\)/);
  assert.match(UPLINK, /readOnly[\s\S]{0,100}type=\{offen \? 'text' : 'password'\}/);
});

test('verbinden_lebt_in_der_plattform_karte', () => {
  assert.doesNotMatch(UPLINK, /data-section="plattformen-verbinden"/);
  assert.match(UPLINK, /Verbinden geht in der jeweiligen Plattform-Karte oben\./);
  assert.match(UPLINK, /chat=\{chatVerbindungen\.find/);
  assert.match(ZIEL, /chat\.knopfText/);
  assert.match(ZIEL, /chat\.statusText/);
  assert.match(ZIEL, /uplinkConnectUrl\(chat\.id\)/);
  // Verbinden laeuft ueber den bestehenden Streamer-OAuth, nicht ueber einen
  // eigenen Pfad: ein zweiter Grant fuer dasselbe Konto hiess zwei Zugaenge,
  // von denen einer irgendwann der falsche war.
  assert.match(UPLINK_API, /\/twitch\/raid\/auth\?scope_profile=uplink/);
  assert.doesNotMatch(UPLINK_API, /uplink\/connect\/\$\{platform\}`;/);
  assert.match(UPLINK_API, /Mit \$\{p\.label\} verbinden/);
  assert.match(UPLINK_API, /'Neu verbinden'/);
  assert.match(UPLINK_API, /Folgt später/);
});

test('trennen_sitzt_in_der_plattform_karte_mit_hinweis_auf_den_raid_bot', () => {
  // Trennen nimmt den ganzen Zugang zurueck. Ohne den Satz schaltet man
  // Leuten unbemerkt die automatischen Raids ab.
  assert.match(UPLINK_API, /automatischen Raids auf, bis du dich neu verbindest/);
  assert.match(ZIEL, /TRENNEN_HINWEIS/);
  assert.match(ZIEL, /trenneUplinkPlattform\(chat\.id, csrfToken/);
  assert.match(ZIEL, /^\s+Trennen$/m);
  assert.match(ZIEL, /Ja, trennen/);
  assert.match(UPLINK_API, /connect\/\$\{platform\}\/disconnect/);
});

test('vier_dock_adressen_mit_namen_beim_erzeugen', () => {
  // Vier Fenster hinter einem Token; die Namen sind die, die in OBS
  // eingetragen werden.
  assert.match(UPLINK_API, /titel: 'Chat', feld: 'chat'/);
  assert.match(UPLINK_API, /titel: 'Aktivität', feld: 'activity'/);
  assert.match(UPLINK_API, /titel: 'Stream-Infos', feld: 'stream_info'/);
  assert.match(UPLINK_API, /titel: 'Kanalpunkte', feld: 'points'/);
  // Der Name steht sichtbar vor der Adresse, sonst sind es vier gleich
  // aussehende Zeilen.
  assert.match(UPLINK, /<span className="w-28 shrink-0 text-\[11px\] font-semibold text-white">\{titel\}<\/span>/);
  assert.match(UPLINK, /eigene\.map\(\(dock\) => \(/);
  assert.match(UPLINK, /Diese Adressen werden nur jetzt einmal angezeigt/);
});
