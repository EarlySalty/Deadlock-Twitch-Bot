# Paket B: Werbemanager und Werbebegriffe

Rechercheagent: a9c777bb168901063. Bericht von der Hauptsession aus der Agentenübergabe übernommen. Snapshot `9315b3cf7e4a6feeffc32b603db26eca78b03a2c`, keine Live-Ausführung. P1: falsche Steuerungs-, Schutz- oder Kaufentscheidung möglich. P2: erhebliches Verständnisproblem.

## Fundstellen

Alle Pfade relativ zum Repository, gelesen unter `/tmp/twitch-kommunikationsaudit-20261007/`.

- UI: `bot/dashboard_v2/src/components/verwaltung/AdManagerSection.tsx`
- Texte: `bot/dashboard_v2/src/components/verwaltung/adManagerTexte.ts`
- API: `rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs`
- Logik: `rust/crates/tb-analytics/src/ad_manager.rs`
- Worker: `rust/bin/tb-bot/src/ad_manager_wiring.rs`

## B01: Zweck und Zuständigkeiten fehlen am Einstieg

P2, `/twitch/verwaltung#werbung` und Rundgang. Sichtbar sind „Intelligenter Werbemanager“, „Konfiguration“ und „Budget“. Es fehlt eine unmittelbar sichtbare Erklärung, dass Twitch-Videowerbung gesteuert wird, was der Bot selbst startet und was weiterhin bei Twitch eingestellt wird. Strategiebeschreibungen liegen überwiegend im Tooltip.

Belege: UI:488,565,573-595,645-675; Logik:554-605; `bot/dashboard_v2/src/components/onboarding/OnboardingGuide.tsx:13`.

Korrektur: Zweck vor die Einstellungen, Twitch-Plan und eigenes Budget erklären, Community-Einladungen abgrenzen, zur passenden Twitch-Einstellung führen. Text/Bedienung, hohe Belegstärke.

## B02: Ungespeicherter Schalter erscheint als Live-Zustand

P1, Hauptschalter. „Aktiv“, „Aus“ und „Aus. Der Bot schaut nur zu.“ werden aus dem lokalen Entwurf angezeigt. Der Schalter ändert zunächst `draft`; die API wird erst beim Speichern aufgerufen. Die laufende Automatik kann deshalb trotz sichtbarem „Aus“ eingeschaltet bleiben.

Belege: UI:368-380,466-470,490-508,730-744; Worker:273.

Korrektur: Gespeicherten Betriebszustand und ungespeicherten Entwurf getrennt anzeigen. Sofortiges Schalten wäre eine zusätzliche Verhaltensentscheidung. Bedienung/Verhalten, hohe Belegstärke.

## B03: Manuelle Werbung folgt nicht dem beworbenen Matchschutz

P1, Abschnitt „Von Hand“. Neben „Match schützen & Queue nutzen“ und „Aus. Der Bot schaut nur zu.“ steht „90 Sek. Werbung“. Der manuelle Knopf ist unabhängig vom Hauptschalter verfügbar, wenn Live-Status und Twitch-Recht passen. Match- und Startschutz der Automatik werden im manuellen Ausführungspfad nicht angewandt; eigene Prüfungen wie die Twitch-Sperrzeit bleiben bestehen.

Belege: UI:461-462,828-835; API:461-520; Worker:548-558,575-698.

Korrektur: Den eigenständigen Werbestart direkt am Knopf erklären. Gemeinsam entscheiden, ob im Match eine Bestätigung oder Sperre gelten soll. Text/Bedienung/Entscheidung, hohe Belegstärke.

## B04: „Mindestabstand“ verspricht eine nicht angewandte Einstellung

P1, Feineinstellungen. „Gilt für Twitch-Pausen und für Werbung, die du selbst startest.“ Die Einstellung `min_interval_minutes` wird validiert und gespeichert, im geprüften Entscheider und Worker aber nicht zur Abstandsprüfung herangezogen. Manuelle Werbung prüft stattdessen acht Minuten. Das Verschieben der Twitch-Werbung orientiert sich an Fälligkeit und verfügbaren Pausen.

Belege: UI:683-691; Logik:161-163,507-519,954-984; Worker:639-651.

Korrektur: Gewünschte Abstandsregel implementieren oder die Einstellung entfernen. Ein Tooltip-Fix reicht nicht. Verhaltensänderung/Entscheidung, hohe Belegstärke nach Referenzprüfung im Steuerungspfad.

## B05: Entscheidungen erscheinen als ausgeführte Werbung

P1, Live-Status und Verlauf. „Werbung in der Queue gestartet“, „Gerade läuft ein Werbeblock“ und „Werbung gestartet“ entstehen teilweise aus einer Entscheidung vor der Ausführung. Ein geplanter Start kann danach zurückgehalten werden. „Letzte Bot-Aktion“ nennt den Start unabhängig vom Ergebnis; Erfolg oder Fehler stehen im Tooltip.

Belege: Texte:7-45; Worker:421-446,456-464,507-570; UI:170-181,796-800.

Korrektur: Geplant, angefragt, bestätigt, fehlgeschlagen und unklar getrennt anzeigen. Laufende Werbung aus bestätigtem Beginn und Dauer ableiten. Bedienung/Verhalten, hohe Belegstärke.

## B06: „Verschoben“ kann ohne Verschiebung bei Twitch erscheinen

P1, Matchschutz und Verlauf. „Verschoben, dein Match läuft“ beziehungsweise „Verschoben, kurze Ruhe nach dem Match“ übersetzen eine interne Postpone-Entscheidung. Ohne verfügbare Twitch-Pausen löst diese Entscheidung keinen Twitch-Aufruf aus. Twitch kann seinen geplanten Block weiterhin ausspielen.

Belege: Logik:524-539; Worker:545; Texte:15-21.

Korrektur: Einen zurückgehaltenen eigenen Werbestart von einer bestätigten Verschiebung bei Twitch unterscheiden. Fehlende Twitch-Pausen samt Konsequenz sichtbar machen. Text/Bedienung, hohe Belegstärke.

## B07: Twitch-Überschrift über Vorschau aus dem eigenen Budget

P1, Budget. Unter „Twitch-Werbungs-Manager“ erscheint „… Blöcke à … Sek., etwa alle … Min.“ Die Quelle wird bei vorhandener nächster Twitch-Werbung als Twitch bezeichnet. Die angezeigten Planwerte berechnet der Worker jedoch aus dem eigenen gespeicherten Stundenbudget. Auch im passiven Modus bleibt eine Blockvorschau sichtbar.

Belege: API:253-265; Worker:356-364; UI:653-656; Logik:507-519.

Korrektur: Twitch-Plan und eigene Budgetberechnung trennen, unbekannte Mengen als unbekannt kennzeichnen, im passiven Modus keine aktive Verteilung suggerieren. Bedienung/Verhalten, hohe Belegstärke.

## B08: Chat-Hinweis klingt umfassender als seine Abdeckung

P2. „Der Bot schreibt kurz vor einer Werbung eine Zeile in den Chat, wenn der Werbemanager aktiv ist.“ Tatsächlich unterdrückt die Logik Hinweise unter anderem bei laufendem oder unbekanntem Matchstatus. Ohne Twitch-Plan bestehen weitere Einschränkungen. Der manuelle Ausführungspfad besitzt keinen entsprechenden Versand.

Belege: Logik:660-717; Worker:400-412 und manueller Ausführungspfad.

Korrektur: Abdeckung konkret benennen. Entscheiden, ob auch manuelle und unvermeidbare Twitch-Blöcke angekündigt werden sollen. Text/Verhalten/Entscheidung, hohe Belegstärke.

## B09: Bezahltes „Werbefrei“ verspricht zu viel

P1, Chat nach Bot-Timeout und Preisangebot. „Werbefrei-Abo“, „alle Bot-Features ohne automatische Nachrichten“ beziehungsweise „Werbefreier Chat, Raid-Vorrang und Lurker-Erinnerung“ können das Ende sämtlicher automatischer Nachrichten oder Twitch-Videowerbung suggerieren. Der Katalog verkauft hingegen `chat.promos.disable`, also eine konkrete Promo-Freigabe. Werbehinweise haben einen eigenen Schalter.

Belege: `rust/crates/tb-chat/src/moderation.rs:83-87`; `rust/bin/tb-bot/src/chat_wiring.rs:2003-2011`; `rust/crates/tb-analytics/src/billing/catalog.rs:115-134`; `rust/crates/tb-chat/src/promos.rs:3001-3041`; Logik:660-667.

Korrektur: Den tatsächlichen Leistungsumfang benennen, etwa keine automatischen Community-Einladungen, sofern genau das gewünscht ist. Von Twitch-Videowerbung und anderen Bot-Hinweisen abgrenzen. Text/Leistungsentscheidung, Codebeleg hoch; tatsächlicher Versand und Checkout nicht live geprüft.

## B10: Budgetbilanz vermischt Zeiträume und Entscheidungen

P1, Verlauf mit „Blöcke“, „Im Fenster“ und „Budget“. Als Zähler werden Commercial-Entscheidungen der gesamten Session summiert, als Nenner ein Stundenbudget angezeigt. Auch bei Twitch als Quelle stammt der Nenner aus der eigenen Einstellung. Manuelle Aktionen liegen in einem anderen Speicher.

Belege: Logik:1367-1377,1451; UI:864-876.

Korrektur: Bestätigte Ausspielungen zählen, Zeitraum benennen und Zähler wie Nenner auf dieselbe Grundlage bringen. Eigene, manuelle und Twitch-geplante Werbung unterscheiden. Verhalten/Bedienung, hohe Belegstärke.

## B11: Gespeicherte Werbedaten wirken aktuell

P2, Analyse, Tab „Monetization“. „Preroll-frei“, „verbleibend“ und „Snooze“ zeigen den neuesten gespeicherten Datensatz ohne Altersgrenze als aktuelle Restzeit und Vorrat. Der Erfassungszeitpunkt allein macht die Werte nicht aktuell.

Belege: `rust/crates/tb-dashboard-api/src/handlers/ads_schedule.rs:61-91`; `bot/dashboard_v2/src/pages/Monetization.tsx:221-243`.

Korrektur: Stand der Erfassung und veraltete Daten kennzeichnen. Begriffe erklären, etwa Werbung beim Betreten des Streams und verfügbare Fünf-Minuten-Verschiebungen. Text/Bedienung, statischer Beleg hoch; Auftreten alter Daten nicht live gemessen.

## Abdeckung

Geprüft: Einstieg, Rundgang, beide Strategien, Hauptschalter, Budget, Feineinstellungen, manuelle Aktionen, Steam-Abhängigkeit, Chat-Hinweise, Verlauf, Ausführung, Werbezeitplan und Werbefrei-Verkaufsbegriffe. Routen in `rust/crates/tb-dashboard-api/src/lib.rs:910-920`, Workerstart in `rust/bin/tb-bot/src/main.rs:1633-1649`.

Nicht geprüft: angemeldete Browserzustände, echte Twitch-Rechte, gespeicherte Produktionseinstellungen, reale Werbeausspielungen und Produktions-SHA. Keine Tests oder externen Aktionen. Eine Textkorrektur allein reicht insbesondere bei B04, B07 und B10 nicht.
