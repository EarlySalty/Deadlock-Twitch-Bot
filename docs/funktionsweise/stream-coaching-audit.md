# Stream-Coaching-Audit

## Worum es geht

Der Stream-Coaching-Audit ist ein internes Admin-Werkzeug. Es hört bei den
Streams der eigenen Leute mit, sucht im gesprochenen Wort nach problematischen
Stellen und legt dazu ein privates Protokoll mit Zeitstempeln an — als
Grundlage für ein Coaching-Gespräch. Das Werkzeug moderiert nichts, verhängt
keine Sanktionen und postet nichts öffentlich.

## Was der Bot tut

- Er fragt jede Minute bei Twitch ab, welcher der eingetragenen Kanäle gerade
  sendet, und nimmt jeden davon in Blöcken von zwei Minuten mit, parallel,
  nicht nacheinander. Beim Start geht eine private Nachricht an den Admin.
  Ausgewertet wird erst, wenn der Stream zu Ende ist, damit die
  Spracherkennung während der Sendung frei bleibt.
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
- Pro Block entsteht ein Protokoll auf der Platte. Die private Nachricht an
  den Admin kommt am Sendungsende und nennt nur Funde, die Twitch ahnden
  würde. Jeder Fund enthält den erkannten Wortlaut, Datum und UTC-Uhrzeit,
  ein ungefähres Stream-Zeitfenster und direkt darunter einen fertigen
  Copy-Paste-Grund für die Twitch-Meldung.
- Aufnahmen mit Fund bleiben liegen, damit jemand nachhören kann. Saubere
  Blöcke werden gelöscht.

## Wann es passiert

- Der Dienst läuft dauerhaft unter systemd; es gibt keinen manuellen Aufruf
  mehr und keinen VOD- oder Datei-Modus.
- Aufgenommen wird nur, solange ein Kanal sendet, höchstens 24 Stunden
  Mitschnitt je Sendung. Die Grenze zählt aufgenommene Zeit, nicht wie lange
  die Sendung schon vor unserem Start lief.
- Ist ein Kanal offline, wertet der Dienst die Aufnahme aus und schickt eine
  Abschlussnachricht mit den ToS-Funden.
- Die Aufnahme läuft weiter, wenn die Auswertung hinterherhinkt. Sie pausiert
  nur, wenn die gespeicherten Mitschnitte 12 GB überschreiten.

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

- Zum Aufschreiben des Gesagten verlässt der Ton den Rechner nicht. Zeigt die
  Transkriptions-URL nach außen, startet der Dienst gar nicht erst, außer das
  wurde ausdrücklich erlaubt.
- Vom Stream selbst wird zusätzlich eine durchgehende Tonaufnahme angelegt.
  Nach dem Stream wandert sie in unser Google Drive, damit später jemand
  nachhören kann. Das passiert nicht sofort, sondern beim nächsten
  Aufräumdurchgang, meist innerhalb weniger Stunden.
- Zusammen mit der Tonaufnahme gehen auch die Protokolle des Streams dorthin.
  Ist das Mitschreiben des Gesagten eingeschaltet, ist der vollständige
  Wortlaut dabei, ungekürzt und ohne Schwärzung. Wer das für seinen Kanal nicht
  will, sagt Bescheid: dann wird weder aufgenommen noch etwas hochgeladen.
- Kommt die Tonaufnahme nicht in unser Google Drive, wird sie auf dem Rechner
  gelöscht: nach der eingestellten Frist plus zwei Wochen Aufschlag, mit dem
  wir einen kaputten Upload noch reparieren können. Liegen bleibt sie nicht.
- An das Modell gehen Transkriptausschnitte, vorher durch die Schwärzung
  geschickt. Die Schwärzung kennt die bekannten Muster und sonst nichts:
  anderer Wortlaut geht mit. Deshalb ist der fremde Anbieter eine bewusste,
  einzeln gesetzte Entscheidung.
- Jeder Beleg im Protokoll läuft durch die Schwärzung und trägt eine Prüfsumme
  des Originals. Die Schwärzung kennt die bekannten Muster und sonst nichts:
  anderer Wortlaut aus demselben Abschnitt steht im Protokoll. Es ist deshalb
  eine zugriffsbeschränkte Akte, kein zitatfreier Text. Die private Nachricht
  enthält den Klartext nur flüchtig in der privaten Admin-Nachricht. Die
  persistente Akte enthält weiter nur das geschwärzte Zitat und die Prüfsumme.
- Ein vollständiges Rohtranskript wird nur gespeichert, wenn das ausdrücklich
  eingeschaltet ist. Standard ist: nicht speichern.
- Auf dem Rechner werden Aufnahmen und Protokolle nach der eingestellten Frist
  gelöscht (Standard 30 Tage). Dateien liegen nur für den eigenen Benutzer
  lesbar.
- **Im Google Drive gilt diese Frist nicht.** Was dort einmal liegt, bleibt
  liegen, bis es jemand von Hand löscht. Wer will, dass seine Aufnahmen dort
  verschwinden, sagt Bescheid, dann räumen wir sie weg.
- Ein aufbewahrter Prüf-Ausschnitt enthält Bild und Ton, weil er die
  Twitch-Spur so speichert, wie sie kommt. Geprüft wird ausschließlich der Ton.
  Das ist nicht die durchgehende Tonaufnahme von oben: die ist reiner Ton.

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
Zum Start und zum Ende jeder Sendung kommt eine private Nachricht. Fehlt
beides, ist der Dienst störanfällig: Twitch-Abfrage, streamlink oder
Zustellung. Ein Ausfall der Prüfung meldet sich außerdem ausdrücklich.

**Wie zuverlässig sind die Funde?**
Es sind Hinweise, keine Urteile. Sprache-zu-Text und Modell können sich
verhören oder Kontext falsch deuten.

**Funktioniert das auch für fertige VODs?**
Nein, nicht mehr. Geprüft wird ausschließlich live mitgeschnittenes Material.
