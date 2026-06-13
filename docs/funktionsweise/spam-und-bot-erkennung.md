# Spam- & Bot-Erkennung

## Worum es geht

Der Bot hört in jedem betreuten Partner-Kanal den Chat mit und erkennt automatisch Spam sowie gefälschte Accounts (z. B. Fake-/Viewer-Bots, die mit Werbe- oder Scam-Nachrichten in den Chat schreiben). Auffällige Nachrichten werden gelöscht und der dahinterstehende Account wird gebannt — ohne dass der Streamer eingreifen muss. Ziel ist ein sauberer Chat, ohne dass echte Zuschauer in Gefahr geraten.

## Was der Bot tut

- Er prüft eingehende Chat-Nachrichten laufend auf Anzeichen für Spam und automatisierte Fake-Accounts.
- Stuft er eine Nachricht als klaren Spam-/Bot-Fall ein, löscht er die Nachricht und bannt den Account im jeweiligen Kanal.
- Der Ban läuft über den Bot-Account selbst (der Bot ist als Moderator im Kanal aktiv), nicht über den Streamer-Account.
- Nach einem Ban hinterlässt der Bot standardmäßig einen kurzen Hinweis im Chat, dass der betreffende Account wegen Spam-Verdacht gebannt wurde, mit dem Hinweis auf den Rückgängig-Befehl.
- Parallel wird der Vorfall an den internen Discord-Moderations-Kanal gemeldet, damit das Team einen Überblick über die automatischen Aktionen behält.
- Die Erkennung wird laufend nachgeschärft und verbessert sich über die Zeit, sodass neue Spam-Versuche zuverlässiger erkannt werden. Wie das im Detail geschieht, ist bewusst nicht dokumentiert.
- Die Moderation ist bewusst konservativ ausgelegt: Es muss ein deutliches Gesamtbild zusammenkommen, bevor gehandelt wird, damit echte Zuschauer praktisch nie fälschlich getroffen werden.

## Wann es passiert

- Die Erkennung läuft permanent und automatisch — sie ist nicht an- oder abschaltbar und immer aktiv, solange der Bot im Kanal ist.
- Gehandelt wird erst, wenn sich aus einer Nachricht ein hinreichend klares Spam-/Bot-Bild ergibt. Die einzelnen Hinweise, ihre Gewichtung und die genaue Auslöseschwelle sind bewusst nicht dokumentiert, damit Spammer den Filter nicht gezielt umgehen können.
- Bestimmte Accounts sind grundsätzlich ausgenommen und werden nicht automatisch gebannt: der Streamer selbst, die Moderatoren des Kanals, der Bot-Account und bekannte/etablierte Stammgäste sowie offiziell bekannte Chat-Bots.
- Damit ein Ban technisch ausgeführt werden kann, muss der Bot im Kanal Moderator-Rechte haben. Fehlen diese Rechte, kann der Bot zwar erkennen, aber nicht durchgreifen.

## Was Streamer/Viewer sehen

- Spam- und Fake-Bot-Nachrichten verschwinden in der Regel sehr schnell wieder aus dem Chat, oft bevor echte Zuschauer sie überhaupt mitbekommen.
- Nach einem automatischen Ban erscheint standardmäßig ein kurzer Auto-Mod-Hinweis im Chat, dass der Account wegen Spam-Verdachts gebannt wurde — inklusive Hinweis, wie sich das bei einem Fehlgriff rückgängig machen lässt.
- Für den Streamer bedeutet das praktisch: weniger Werbe-Spam und Scam-Bots im Chat, ohne selbst ständig moderieren zu müssen.
- Echte Zuschauer merken im Normalfall nichts von der Moderation — sie werden durch die konservative Auslegung praktisch nie versehentlich getroffen.

## Was Streamer einstellen können

- Die automatische Spam- und Bot-Erkennung ist immer an und nicht als Funktion abschaltbar — sie gehört zum Grundverhalten des Bots in jedem betreuten Kanal.
- Voraussetzung ist, dass der Bot im Kanal als Moderator hinterlegt ist; ohne Mod-Rechte kann er Nachrichten nicht löschen oder bannen.
- Ein versehentlich getroffener Account lässt sich über den Unban-Befehl wieder freigeben.
- Optional gibt es einen „stillen" Modus, in dem der Ban weiterhin ausgeführt, der erklärende Hinweis im Chat aber unterdrückt wird. Wer das nutzen möchte, klärt das mit dem Team.

## Grenzen & Sonderfälle

- Die Erkennung greift nur in Kanälen, in denen der Bot aktiv ist und Moderator-Rechte hat. Ohne diese Rechte sieht der Bot zwar mit, kann aber keinen Ban setzen.
- Ist ein Account bereits gebannt, wird das sauber als Erfolg behandelt und nicht doppelt versucht.
- Ausgenommene Accounts (Streamer, Moderatoren, Stammgäste, bekannte Bots, der Bot selbst) werden bewusst nicht automatisch moderiert — das schützt die Community vor Fehlbann, kann aber bedeuten, dass ein als Mod eingetragener Account nicht über diese Automatik gebannt wird.
- In seltenen Fällen kann es kurzzeitig zu einer Fehleinschätzung kommen; genau dafür gibt es den Unban-Befehl, mit dem sich ein Treffer sofort korrigieren lässt.
- Die Spam-/Bot-Erkennung ist getrennt von anderen Schutzmechanismen wie der netzwerkweiten Bannliste oder der Scam-/Fake-Server-Warnung — diese haben eigene Auslöser und Abläufe.

## Häufige Fragen

**Muss ich die Spam-Erkennung selbst einschalten?**
Nein. Sie ist in jedem betreuten Kanal automatisch aktiv und kann nicht abgeschaltet werden. Du musst lediglich sicherstellen, dass der Bot in deinem Kanal Moderator ist.

**Warum braucht der Bot Moderator-Rechte?**
Nur als Moderator darf er Nachrichten löschen und Accounts bannen. Ohne diese Rechte erkennt er Spam zwar, kann aber nicht durchgreifen.

**Kann der Bot aus Versehen einen echten Zuschauer bannen?**
Das ist sehr unwahrscheinlich. Die Erkennung ist bewusst konservativ: Es muss ein deutliches Gesamtbild zusammenkommen, bevor gebannt wird, und Stammgäste, Moderatoren und der Streamer selbst sind ohnehin ausgenommen. Sollte es doch einmal passieren, lässt sich der Account mit einem Befehl sofort wieder entbannen.

**Wie erkenne ich, dass der Bot jemanden gebannt hat?**
Standardmäßig hinterlässt er nach einem Ban einen kurzen Auto-Mod-Hinweis im Chat. Zusätzlich wird jede Aktion intern an einen Discord-Moderations-Kanal gemeldet.

**Nach welchen Kriterien entscheidet der Bot, ob etwas Spam ist?**
Die Erkennung verdichtet mehrere Hinweise aus einer Nachricht zu einer Gesamteinschätzung. Die genauen Signale, ihre Gewichtung und die Schwelle sind bewusst nicht öffentlich — würden wir sie nennen, könnten Spammer sie gezielt umgehen.

**Werden meine Moderatoren oder Stammgäste auch geprüft?**
Sie sind von der automatischen Ban-Logik ausgenommen. Der Streamer, die Kanal-Moderatoren, bekannte Stammgäste sowie offiziell bekannte Chat-Bots werden nicht automatisch gebannt.

**Was passiert, wenn der Bot wirklich daneben liegt?**
Du (oder das Team) macht den Ban mit dem Unban-Befehl rückgängig. Die Erkennung wird zudem laufend nachgeschärft, damit solche Fehlgriffe seltener werden.

**Kann ich den Hinweis im Chat ausblenden?**
Ja, es gibt einen stillen Modus, in dem weiterhin gebannt wird, der erklärende Chat-Hinweis aber wegfällt. Das richtet das Team auf Wunsch ein.
