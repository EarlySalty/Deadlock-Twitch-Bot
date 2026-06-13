# Stream-Coaching- & Sprach-Audit

## Worum es geht

Der Stream-Coaching-Audit ist ein internes Admin-Werkzeug, mit dem das Team eine ausdrücklich freigegebene Twitch-Aufnahme auf problematische Sprache prüft — vor allem auf Slurs und ähnlich heikle Äußerungen. Es entsteht ein privates, belegbares Protokoll mit Zeitstempeln, das als Grundlage für ein Coaching-Gespräch oder die Prüfung eines Partner-Kandidaten dient. Das Werkzeug moderiert nichts automatisch, verhängt keine Sanktionen und postet nichts öffentlich.

## Was der Bot tut

- Nimmt von einer freigegebenen Quelle einen Ausschnitt auf — entweder ein fertiges Twitch-VOD oder einen kurzen, zeitlich begrenzten Mitschnitt eines laufenden Live-Streams.
- Verarbeitet ausschließlich den Ton (kein Video) und wandelt ihn in Text um (Transkription).
- Durchsucht das Transkript zweistufig nach auffälligen Stellen: zuerst über lokale, sehr treffsichere Regeln, danach optional über eine zusätzliche KI-Kontextprüfung, die weichere Grenzfälle einordnet.
- Erstellt ein privates Protokoll, in dem jede Fundstelle mit Zeitpunkt und — bei VODs — einem direkten Sprunglink auf genau die Stelle hinterlegt ist.
- Stellt das Ergebnis nur bei tatsächlichen Funden zu: eine private Nachricht ans Team. Gibt es nichts zu melden, bleibt es still.
- Maskiert heikle Begriffe für die breiter sichtbaren Wege; nur das streng private Admin-Protokoll enthält den Wortlaut im Klartext, damit ein Fund nachprüfbar bleibt.
- Räumt die Roh-Audio- und Zwischendateien nach dem Lauf wieder weg; dauerhaft gespeichert wird nur das private Protokoll.

## Wann es passiert

- Ein Lauf startet nur, wenn die Aufnahme ausdrücklich als autorisiert markiert wurde. Ohne diese Freigabe passiert nichts.
- Es gibt zwei typische Auslöser: ein manueller Audit eines bestehenden VODs (z. B. zur Nachbesprechung nach dem Stream) und eine Live-Beobachtung, die einen laufenden Stream in kurzen Abschnitten mitschneidet und fortlaufend prüft.
- Im Live-Modus wird jeweils ein kurzes Fenster aufgenommen, sofort transkribiert und geprüft; neue Funde tauchen dadurch mit geringer Verzögerung auf.
- Ist ein zu beobachtender Kanal gerade offline, wartet die Beobachtung von selbst auf den nächsten Live-Start, statt abzubrechen.
- Beim Start einer Live-Beobachtung geht eine kurze, private Status-Nachricht ans Team, damit klar ist, dass die Überwachung läuft.
- Es gibt zusätzlich einen Archiv-Weg, bei dem ein Mitschnitt bzw. VOD privat zur automatischen Untertitelung abgelegt und das Ergebnis dann geprüft wird; von der Plattform zensierte Stellen werden dabei lokal nachgeprüft, damit der echte Wortlaut im Protokoll landet.

## Was Streamer/Viewer sehen

- Zuschauer im Twitch-Chat sehen davon nichts. Es wird nichts in den öffentlichen Chat geschrieben und nichts öffentlich veröffentlicht.
- Streamer und reguläre Zuschauer bemerken im laufenden Betrieb in der Regel nichts; es handelt sich um eine interne Prüfung, kein sichtbares Feature.
- Sichtbar ist das Ergebnis nur für das Team: ein privates Protokoll und — nur bei Funden — eine private Nachricht mit den Fundstellen, Zeitpunkten und ggf. Sprunglinks.

## Grenzen & Sonderfälle

- Das Werkzeug ergreift keine automatischen Maßnahmen. Es bannt, verwarnt oder sanktioniert niemanden — es liefert nur Belege für eine menschliche Entscheidung.
- Transkription und KI-Einordnung können sich irren (verhörte Wörter, falsch eingeordneter Kontext). Jede Fundstelle ist als Hinweis zu verstehen und muss am echten Kontext der Aufnahme manuell gegengeprüft werden.
- Geprüft wird ausschließlich gesprochene Sprache aus dem Ton. Bildinhalte, Texteinblendungen oder reiner Chat-Text sind nicht Gegenstand dieser Prüfung.
- Stille ist das erwartete Normalergebnis: kein Fund bedeutet keine Nachricht. Ausbleibende Meldungen heißen also nicht, dass etwas defekt ist.
- Im Live-Modus wird in Abschnitten geprüft; eine Äußerung genau an einer Abschnittsgrenze kann theoretisch ungünstig fallen. Für die belastbare Auswertung dient ohnehin die VOD-/Archiv-Prüfung mit Sprunglinks.
- Die exakte Logik, ab wann eine Stelle als auffällig gilt, ist bewusst nicht im Detail dokumentiert. Die lokalen Regeln sind auf hohe Treffsicherheit ausgelegt; die zusätzliche KI-Prüfung fängt weichere Grenzfälle ab.

## Datenschutz

- Ein Audit läuft nur auf ausdrücklich freigegebenen Aufnahmen — die Autorisierung ist Pflicht und kein Standardverhalten.
- Es wird kein vollständiges Rohtranskript dauerhaft aufbewahrt. Gespeichert wird nur das private Protokoll mit den Fundstellen; die Roh-Audio- und Zwischendateien werden nach dem Lauf entfernt.
- Heikle Begriffe werden für die breiter sichtbaren Ausgabewege maskiert. Den unmaskierten Klartext-Beleg gibt es ausschließlich im streng privaten Admin-Protokoll, das zugriffsbeschränkt abgelegt ist.
- Die Übertragung von Ton oder Text an externe Dienste (für Transkription oder die zusätzliche KI-Prüfung) ist nicht automatisch aktiv, sondern muss pro Lauf gesondert freigegeben werden. Standardmäßig läuft die Transkription lokal.
- Protokolle sind privat und nicht zum Teilen gedacht.

## Häufige Fragen

**Schreibt der Bot etwas in meinen Twitch-Chat, wenn er etwas findet?**
Nein. Das Werkzeug postet grundsätzlich nichts in den öffentlichen Chat und veröffentlicht nichts öffentlich. Funde gehen ausschließlich als private Nachricht ans Team.

**Werde ich automatisch gebannt oder verwarnt, wenn etwas gefunden wird?**
Nein. Es gibt keine automatische Sanktion. Das Werkzeug sammelt nur Belege mit Zeitstempel; jede Konsequenz entscheidet ein Mensch nach manueller Prüfung.

**Wird mein Stream heimlich überwacht?**
Eine Prüfung läuft nur auf ausdrücklich freigegebenen Aufnahmen. Ohne diese Freigabe startet kein Audit.

**Was passiert mit der Aufnahme und dem Transkript hinterher?**
Die Roh-Audio- und Zwischendateien werden nach dem Lauf gelöscht. Ein vollständiges Rohtranskript wird nicht dauerhaft gespeichert — dauerhaft bleibt nur das private Protokoll mit den konkreten Fundstellen.

**Warum kommt manchmal gar keine Meldung?**
Weil nichts Auffälliges gefunden wurde. Kein Fund bedeutet keine Nachricht — Stille ist das erwartete Ergebnis und kein Fehler.

**Wie zuverlässig sind die Funde?**
Sie sind Hinweise, keine endgültigen Urteile. Sprache-zu-Text und KI-Einordnung können sich verhören oder den Kontext falsch deuten. Deshalb wird jede Fundstelle am echten Aufnahme-Kontext manuell gegengeprüft, bevor irgendetwas daraus folgt.

**Sieht jemand den genauen Wortlaut einer Fundstelle?**
Für die breiter sichtbaren Wege werden heikle Begriffe maskiert. Den unmaskierten Klartext gibt es nur im streng privaten Admin-Protokoll, damit ein Fund nachprüfbar bleibt — und das ist nicht zum Teilen bestimmt.

**Funktioniert das nur für fertige VODs oder auch live?**
Beides. Es kann ein bestehendes VOD nachträglich geprüft werden (mit direkten Sprunglinks auf die Stellen) oder ein laufender Stream in kurzen Abschnitten fortlaufend mitgeprüft werden. Ist der Kanal gerade offline, wartet die Live-Beobachtung von selbst auf den nächsten Stream-Start.
