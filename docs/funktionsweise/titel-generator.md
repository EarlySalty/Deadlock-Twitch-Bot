# Titel-Studio

Das Titel-Studio erstellt Vorschläge für deinen Deadlock-Stream. Du findest es im Twitch-Dashboard unter „Titel“. Der Chat-Befehl `!title` nutzt dieselbe Titelerzeugung und dieselbe gespeicherte Verbotsliste.

## Titel bauen

Beschreibe mit ein paar Stichwörtern, was heute ansteht. Ohne Stichwörter entstehen allgemeine Vorschläge für einen Deadlock-Stream. Dein gespeicherter Standard-Stil, frühere Titel und deine Bewertungen helfen dabei, den Ton zu treffen.

Der Hauptvorschlag soll trocken und klar sein. Die weiteren Richtungen arbeiten mit Selbstironie oder einem konkreten Tagesdetail. Die gewünschte Länge richtet sich nach deiner bisherigen Titel-Länge. Die Ausgabe wird auf höchstens 140 Zeichen begrenzt, einschließlich der erkannten Mitstreamer.

Du kannst einen Vorschlag bewerten, bearbeiten oder eine andere Richtung auswählen.

## Gemeinsam streamen

Mit eingeschalteter Live-Option berücksichtigt das Titel-Studio den verfügbaren Spielzustand und gemeinsames Streamen. Auch bei leeren Stichwörtern bleibt die Live-Option ausgeschaltet, wenn du sie ausschaltest.

Die Erkennung nutzt diese Reihenfolge:

1. Eine gemeinsame Twitch-Sitzung mit geteiltem Chat. Eine zusätzliche Stream-Abfrage bestätigt, welche Beteiligten gerade live sind.
2. Eine gemeinsame Deadlock-Party aus der Steam-Präsenz. Maßgeblich ist dieselbe nicht leere Party-ID, nicht ein ähnlicher Name.
3. Ein gemeinsamer Discord-Sprachkanal mit passendem Server und Kanal.

Party- und Sprachkanal-Zustände dürfen höchstens zehn Minuten alt sein. Für diese beiden Wege braucht es verknüpfte Konten und einen aktuellen Twitch-Live-Stand. Ein veralteter, fehlender oder ungültiger Stand begründet keine Erkennung. Die Zuordnung und Zusammenführung erfolgen über Konto-IDs. In die Vorschläge kommen höchstens zwei Twitch-Logins, beispielsweise `mit @kollege und @zweiter`.

Unter der Live-Option erscheint dann „Erkannt: du streamst mit @kollege“. @-Nennungen aus der Modellantwort werden vor der Ausgabe entfernt; die bestätigten Logins werden anschließend in einheitlicher Schreibweise ergänzt. Das gilt für Hauptvorschlag und Alternativen.

Ist Twitch oder der Steam-Dienst vorübergehend nicht erreichbar, werden die übrigen verfügbaren Angaben genutzt. Ein Ausfall der Erkennung verhindert für sich allein keinen Titelvorschlag.

## Deine ausgeschlossenen Formulierungen

In „Dein Standard-Stil“ gibt es das Feld „Das will ich nie im Titel“. Du kannst bis zu 40 Wörter oder Sätze mit jeweils höchstens 60 Zeichen speichern, einen Eintrag pro Zeile. Leere und doppelte Einträge werden entfernt. Groß- und Kleinschreibung unterscheiden sich beim Vergleich nicht; mehrere Leerzeichen werden vereinheitlicht.

Die Formulierungen stehen als Textdaten im Auftrag an das Modell. Zusätzlich wird jeder fertige Vorschlag geprüft, auch nach dem Ergänzen von Mitstreamern. Die Prüfung sucht den gespeicherten Wortlaut als Teil des Titels. Eine zuverlässige Erkennung jeder sinngemäßen Umschreibung ist damit nicht verbunden.

Nach „So nicht“ kannst du die angezeigte Formulierung freiwillig in die Liste übernehmen und anschließend speichern. Bei 40 Einträgen weist die Oberfläche darauf hin, zuerst einen Eintrag zu entfernen. Es gibt dafür keinen Pflichtdialog.

Die Einstellungen gehören zur Twitch-ID deiner angemeldeten Sitzung. Eine andere Streamer-Angabe in einer Anfrage erlaubt keinen Schreibzugriff auf dessen Liste. Die bestehende administrative Leseansicht bleibt davon getrennt.

## Unpassende Vorschläge und Ausfälle

Die eingebaute Liste typischer Füllphrasen bleibt zusätzlich aktiv. Sie erkennt unter anderem einfache Varianten mit anderen Leerzeichen, Bindestrichen oder Apostrophen. Unpassende Vorschläge werden verworfen; brauchbare Alternativen können an ihre Stelle treten.

Bleibt kein brauchbarer Vorschlag übrig, gibt es genau einen inhaltlichen Neuversuch. Im allgemeinen Modus ohne Stichwörter können danach vorbereitete Ersatz-Titel verwendet werden. Auch diese durchlaufen dieselbe Prüfung. Bleibt die Auswahl leer, meldet das Dashboard, dass du Stichwörter oder ausgeschlossene Formulierungen anpassen kannst. Ein ungeprüfter Ersatz-Titel wird nicht automatisch veröffentlicht.

## Auf Twitch übernehmen

Ohne eingeschaltete Automatik entscheidest du, ob du einen Vorschlag übernimmst. Mit verbundenem Twitch-Schreibrecht und der experimentellen Option „automatisch auf Twitch setzen“ kann das Titel-Studio den Hauptvorschlag direkt setzen. Die Oberfläche zeigt, ob Twitch die Änderung bestätigt hat.

Ein Tempolimit schützt die Titelerzeugung vor zu vielen Anfragen. Fehlt das Twitch-Schreibrecht oder muss die Verbindung erneuert werden, zeigt das Dashboard den entsprechenden Verbindungsweg an.
