# Plan: Lesezeichen-Hinweis und Name "Partner Dashboard"

status: aktiv
datum: 2026-09-05
contract: CONTRACT.md

## Ziel

Siehe CONTRACT.md.

## Nicht-Ziele

Siehe CONTRACT.md.

## Milestones

### M1: Browser-Erkennung als reine Funktion, Test zuerst
Änderungen: `bot/dashboard_v2/tests/lesezeichenHinweis.test.ts` (neu), dann `bot/dashboard_v2/src/utils/browserErkennung.ts` (neu), Testliste in `package.json`.
Export: `erkenneBrowser(eingabe: { userAgent: string; brave: boolean; platform: string; mobile: boolean })` liefert `{ browser, mobil, mac }`; `lesezeichenAnleitung(erkennung)` liefert `{ position: 'links' | 'rechts' | 'menue' | 'unbekannt', symbol: 'stern' | 'herz' | 'teilen' | 'menue' | null, tastenkombi: ['Strg', 'D'] | ['⌘', 'D'], hinweis: string }`.
Erwarteter Zwischenzustand: Test läuft zuerst rot (Modul fehlt), roter Lauf mit Fehlermeldung unten im Verlauf festgehalten, danach grün.
Validierung: `cd bot/dashboard_v2 && node --import tsx --test tests/lesezeichenHinweis.test.ts`
Stop-Regel: Erkennung braucht DOM-Zugriff oder ist nicht deterministisch, dann zurück zum Entwurf.

### M2: Karte LesezeichenHinweis und Einbau vor der WelcomeTour
Änderungen: `LesezeichenHinweis.tsx` (neu; Hook liest navigator, ruft die reine Funktion, wartet `navigator.brave.isBrave()` ab), `InternalHomeLanding.tsx` (mountet die Karte, reicht `startErlaubt` an die WelcomeTour), `WelcomeTour.tsx` (neue optionale Prop `startErlaubt`, Default true; `resetWelcomeTour` löscht auch den Hinweis-Schlüssel `lesezeichen-hinweis-erledigt`), `dictionary.ts` (alle neuen Texte).
Optik: Panel-Look wie WelcomeTour-Popover (panel-card, Gold-Verlauf auf dem Hauptknopf), Adressleisten-Nachbildung mit pulsierendem Symbol, Pfeil nach oben, kbd-Tasten, Knöpfe "Link kopieren" und "Erledigt", X zum Schließen, Escape schließt.
Erwarteter Zwischenzustand: Im Preview-Modus (`npm run dev:preview`) erscheint die Karte beim ersten Laden vor der Tour; nach "Erledigt" startet die Tour; Reload zeigt die Karte nicht mehr.
Validierung: `cd bot/dashboard_v2 && npm run lint && npm run build`
Stop-Regel: WelcomeTour-Verhalten ändert sich über den verzögerten Start hinaus, dann stoppen.

### M3: Umbenennung und Form-Test
Änderungen: `index.html`, `Header.tsx`, `InternalHomeLanding.tsx`, `PricingTour.tsx`, `dictionary.ts`, `tests/partnerDashboardName.test.ts` (neu: title, Header-Badge, keine Vorkommen von "Twitch Analytics" und "Analyse Dashboard"/"Analyse-Dashboard" mehr in src außer dem SocialMediaAdmin-Kommentar, Karte in InternalHomeLanding gemountet, WelcomeTour bekommt `startErlaubt`).
Validierung: `cd bot/dashboard_v2 && npm test`
Stop-Regel: ein bestehender Test wird rot, dann Ursache klären, nicht den Test ändern.

### M4: Preview-Build für die Sichtprüfung
Änderungen: keine Quelldateien; `npm run build:preview` erzeugt `dist-preview`.
Validierung: `cd bot/dashboard_v2 && npm run build:preview && ls dist-preview/index.html`
Stop-Regel: Build rot, dann M2/M3 nachbessern.

## Verlauf

- 2026-09-05: Plan angelegt.
- 2026-09-05 M1 roter Lauf: `node --import tsx --test tests/lesezeichenHinweis.test.ts` bricht ab, `not ok 1 - tests/lesezeichenHinweis.test.ts`, Fehler `Error [ERR_MODULE_NOT_FOUND]: Cannot find module '/home/nathanael/repos/_ttb-wt-lesezeichen/bot/dashboard_v2/src/utils/browserErkennung'` (Modul fehlt noch).
- 2026-09-05 M1 grün: Modul `src/utils/browserErkennung.ts` gebaut, Testliste in package.json ergänzt. `node --import tsx --test tests/lesezeichenHinweis.test.ts` -> tests 8, pass 8, fail 0.
- 2026-09-05 M2 grün: `LesezeichenHinweis.tsx` gebaut, in `InternalHomeLanding` vor der WelcomeTour gemountet, WelcomeTour hat Prop `startErlaubt` (Default true) und `resetWelcomeTour` löscht zusätzlich `lesezeichen-hinweis-erledigt`. `npm run build` -> built ok. `npm run lint` -> nur 1 Fehler, vorbestehend und außerhalb des Scopes (`src/hooks/dashboardProfileCache.ts:17` no-useless-assignment, identisch auf origin/main); keine Fehler aus den geänderten Dateien.
