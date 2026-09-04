# Contract: Partner-Übersicht auf /streamer/v2 nach Deadlock-Live und Impact gliedern

status: erledigt
datum: 2026-09-04
klasse: mittel
repo: Deadlock-Twitch-Bot (website/)

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Die Sektion "Wer schon dabei ist" auf `/streamer/v2/` zeigt oben nur die bis zu
drei stärksten Partner, die gerade Deadlock streamen, darunter zwei
ausklappbare Listen: weitere Deadlock-Live-Streamer und alle übrigen aktiven
Partner nach Impact, sodass die starken Vorschau-Kanäle oben links und die
kleinen Kanäle unten rechts landen.

Nutzer-Befund (wörtlich): "hier nur Streamer max 3 die Deadlock gerade
streamen", "ausklappbare Liste für die anderen die auch Deadlock streamen",
"sortiert nach höchste Zuschauer also meister Impact", "darunter ausklappbare
Liste mit Leuten die Partner sind, sortiert nach dem meisten Impact also
Streams und Viewer 50/50", "die Liste soll immer die sein die gerade aktiv bei
uns Partner sind".

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Embeds nur für Deadlock-Live: Als große Karten mit Twitch-Embed
  erscheinen höchstens 3 Partner, die live sind UND deren Kategorie aus der
  API "Deadlock" ist. Sortierung: Zuschauer absteigend, Platz 1 oben links.
  Partner, die live sind, aber ein anderes Spiel streamen, bekommen kein
  Embed und keine große Vorschaukarte.
- REQ-02 Ausklappliste "Weitere Deadlock-Streams": Gibt es mehr als 3
  Deadlock-Live-Partner, stehen die übrigen direkt unter den Embeds in einer
  eingeklappten Liste mit Kopfzeile "N weitere streamen gerade Deadlock" und
  Auf-/Zuklapp-Knopf. Sortierung Zuschauer absteigend. Jede Zeile zeigt
  Avatar, Name, LIVE-Punkt, Zuschauerzahl und verlinkt das Twitch-Profil.
  Gibt es keine weiteren, entfällt der Block ganz.
- REQ-03 Ausklappliste "Alle Partner": Darunter eine eingeklappte Liste mit
  Kopfzeile "Alle N Partner" (N = alle Partner aus der API, die nicht schon in
  REQ-01 oder REQ-02 gezeigt werden) und Auf-/Zuklapp-Knopf. Sortierung nach
  Impact-Wert absteigend: Impact = 0,5 × (dlStreams30d / max dlStreams30d
  aller Partner) + 0,5 × (avgViewers30d / max avgViewers30d aller Partner);
  ist ein Maximum 0, zählt der Anteil als 0. Gleichstand: Name alphabetisch.
  Zeile: Avatar, Name, Kennzahlen "N Deadlock-Streams, Ø N Zuschauer" (nur
  wenn > 0), Link aufs Twitch-Profil. Partner, die live in einem anderen
  Spiel sind, stehen hier mit kleinem LIVE-Punkt und Spielnamen.
- REQ-04 Aufgeklappt zeigt jede Liste alle ihre Einträge als Raster (Desktop
  4 Spalten, Mobil 1 bis 2), Reihenfolge zeilenweise von oben links nach unten
  rechts. Der Klappzustand ist reiner Client-State, Standard eingeklappt.
- REQ-05 Zähler und Einleitung im Kopf bleiben: "N Partner" mit der echten
  Gesamtzahl aus der API und der bestehende Einleitungssatz.
- REQ-06 Leerzustände ehrlich: Sind 0 Partner live in Deadlock, entfällt der
  Embed-Bereich und ein Satz sagt "Gerade streamt kein Partner Deadlock. Schau
  später wieder rein." Fehler oder leere API: bestehender Leerzustand bleibt.
- REQ-07 Texte in Nanis Stimme, echte Umlaute, keine Em-Dashes, kein
  SaaS-Vokabular (bestehende FORBIDDEN-Liste im Test bleibt scharf).

## Invarianten (darf sich nicht ändern)

- INV-01 Nur `website/src/components/partner-clean/PartnerNetwork.tsx`,
  `website/src/hooks/useNetworkStreamers.ts`, `website/src/lib/partnerNetwork.ts`
  und `website/tests/` ändern sich; Hero, PartnerPitch, Seitenreihenfolge und
  v1 bleiben byteidentisch.
- INV-02 Daten nur aus `GET /twitch/api/v2/public/network`; der Endpunkt
  filtert bereits auf aktive Partner, keine zweite Quelle, kein neuer
  Endpunkt, keine hart codierten Logins.
- INV-03 Deadlock-Erkennung ausschließlich über das API-Feld `game` gleich
  "Deadlock" (Groß-/Kleinschreibung egal), nie über Titel oder Login.
- INV-04 Ein einziger Fetch pro Seite bleibt (Hook wird einmal in
  `StreamerNetworkPage.tsx` aufgerufen, Props unverändert).
- INV-05 Bestehende Tests werden nicht gelöscht oder abgeschwächt; Build,
  `tsc --noEmit` und `node --test` bleiben grün.
- INV-06 `/streamer/v2` bleibt noindex; `/streamer` unverändert.

## Nicht-Ziele

- Änderungen am Rust-Backend oder an der Netzwerk-API.
- Umschalten auf `/streamer`.
- Änderungen an anderen Sektionen der Seite.

## Erlaubter Änderungsbereich

- website/src/components/partner-clean/PartnerNetwork.tsx
- website/src/components/partner-clean/partnerShared.tsx
- website/src/hooks/useNetworkStreamers.ts
- website/src/lib/partnerNetwork.ts
- website/tests/
- .tasks/2026-09-04-partnernetz-sortierung/

## Verbotene Änderungen

- website/src/pages/, website/src/App.tsx, website/src/components/sections/
- website/src/components/partner-clean/ außer den beiden oben genannten Dateien
- rust/, Caddyfile, website/v2/index.html
- Lint-, TypeScript- und Build-Konfiguration

## Offene Produktfragen

- keine (Defaults: Impact-Normierung gegen das Maximum der Liste; Listen
  standardmäßig eingeklappt; Live-in-anderem-Spiel landet in "Alle Partner"
  mit LIVE-Punkt)

## Amendments

- 2026-09-04, REQ-03, alt: Live in anderem Spiel mit LIVE-Punkt und Spielname -> neu: Nicht-Deadlock-Streams sind irrelevant, solche Partner erscheinen in "Alle Partner" ohne LIVE-Punkt und ohne Spielname, wie offline; Nutzer-Wortlaut "allgemeine Streams sind irrelevant", entschieden von User
- 2026-09-04, Erlaubter Änderungsbereich, alt: PartnerPitch.tsx verboten -> neu: website/src/components/partner-clean/PartnerPitch.tsx nur für den Live-Punkt der Avatar-Laufleiste (Deadlock-Prüfung wie in PartnerNetwork, Merge-Gate-Befund 2), entschieden von Orchestrator (nur technisch, reversibel)
