# Community-Features (Leaderboard, Voice-Reaction, Partner-Recruiting)

## Worum es geht

Der Bot bündelt drei Funktionen, die die Deadlock-Streamer-Community zusammenbringen und sichtbar machen. Das **Leaderboard** zeigt im Discord, welche Streamer wie viele Zuschauer haben. Das **Partner-Recruiting** findet automatisch aktive Deutsche Deadlock-Streamer und lädt sie zur Community ein. Die **Voice-Reaction** lässt den Bot kurz in einen Stream reinhören und passend im Chat reagieren — als zusätzlicher, persönlicher Kontaktpunkt. Alle drei laufen im Hintergrund; nur das Leaderboard wird aktiv per Befehl aufgerufen.

## Was der Bot tut

**Leaderboard**
- Stellt im Discord den Befehl `!twl` bereit, der eine interaktive Rangliste der getrackten und beobachteten Deadlock-Streamer anzeigt.
- Wertet die Zuschauerzahlen der letzten 30 Tage aus und zeigt pro Streamer den Durchschnitt, den Höchstwert und die Anzahl der Messpunkte.
- Trennt die Anzeige in zwei Gruppen: aktive Partner der Community einerseits und sonstige Deadlock-Streamer aus der Spielkategorie andererseits.
- Lässt die Liste live umsortieren und filtern, ohne dass der Befehl neu eingegeben werden muss.

**Partner-Recruiting**
- Beobachtet im Hintergrund laufend, welche Streamer regelmäßig Deadlock spielen, aber noch nicht Teil der Community sind.
- Erkennt aus diesen Beobachtungen geeignete Kandidaten und spricht sie an, sobald sie gerade live sind.
- Schickt dann eine kurze, freundliche Nachricht in den Chat des Streamers, in der sich die Community vorstellt und auf Details in der Bio verweist.
- Folgt dem Kanal und tritt ihm bei, bevor die Nachricht gesendet wird.
- Merkt sich jeden Kontaktversuch und spricht denselben Streamer eine ganze Weile lang nicht erneut an.

**Voice-Reaction**
- Nimmt bei einem laufenden Stream einen kurzen Audio-Ausschnitt auf, schreibt ihn in Text um und lässt eine KI entscheiden, ob und wie der Bot im Chat reagiert.
- Kann so wirken, als würde der Bot dem Stream tatsächlich zuhören und sich auf das Gesagte beziehen.
- Reagiert auch, wenn der Streamer den Bot direkt im eigenen Chat anschreibt oder per @-Erwähnung anspricht.
- Schickt jede mögliche Antwort vor dem Senden durch einen Sicherheitsfilter, der Links, fremde Erwähnungen und zu lange Texte entfernt und unpassende Antworten ganz verwirft.
- Kann interessante Gesprächsverläufe zusätzlich als Hinweis ins Discord-Team weiterleiten.

## Wann es passiert

**Leaderboard**
- Wird ausschließlich durch den Discord-Befehl `!twl` ausgelöst, und das nur in den dafür freigegebenen Statistik-Kanälen. In anderen Kanälen weist der Bot freundlich auf die erlaubten Kanäle hin.

**Partner-Recruiting**
- Läuft komplett automatisch in regelmäßigen Abständen mit, ohne dass jemand etwas auslösen muss.
- Ein Kandidat wird nur angesprochen, wenn er gerade live ist, in letzter Zeit ausreichend regelmäßig Deadlock gestreamt hat, noch kein Partner ist und nicht erst kürzlich schon kontaktiert wurde. Die genauen Kriterien, nach denen jemand als geeignet gilt, sind bewusst nicht dokumentiert.
- Pro Tag und pro Durchlauf gibt es feste Obergrenzen, wie viele Streamer angesprochen werden, und zwischen einzelnen Nachrichten liegt eine Pause — damit das nie nach Massen-Werbung aussieht.

**Voice-Reaction**
- Ist standardmäßig ausgeschaltet und wird nur aktiv, wenn der Betreiber sie ausdrücklich einschaltet.
- Greift dann bei Streamern mit einem offenen Gesprächsfaden — etwa direkt nach einer Recruiting-Ansprache oder einem Raid.
- Eine Audio-Reaktion entsteht, während der jeweilige Kanal live ist; eine Chat-Reaktion entsteht, sobald der Streamer selbst etwas schreibt oder den Bot mit @ erwähnt.

## Was Streamer/Viewer sehen

**Leaderboard**
- Eine Discord-Nachricht mit einer übersichtlichen Rangliste: Streamer-Name, Durchschnitts-Zuschauer, Spitzenwert und Anzahl der Messpunkte, getrennt nach Partnern und übrigen Deadlock-Streamern.
- Unter der Nachricht eine Reihe von Knöpfen, mit denen sich die Ansicht sofort anpassen lässt (siehe nächster Abschnitt).
- Wer nicht den Befehl selbst abgesetzt hat, kann die Knöpfe nicht bedienen und bekommt einen entsprechenden Hinweis.

**Partner-Recruiting**
- Der angesprochene Streamer sieht im eigenen Chat eine einmalige, freundliche Nachricht der Community mit Hinweis auf die Bio. Es werden keine Links direkt in den Chat geschrieben.
- Außerhalb dieser einen Nachricht ist für Streamer und Zuschauer nichts vom Recruiting sichtbar.

**Voice-Reaction**
- Im besten Fall eine kurze, natürliche Chat-Nachricht des Bots, die sich auf den Stream oder das Gesagte bezieht.
- Diese Nachricht enthält nie Links und keine fremden @-Erwähnungen, weil der Sicherheitsfilter sie vorher entfernt.
- Wenn der Filter keine sinnvolle Antwort übriglässt, sieht man schlicht nichts — der Bot schweigt dann lieber.

## Was Streamer einstellen können

**Leaderboard**
- Über die Knöpfe unter der Nachricht: das Sortierkriterium (Durchschnitts-Zuschauer, Messpunkte, Spitzenwert oder Name), die Reihenfolge (auf- oder absteigend), einen Partner-Filter (alle, nur Partner oder ohne Partner), eine Mindestzahl an Messpunkten, einen Mindest-Durchschnitt an Zuschauern und wie viele Plätze angezeigt werden.
- Mit einem Knopf lassen sich die Daten neu laden, mit einem weiteren alle Filter zurücksetzen, und mit einem dritten die Ansicht schließen.
- Alternativ direkt beim Befehl als Text, zum Beispiel `!twl samples=15 avg=25 partner=only sort=avg order=desc`. Über `!twl help` zeigt der Bot die verfügbaren Optionen an.

Partner-Recruiting und Voice-Reaction sind reine Betreiber-Funktionen und haben für einzelne Streamer keine sichtbaren Einstellungen.

## Grenzen & Sonderfälle

- **Leaderboard nur in Statistik-Kanälen:** In anderen Discord-Kanälen funktioniert `!twl` nicht; der Bot nennt die erlaubten Kanäle.
- **Leaderboard ist persönlich bedienbar:** Nur wer den Befehl abgesetzt hat, kann die Knöpfe nutzen. Nach längerer Inaktivität werden die Knöpfe deaktiviert.
- **Datenbasis 30 Tage:** Das Leaderboard betrachtet nur die letzten 30 Tage; Streamer ohne aktuelle Messdaten tauchen nicht auf.
- **Recruiting ist konservativ und einmalig:** Es gibt harte Tages- und Durchlaufgrenzen, eine lange Pause vor einem erneuten Kontakt und eine bewusste Ausnahme für größere Streamer, die mit dieser Ansprache nicht abgeholt werden. Auch ein fehlgeschlagener Versuch setzt die Pause — niemand wird doppelt angeschrieben.
- **Recruiting fasst keine geschützten Kanäle an:** Streamer auf internen Sperr-/Schutzlisten werden gar nicht erst als Kandidaten betrachtet.
- **Voice-Reaction ist opt-in und filtergesichert:** Ohne ausdrückliche Freischaltung passiert nichts. Selbst eingeschaltet wird jede Antwort erst durch den Sicherheitsfilter geprüft; bleibt nichts Sinnvolles übrig, sendet der Bot nichts.
- **Voice-Reaction reagiert nicht auf jeden:** Im Chat reagiert der Bot nur auf den Streamer selbst oder auf direkte @-Erwähnungen — nicht auf beliebige Zuschauer.

## Häufige Fragen

**Wie sehe ich das Streamer-Leaderboard?**
Mit dem Befehl `!twl` in einem der dafür vorgesehenen Discord-Statistik-Kanäle. Es erscheint eine Rangliste mit Knöpfen, über die du Sortierung und Filter direkt anpassen kannst.

**Warum kann ich die Knöpfe am Leaderboard nicht bedienen?**
Die Steuerung gehört der Person, die den Befehl abgesetzt hat. Setze einfach selbst `!twl` ab, dann kannst du die Ansicht steuern.

**Welchen Zeitraum zeigt das Leaderboard?**
Die letzten 30 Tage. Pro Streamer siehst du den Durchschnitt der Zuschauer, den Höchstwert und die Anzahl der Messpunkte.

**Der Bot hat in meinem Chat eine Partner-Einladung geschrieben — was ist das?**
Du bist als regelmäßiger Deadlock-Streamer aufgefallen und die Community lädt dich ein, Partner zu werden. Die Details stehen in der Bio. Es ist eine einmalige Nachricht; bei Desinteresse musst du nichts weiter tun.

**Bekomme ich diese Einladung mehrfach?**
Nein. Nach einem Kontaktversuch — egal ob die Nachricht ankam oder nicht — gibt es eine lange Pause, bevor überhaupt ein erneuter Kontakt möglich wäre. Es gibt zusätzlich harte Tageslimits, damit das nie nach Spam aussieht.

**Nach welchen Kriterien sucht der Bot Streamer für die Einladung aus?**
Er bewertet, wie regelmäßig und aktiv jemand Deadlock streamt, und wählt daraus passende Kandidaten. Die genaue Auswahl-Logik ist bewusst nicht öffentlich, damit sie sich nicht gezielt austricksen lässt.

**Schreibt der Bot mir auch, wenn ich schon ziemlich groß bin?**
Über diese Einladung nicht. Sie richtet sich an aktive Streamer im kleineren bis mittleren Bereich; größere Kanäle werden damit bewusst nicht angesprochen.

**Was ist die Voice-Reaction und reagiert der Bot wirklich auf meinen Stream?**
Der Bot kann kurz in einen laufenden Stream reinhören, das Gehörte in Text umwandeln und passend im Chat reagieren. Die Funktion ist standardmäßig aus und wird nur gezielt eingeschaltet. Jede Antwort läuft vorher durch einen Sicherheitsfilter, der Links und fremde Erwähnungen entfernt — passt nichts, schweigt der Bot.

**Wie spreche ich den Bot im Chat an?**
Wenn ein Gesprächsfaden offen ist, reagiert der Bot, sobald du als Streamer selbst etwas schreibst oder ihn mit @ erwähnst. Auf normale Zuschauer reagiert er in diesem Modus nicht.
