# Pricing-Umbau: Free, Plus, Pro

status: abgestimmt mit Grok am 2026-08-23 (zwei Runden, Kanal `pricing-718c`)
loest ab: `.tasks/2026-08-09-pricing-premium-umbau/SPEC.md` (Free plus Premium, nie umgesetzt)

## Warum

Drei Preiswahrheiten gleichzeitig im Produkt:

| Ort | Stand heute |
|---|---|
| Landingpage `/streamer/v2/` | Free 0, Netzwerk Plus 4,99, Creator Pro 9,99 |
| Dashboard `/twitch/pricing` | acht Plaene mit Bundle-Baukasten, 1,99 bis 4,99, netto gerechnet, brutto beschriftet |
| Spec vom 09.08. | Free plus Premium 2,99 |

Die Landingpage ist oeffentlich und verspricht bereits drei Stufen. Ein Rueckbau auf zwei
waere Wortbruch, also zieht das Dashboard nach.

## Katalog

```
free   Netzwerk Free    0 EUR fuer immer
plus   Netzwerk Plus    4,99 / Monat   49,90 / Jahr
pro    Creator Pro      9,99 / Monat   99,90 / Jahr
```

Alles Endpreise, Kleinunternehmer nach Paragraph 19 UStG, kein Umsatzsteuerausweis.

**Rechenfehler auf der Landingpage, entschieden am 2026-08-23:** dort stand "39,99 im Jahr,
zwei Monate geschenkt". 4,99 mal 12 sind 59,88, zwei Monate geschenkt sind 49,90. Die 39,99
waren acht Monatspreise, also vier geschenkte Monate. **Der Preis wird korrigiert, nicht der
Text:** Plus 49,90, Pro 99,90 (9,99 mal 10). Zwei Monate geschenkt heisst zwei Monate.
Bereits umgesetzt in `website/src/data/networkPage.ts`.

## Feature-Schnitt

Die Trennlinie in einem Satz: Free zeigt dir deinen letzten Stream, Plus zeigt dir deine
Entwicklung.

**Nachtrag 2026-08-23, Entscheidung des Nutzers:** es gibt noch **kein Clip-Werkzeug als
Produkt**. Was existiert, ist der `!clip`-Befehl im Chat; alles andere (Fetch, Upload,
Warteschlange, Auto-Posting, Wasserzeichen) ist interner Unterbau und nichts davon steht
noch in einem nutzersichtbaren Text. Deshalb sind alle Clip-Zeilen aus den Feature-Listen
verschwunden, in `catalog.rs`, auf der Landingpage und hier. Verkauft wird nichts, was es
nicht gibt.

Damit bleibt bei **Creator Pro** nur der Vorrang uebrig. Die Stufe bleibt im Katalog
sichtbar, ist aber **nicht buchbar**: `BillingPlan::buchbar = false` klemmt
`checkout_available` und den Checkout-Einstieg, die Karte zeigt den ruhigen Zustand
"Bald buchbar". Wer Clips baut, nimmt die Zeilen danach wieder auf und setzt `buchbar`
zurueck, nicht vorher.

Die Kontingent-Logik im Code bleibt trotzdem bestehen (Zaehlung ueber die Aufnahme in
unsere DB, fremde Zuschauer-Clips zaehlen nicht). Sie ist Vorbereitung und sperrt heute
niemanden aus einem beworbenen Feature aus, weil kein Text mehr ein Kontingent verspricht.

**Free** bleibt vollwertig und wird nicht kuenstlich beschnitten.

- Auto-Raid in beide Richtungen
- Kompletter Chat-Schutz
- Alle Chat-Befehle
- Go-Live-Post im Community-Discord
- Overlay-Builder und Sendeplanung
- Tagesform des letzten Streams

**Plus (4,99)**

- Voller Verlauf statt nur letzter Stream, Zeitraumvergleiche, Wachstum
- KI-Analyse, KI-Chat, Coaching, KI-Wochenreport
- Werbefreier Chat, Raid-Vorrang, Lurker-Erinnerung, eigener Bot-Name

**Pro (9,99), sichtbar aber nicht buchbar**

- Alles aus Plus
- Vorrang bei Support und neuen Funktionen

Ausdruecklich **nicht** im Katalog: White-Label und API. Das ist ein Bot fuer deutsche
Deadlock-Streamer, keine Plattform mit Wiederverkaeufern.

## Reihenfolge

Serverseitige Paywall zuerst, Design danach. Eine Verkaufsseite vor einer umgehbaren Paywall
verkauft nichts: heute pruefen 4 von 107 Handlern den Plan.

### M1 Serverseitige Gates

Ein Praedikat `plan_stufe(streamer)` in `tb-analytics`, alle offenen Handler fragen es.
Wichtig: ohne Stufe liefert der Verlaufsendpunkt **das letzte Stream-Fenster statt 403**,
damit Free nicht kaputt aussieht.

Stop-Regel: ein Endpunkt, der im Frontend gesperrt aussieht, aber per direktem API-Aufruf
antwortet, gilt als nicht erledigt.

### M2 Katalog auf drei Stufen

Brutto-Betraege, neue Lookup-Keys, Jahrespreis als eigener Betrag hinterlegt statt ueber
einen Rabattsatz gerechnet. Alte Plan-IDs bleiben lesbar, sie stehen in der DB.

### M3 Trial 14 plus 14

14 Tage Plus automatisch beim ersten Login, danach einmalig weitere 14 einloesbar. Heute
sind es 30 Tage einmalig, gestartet auf Knopfdruck. Zweite Stelle nicht vergessen:
`trial_period_days` in der Stripe-Checkout-Session.

### M4 Bestandsmigration

- Der eine Jahreszahler wird Plus, Ablaufdatum unveraendert. Er bekommt mehr als vorher.
- Zwei unbefristete Admin-Geschenke werden unbefristetes Plus.
- Laufende Trials laufen als Plus-Trial bis zu ihrem Datum weiter.
- Abgelaufene Trials und alle ohne Plan werden Free.

### M5 Steuerliche Auszeichnung

"inkl. MwSt." verschwindet ueberall. Stattdessen einmal der Pflichthinweis an der
Preisangabe, im Checkout und auf der Rechnung. Stripe auf tax behavior `inclusive`.

### M6 Verkaufsflaeche

Eine Seite, kein Vergleichsraster.

- Oben seine eigene Wachstumskurve aus der DB, hinter der Sperre unscharf, mit geschwaerzten
  Zahlen ("47 Tage, Durchschnitt, Peak"). Echte Daten, keine Platzhaltergrafik.
  Die Unschaerfe bleibt dezent.
- Darunter genau eine Karte mit Plus, Jahrespreis vorausgewaehlt, ein Knopf, darunter klein
  "jederzeit kuendbar".
- Pro als eine ausklappbare Zeile darunter, nicht als dritte Saeule.
- Free taucht als Karte gar nicht auf, der Betrachter hat es schon.
- Gesperrte Karten im Dashboard bleiben sichtbar und oeffnen bei Tipp ein Sheet mit dem
  Preis, statt auf diese Seite zu springen.
- Kein Empfohlen-Badge, keine Minus-Kreuze.

### M7 Landingpage angleichen

Erledigt am 2026-08-23: `website/src/data/networkPage.ts` steht auf 49,90 und 99,90, die
Aussage "zwei Monate geschenkt" stimmt jetzt rechnerisch. Bleibt zu pruefen, dass Dashboard
und Checkout dieselben Zahlen zeigen.

### M8 Conversion messbar machen

Drei Zaehler in der eigenen DB, kein Werkzeug von aussen: Sperre gesehen, Sheet geoeffnet,
Checkout gestartet.

## Offen, braucht den Nutzer

Stripe-Preise anlegen und die bestehende Subscription umhaengen. Der Stripe-Zugang liegt
nicht im Claude-Scope.
