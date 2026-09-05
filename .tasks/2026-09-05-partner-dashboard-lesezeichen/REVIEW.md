# Review: Lesezeichen-Hinweis und Name "Partner Dashboard"

status: erledigt
datum: 2026-09-05
contract: CONTRACT.md
ergebnis: FREIGABE

## Urteil

FREIGABE. Alle acht REQ sind erfüllt, alle fünf INV gehalten, der Scope ist sauber,
228/228 Tests grün, Build grün, Lint auf den geänderten Dateien ohne Fehler. Es gibt
nur drei geringfügige Anmerkungen ohne Merge-Blocker.

## REQ-Prüfung

- REQ1 erfüllt. Home mountet den Hinweis vor der Tour
  (bot/dashboard_v2/src/pages/InternalHomeLanding.tsx:472), die Tour startet erst bei
  `startErlaubt` (bot/dashboard_v2/src/components/onboarding/WelcomeTour.tsx:150,197).
  Merker im localStorage unter neuem Schlüssel
  (bot/dashboard_v2/src/components/onboarding/LesezeichenHinweis.tsx:12,89,103), X,
  Escape und "Erledigt" schließen (LesezeichenHinweis.tsx:107,116,188,248).
  "Tour neu starten" räumt den Merker mit
  (bot/dashboard_v2/src/components/onboarding/WelcomeTour.tsx:535).
- REQ2 erfüllt. Reihenfolge Brave, Edg/, OPR/, Vivaldi/, Firefox/, Chrome/, Safari/
  in bot/dashboard_v2/src/utils/browserErkennung.ts:50-73, Brave über den Prop aus
  `navigator.brave.isBrave()` (LesezeichenHinweis.tsx:51-62). Edge, Opera und Vivaldi
  werden vor Chrome geprüft, obwohl alle "Chrome/" tragen; Firefox trägt kein
  "Chrome/". Korrekt.
- REQ3 erfüllt. Adressleisten-Nachbildung mit pulsierendem Symbol links bzw. rechts
  (LesezeichenHinweis.tsx:143,205-214), Karte oben auf der Symbolseite mit Pfeil nach
  oben (LesezeichenHinweis.tsx:146-150,177-184).
- REQ4 erfüllt (Desktop). kbd-Elemente Strg+D bzw. ⌘+D und Knopf "Link kopieren" mit
  "Kopiert"-Bestätigung (LesezeichenHinweis.tsx:220-241). Siehe Gering-1 zur mobilen
  Ausblendung.
- REQ5 erfüllt. Mobil zentrierte Karte unten ohne Pfeilkarte, Android- und
  iOS-Menütext (browserErkennung.ts:91-107, LesezeichenHinweis.tsx:141,146-147).
- REQ6 erfüllt. Unbekannt oder Erkennung noch offen: Fallback rechts oben ohne
  Positionsangabe, nur Tastenkombi und Link kopieren
  (LesezeichenHinweis.tsx:17-22,140-150; browserErkennung.ts:141-148).
- REQ7 erfüllt, alle sechs Stellen. index.html:7 Titel, Header.tsx:101 Badge,
  InternalHomeLanding.tsx:587 Home-Knopf, InternalHomeLanding.tsx:607 Zugriffstext,
  PricingTour.tsx:365 Knopf, dictionary.ts:79 Backlink samt englischer Übersetzung.
  Zusatzsuche nach "Twitch Analytics", "Analyse Dashboard", "Analyse-Dashboard" und
  "Analytics Cockpit" im Quelltext: nur noch ein Vorkommen in
  src/pages/SocialMediaAdmin.tsx:21, das ist ein bestehender Kommentar außerhalb des
  Scopes und in REQ7 nicht genannt.
- REQ8 erfüllt. Alle neuen sichtbaren Texte über t(), Einträge im dictionary
  (dictionary.ts:79,445-466), echte Umlaute, keine Em-Dashes, Nutzersprache.
  'Schließen' ist als Bestandsschlüssel vorhanden (dictionary.ts:232).

## INV-Prüfung

- INV1 gehalten. WelcomeTour-Schlüssel `welcome-tour-dismissed` unverändert,
  PricingTour- und AnalyticsTour-Schlüssel nicht angefasst. Nur der Tour-Start hängt
  jetzt an `startErlaubt`; onComplete-Kette (completionLabel "Zur Abo-Seite",
  Entfernen von pricing-tour-dismissed und -pending), Escape und Step-Logik bleiben
  identisch (WelcomeTour.tsx:150,194-213,472-482).
  Die vier Zustände wurden durchgespielt: kein toter Zustand und kein gleichzeitiges
  Erscheinen. Merker gesetzt und Tour offen: Hinweis meldet beim Mount `onErledigt`
  (LesezeichenHinweis.tsx:93-95), Tour startet danach. Merker frei und Tour bereits
  weggeklickt (Bestandsnutzer): Hinweis erscheint einmal, Tour startet nicht.
  Beide gesetzt: nichts. Beide frei: erst Hinweis, dann Tour.
- INV2 gehalten. Keine Rust-, Backend-, Migrations- oder Endpoint-Änderung.
- INV3 gehalten. `npm test` 228/228 grün, `npm run build` grün, Lint der geänderten
  Dateien ohne Fehler (nur bestehende dismissTour-exhaustive-deps-Warnungen in
  WelcomeTour und PricingTour, die schon auf origin/main stehen). Siehe Gering-3.
- INV4 gehalten. Keine neuen Code-Kommentare in den geänderten oder neuen Dateien.
- INV5 gehalten. browserErkennung.ts ist rein, nimmt ein Eingabeobjekt, kein
  DOM-Zugriff, läuft unter node:test (tests/lesezeichenHinweis.test.ts).

## Browser-Erkennung im Detail

- Leerer UA: kein Match, browser 'unbekannt', Fallback greift (browserErkennung.ts:72).
- navigator.brave ohne isBrave: else-Zweig, abschliessen(false)
  (LesezeichenHinweis.tsx:63-65).
- Abgelehntes Promise und Timeout: beide führen zu abschliessen(false), Timer wird im
  then/catch gelöscht (LesezeichenHinweis.tsx:52-62). Feuert der Timeout zuerst und
  löst das Promise danach, überschreibt der echte Wert harmlos den Fallback.
- Hook läuft einmalig (Dependency [] in useAnleitung, LesezeichenHinweis.tsx:70), die
  aktiv-Sperre verhindert setState nach Unmount.
- iOS-Chrome (CriOS) und iOS-Firefox (FxiOS) tragen kein "Chrome/", werden aber über
  iPhone in erkenneMobil zu 'ios' und bekommen ohnehin den Mobil-Text
  (browserErkennung.ts:41-42,100-107). iPadOS-Safari meldet sich als Macintosh ohne
  iPad-Kennung und landet als Safari-macOS mit ⌘+D; das ist ein sinnvolles Ergebnis,
  kein Fehler.

## Regressionstest-Abdeckung

tests/lesezeichenHinweis.test.ts deckt alle im Contract genannten Fälle ab: Brave
links Stern, Chrome/Edge/Firefox/Vivaldi rechts Stern, Opera rechts Herz, Safari
Teilen mit ⌘+D, Android Menütext mit Stern, iOS Home-Bildschirm, macOS ⌘+D gegen
Windows Strg+D, plus unbekannter Browser ohne Positionsangabe. Die Erkennung wird
zusätzlich pro Marke per erkenneBrowser().browser geprüft (Test 49-68).
tests/partnerDashboardName.test.ts sichert die sechs Umbenennungen und die
Mount-Reihenfolge als Formtest ab.

## Scope

Alle geänderten Dateien liegen im erlaubten Bereich (bestätigt per
git diff --name-status). Keine Datei in rust/, admin_dashboard/ oder website/. Keine
bestehende Testdatei geändert, nur die Testliste in package.json erweitert.

## Gering (kein Blocker)

1. LesezeichenHinweis.tsx:220,234. Auf Mobil werden Tastenkombination und "Link
   kopieren" ausgeblendet (`!istMobil`). REQ4 sagt "immer sichtbar, unabhängig vom
   Browser", REQ5 gibt für Mobil eine reine Textkarte vor. Die spezifischere REQ5
   überstimmt hier; die Ausblendung ist nachvollziehbar (kein Strg+D auf dem Handy).
   Wenn REQ4 wortwörtlich gelten soll, den Kopier-Knopf auch mobil zeigen. Fixvorschlag
   optional: Kopier-Knopf aus der `!istMobil`-Bedingung herausnehmen.
2. LesezeichenHinweis.tsx:130-138. Ohne HTTPS oder ohne Fokus wirft
   navigator.clipboard, der Fehler wird sauber abgefangen (kein Crash), aber der Nutzer
   bekommt keine Rückmeldung. Fixvorschlag optional: bei Fehler kurz einen Hinweis
   statt stiller Rücksetzung anzeigen.
3. bot/dashboard_v2 npm run lint schlägt wegen eines bestehenden Fehlers in
   src/hooks/dashboardProfileCache.ts (no-useless-assignment) fehl. Der Fehler steht
   identisch auf origin/main und liegt außerhalb des Scopes; dieser Diff bringt keinen
   neuen Lint-Fehler. Kein Merge-Blocker für diese Aufgabe, aber INV3 ist auf der
   Baseline nicht erfüllt und gehört separat behoben.

## Nachtrag 2026-09-05

Zwei Amendments am Contract lösen die Mobil-Abweichung aus Fund 1 sauber auf:

- REQ4/REQ5: Mobil entfällt zusätzlich die Tastenkombination (kein Strg+D auf dem
  Telefon), der Knopf "Link kopieren" bleibt aber auch mobil sichtbar. Umgesetzt in
  LesezeichenHinweis.tsx: der Kopier-Knopf steht nicht mehr in der `!istMobil`-Bedingung,
  der `<span />`-Platzhalter ist raus. Die Tastenkombination bleibt hinter `!istMobil`.
- REQ2: Die reine Erkennungsfunktion wertet jetzt das Feld `mobile` aus
  `navigator.userAgentData.mobile` zusätzlich zur User-Agent-Zeichenkette aus; ist eines
  von beiden mobil, gilt mobil. Umgesetzt in browserErkennung.ts (`erkenneMobil` bekommt
  `mobile` und liefert bei gesetztem Signal `android`). Neue Tests in
  lesezeichenHinweis.test.ts: Desktop-Chrome-UA mit `mobile: true` liefert die
  Mobil-Anleitung (Position menue), Android-UA mit `mobile: false` bleibt mobil.

Die Sichtprüfung liegt als Screenshots unter `screens/` vor, v1 (brave, chrome, opera,
safari, android) und v2 (v2-brave, v2-chrome, v2-safari).
