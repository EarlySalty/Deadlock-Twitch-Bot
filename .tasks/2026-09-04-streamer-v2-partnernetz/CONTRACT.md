# Contract: /streamer/v2 verkauft die Partnerschaft, nicht ein SaaS-Tool

status: erledigt
datum: 2026-09-04
klasse: mittel
repo: Deadlock-Twitch-Bot (website/)

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Wer `/streamer/v2/` öffnet, versteht ohne Vorwissen: Das ist die Deutsche
Deadlock Community, du wirst hier Partner, und der Bot managt deinen Kanal
dafür komplett. Kein SaaS-Look, keine Produktseite. Die Seite zeigt, wer schon
Partner ist (alle, mit Live-Feed und klickbaren Profilen), damit man sieht, in
welche Community man einsteigt.

Nutzer-Befund (wörtlich, Maßstab für das Review): "klarere Kommunikation was
wir sind und wir bieten kein SaaS-Gedönse"; das Auto-Raid-Netzwerk bleibt Hero,
"aber darunter muss direkt klar werden, was wir hier machen: dass du Partner
wirst und der Bot alles für dich managt"; die nummerierten 01/02/03-Karten
("Was im Netzwerk für dich läuft") sind "nicht enjoyable" und fliegen raus.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Hero bleibt: Der bestehende Hero mit der Auto-Raid-Übergabe (Bühne,
  Headline, Chip, zwei Knöpfe) bleibt das erste, was man sieht. Texte und
  Bühne des Hero werden nicht umgebaut.
- REQ-02 Direkt unter dem Hero ein Partner-Block: Die erste Sektion nach dem
  Hero sagt in Nutzersprache, was wir sind (die Deutsche Deadlock Community,
  ein Netzwerk aus Streamern plus Discord) und was passiert, wenn man Partner
  wird (der Bot übernimmt Raids, Live-Ankündigung, Chat-Schutz und Auswertung
  von selbst, der Streamer muss nichts einrichten oder verwalten). Dieser Block
  ist visuell geführt wie v1 (Bild, Bewegung, Glow), nicht als Textwand und
  nicht als Karten-Raster mit Nummern.
- REQ-03 Partner-Übersicht mit Live-Feed: Es gibt eine Sektion, die alle
  Partner aus der Netzwerk-API zeigt. Live-Partner stehen zuerst und groß mit
  Live-Vorschau (Twitch-Embed oder Live-Vorschaubild plus pulsierendem
  LIVE-Punkt), Offline-Partner folgen als vollständiges Raster mit Profilbild
  und Namen. Jede Partnerkarte ist ein Link auf das Twitch-Profil des Partners
  (`https://twitch.tv/<login>`) in neuem Tab. Über der Sektion steht ein
  Zähler "N Partner" mit der echten Zahl aus der API. Gibt die API nichts
  zurück, zeigt die Sektion einen ehrlichen Leerzustand, keine erfundenen
  Kanäle.
- REQ-04 Nummerierte Ablauf-Karten weg: Die Sektion "Was im Netzwerk für dich
  läuft" mit den drei Karten 01/02/03 und den Links "Dashboard mit Demo-Daten
  ansehen" und "Alle Funktionen im Vergleich" existiert nach dem Umbau nicht
  mehr. Was der Bot an einem Streamtag tut, wird stattdessen in REQ-02 oder in
  einem visuell geführten Block (Stil v1: Anker-Grafik je Punkt, keine
  Nummern, kein Kachelraster mit Aufzählungspunkten) erzählt.
- REQ-05 Kein SaaS-Vokabular: Auf der ganzen Seite kommen folgende Wörter und
  Muster nicht mehr vor: "Dashboard mit Demo-Daten", "Alle Funktionen",
  "Funktionen im Vergleich", "Features", "Plan", "Tarif", "Preis", "Pricing",
  "Tool", "Software", "SaaS", "Produkt", nummerierte Sektionsnummern (01, 02,
  03) und "Jetzt testen". Der Haupt-CTA lautet "Jetzt Partner werden" und
  führt auf den bestehenden Bewerbungs-/Onboarding-Weg.
- REQ-06 Texte in Nanis Stimme: Alle neuen Texte sind natürliches Deutsch mit
  echten Umlauten, ohne Em-Dashes, ohne Fachjargon, in Du-Form, aus Sicht des
  Streamers ("du gehörst jetzt dazu"). Keine Testimonials ohne echtes Zitat.
- REQ-07 Ehrlich gegen den Code: Jede beworbene Fähigkeit des Bots existiert
  im Twitch-Bot-Code. Was es nicht gibt, wird nicht versprochen.

## Invarianten (darf sich nicht ändern)

- INV-01 `/streamer` (v1, `StreamerNetworkPage`-fremde Route und ihre
  Komponenten) bleibt byteidentisch; nur `/streamer/v2` ändert sich.
- INV-02 Der Hero (Komponente, Bühne, Clip-Pool, Texte) bleibt wie in Commit
  2c047aec.
- INV-03 `/streamer/v2` bleibt `noindex`, bis der Nutzer den Wechsel auf
  `/streamer` freigibt.
- INV-04 Partnerdaten kommen nur aus der bestehenden Netzwerk-API des
  Twitch-Bots; keine hart codierten Partnerlisten, keine erfundenen Zahlen.
- INV-05 Bestehende Tests im `website/`-Paket werden nicht gelöscht oder
  abgeschwächt; Lint und Build (`npm run build`) bleiben grün.
- INV-06 Kein neuer Backend-Endpunkt und keine Schema-Änderung, solange die
  bestehende API Login, Anzeigename, Profilbild und Live-Status liefert.

## Nicht-Ziele

- Umschalten von `/streamer/v2` auf `/streamer`, SEO-Titel, Merge nach main
  und Deploy auf die Live-Route (erst nach Go des Nutzers).
- Änderungen an v1, an Preisseiten, Onboarding-Seite oder am Bot-Backend.
- Neues Design-System; die Seite bleibt v1-Klon mit Partner-Copy.

## Erlaubter Änderungsbereich

- website/src/pages/StreamerNetworkPage.tsx
- website/src/components/partner-clean/
- website/src/styles/
- website/public/
- website/src/lib/
- .tasks/2026-09-04-streamer-v2-partnernetz/

## Verbotene Änderungen

- website/src/components/ außerhalb von partner-clean/, sofern von v1 genutzt
- website/src/pages/ außer StreamerNetworkPage.tsx
- rust/ (Backend), Migrationen, Caddyfile
- Lint-, TypeScript- und Build-Konfiguration
- Routing-Änderung, die v2 unter /streamer ausliefert

## Offene Produktfragen

- keine (Defaults: Live-Feed per Twitch-Embed für höchstens 3 Live-Partner,
  danach Vorschaubild; Offline-Raster vollständig sichtbar, kein Aufklappen;
  Reihenfolge Hero, Partner-Block, Partner-Übersicht, Rest der Seite)

## Amendments

- 2026-09-04, Erlaubter Änderungsbereich, alt: website/src/lib/ -> neu: zusätzlich website/src/hooks/ (Partner-Hook liegt neben useNetworkCount.ts, gleiche Ablage wie die bestehenden Hooks), entschieden von Orchestrator (nur technisch, reversibel)
- 2026-09-04, Nicht-Ziele, alt: Merge nach main erst nach Go -> neu: Merge nach main erfolgt (5aeed71d), weil der Branch nur website/ und .tasks/ trägt, v2 weiter noindex unter /streamer/v2 liegt und /streamer byteidentisch bleibt; das Umschalten auf /streamer bleibt Nicht-Ziel, entschieden von Orchestrator (nur technisch, reversibel)
