# KI-Titelgenerator

## Worum es geht

Der KI-Titelgenerator schlägt Streamern fertige Stream-Titel für Deadlock vor. Statt selbst zu grübeln, gibt man ein paar Stichworte zum geplanten Stream an, und der Bot formuliert daraus einen einladenden Titel plus zwei Alternativen. Der Vorschlag orientiert sich an den eigenen Titeln, die in der Vergangenheit gut liefen, an dem, was bei vergleichbaren Deadlock-Streamern funktioniert, und – falls hinterlegt – am eigenen Deadlock-Rang und dem aktuell gespielten Helden.

## Was der Bot tut

- Nimmt vom Streamer ein paar Keywords entgegen (z. B. "ranked solo grind") und baut daraus einen kompletten, lesbaren Stream-Titel.
- Liefert immer einen Hauptvorschlag und zusätzlich bis zu zwei Alternativen.
- Stützt sich auf die bisherige Titel-Historie des jeweiligen Streamers und darauf, welche dieser Titel besser oder schlechter performt haben.
- Bezieht gelerntes Wissen über erfolgreiche Deadlock-Titel anderer, vergleichbarer Streamer mit ein.
- Übernimmt den Deadlock-Rang des Streamers in den Titel, wenn dieser bekannt ist und die Keywords dazu passen.
- Kann auf Wunsch den aktuellen Live-Spielzustand einbeziehen (welcher Held gerade gespielt wird, ob solo oder in Party).
- Passt den Stil an die eigenen Gewohnheiten an: Wer normalerweise Emojis nutzt, bekommt gegebenenfalls einen sparsamen Emoji, wer keine nutzt, bekommt keine.
- Hält Vorschläge bewusst nah an dem, was beim Streamer schon funktioniert hat, statt wild Neues zu erfinden; bei ungewohnten Keywords formuliert er eher konservativ.
- Räumt typische KI-Macken auf: keine erfundenen Ränge, keine generischen Füllphrasen wie "heute ist es soweit", keine umgeschriebenen Keywords, und hält sich an die für Twitch zulässige Titel-Länge.
- Lernt im Hintergrund laufend dazu: Ein nächtlicher Vorgang wertet aus, welche Titel zuletzt gut liefen, und füttert damit die Wissensbasis.
- Erstellt für aktive Partner regelmäßig (wöchentlich, über die letzten Wochen) eine Auswertung der eigenen Titel-Performance mit Stärken, Schwächen, erkannten Mustern und konkreten Handlungsempfehlungen.

## Wann es passiert

- **Auf Abruf im Chat:** Der Streamer oder ein Moderator des Kanals tippt den Titel-Befehl mit Keywords. Der Bot meldet kurz "Generiere deinen Titel..." und liefert dann den Vorschlag.
- **Auf Abruf im Dashboard:** Im Streamer-Dashboard lässt sich ein Titelvorschlag mit denselben Eingaben (Keywords, optional Live-Bezug) anfordern.
- **Live-Bezug optional:** Nur wenn der Streamer das ausdrücklich verlangt (im Chat über einen Zusatz-Schalter am Befehl, im Dashboard über die entsprechende Option) und nur wenn er gerade tatsächlich in Deadlock spielt, fließt der aktuelle Spielzustand mit ein.
- **Rang-Bezug automatisch:** Ist der Streamer mit seinem Spiel-Account verknüpft, wird sein aktueller Rang berücksichtigt, ohne dass er etwas extra tun muss.
- **Wissensaufbau nächtlich:** Einmal pro Tag im Hintergrund, ohne Zutun des Streamers.
- **Insight-Auswertung wöchentlich:** Einmal pro Woche im Hintergrund, automatisch für aktive Partner.

## Was Streamer/Viewer sehen

- **Im Chat:** Eine kurze Bestätigung, dass generiert wird, gefolgt vom Hauptvorschlag und – falls vorhanden – den Alternativen in einer Zeile.
- **Im Dashboard:** Den Hauptvorschlag, die Alternativen und eine Übersicht der eigenen letzten Titel mit ihren Leistungswerten als Orientierung.
- **Insights im Dashboard:** Falls eine aktuelle Auswertung vorliegt, sieht der Partner Stärken, Schwächen, Muster und drei konkrete Empfehlungen zu seinen Titeln.
- **Viewer:** Bekommen vom Generator selbst nichts mit. Der Befehl ist Streamern und Moderatoren vorbehalten; sichtbar wird höchstens das Ergebnis, wenn der Streamer den vorgeschlagenen Titel tatsächlich übernimmt.

## Was Streamer einstellen können

- **Keywords:** Der Streamer bestimmt über die Stichworte, worum es im Titel gehen soll. Das ist die Hauptsteuerung.
- **Live-Bezug an/aus:** Über einen Zusatz-Schalter am Befehl bzw. eine Option im Dashboard lässt sich entscheiden, ob der aktuelle Spielzustand einbezogen wird.
- **Übernahme bleibt freiwillig:** Der Bot setzt den Titel nicht selbst – der Streamer entscheidet, ob und welchen Vorschlag er bei Twitch einträgt.
- Eine darüber hinausgehende Konfiguration (Tonalität, Emoji-Verhalten) gibt es nicht; der Bot leitet diese aus den eigenen bisherigen Titeln ab.

## Grenzen & Sonderfälle

- **Tempolimit:** Pro Streamer ist die Zahl der Generierungen in einem kurzen Zeitfenster begrenzt, damit der Dienst nicht überlastet wird. Im Dashboard ist das Limit großzügiger als im Chat. Wird es erreicht, nennt der Bot, wie viele Sekunden bis zur nächsten Anfrage zu warten sind.
- **Rang nur mit Verknüpfung:** Der Deadlock-Rang taucht nur auf, wenn der Streamer seinen Spiel-Account verknüpft hat. Ohne Verknüpfung wird ohne Rang generiert.
- **Live-Daten nur im Spiel:** Der Live-Bezug funktioniert ausschließlich, solange der Streamer gerade in Deadlock ist. Ist er nicht im Spiel, wird ohne Live-Kontext generiert, auch wenn die Option aktiv ist.
- **Frische Wissensbasis ist dünn:** Solange noch wenig Verlaufsdaten vorliegen, fallen die Vorschläge generischer aus. Mit jeder Nacht und mehr gesammelten Stream-Daten werden sie treffsicherer.
- **Performance ist eine Näherung:** Die Bewertung "guter" vs. "schlechter" Titel beruht auf indirekten Anhaltspunkten aus den Stream-Daten, nicht auf echten Klickraten. Sie zeigt Tendenzen, keine harte Wahrheit.
- **Insights brauchen genug Daten:** Die wöchentliche Auswertung wird nur erstellt, wenn im Auswertungszeitraum genügend Streams mit Titeln vorliegen.
- **Nur Streamer/Mods:** Der Chat-Befehl reagiert nicht auf normale Zuschauer.
- **Gelegentliche Fehler:** Klappt die Generierung technisch nicht, meldet der Bot das knapp und bittet, es später erneut zu versuchen.

## Häufige Fragen

**Wie lasse ich mir einen Titel vorschlagen?**
Als Streamer oder Moderator den Titel-Befehl mit ein paar Stichworten zum geplanten Stream im eigenen Chat eingeben (z. B. "ranked solo grind"). Der Bot antwortet mit einem Hauptvorschlag und Alternativen. Dasselbe geht auch über das Streamer-Dashboard.

**Woher weiß der Bot, was ein guter Titel ist?**
Er schaut sich an, welche deiner bisherigen Titel im Verhältnis besser oder schlechter liefen, und bezieht gelerntes Wissen über erfolgreiche Deadlock-Titel anderer, vergleichbarer Streamer mit ein. Daraus formt er einen Vorschlag in deinem Stil.

**Warum steht in meinem Vorschlag kein Rang, obwohl ich einen habe?**
Der Rang erscheint nur, wenn dein Spiel-Account mit dem Bot verknüpft ist und der Rang zu deinen Keywords passt. Ohne Verknüpfung kennt der Bot deinen Rang nicht und lässt ihn weg.

**Was macht die Live-Option?**
Mit der Live-Option bezieht der Bot deinen aktuellen Spielzustand ein – etwa welchen Helden du gerade spielst und ob du solo oder in einer Party bist. Das funktioniert nur, während du tatsächlich Deadlock spielst.

**Ich bekomme die Meldung, ich soll warten – warum?**
Es gibt ein kurzes Tempolimit pro Streamer, damit der Dienst nicht überlastet wird. Sobald die genannte Wartezeit abgelaufen ist, kannst du wieder einen Vorschlag anfordern. Im Dashboard ist das Limit höher als im Chat.

**Setzt der Bot den Titel automatisch bei Twitch?**
Nein. Der Bot schlägt nur vor. Ob und welchen Titel du übernimmst, entscheidest du selbst.

**Warum sind die Vorschläge am Anfang so generisch?**
Die Wissensbasis wächst mit der Zeit: Ein nächtlicher Vorgang lernt laufend dazu, welche Titel gut funktionieren. Solange noch wenig Verlauf vorliegt, fallen die Vorschläge allgemeiner aus und werden mit mehr Daten treffsicherer.

**Können meine Zuschauer den Befehl nutzen?**
Nein. Der Titel-Befehl ist nur für den Streamer und seine Moderatoren gedacht und reagiert nicht auf normale Zuschauer.

**Was sind die wöchentlichen Insights?**
Für aktive Partner wertet der Bot regelmäßig die eigene Titel-Performance der letzten Wochen aus und zeigt im Dashboard Stärken, Schwächen, erkannte Muster und drei konkrete Empfehlungen. Das hilft, das eigene Titel-Verhalten gezielt zu verbessern.
