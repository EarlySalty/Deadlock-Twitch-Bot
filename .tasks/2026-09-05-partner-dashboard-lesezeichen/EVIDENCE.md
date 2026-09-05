# Evidence: Lesezeichen-Hinweis und Name "Partner Dashboard"

status: erledigt
datum: 2026-09-05
contract: CONTRACT.md

## Analoge Implementierungen

- bot/dashboard_v2/src/components/onboarding/WelcomeTour.tsx:6 STORAGE_KEY `welcome-tour-dismissed`, :184 setItem beim Schließen, :194 getItem beim Start, :527 `resetWelcomeTour()` löscht den Schlüssel.
- bot/dashboard_v2/src/components/onboarding/WelcomeTour.tsx:229 Escape schließt die Tour; :420 Portal auf document.body mit framer-motion AnimatePresence.
- bot/dashboard_v2/src/components/banners/TrialBanner.tsx:20 und :37 Dismiss-Muster per localStorage mit ISO-Datum.
- bot/dashboard_v2/src/components/onboarding/AnalyticsTour.tsx:6 `analytics-tour-dismissed`, :7 `analytics-tour-pending` (INV1, nicht anfassen).

## Bestehende Abstraktionen

- bot/dashboard_v2/src/pages/InternalHomeLanding.tsx:465 mountet `<WelcomeTour completionLabel="Zur Abo-Seite" onComplete=…>`; hier hängt der Hinweis davor.
- bot/dashboard_v2/src/components/layout/DashboardSidebar.tsx:15 importiert `resetWelcomeTour`, :269 Knopf "Tour neu starten".
- bot/dashboard_v2/src/i18n/dictionary.ts:79 Eintrag `'← Analyse-Dashboard': '← Analytics dashboard'` (Deutsch ist Schlüssel, Englisch Wert).
- bot/dashboard_v2/src/utils/ enthält reine Hilfsmodule (engagementKpi.ts, monetization.ts) mit eigenen node:test-Dateien in tests/.

## Relevante Tests

- bot/dashboard_v2/package.json Skript `test`: explizite Dateiliste für `node --import tsx --test`; neue Tests müssen dort eingetragen werden.
- bot/dashboard_v2/tests/dashboardShell.test.ts:1-10 Muster für Quelltext-Form-Tests (readFileSync + assert.match).
- bot/dashboard_v2/tests/i18n.test.ts:14-24 Muster für dictionary-Tests über `translate('en', …)`.

## Öffentliche Schnittstellen und Verträge

- bot/dashboard_v2/index.html:7 `<title>Twitch Analytics Cockpit</title>` (wird Lesezeichen-Name).
- rust/crates/tb-dashboard-api/src/lib.rs:1618 liefert `/twitch/dashboard-v2/*path` aus dem gebauten dist; kein Backend-Eingriff nötig.

## Änderungsfläche

- bot/dashboard_v2/src/components/layout/Header.tsx:101 Badge "Twitch Analytics".
- bot/dashboard_v2/src/pages/InternalHomeLanding.tsx:581 Knopf "Analyse Dashboard", :601 Text "Analyse-Dashboard".
- bot/dashboard_v2/src/components/onboarding/PricingTour.tsx:363 "Zum Analyse-Dashboard".
- neu: bot/dashboard_v2/src/utils/browserErkennung.ts, bot/dashboard_v2/src/components/onboarding/LesezeichenHinweis.tsx, bot/dashboard_v2/tests/lesezeichenHinweis.test.ts, bot/dashboard_v2/tests/partnerDashboardName.test.ts.

## Offene Architekturfrage

- keine
