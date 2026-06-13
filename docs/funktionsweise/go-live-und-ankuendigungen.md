# Go-Live-Erkennung & Ankündigungen

## Worum es geht

Der Bot überwacht laufend die betreuten Twitch-Kanäle und merkt automatisch, sobald ein Streamer live geht oder seinen Stream beendet. Geht ein Streamer mit Deadlock live, postet der Bot dafür eine Discord-Ankündigung mit Stream-Vorschau, Titel und einem Klick-Button zum Kanal. Endet der Stream, wird dieselbe Ankündigung in eine Offline-/VOD-Ansicht umgewandelt. Das Aussehen der Ankündigung kann jeder Streamer über sein Dashboard selbst anpassen.

## Was der Bot tut

- **Live-Status erkennen:** Der Bot weiß zu jedem Zeitpunkt, welche der betreuten Kanäle gerade live sind, welchen Titel und welche Kategorie sie laufen haben und wie viele Zuschauer zusehen.
- **Go-Live-Ankündigung posten:** Geht ein Streamer mit Deadlock live, schickt der Bot ein Discord-Embed in den dafür vorgesehenen Ankündigungs-Kanal. Das Embed zeigt Streamtitel, Kategorie, Zuschauerzahl und eine aktuelle Stream-Vorschau und enthält einen Button, der zum Twitch-Kanal führt.
- **Optional eine Live-Ping-Rolle anpingen:** Wenn aktiviert, wird beim Go-Live eine Rolle erwähnt, die interessierte Community-Mitglieder abonnieren können, um benachrichtigt zu werden. Der Bot legt diese Rolle bei Bedarf automatisch an.
- **Stream-Sitzung mitschreiben:** Während der Stream läuft, hält der Bot eine Sitzung offen und sammelt regelmäßig Werte wie Zuschauerzahl und Titel — das ist die Grundlage für die spätere Statistik-Auswertung.
- **Stream-Ende erkennen und Ankündigung umwandeln:** Geht der Streamer offline, schließt der Bot die Sitzung ab und wandelt die ursprüngliche Go-Live-Nachricht in eine Offline-Ansicht um: Sie ist klar als beendet markiert, zeigt den letzten Titel und ein Vorschaubild des letzten Streams und verweist per Button auf den Mitschnitt (VOD).
- **Vorschaubild aktuell halten:** Damit Discord nicht das Vorschaubild eines früheren Streams anzeigt, sorgt der Bot dafür, dass das Thumbnail bei jeder Ankündigung frisch geladen wird.

## Wann es passiert

- **Erkennungswege:** Der Bot kombiniert zwei Wege. Zum einen bekommt er von Twitch direkte Live-/Offline-Meldungen in Echtzeit (der schnelle Weg). Zum anderen prüft er die Kanäle zusätzlich in einem kurzen, gleichmäßigen Takt selbst nach (als Absicherung, falls eine Echtzeit-Meldung ausbleibt). So wird ein Go-Live in der Regel innerhalb weniger Sekunden erkannt.
- **Eine Go-Live-Ankündigung wird gepostet, wenn:**
  - der Kanal zu den betreuten/freigeschalteten Streamern gehört,
  - der Streamer von offline auf live wechselt **und**
  - dabei in der **Deadlock-Kategorie** streamt.
- **Keine Ankündigung gibt es**, wenn ein betreuter Streamer live geht, aber gerade **ein anderes Spiel** als Deadlock spielt. Wechselt er später im laufenden Stream auf Deadlock, holt der Bot die Ankündigung zu diesem Zeitpunkt nach.
- **Die Offline-/VOD-Umwandlung passiert**, sobald der Bot das Stream-Ende erkennt (entweder durch die Twitch-Echtzeitmeldung oder den nächsten Nachprüf-Takt).
- **Schutz gegen Flackern:** Kurze Aussetzer, bei denen ein Stream binnen Sekunden „offline“ und gleich wieder „online“ erscheint, fängt der Bot ab, damit nicht doppelte Ankündigungen oder voreilige Stream-Enden ausgelöst werden.
- **Automatisches Archivieren:** War ein betreuter Kanal längere Zeit (Größenordnung gut eine Woche) gar nicht mehr live, nimmt der Bot ihn von selbst aus der aktiven Überwachung. Das ist gewollt und kein Fehler.

## Was Streamer/Viewer sehen

- **Beim Go-Live:** Eine Discord-Nachricht im Ankündigungs-Kanal mit Embed (Titel, Kategorie, Zuschauerzahl, Stream-Vorschaubild) und einem Button, der direkt zum Twitch-Kanal führt. Ist die Live-Ping-Rolle aktiv, steht über dem Embed eine Erwähnung dieser Rolle.
- **Während des Streams:** Die Ankündigung bleibt im Kanal stehen; der Button führt weiter zum laufenden Stream.
- **Nach dem Stream-Ende:** Es kommt **keine zweite Nachricht** dazu — stattdessen wird die bestehende Go-Live-Nachricht inhaltlich ersetzt: Sie zeigt nun, dass der Kanal offline ist, nennt den letzten Streamtitel und die Kategorie, zeigt ein Vorschaubild des letzten Streams und bietet einen Button zum Mitschnitt (VOD) an.

## Was Streamer einstellen können

Jeder Streamer kann das Aussehen seiner Go-Live-Ankündigung im Dashboard anpassen. Anpassbar sind unter anderem:

- **Titel** der Ankündigung (mit der Möglichkeit, ihn auf den Stream zu verlinken).
- **Beschreibung** — wahlweise der Streamtitel, ein eigener Text oder eigener Text plus Streamtitel; optional automatisch gekürzt.
- **Zusatzfelder** (z. B. „Zuschauer“, „Kategorie“) mit eigenen Beschriftungen und Werten.
- **Autorzeile, Fußzeile und Farbe** des Embeds.
- **Bilder:** ob das große Bild die Live-Stream-Vorschau oder ein eigenes Bild ist, plus ein optionales kleines Vorschaubild.
- **Button-Beschriftung** und Ziel.
- **Live-Ping-Rolle:** ein- oder ausschalten; ist sie aus, wird beim Go-Live keine Rolle angepingt.

### Platzhalter

In den Textfeldern lassen sich Platzhalter verwenden, die der Bot beim Posten automatisch mit den echten Stream-Werten ersetzt. Unterstützt werden:

- `{channel}` — Anzeigename des Streamers
- `{url}` — Link zum Twitch-Kanal
- `{title}` — aktueller Streamtitel
- `{game}` — Kategorie/Spiel (in der Regel Deadlock)
- `{viewer_count}` — aktuelle Zuschauerzahl
- `{uptime}` — wie lange der Stream schon läuft
- `{started_at}` — Startzeitpunkt
- `{language}` — Sprache des Streams
- `{tags}` — gesetzte Stream-Tags
- `{mention_role}` — die Erwähnung der Live-Ping-Rolle

Ein Platzhalter, den der Bot nicht kennt, bleibt unverändert stehen. Felder, die der Streamer nicht selbst anpasst, behalten die sinnvollen Standardwerte.

### Vorschau und Test

Im Dashboard gibt es eine **Vorschau** der Ankündigung sowie eine **Test-Senden**-Funktion, mit der sich das fertige Embed prüfen lässt, bevor es im Ernstfall ausgelöst wird. Discord begrenzt die Länge einzelner Felder; sehr lange Texte werden beim Posten automatisch gekürzt, und das Dashboard warnt vorab, wenn eine Konfiguration zu lang werden könnte.

## Grenzen & Sonderfälle

- **Nur Deadlock löst die Ankündigung aus.** Ein betreuter Kanal, der in einer anderen Kategorie live geht, bekommt zunächst keine Ankündigung; sie kommt erst, wenn auf Deadlock gewechselt wird.
- **Nur betreute/freigeschaltete Kanäle** werden überwacht und angekündigt — nicht beliebige Twitch-Kanäle.
- **Die Offline-Nachricht ersetzt die Live-Nachricht.** Wurde die ursprüngliche Go-Live-Nachricht in Discord zwischenzeitlich gelöscht, kann der Bot sie nicht mehr in die Offline-Ansicht umwandeln.
- **Vorschaubild kann kurz hinterherhinken.** Twitch aktualisiert seine Stream-Vorschaubilder nicht sekundengenau; das Bild in der Ankündigung kann daher kurz nach Streamstart noch leicht veraltet sein.
- **Kurzes Flackern** des Live-Status (sofort offline und wieder online) wird bewusst abgefedert und führt nicht zu Doppel-Postings.
- **Längere Inaktivität führt zum automatischen Entfernen** aus der aktiven Überwachung; geht der Streamer danach wieder regelmäßig live, wird er wieder aufgenommen.

## Häufige Fragen

**F: Wie schnell merkt der Bot, dass ich live bin?**
A: In der Regel innerhalb weniger Sekunden. Der Bot bekommt von Twitch eine direkte Live-Meldung in Echtzeit und prüft die Kanäle zusätzlich in kurzen Abständen selbst nach, falls eine Meldung mal ausbleibt.

**F: Ich bin live gegangen, aber es kam keine Ankündigung. Woran liegt das?**
A: Die Ankündigung wird nur ausgelöst, wenn du in der **Deadlock-Kategorie** streamst. Läufst du in einem anderen Spiel, kommt zunächst keine Ankündigung — sie wird nachgeholt, sobald du im selben Stream auf Deadlock wechselst. Außerdem muss dein Kanal zu den freigeschalteten/betreuten Streamern gehören.

**F: Was passiert mit der Ankündigung, wenn ich offline gehe?**
A: Es kommt keine zweite Nachricht. Die bestehende Go-Live-Nachricht wird in eine Offline-Ansicht umgewandelt: Sie markiert klar, dass du offline bist, zeigt deinen letzten Titel und ein Vorschaubild und verweist per Button auf den Mitschnitt (VOD).

**F: Kann ich das Aussehen der Ankündigung selbst anpassen?**
A: Ja. Über dein Dashboard kannst du Titel, Beschreibung, Zusatzfelder, Farbe, Bilder, den Button und mehr einstellen. Es gibt dort auch eine Vorschau und eine Test-Senden-Funktion.

**F: Was sind diese Platzhalter wie `{title}` oder `{viewer_count}`?**
A: Das sind Bausteine, die der Bot beim Posten automatisch durch die echten Stream-Werte ersetzt — `{title}` wird zu deinem Streamtitel, `{viewer_count}` zur aktuellen Zuschauerzahl usw. So bleibt deine Vorlage allgemein und der Bot füllt sie pro Stream konkret aus.

**F: Was ist die Live-Ping-Rolle und kann ich sie abschalten?**
A: Das ist eine Discord-Rolle, die Community-Mitglieder sich geben können, um beim Go-Live benachrichtigt zu werden. Der Bot legt sie bei Bedarf automatisch an und erwähnt sie in der Ankündigung. Im Dashboard kannst du das Anpingen komplett ausschalten — dann wird beim Go-Live keine Rolle erwähnt.

**F: Warum zeigt die Ankündigung manchmal ein etwas veraltetes Vorschaubild?**
A: Twitch erzeugt seine Stream-Vorschaubilder nicht in Echtzeit. Der Bot lädt das Bild bei jeder Ankündigung frisch nach, damit nicht das Bild des vorherigen Streams hängenbleibt; trotzdem kann das von Twitch gelieferte Bild kurz nach Streamstart noch leicht hinterherhinken.

**F: Mein Kanal ist aus der Übersicht verschwunden, obwohl nichts kaputt ist — warum?**
A: War ein Kanal längere Zeit gar nicht mehr live, nimmt der Bot ihn automatisch aus der aktiven Überwachung. Das ist gewollt. Sobald du wieder regelmäßig live gehst, wird der Kanal wieder berücksichtigt.
