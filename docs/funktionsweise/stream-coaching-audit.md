# Stream-Coaching-Audit

## Worum es geht

Der Stream-Coaching-Audit ist ein internes Admin-Werkzeug. Es hört bei den
Streams der eigenen Leute mit, sucht im gesprochenen Wort nach problematischen
Stellen und legt dazu ein privates Protokoll mit Zeitstempeln an — als
Grundlage für ein Coaching-Gespräch. Das Werkzeug moderiert nichts, verhängt
keine Sanktionen und postet nichts öffentlich.

## Was der Bot tut

- Er fragt jede Minute bei Twitch ab, welcher der eingetragenen Kanäle gerade
  sendet, und nimmt jeden davon in Blöcken von zwei Minuten mit — parallel,
  nicht nacheinander. Kurze Blöcke, weil die Spracherkennung mit anderen
  Funktionen des Bots geteilt wird.
- Aufgenommen wird live, nicht aus dem VOD: ob ein Kanal seine VODs behält,
  entscheidet der Kanal, und ein Audit, das darauf baut, fällt still aus.
- Die Aufnahme wird auf demselben Rechner in Text umgewandelt. Der Ton
  verlässt die Maschine nicht.
- Das Transkript läuft zweistufig durch die Prüfung: zuerst drei feste Regeln
  für eindeutige Fälle, danach ein Modellschritt über **alle** Segmente des
  Blocks — nicht nur die ohne Reizwort. Er läuft in Stapeln von 20 Segmenten;
  über eine Stapelgrenze hinweg sieht das Modell keinen Zusammenhang. An das Modell gehen
  geschwärzte Ausschnitte mit anonymer Nummer, nicht der Kanalname und nicht
  die Stream-ID.
- Pro Block entsteht ein Protokoll auf der Platte und, wenn es etwas zu melden
  gibt, eine kurze private Nachricht an den Admin.
- Aufnahmen mit Fund bleiben liegen, damit jemand nachhören kann. Saubere
  Blöcke werden gelöscht.

## Wann es passiert

- Der Dienst läuft dauerhaft unter systemd; es gibt keinen manuellen Aufruf
  mehr und keinen VOD- oder Datei-Modus.
- Aufgenommen wird nur, solange ein Kanal sendet, höchstens sechs Stunden
  Sendungszeit je Sendung.
- Ist ein Kanal offline, wartet der Dienst auf den nächsten Live-Start.
- Kommt die Auswertung nicht hinterher — 180 wartende Blöcke, also sechs
  Stunden Ton —, startet keine
  neue Aufnahme, bis der Rückstand abgebaut ist.

## Was Streamer und Zuschauer sehen

- Nichts. Es wird nichts in den Chat geschrieben und nichts veröffentlicht.
- Sichtbar ist das Ergebnis nur für den Admin: das Protokoll auf der Platte und
  bei Funden eine private Nachricht.

## Grenzen und Sonderfälle

- Keine automatischen Maßnahmen. Das Werkzeug liefert Belege, entschieden wird
  von Menschen.
- Transkription und Modell können sich irren. Jede Fundstelle ist ein Verdacht
  und muss am echten Kontext geprüft werden.
- Geprüft wird nur gesprochene Sprache. Bild, Einblendungen und Chat sind nicht
  Gegenstand.
- Keine Meldung heißt in aller Regel: nichts gefunden. Ein Abschnitt ohne
  gesprochenes Wort (Musik, Spielton, Pause) ist normal und wird nicht
  gemeldet; bleiben aber 20 Abschnitte am Stück stumm, meldet sich der Dienst.
  Gemeldet wird außerdem, wenn der Modellschritt ausfiel, ein Block aufgegeben
  wurde, die Aufnahme wegen Rückstands pausiert oder die Twitch-Abfrage
  scheitert. Nimmt Discord über Stunden keine Nachricht an,
  bleibt der Befund im Protokoll auf der Platte stehen und wird weiter
  angeboten — dann ist Stille kein Beweis für einen ruhigen Stream.
- An Blockgrenzen kann eine Äußerung ungünstig fallen. Die Zeitstempel im
  Protokoll zählen ab Sendungsbeginn, damit die Stelle im VOD wiederzufinden
  ist, solange es das VOD gibt.

## Datenschutz

- Der Ton verlässt den Rechner nicht. Zeigt die Transkriptions-URL nach außen,
  startet der Dienst gar nicht erst, außer das wurde ausdrücklich erlaubt.
- An das Modell gehen Transkriptausschnitte, vorher durch die Schwärzung
  geschickt. Die Schwärzung kennt die bekannten Muster und sonst nichts:
  anderer Wortlaut geht mit. Deshalb ist der fremde Anbieter eine bewusste,
  einzeln gesetzte Entscheidung.
- Jeder Beleg im Protokoll läuft durch die Schwärzung und trägt eine Prüfsumme
  des Originals. Die Schwärzung kennt die bekannten Muster und sonst nichts:
  anderer Wortlaut aus demselben Abschnitt steht im Protokoll. Es ist deshalb
  eine zugriffsbeschränkte Akte, kein zitatfreier Text. Die private Nachricht
  enthält weder Zitat noch Prüfsumme.
- Ein vollständiges Rohtranskript wird nur gespeichert, wenn das ausdrücklich
  eingeschaltet ist. Standard ist: nicht speichern.
- Aufnahmen und Protokolle werden nach der eingestellten Frist gelöscht
  (Standard 30 Tage). Dateien liegen nur für den eigenen Benutzer lesbar.
- Ein aufbewahrter Mitschnitt enthält Bild und Ton, weil er die Twitch-Spur so
  speichert, wie sie kommt. Geprüft wird ausschließlich der Ton.

## Häufige Fragen

**Schreibt der Bot etwas in meinen Chat, wenn er etwas findet?**
Nein. Funde gehen ausschließlich als private Nachricht an den Admin.

**Werde ich automatisch gebannt oder verwarnt?**
Nein. Es gibt keine automatische Sanktion.

**Was passiert mit der Aufnahme hinterher?**
Ein sauberer, vollständig geprüfter Block wird gelöscht. Gibt es einen Fund
oder blieb die Prüfung unvollständig, bleibt die Aufnahme bis zum Ablauf der
Aufbewahrungsfrist liegen, damit jemand nachhören kann.

**Warum kommt manchmal gar keine Meldung?**
Weil nichts gefunden wurde. Ein Ausfall der Prüfung meldet sich dagegen
ausdrücklich.

**Wie zuverlässig sind die Funde?**
Es sind Hinweise, keine Urteile. Sprache-zu-Text und Modell können sich
verhören oder Kontext falsch deuten.

**Funktioniert das auch für fertige VODs?**
Nein, nicht mehr. Geprüft wird ausschließlich live mitgeschnittenes Material.
