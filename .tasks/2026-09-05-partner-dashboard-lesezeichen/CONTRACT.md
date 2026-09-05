# Contract: Lesezeichen-Hinweis und Name "Partner Dashboard"

status: erledigt
datum: 2026-09-05
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: nur `status:` und Anhänge unter `## Amendments`.

## Ziel

Partner, die das Dashboard zum ersten Mal öffnen, bekommen einen Hinweis, sich das
Partner Dashboard als Lesezeichen zu speichern. Der Hinweis erkennt den Browser und
zeigt, wo das Lesezeichen-Symbol in genau diesem Browser sitzt. Das Dashboard heißt
in Tab-Titel und Oberfläche überall "Partner Dashboard" statt "Twitch Analytics"
oder "Analyse Dashboard".

## Anforderungen (user-sichtbares Verhalten)

- REQ1: Beim ersten Öffnen der Home-Route (`/twitch/dashboard`) erscheint der Hinweis
  "Speichere dir dein Partner Dashboard", bevor die WelcomeTour startet. Die
  WelcomeTour beginnt erst, wenn der Hinweis geschlossen wurde. Nach "Erledigt" oder
  Schließen (X, Escape) erscheint er im selben Browser nicht mehr; Merker im
  localStorage, weil Lesezeichen je Browser gelten. "Tour neu starten" in der
  Sidebar setzt auch diesen Merker zurück.
- REQ2: Der Hinweis erkennt den Browser (Brave, Chrome, Edge, Firefox, Opera, Vivaldi,
  Safari, sonst) und nennt Symbol und Position in Worten:
  Brave Desktop: Stern links neben der Adresse.
  Chrome, Edge, Firefox, Vivaldi: Stern rechts in der Adressleiste.
  Opera: Herz rechts in der Adressleiste.
  Safari (macOS): Teilen-Knopf oben rechts, dann "Lesezeichen hinzufügen".
  Brave wird über `navigator.brave.isBrave()` erkannt, Edge über "Edg/", Opera über
  "OPR/", Vivaldi über "Vivaldi/", Firefox über "Firefox/", Safari über "Safari/"
  ohne "Chrome/", Rest mit "Chrome/" ist Chrome.
- REQ3: Die Karte zeigt eine Nachbildung der Adressleiste mit der Dashboard-Adresse
  und dem hervorgehobenen, pulsierenden Symbol an der richtigen Seite (Brave links,
  sonst rechts; Herz bei Opera). Die Karte sitzt am oberen Rand auf der Seite des
  Symbols (links bei Brave, sonst rechts) mit einem Pfeil nach oben Richtung
  Adressleiste.
- REQ4: Immer sichtbar, unabhängig vom Browser: die Tastenkombination Strg + D
  (auf macOS ⌘ + D) als kbd-Elemente und ein Knopf "Link kopieren", der die
  Dashboard-Adresse in die Zwischenablage legt und kurz "Kopiert" bestätigt.
- REQ5: Mobil (Android, iOS): keine Pfeilkarte und keine Adressleisten-Nachbildung,
  sondern eine Karte unten zentriert mit Text. Android: "Menü mit den drei Punkten
  oben rechts, dann Stern". iOS: "Teilen-Symbol, dann Zum Home-Bildschirm".
- REQ6: Unbekannter Browser oder Erkennung noch nicht abgeschlossen: Karte rechts
  oben, nur Tastenkombination und "Link kopieren", keine Positionsangabe.
- REQ7: Umbenennung: `<title>` in index.html wird "Partner Dashboard · Deutsche
  Deadlock Community" (daraus entsteht der Lesezeichen-Name). Header-Badge "Twitch
  Analytics" wird "Partner Dashboard". Knopf "Analyse Dashboard" auf Home wird
  "Zur Analyse" (er führt in den Analyse-Tab). Text "keinen Zugriff auf das
  Analyse-Dashboard" wird "keinen Zugriff auf die Analyse". PricingTour
  "Zum Analyse-Dashboard" wird "Zur Analyse". Backlink "← Analyse-Dashboard" wird
  "← Partner Dashboard" mit englischer Übersetzung "← Partner dashboard".
- REQ8: Alle neuen Texte laufen über `t()` mit Einträgen im dictionary (Deutsch als
  Schlüssel, Englisch als Wert), echte Umlaute, keine Em-Dashes, Nutzersprache
  ohne Fachvokabular.

## Invarianten (darf sich nicht ändern)

- INV1: WelcomeTour, PricingTour und AnalyticsTour behalten ihre localStorage-Schlüssel
  und ihre Reihenfolge Home → Abo-Seite → Analyse. Nur der Start der WelcomeTour
  wartet auf den Hinweis.
- INV2: Keine Backend-Änderung, keine Migration, kein neuer Endpoint, kein Rust.
- INV3: Bestehende Tests aus `npm test` bleiben grün, `npm run lint` und
  `npm run build` laufen sauber durch.
- INV4: Keine Code-Kommentare im neuen oder geänderten Code.
- INV5: Die Browser-Erkennung ist eine reine Funktion ohne DOM-Zugriff, die ein
  Eingabeobjekt (userAgent, brave, platform, mobile) bekommt und unter node:test
  ohne Browser läuft.

## Nicht-Ziele

- Kein PWA-Manifest, kein "Als App installieren".
- Kein serverseitiger Merker je Twitch-User-ID.
- Rechtstexte (AGB nennt "Analyse-Dashboard" als Zusatzleistung) bleiben unverändert.
- admin_dashboard und website bleiben unverändert.

## Erlaubter Änderungsbereich

- bot/dashboard_v2/index.html
- bot/dashboard_v2/package.json
- bot/dashboard_v2/src/utils/browserErkennung.ts
- bot/dashboard_v2/src/components/onboarding/LesezeichenHinweis.tsx
- bot/dashboard_v2/src/components/onboarding/WelcomeTour.tsx
- bot/dashboard_v2/src/components/onboarding/PricingTour.tsx
- bot/dashboard_v2/src/components/layout/Header.tsx
- bot/dashboard_v2/src/components/layout/DashboardSidebar.tsx
- bot/dashboard_v2/src/pages/InternalHomeLanding.tsx
- bot/dashboard_v2/src/i18n/dictionary.ts
- bot/dashboard_v2/tests/lesezeichenHinweis.test.ts
- bot/dashboard_v2/tests/partnerDashboardName.test.ts
- .tasks/2026-09-05-partner-dashboard-lesezeichen/

## Verbotene Änderungen

- rust/
- bot/admin_dashboard/
- website/
- bestehende Testdateien (außer der Testliste in package.json)
- bestehende localStorage-Schlüssel umbenennen

## Regressionstest (vor der Implementierung rot)

`tests/lesezeichenHinweis.test.ts` prüft die reine Erkennungsfunktion: Brave-Eingabe
liefert Position links und Symbol Stern, Chrome/Edge/Firefox/Vivaldi rechts und
Stern, Opera rechts und Herz, Safari macOS Teilen, Android und iOS die Menütexte,
macOS-Plattform liefert ⌘ + D statt Strg + D. Der rote Lauf (Modul fehlt) wird mit
Fehlermeldung in PLAN.md festgehalten.

## Offene Produktfragen

- keine

## Amendments
