# Konzept: Streamer-Landing v3 nach dem Nvidia-Muster

status: Entwurf (2026-09-17)

## Anlass

Rückmeldung zur aktuellen Seite (`/streamer`, intern v2): zu viele Erklär- und Funktionsblöcke, "ich habe schon genug Bots, erst recht wenn ich nicht ständig Deadlock spiele", dazu Misstrauen beim Thema Daten. Der Betreiber will mittelfristig Geld verdienen, das Netzwerk lebt aber von vielen Partnern.

## Was Nvidia gemacht hat

Nvidia hat nicht die Hardware geändert, sondern die Kategorie. Dasselbe Silizium wanderte über eine Software-Schicht (CUDA) aus dem Bucket "Grafikkarte für Gamer" in den Bucket "Accelerated Computing", später "AI Factory".

| Jahr | Schritt | Wort dafür |
|---|---|---|
| 1999 | GeForce 256 | "die erste GPU", Kategorie erfunden statt Produkt verkauft |
| 2006 | CUDA und Tesla | Grafikchip wird Rechenplattform, "Accelerated Computing" |
| 2012 bis 2016 | Deep-Learning-Welle | fremder Erfolg (AlexNet auf GeForce) wird eigener Beweis |
| 2016 | DGX-1 | "AI supercomputer in a box", persönlich an OpenAI übergeben |
| 2017 | GeForce-EULA | Rechenzentrum per Lizenz verboten, die Preislinie ist juristisch, nicht technisch |
| 2021 | Omniverse | aus Rendering wird "Digital Twin", Zielgruppe Industrie |
| 2024 | "AI Factory" | Rechenzentrum wird Produktionsanlage statt IT-Kosten |
| 2025 | Quadro wird RTX PRO | Profi-Linie sichtbar weg von Gaming |

Mechaniken dahinter:

1. Kategorie umbenennen statt Produkt verbessern. Wer die Kategorie benennt, bestimmt den Vergleichsmaßstab.
2. Plattform statt Produkt. CUDA ist der Ort, an dem Kunden ihre eigene Arbeit ablegen. Gebunden wird über das, was die Leute dort aufgebaut haben, nicht über die Karte.
3. Beweis über Namen (OpenAI, BMW), nicht über Zahlenversprechen.
4. Sprache wechseln: fps, Benchmarks und "Karte" abgelegt, Fabrik, Token und Durchsatz eingeführt.
5. Gleiches Silizium, drei Preise. Getrennt wird über Lizenz, Support und Freischaltung, nicht über Transistoren.

Quellen: developer.nvidia.com/cuda, Forbes 2025-03-23 "What Is AI Factory", datacenterdynamics.com zur GeForce-EULA, en.wikipedia.org/wiki/Quadro, en.wikipedia.org/wiki/Nvidia_RTX.

## Übertragung auf uns

Neue Kategorie: nicht "Bot", sondern das Partner-Netz der deutschen Deadlock-Streamer. Der Bot ist nur der Anschluss, so wie CUDA nur der Zugang zur Rechenleistung war. Damit greift "ich habe schon genug Bots" nicht mehr: ein weiterer Bot ist ersetzbar, ein Anschluss ans Netz nicht.

Unser CUDA ist die gemeinsame Zuschauer- und Vertrauensschicht: eingehende Raids, Platz im Netz, gemeinsame Sperrliste, Partner-Ruf. Wer geht, verliert keine Software, sondern den Zuschauerstrom.

Wörter ablegen: Bot (als Hauptwort), Funktionen, Dashboard, Analytics, Restreaming, Automatik, verbinden.
Wörter einführen: Netz, Partner, Anschluss, Zuschauer weiterreichen, gemeinsame Sperrliste, deine Zuschauer bleiben deine, läuft neben deinen Bots.

Deadlock-Frage offen beantworten: der Anschluss gilt immer, weitergereicht wird nach Deadlock-Streams. Wer nur ab und zu Deadlock streamt, gehört trotzdem dazu.

Kostenlos und bezahlt aus derselben Infrastruktur:

- Kostenlos bleibt die Mitgliedschaft: Raid-Netz, Live-Ankündigung, Schutz, Clips. Das Netz ist so viel wert wie die Zahl seiner Live-Partner, eine Bezahlschranke am Eingang würgt es ab.
- Bezahlt wird später die Sendetechnik: ein Upload, der Server verteilt an Twitch, YouTube und Kick (Uplink), dazu die Clip-Ausspielung auf Social Media. Trennlinie ist verbrauchte Serverleistung und persönliche Freischaltung. Diese Linie ist spielunabhängig und damit der eigentliche Geldweg.
- Auf der Seite v3 stehen keine Preise und keine Upgrade-Hinweise.

Exklusivität entsteht über die Aufnahme durch den Betreiber, nicht über den Preis.

## Seitenaufbau v3

Fünf Sektionen plus Community-Block, gebaut aus den bestehenden `partner-clean`-Komponenten im v1-Look (kein Neu-Design, keine luftigen Protocol-Layouts).

1. Hero: Leitsatz, großes Live-Embed, echte Partnerzahl, Knopf "Partner werden". Direkt am Knopf drei Klartext-Zeilen zum Vertrauen.
2. Das Netz live: Live-Partner groß, alle Partner ausklappbar (bestehendes `PartnerNetwork`).
3. So wandern Zuschauer: gekürzter Raid-Erklärer, dazu echte Netz-Zahlen, soweit die öffentliche API sie liefert. Keine erfundenen Zahlen.
4. "Läuft neben deinen Bots": ein Satz dazu, dass nichts ersetzt wird, ein Satz zur Deadlock-Frage, die sechs Vorteile als Chips hinter "Mehr anzeigen". BanFeed, ClipManager und Community entfallen als eigene Sektionen.
5. Vertrauen in Klartext: was der Bot darf, was er nicht kann, was gespeichert wird, wie man in einem Klick rauskommt. Der Hex-Blob und "AES-256-GCM" wandern hinter den Link zum Sicherheitskonzept. Jede Aussage gegen den Code geprüft.
6. Community-Block: "Du streamst nicht selbst? Schlag jemanden vor." Andockpunkt siehe AUFTRAG.
7. Abschluss-Knopf.

Hero-Leitsätze zur Auswahl:

1. Kein zweiter Bot. Dein Anschluss ans deutsche Deadlock-Netz.
2. Deutsche Deadlock-Streamer reichen sich ihre Zuschauer weiter. Du kannst dabei sein.
3. Allein streamen ist die schwerste Variante. Hier reicht dich ein ganzes Netz weiter.

Gebaut wird mit Leitsatz 1, die anderen beiden stehen als Konstante zum schnellen Tausch bereit.

## Community findet und pitcht Streamer

Idee des Betreibers: die Community schlägt Streamer vor und wirbt für das Netz.

Rahmen aus den bestehenden Regeln: kein automatisches Outreach, der Betreiber entscheidet über jede Aufnahme, Identität über die Twitch-User-ID, keine zweite Kandidaten-Ablage neben dem Scout-Bestand.

Richtung: ein Vorschlag aus der Community landet als Kandidat mit Quelle "Community" im bestehenden Scout-Bestand, der Betreiber sieht ihn in seinem Report und entscheidet. Wird der Vorgeschlagene Partner, bekommt der Vorschlagende sichtbare Anerkennung im Discord. Der Bau dieses Wegs ist ein eigenes Paket nach dem Vorcheck, v3 verlinkt zunächst nur den vorhandenen Weg.
