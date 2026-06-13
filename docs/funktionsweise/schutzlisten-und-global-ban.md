# Schutzlisten & globale Bannliste

## Worum es geht

Der Bot betreibt mehrere Kanäle desselben Partner-Netzwerks gemeinsam. Damit ein Störer, der in einem Kanal auffällt, nicht einfach zum nächsten Kanal weiterzieht, gibt es eine netzwerkweite Bannliste und weitere Schutzlisten. Ihr Zweck: bekannte Störer und unseriöse Kanäle einmal zentral erfassen und dann über alle angeschlossenen Partner-Kanäle hinweg fernhalten — ohne dass jeder Streamer das selbst nachpflegen muss.

## Was der Bot tut

- Er führt eine **netzwerkweite Bannliste** für Accounts, die als Störer eingestuft wurden. Wer dort steht, wird über alle aktiven Partner-Kanäle hinweg gebannt — nicht nur in dem Kanal, wo er ursprünglich aufgefallen ist.
- Er setzt diese Bans **proaktiv** durch: Auch wenn ein gelisteter Account in einem Kanal noch nie geschrieben hat, wird er dort vorsorglich gesperrt, damit er gar nicht erst zum Problem wird.
- Er hat zusätzlich ein **reaktives Sicherheitsnetz**: Schreibt ein gelisteter Account in irgendeinem Partner-Kanal, wird er sofort dort gebannt und seine Nachricht entfernt — selbst wenn die vorsorgliche Sperre diesen Kanal noch nicht erreicht hatte.
- Er pflegt eine **Schutzliste gegen unseriöse Kanäle** für das automatische Raiden: Kanäle, die als problematisch oder nicht seriös bekannt sind, werden als Raid-Ziel ausgeschlossen. Der Bot leitet die Zuschauer eines Streamers also nie aktiv dorthin.
- Er schützt die eigenen Streamer: Ein Partner-Kanal oder der Streamer selbst landet niemals auf der Bannliste und wird nie als Ziel gebannt. Fehlbans gegen eigene Leute sind dadurch praktisch ausgeschlossen.

## Wann es passiert

- Die **vorsorgliche Durchsetzung** läuft im Hintergrund über die angeschlossenen Kanäle. Sie ist bewusst so getaktet, dass sie zu unauffälligen Zeitpunkten greift und nicht mitten im laufenden Stream sichtbar wird. Die genaue Auslöselogik und Taktung sind nicht öffentlich dokumentiert.
- Das **reaktive Sicherheitsnetz** greift in dem Moment, in dem ein gelisteter Account in einem Partner-Kanal eine Nachricht schreibt.
- Der **Raid-Ausschluss** greift bei jeder automatischen Raid-Zielauswahl: Steht ein möglicher Kandidat auf der Schutzliste, wird er übersprungen.
- Welche Accounts oder Kanäle überhaupt auf eine dieser Listen kommen, und nach welchen Kriterien, ist bewusst nicht offengelegt — sonst ließe sich die Schutzwirkung gezielt umgehen.

## Was Streamer/Viewer sehen

- Wird ein gelisteter Account beim Schreiben erwischt, erscheint im betroffenen Kanal ein kurzer Hinweis, dass die Person netzwerkweit auf der Bannliste steht (Verstoß gegen die Community-Richtlinien) und deshalb hier gebannt wurde. Die Nachricht der Person verschwindet.
- Die **vorsorglichen** Bans laufen dagegen still: keine Chat-Nachricht, keine Ankündigung. Für Zuschauer ist nicht erkennbar, dass im Hintergrund vorgesorgt wurde — das ist gewollt, damit niemandem mitten im Stream ein verwirrender Ban-Vorgang auffällt.
- Beim automatischen Raiden merken Streamer und Zuschauer von der Schutzliste nichts Direktes; sie sorgt nur dafür, dass Raids ausschließlich zu geeigneten Zielen gehen.

## Grenzen & Sonderfälle

- Der Bot kann in einem Kanal nur dann bannen, wenn er dort die nötigen Moderationsrechte hat. Fehlen die Rechte, wird der Ban später erneut versucht, sobald die Voraussetzungen stimmen.
- Den Streamer selbst und andere Partner-Kanäle bannt der Bot grundsätzlich nie — diese Absicherung hat immer Vorrang.
- Steht ein Account schon im jeweiligen Kanal auf der Sperrliste, wird das als erledigt behandelt; doppelte Bans entstehen nicht.
- Bei externen Kanälen, die wiederholt auffällig in Partner-Kanäle hereinraiden, geht der Bot nicht sofort hart vor, sondern mit einer Schonfrist — wandelt sich so ein Kanal zwischenzeitlich in einen seriösen Partner, wird kein Ausschluss angewandt. Die genauen Schwellen und Fristen sind nicht öffentlich.
- Die Listen wirken nur in Kanälen des Partner-Netzwerks, nicht auf Twitch allgemein.

## Häufige Fragen

**Was ist die netzwerkweite Bannliste?**
Eine zentrale Liste von Accounts, die im Partner-Netzwerk als Störer eingestuft wurden. Wer dort steht, wird über alle aktiven Partner-Kanäle hinweg gebannt, nicht nur dort, wo er zuerst aufgefallen ist. So kann ein Störer nicht einfach zum nächsten Kanal weiterziehen.

**Warum wurde jemand in meinem Kanal gebannt, obwohl er bei mir nie etwas gemacht hat?**
Weil dieser Account netzwerkweit auf der Bannliste steht. Der Bot hält solche Accounts vorsorglich aus allen angeschlossenen Kanälen heraus — der Schutz gilt netzwerkweit, nicht nur für den Kanal, in dem die Person ursprünglich auffiel.

**Kann der Bot aus Versehen einen normalen Zuschauer oder mich selbst bannen?**
Nein. Streamer und Partner-Kanäle sind ausdrücklich geschützt und können nicht auf die Liste geraten oder gebannt werden. Die Bannliste enthält gezielt eingestufte Accounts, kein automatisches Raster über normale Zuschauer.

**Bekommt man mit, wenn der Bot jemanden bannt?**
Nur im reaktiven Fall: Schreibt eine gelistete Person, erscheint ein kurzer Hinweis im Chat und ihre Nachricht wird entfernt. Die vorsorglichen Bans im Hintergrund laufen komplett still, ohne Chat-Nachricht oder Ankündigung.

**Nach welchen Kriterien landet jemand auf der Bannliste?**
Das wird bewusst nicht offengelegt. Würde man die genauen Kriterien veröffentlichen, ließen sie sich gezielt umgehen — was den Schutz für alle Kanäle schwächen würde.

**Wird beim automatischen Raiden auch auf solche Listen geachtet?**
Ja. Kanäle, die als unseriös oder problematisch bekannt sind, stehen auf einer Schutzliste und werden beim automatischen Raiden als Ziel ausgeschlossen. Der Bot leitet die Zuschauer eines Streamers also nie aktiv zu einem solchen Kanal.

**Was passiert mit einem externen Kanal, der ständig in Partner-Kanäle hereinraidet?**
Solche Kanäle werden beobachtet und bei wiederholt auffälligem Verhalten mit einer Schonfrist behandelt, bevor ein Ausschluss greift. Entwickelt sich der Kanal in dieser Zeit zu einem seriösen Partner, passiert nichts weiter. Die genauen Schwellen und Fristen sind nicht öffentlich.

**Kann ein Ban rückgängig gemacht werden?**
Über die Listenpflege ja — das ist eine Admin-Aufgabe. Solange ein Account auf der netzwerkweiten Bannliste steht, wird er aber konsequent über alle Partner-Kanäle hinweg ferngehalten.
