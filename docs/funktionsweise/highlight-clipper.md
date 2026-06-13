# Highlight-Clipper

## Worum es geht

Der Bot erstellt für betreute Partner-Streamer automatisch kurze Highlight-Clips aus ihren Deadlock-Spielen — die guten Momente, also Outplays, Multikills, Teamfights und knappe Clutch-Situationen, nicht einfach jeder Kill. Der Streamer muss dafür nichts tun: Sobald ein Spiel vorbei ist, sucht der Bot die starken Szenen, schneidet sie aus der Stream-Aufzeichnung und stellt sie als fertige Video-Clips bereit. Ziel ist, dass gute Spielszenen nicht im langen VOD untergehen, sondern direkt teilbar vorliegen.

## Was der Bot tut

- Er beobachtet die abgeschlossenen Deadlock-Matches der betreuten Partner-Streamer.
- Pro Match analysiert er den Spielverlauf und sucht die wirklich sehenswerten Momente heraus — Skill und Outplay, nicht bloße Kills bei wenig Gegner-HP.
- Reine Solo-Kills ohne besonderen Wert werden bewusst aussortiert; gewollt sind Multikills, Teamfights und knappe Outplay-/Clutch-Situationen.
- Als Videoquelle nutzt der Bot die Twitch-Aufzeichnung (das VOD) des Streams, nicht eine separate Live-Mitschneidung. Er rechnet die Spielzeit des Highlights auf die Zeitachse des VODs um und schneidet genau diesen Ausschnitt heraus.
- Jeder Clip wird knapp und action-zentriert zugeschnitten: kurzer Anlauf vor der Szene, kurzer Nachlauf danach, mit einer Obergrenze für die Cliplänge, damit kein Leerlauf entsteht.
- Die fertigen Clips werden komprimiert (auf ein handliches Format heruntergerechnet) und mit einer kurzen Beschriftung versehen, die die Szene einordnet (z. B. die Art des Moments und bei knappen Situationen den verbleibenden Lebensbalken).
- Die fertigen Clips werden anschließend automatisch bereitgestellt, sodass das Team und der Streamer sie ansehen und weiterverwenden können.

## Wann es passiert

- Der Bot prüft in regelmäßigen Abständen, ob für die betreuten Partner neue, kürzlich gespielte Matches vorliegen. Es ist kein Live-Vorgang während des Spiels, sondern läuft nach Spielende.
- Berücksichtigt werden nur recht frische Matches; sehr alte Spiele werden nicht nachträglich aufgegriffen. Jedes Match wird nur einmal verarbeitet, doppelte Clips zum selben Spiel entstehen also nicht.
- Voraussetzung ist, dass der Streamer ein aktiver Partner ist und sein Deadlock-/Steam-Account dem Bot bekannt ist. Ohne diese Zuordnung kann der Bot die Spiele nicht den richtigen Personen zuordnen.
- Damit ein Clip entstehen kann, muss zum Spielzeitpunkt eine passende Twitch-Aufzeichnung (VOD) existieren, die den Zeitraum des Matches abdeckt. Gibt es kein passendes VOD, wird kein Clip erstellt.
- Welche Szenen letztlich als Highlight gelten und welche aussortiert werden, entscheidet eine interne Bewertung. Welche Merkmale dabei wie zusammenwirken und welche Grenzwerte gelten, ist bewusst nicht dokumentiert — das ist der Kern der Clipper-Erkennung und gehört zum Betriebsgeheimnis.

## Was Streamer/Viewer sehen

- Nach einem gespielten Match tauchen, sofern es sehenswerte Szenen gab, automatisch ein oder mehrere kurze Clips dieses Spiels auf.
- Jeder Clip zeigt einen einzelnen Highlight-Moment in handlicher Länge, knapp um die Action herum geschnitten.
- Zu jedem Clip gibt es eine kurze Beschriftung, die die Szene einordnet — etwa um welche Art Highlight es sich handelt und, bei knappen Situationen, mit wie wenig Leben der Spieler die Szene überstanden hat.
- Im Stream selbst und im Twitch-Chat ist von dem Vorgang nichts zu sehen; die Erstellung läuft im Hintergrund nach dem Spiel.

## Grenzen & Sonderfälle

- **Kein VOD, kein Clip:** Die einzige Videoquelle ist die Twitch-Aufzeichnung des Streams. Wenn der Streamer das Speichern von VODs deaktiviert hat, das VOD bereits gelöscht/abgelaufen ist oder es den Spielzeitraum nicht abdeckt, kann der Bot nichts schneiden — auch wenn das Spiel selbst grandios war.
- **Nur Partner mit bekanntem Account:** Es werden nur aktive Partner-Streamer berücksichtigt, deren Spiel-Account dem Bot zugeordnet ist. Fehlt diese Zuordnung, findet der Bot die Matches der Person nicht.
- **Bewusst selektiv:** Dass zu einem Spiel kein Clip entsteht, ist normal und kein Fehler. Der Bot sortiert wenig spektakuläre Szenen (insbesondere reine Solo-Kills) absichtlich aus — lieber wenige starke Clips als viele belanglose.
- **Zeitabgleich Spiel ↔ Aufzeichnung:** Der Bot rechnet die Spielzeit auf die VOD-Zeitachse um. In seltenen Fällen kann der Ausschnitt dadurch leicht verschoben wirken; das Framing ist auf die eigentliche Action ausgelegt.
- **Größen-/Längenlimit:** Übermäßig lange oder zu große Clips werden vermieden bzw. nicht ausgeliefert. Sehr ausgedehnte Szenen werden auf eine handliche Länge begrenzt.
- **Verarbeitung nach Spielende:** Da der Bot nur in Abständen nachschaut und das fertige VOD braucht, erscheinen Clips nicht sofort, sondern mit etwas Verzögerung nach dem Match.

## Häufige Fragen

**Erstellt der Bot die Clips live während ich spiele?**
Nein. Der Bot arbeitet nach Spielende: Er nimmt sich abgeschlossene Matches vor und schneidet die Highlights aus deiner Twitch-Aufzeichnung (dem VOD). Es gibt deshalb eine gewisse Verzögerung, bis die Clips fertig sind.

**Warum hat der Bot von meinem Spiel keinen Clip gemacht, obwohl ich gute Szenen hatte?**
Dafür gibt es typische Gründe: Es existierte kein passendes VOD (z. B. weil das Speichern von Aufzeichnungen aus ist oder das VOD schon weg war), dein Spiel-Account war dem Bot nicht zugeordnet, oder die Szenen wurden von der internen Bewertung als nicht sehenswert genug eingestuft. Der Bot ist bewusst selektiv und lässt schwache Momente aus.

**Woher nimmt der Bot das Video für die Clips?**
Ausschließlich aus der Twitch-Aufzeichnung (VOD) deines Streams. Er macht keine eigene parallele Aufnahme. Ohne ein VOD, das den Spielzeitraum abdeckt, kann er nichts schneiden.

**Werden alle Kills geclippt?**
Nein. Reine Solo-Kills ohne besonderen Wert werden absichtlich aussortiert. Der Bot zielt auf Skill und Outplay: Multikills, Teamfights und knappe Clutch-Situationen.

**Nach welchen Kriterien entscheidet der Bot, was ein Highlight ist?**
Das macht eine interne Bewertung, die mehrere Hinweise zu einer Gesamteinschätzung verdichtet. Welche Merkmale genau einfließen und mit welchen Grenzwerten, ist bewusst nicht offengelegt — das ist der Kern der Funktion. Praktisch heißt es: starke, knappe und teamfight-lastige Szenen werden bevorzugt, belanglose ausgelassen.

**Muss ich als Streamer etwas einrichten, damit das funktioniert?**
Du musst aktiver Partner sein und dein Spiel-Account muss dem Bot bekannt sein. Wichtig ist außerdem, dass das Speichern von Twitch-Aufzeichnungen (VODs) bei dir aktiviert ist — sonst fehlt dem Bot die Videoquelle. Den Rest erledigt der Bot automatisch.

**Sehen meine Zuschauer im Stream oder Chat etwas davon?**
Nein. Die Clip-Erstellung läuft komplett im Hintergrund nach dem Spiel. Im Live-Stream und im Twitch-Chat passiert dabei nichts Sichtbares.

**Wie lang sind die Clips?**
Kurz und auf die Action konzentriert: ein kleiner Anlauf vor der Szene, ein kurzer Nachlauf danach, mit einer Obergrenze für die Länge. Lange Leerlauf-Passagen werden bewusst vermieden.
