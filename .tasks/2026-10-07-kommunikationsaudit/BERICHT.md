# Kommunikationsaudit Twitch-Bot

Stand: 7. Oktober 2026. Vier Rechercheagenten haben 44 Befunde und Klärungspunkte dokumentiert, hier zu Änderungspaketen gebündelt. Keine Produktänderungen vorgenommen.

## Urteil

Das Problem ist an mehreren Stellen größer als unklare Formulierungen. Der Einstieg verspricht sehr viel Eigenständigkeit, erklärt deren Grenzen aber schlecht. Manche Schalter und Statusanzeigen vermitteln sogar einen anderen Zustand als den, nach dem der Bot handelt. Diese Fälle brauchen neben besserem Text eine Korrektur der Bedienung oder des Verhaltens.

Öffentlich live bestätigt sind unter anderem „Der Bot managt ihn ab dann komplett“, „Volle Kontrolle“ sowie die Behauptung, Titeländerungen seien technisch unmöglich. Der geprüfte Code enthält dagegen Titeländerungen nach Zusatzfreigabe und keinen getrennten Raid-Teilnahmeschalter.

## Auto-Raid: Deine gewünschte Richtung und der heutige Stand

| Punkt | Deine gewünschte Richtung | Befund im geprüften Stand |
| --- | --- | --- |
| Partnerschaft | Auch ohne Teilnahme am Raid-Netzwerk möglich | Der angebotene Ausstieg trennt den Bot und beendet die Partnerschaft. |
| Gegenseitigkeit | Ohne Teilnahme keine automatischen Netzwerk-Raids senden oder daraus empfangen | Quellen und Empfänger werden unterschiedlich geprüft. Ein abgeschalteter Sendeweg verhindert den Empfang nicht automatisch. |
| Zielgruppe | Raids innerhalb unseres Netzwerks | Kategorieziele außerhalb des Netzwerks sind eingebunden. Ein Förderpfad kann sogar vor der Partnerauswahl greifen. |
| Spielbezug | Auf Deadlock begrenzt und verständlich erklärt | Auch eine kurze Just-Chatting-Phase nach Deadlock ist vorgesehen. „Solange du live bist“ ist andererseits zu weit. |
| Kontrolle | Vorher wissen, was der Bot selbst tut, und die Teilnahme bewusst wählen | „Volle Kontrolle“ steht neben einem fehlenden getrennten Ausschalter. Ein Statusbefehl berücksichtigt nicht alle tatsächlichen Pausen. |

Belege und Zustandsmatrix: [Paket A](BEFUND-A.md), insbesondere A-01 bis A-06. Die neue Teilnahmeoption ist eine Verhaltensänderung, kein bloßes Umbenennen eines vorhandenen Schalters. Ein Ausschluss muss auch alternative Auswahlpfade berücksichtigen.

## Vorrangige Änderungspakete

Die Priorisierung ist ein Vorschlag für unsere Besprechung, kein Umsetzungsauftrag.

| Reihenfolge | Bereich | Was Streamer heute missverstehen können | Was sich ändern muss | Art und Belege |
| --- | --- | --- | --- | --- |
| 1 | Raid-Teilnahme | „Ich kann Partner bleiben und nur die Raids abschalten.“ | Partnerschaft und beidseitige Raid-Teilnahme trennen; Senden und Empfang verständlich anzeigen. | Produkt/Verhalten, A-01/A-02 |
| 1 | Raid-Ziele | „Der Bot raidet ausschließlich Netzwerkpartner.“ | Gewünschte Netzwerkgrenze durchsetzen und Hilfen mit der Auswahl in Einklang bringen. | Produkt/Verhalten/Text, A-03/A-04 |
| 1 | Freigaben und Vertrauen | „Ich gebe nur Raids frei; Titel kann der Bot technisch gar nicht ändern.“ | Tatsächliche Zwecke der Basisrechte und bewusste Zusatzfreigaben erklären. Falsche Ausschließlichkeitsbehauptungen entfernen. | Text, A-05/C-01 |
| 1 | Ausschalten und Speichern | „Der Schalter steht auf Aus, also ist es aus.“ | Gespeicherten Betrieb und lokalen Entwurf bei Werbung und Titeln getrennt zeigen. Aktionen dürfen nicht unerwartet nach der alten Einstellung handeln. | Bedienung/Verhalten, B02/C-06 |
| 1 | Werbung tatsächlich verschoben? | „Twitch hat die Werbung verschoben oder schon gestartet.“ | Planung, Anfrage, Bestätigung und Fehler unterscheiden. Ohne Twitch-Pausen keinen erfolgreichen Aufschub behaupten. | Status/Text, B05/B06 |
| 1 | Schutz vor Eingriffen | „Dieser Schalter warnt nur.“ | Mögliche Timeouts erklären; zusätzliche Gesprächsprüfung von den übrigen Schutzfunktionen unterscheiden. | Text/gegebenenfalls Produkt, C-04/C-05 |
| 1 | Werbe-Mindestabstand | „Mein eingestellter Abstand wird eingehalten.“ | Einstellung anwenden oder entfernen. Im geprüften Pfad wird sie gespeichert, aber nicht als Abstand verwendet. | Verhalten, B04 |
| 1 | „Werbefrei“ | „Danach kommen keine automatischen Nachrichten oder keine Twitch-Werbung mehr.“ | Den gekauften Umfang konkret benennen und Community-Einladungen von Twitch-Videowerbung trennen. | Leistungsbeschreibung, B09 |
| 1 | Veröffentlichungen stoppen | „Automatik aus oder Konto getrennt heißt: Es wird nichts mehr gepostet.“ | Neue Planung von bereits geplanten Posts unterscheiden. Eine mögliche Übernahme durch ein gemeinsames Konto vor der Trennung erklären und einen echten Stopp anbieten. | Bedienung/Produkt, D-07/D-09 |
| 1 | Privatsphäre und Hilfe | „Die KI ignoriert mich vollständig“ oder „Die Hilfefrage bleibt flüchtig“. | Reichweite des Ignorierbefehls erklären; Speicherung und mögliche Weitergabe von Hilfedialogen direkt am Eingabefeld nennen. | Text/Datennutzung, D-03/D-04 |
| 1 | Archivstatus | „Archiv aus“ sei ein sicher gelesener Zustand. | Lesefehler als unbekannt anzeigen, nicht als ausgeschaltete Archivierung. | Statusverhalten, D-11 |
| 2 | Werbemanager-Einstieg | „Was ist das überhaupt, und steuert es mehr Werbung?“ | Vor dem ersten Schalter Zweck, Twitch-Zuständigkeit, eigenen Werbestart und Grenzen erklären. | Text/Bedienung, B01/B03/B07/B08 |
| 2 | Titelgenerator | „Dieser Titel wurde veröffentlicht.“ | Fehlende Freigabeprüfung, Setzfehler und nachträgliche Bearbeitung korrekt anzeigen. | Statusverhalten, C-07/C-08/C-09 |
| 2 | Pausen und Trennung | „Wie lange bleibt es aus, verliere ich etwas, holt ein Login den Bot zurück?“ | Endzeit, automatische Rückkehr, Empfangsfolgen und erneutes Verbinden getrennt erklären. | Text/Status, A-06/A-07/A-08/C-12 |
| 2 | Zahlen und Erfolg | „Die Zahl ist aktuell und gemessen.“ | Ersatzwerte, Zeiträume und veraltete Werbedaten kennzeichnen; bestätigte Ausführung statt bloßer Entscheidungen zählen. | Darstellung/Verhalten, B10/B11/C-10 |
| 2 | Hilfe finden | „Unter Hilfe erfahre ich, warum der Bot gerade etwas tut.“ | Einrichtung, Befehle, Automatik-Erklärungen und Problemlösung miteinander verbinden. | Navigation, C-11 |
| 2 | Clip-Befehl und Fehler | „Mein Clip trägt diesen Titel; Warten repariert fehlende Rechte.“ | Tatsächlichen Clip-Erfolg bestätigen und zwischen Zuschauer-, Streamer- und vorübergehenden Problemen unterscheiden. | Text/Verhalten, D-01/D-02 |
| 2 | Automatische Freigabe | „Einspruch“ habe andere garantierte Schutzwirkung als „Vollautomatik“. | Unterschied der Modi definieren oder zusammenführen. Freigabe ausdrücklich auf Veröffentlichung beziehen, vorbereitenden Download erklären. | Produkt/Text, D-08/D-13 |
| 2 | VOD und Verbindungen | „Es werden nur zukünftige Streams direkt auf YouTube gesichert; verbunden gilt überall.“ | Vorhandene Aufzeichnungen, lokale Zwischenablage und Sicherungsstatus erklären. Verbindungen nach Livestream oder Upload benennen. | Text/Bedienung, D-10/D-12 |
| 2 | KI-Hilfe | „Die Antwort kann nichts erfinden; Schweigen heißt, die Frage wurde nicht erkannt.“ | Keine Fehlerfreiheit garantieren. Bei ausdrücklichen Fragen einen sparsamen Ausfallhinweis oder Hilfeweg vorsehen. | Text/Verhalten, D-05/D-06 |
| 2 | Schutzversprechen | „Spam sieht niemand, weil er vorher abgefangen wird.“ | Tatsächliche Reaktion auf eingehende Nachrichten erklären, keine garantierte Unsichtbarkeit versprechen. | Text, C-02 |
| 2 | Verbindungsstatus | „Vollständig“ bedeute, sämtliche Funktionen seien geprüft. | Funktionsbezogene Freigaben und einen nicht prüfbaren Zustand unterscheiden. | Text/Status, C-03 |

## Was die Erklärung einer Funktion leisten soll

Direkt am Bedienort reichen oft wenige klare Antworten:

- Wofür ist das da? Zum Beispiel: Wann Twitch-Videowerbung laufen soll.
- Was macht der Bot selbst, und wann? Auslöser und sichtbare Folge nennen.
- Was entscheidest du? Voraussetzung, Freigabe und Wirkung des Schalters zeigen.
- Was bleibt bei „Aus“ bestehen? Andere Funktionen, laufende Vorgänge und Empfang getrennt erklären.
- Ist die Änderung schon wirksam? Entwurf, gespeichert, ausgeführt und fehlgeschlagen unterscheiden.

Weitere Details können dahinterliegen. Diese Grundinformationen gehören weder ausschließlich in einen Tooltip noch in eine lange FAQ. Unterschiedliche Bereiche dürfen für denselben Zustand nicht unterschiedliche Erklärungen liefern.

## Noch gemeinsam zu klären

Die gewünschte Partnerschaft ohne Raid-Teilnahme steht als Richtung fest. Offen bleiben die Grenzen:

1. Bezieht sich „keine Unterstützung“ auf automatische Netzwerk-Raids oder auch auf Sichtbarkeit, Live-Ankündigungen und weitere Netzwerkleistungen? Im Auftrag wird keine pauschale Aberkennung anderer Leistungen unterstellt.
2. Soll die kurze Verabschiedung in Just Chatting nach Deadlock weiter dazugehören?
3. Wie soll der Bot-Befehl für einen bewusst manuell angeforderten Raid bei ausgeschalteter Netzwerk-Teilnahme behandelt werden? Manuelle Raids direkt über Twitch sind davon zu unterscheiden.
4. Für Schalter mit verzögertem Speichern: sofort wirksam oder ausdrücklich als Entwurf bis „Speichern“? Der angezeigte Betriebsstatus muss in beiden Fällen stimmen.

Empfehlung: Zuerst das neue Raid-Modell festziehen. Danach die Einstiegs- und Freigabetexte daran ausrichten und irreführende Schalter-/Erfolgsanzeigen getrennt von der reinen Textrunde beheben.

## Abdeckung und Nachweise

Vier Recherchepakete sind abgeschlossen: acht Punkte zu Raids und Partnerschaft, elf zu Werbung, zwölf zum allgemeinen Dashboard und dreizehn zu Bot-/Medienkommunikation. Das sind 44 Einzelbefunde und Klärungspunkte, keine Behauptung über 44 bereits in Produktion beobachtete Fehler. Ähnliche Ursachen wurden oben zusammengeführt.

Geprüft wurden die aktive Landingpage, Anmeldung, Rundgang, Raid-Hilfen, Verwaltung, Werbung, Moderation, Titelgenerator, Analyse, Community, ausgewählte Chatbefehle und Hilfedialoge, Social Media, Clips, VOD und Uplink-Einstiege. Keine vollständige Prüfung jeder Streamer-Benachrichtigung oder des gesamten Relay-Dienstes. Positive Gegenbeispiele sind in den Bereichsberichten festgehalten, etwa die Erklärung stiller Raid-Hinweise, die Community-Leerzustände und Warnungen vor dem Teilen von OBS-Adressen.

- [Auftrag und Grenzen](AUFTRAG.md)
- [Paket A: Auto-Raid und Partnerschaft](BEFUND-A.md)
- [Paket B: Werbung und Werbebegriffe](BEFUND-B.md)
- [Paket C: Einstieg und Dashboard](BEFUND-C.md)
- [Paket D: Bot-Nachrichten, Medien und Uplink-Einstiege](BEFUND-D.md)
- [Live-Stichprobe](LIVE-STICHPROBE.md)
- [Register](REGISTER.md)

Die Agenten prüften einen festen `origin/main`-Snapshot: `9315b3cf7e4a6feeffc32b603db26eca78b03a2c`. Die Hauptsession hat die Kernbefunde zu Raid-Auswahl, Empfängerprüfung, Rechten, Werbeaufschub, Titelautomatik, Upload-Zugang und Archivstatus am Code gegengeprüft sowie öffentliche Landingpage-Texte über HTTP bestätigt. Die 44 Befundüberschriften wurden gezählt; Quellpfade, Zeilenbereiche und lokale Berichtslinks waren bei der abschließenden Prüfung gültig. Beschriebene Missverständnisse sind fachliche Einschätzungen, keine Ergebnisse einer Nutzerstudie.

Keine angemeldete Browserprüfung: Der Browser-Host war nicht verfügbar, geschützte Seiten führten zur Anmeldung. Keine Bot-Nachrichten versendet und keine echten Kontoaktionen ausgelöst. Der lesbare Release-Zeiger lag auf `e0b0dbaf662d7680c4ceaa210bf15f1443693cd8`; die Zuordnung des laufenden Prozesses war nicht lesbar. Quellbefunde sind deshalb nicht pauschal als Live-Fehler zu verstehen. Fremde Änderungen im kanonischen Checkout blieben unangetastet.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 6 belegt | Senke: interne Audit-Akte
