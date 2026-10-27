# Kommunikationsaudit Twitch-Bot und Streamer-Oberflächen

## Auftrag vom 7. Oktober 2026

Alle für Streamer und Zuschauer sichtbaren Twitch-Bereiche auf verständliche Kommunikation prüfen. Neue Streamer müssen erkennen, wofür eine Funktion da ist, was sie tut, wann sie selbst handelt und wie sie sich steuern oder ausschalten lässt. Konkrete Probleme mit Fundstellen dokumentieren und zu einer gemeinsamen Entscheidungsgrundlage bündeln. Noch keine Änderungen an Produkt, Texten, Konfiguration oder Betrieb.

## Ausdrücklich gewünschte Richtung

- Auto-Raids verständlich erklären: Teilnahme am eigenen Netzwerk und Bezug zu Deadlock. Nicht ungeprüft behaupten, dass die gegenwärtige Implementierung ausschließlich innerhalb des Netzwerks raidet.
- Partnerschaft soll auch ohne Unterstützung des Raid-Netzwerks möglich werden. Wer nicht teilnimmt, sendet keine automatischen Netzwerk-Raids und bekommt daraus keine Unterstützung. Dies ist der neue Nutzerwunsch, keine Behauptung zum Istzustand. Welche weiteren Netzwerkleistungen mit „Unterstützung“ gemeint sind, bleibt zur gemeinsamen Besprechung offen.
- Der Werbemanager braucht eine verständliche Erklärung seines Zwecks und seiner Wirkung.
- Den gesamten Twitch-Bereich untersuchen, nicht nur die zwei Beispiele.

## Grundlage und Grenzen

Unveränderter Snapshot von origin/main: 9315b3cf7e4a6feeffc32b603db26eca78b03a2c. Lesepfad: /tmp/twitch-kommunikationsaudit-20261007. Der kanonische Checkout ist stark verändert und liegt hinter origin/main; diese Änderungen gehören nicht zum Auditstand und werden nicht angefasst. Den produktiven Stand gesondert feststellen, wo ohne Zustandsänderungen möglich. Quellcodeprüfung ist kein Live-Nachweis.

Keine Produktänderungen, Commits, Pushes, Deploys, Neustarts, Nachrichtenversand, Datenbankänderungen, Authentifizierung oder Berechtigungsänderungen. Keine Secrets lesen. Keine weiteren Agenten durch Worker. Keine Code-Kommentare schreiben. Nur eigene Berichtsdatei bearbeiten.

## Prüfraster

Je Funktion: Zweck, Zielgruppe, Voraussetzungen, Auslöser, automatisches Verhalten, Grenzen, Folgen von Ein/Aus, Rückweg, Kosten und Datennutzung soweit relevant. Ganze Nutzerwege und ausgelieferte Einstiege prüfen. Sichtbaren Text gegen tatsächliche Rust-Implementierung prüfen; Python ist nur historische Referenz. Erst Graphify, dann gezielte Code-Lektüre. Nicht erreichbare oder historische Oberflächen klar ausweisen.

Je Befund: ID, Priorität, Route/Nutzerweg, sichtbarer Wortlaut, wahrscheinliches Missverständnis, belegtes Istverhalten mit Datei und Zeile, konkrete Korrekturrichtung, Einordnung als Textproblem/Bedienproblem/Verhaltensänderung/offene Produktfrage, Evidenzgrad. Vorschläge nicht als beschlossene Umsetzung formulieren.

## Pakete

A: Auto-Raid, Partnerschaft, Teilnahme, Berechtigungen und Rücknahme.
B: Werbemanager, Twitch-Werbesteuerung, Monetarisierung und angrenzende Werbebegriffe.
C: Einstieg, Landingpage, Onboarding, Hilfe und allgemeine Dashboard-Funktionen einschließlich Analyse und Schutz. Keine tiefe Raid-/Werbemanagerprüfung.
D: Bot-Nachrichten, Chatbefehle, Social Media, Clips, VOD und Uplink-Einstiege. Uplink nur in der Twitch-Oberfläche, kein umfassendes Relay-Audit.

Berichte: BEFUND-A.md bis BEFUND-D.md. Hauptsession konsolidiert in BERICHT.md einschließlich Abdeckung, Priorisierung und Gesprächspunkten. Fertigmeldungen an die Hauptsession, keine Rückfragen direkt an den Nutzer.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: hauptbaum
