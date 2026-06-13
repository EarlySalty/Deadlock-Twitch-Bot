# AI-Chat-Engagement

## Worum es geht

Der Bot kann in Partner-Kanälen als eine Art aufmerksamer Stammgast im Chat
mitlesen und sich an laufenden Gesprächen beteiligen. Ziel ist es, den Chat
lebendiger zu machen und Zuschauer einzubinden — aber so, dass es sich wie eine
echte, ruhige Person anfühlt und nicht wie ein Bot, der Werbung oder Fakten
abspult. Die KI dockt immer an etwas an, das gerade im Chat passiert, und
erfindet keine Spielinhalte. Aktuell läuft dieses Feature im Twitch-Chat noch im
beobachtenden Modus (Shadow), die Antworten werden also nicht live gesendet.

## Was der Bot tut

- Er liest in aktiven Partner-Kanälen die jüngsten Chat-Nachrichten mit und
  baut sich daraus ein Bild vom laufenden Gespräch und der Stimmung im Kanal.
- Wenn eine Nachricht ein natürlicher Anknüpfungspunkt ist, formuliert er eine
  kurze, menschlich klingende Antwort, die zum Gespräch passt — knapp und
  trocken, nicht überdreht.
- Er **dockt immer an Bestehendes an** und eröffnet niemals ein Thema aus dem
  Nichts. Es gibt keine "Hey, lass uns über X reden"-Sprüche.
- Spielbezogene Aussagen (Helden, Fähigkeiten, Patch-Inhalte, Statistiken)
  verankert er in echten Deadlock-Daten aus Wiki und Patch-Ständen. Wenn er
  etwas nicht sicher weiß, erfindet er nichts und spricht das Thema nicht von
  sich aus an.
- Er merkt sich über die Zeit lockere Gesprächsfäden zu einzelnen Zuschauern
  (worüber jemand schon mal gesprochen hat) und kann später natürlich darauf
  zurückkommen — als Beziehungsführung, nicht als Faktenabfrage.
- Seine Persona ist bewusst ruhig, neugierig und zurückhaltend. Er wirkt wie
  jemand, der den Stream wirklich mitverfolgt, statt wie ein Hype-Bot.
- Antworten werden auf eine angenehme Chat-Länge begrenzt; zu lange Ausgaben
  werden gekürzt.
- Jede Entscheidung der KI (ob sie geantwortet hätte und mit welchem Text) wird
  protokolliert, sodass Streamer und Admins später nachvollziehen können, was
  die KI getan hätte.

## Wann es passiert

- Nur in Kanälen von **aktiven Partnern**, bei denen das Feature eingeschaltet
  ist. Ist der Kanal kein aktiver Partner oder das Feature aus, passiert nichts.
- Nur, wenn der Kanal **gerade live ist und Deadlock streamt**. Bei Offline,
  Just Chatting oder einem anderen Spiel bleibt die KI komplett still.
- Nur, wenn die laufende Nachricht ein echter Anknüpfungspunkt für die offene
  Runde ist. Nachrichten, die offensichtlich an eine einzelne Person oder einen
  Bot gerichtet sind, reine Emotes, einzelne Reaktionswörter oder
  Wiederholungs-Spam werden bewusst übergangen.
- Die KI antwortet **nicht auf jede passende Nachricht**. Sie hält bewusst Abstand
  und einen ruhigen Rhythmus, damit sie nicht den Chat zuspammt oder sich
  aufdrängt. Sie ist eine gelegentliche Stimme, kein Dauerredner.
- Wer nicht möchte, dass die KI mit ihm interagiert, kann ausgeschlossen werden
  (Opt-out) — dann bezieht die KI diese Person nicht mehr aktiv ein.

Die genauen Bedingungen, ab wann die KI antwortet, wie sie Anknüpfungspunkte
bewertet und in welchem Rhythmus sie spricht, sind bewusst nicht dokumentiert.
Das gehört zur Auswahl-Logik und bleibt intern.

## Was Streamer/Viewer sehen

- **Im aktuellen Shadow-Modus** sehen Zuschauer im Twitch-Chat **keine**
  Antworten der KI. Die KI liest mit und entscheidet, was sie sagen würde, aber
  der Text wird nicht in den Live-Chat gesendet, sondern nur intern bzw. nach
  Discord zur Beobachtung geloggt.
- Sobald das Feature für einen Kanal freigeschaltet wird, erscheinen die
  Antworten als ganz normale Chat-Nachrichten — von einer **eigenen
  Absender-Identität**, nicht vom Moderations-Bot. Es ist ein eigener
  Chat-Account, der wie ein normaler Stammgast wirkt.
- Streamer und Admins können im Dashboard ein Protokoll einsehen: welche
  Nachricht die KI ausgelöst hat, ob sie geantwortet hätte oder bewusst still
  blieb, und den jeweiligen Antworttext. So lässt sich der Charakter und das
  Verhalten der KI prüfen, bevor sie live geht.

## Was Streamer einstellen können

Pro Kanal lässt sich Folgendes steuern:

- **An/Aus:** Das Engagement-Feature für den eigenen Kanal aktivieren oder
  deaktivieren.
- **Persona-Hinweise:** Zusätzliche Hinweise, die den Ton oder Charakter der KI
  im eigenen Kanal feinjustieren (z. B. wie locker oder zurückhaltend sie
  auftreten soll).
- **Tabu-Themen:** Eine Liste von Themen, die die KI in diesem Kanal niemals
  ansprechen soll.
- **Opt-out einzelner Zuschauer:** Personen, die nicht von der KI angesprochen
  werden möchten, können ausgenommen werden.

## Grenzen & Sonderfälle

- **Shadow-Modus aktuell:** Im Twitch-Chat sendet die KI derzeit nicht live.
  Antworten werden nur beobachtet. Das ist Absicht, bis Qualität und Verhalten
  ausreichend geprüft sind.
- **Kein Themen-Aufschlag:** Die KI eröffnet nie aktiv ein Gespräch oder spricht
  eine Person aus dem Nichts an. Auch stille Stammgäste werden allenfalls indirekt
  über das laufende Thema eingebunden, niemals direkt angesprochen.
- **Keine erfundenen Spielfakten:** Bei unsicherer Faktenlage bleibt die KI
  stumm zum Thema, statt etwas Falsches über Deadlock zu behaupten. Sie nennt
  auch nie ihre eigenen technischen Grenzen oder Wissenslücken.
- **Nur Deadlock-Live-Phasen:** Außerhalb eines laufenden Deadlock-Streams ist
  die KI vollständig inaktiv.
- **Eigener Absender:** Die KI-Antworten kommen von einem separaten Account, nicht
  vom Moderations-Bot. Das ist gewollt, damit Moderation und Plauder-KI nicht
  verwechselt werden.
- **Externer Dienst:** Die KI nutzt einen externen Sprachmodell-Anbieter. Ist
  dieser kurzzeitig nicht erreichbar, bleibt die KI einfach still, statt eine
  fehlerhafte Antwort zu senden.

## Häufige Fragen

**Antwortet der Bot meinen Zuschauern automatisch im Chat?**
Aktuell nicht live. Das Feature läuft im Twitch-Chat im beobachtenden Modus: Die
KI entscheidet zwar, was sie sagen würde, sendet es aber noch nicht. Erst nach
Freischaltung erscheinen Antworten als echte Chat-Nachrichten.

**Wird die KI in jedem Stream aktiv?**
Nein. Nur in Kanälen aktiver Partner, bei denen das Feature eingeschaltet ist,
und nur während der Kanal live ist und Deadlock spielt. Sonst bleibt sie
komplett still.

**Schreibt die KI ständig oder spammt sie den Chat zu?**
Nein. Sie ist als gelegentliche, ruhige Stimme angelegt und hält bewusst Abstand.
Sie antwortet nicht auf jede Nachricht und drängt sich nicht auf.

**Erfindet die KI Sachen über Deadlock?**
Nein, das ist gezielt vermieden. Spielbezogene Aussagen sind in echten
Deadlock-Daten (Wiki, Patches, Statistiken) verankert. Wenn sie etwas nicht
sicher weiß, sagt sie dazu nichts, statt zu raten.

**Fängt die KI von sich aus Gespräche an?**
Nein. Sie dockt immer an etwas an, das gerade im Chat läuft, und eröffnet nie ein
Thema aus dem Nichts.

**Spricht die KI einzelne Zuschauer direkt an?**
Sie kann auf Nachrichten in der Runde reagieren, spricht aber stille Stammgäste
nicht direkt mit Namen an. Wer gar nicht einbezogen werden möchte, kann per
Opt-out ausgenommen werden.

**Kann ich den Ton der KI an meinen Kanal anpassen?**
Ja. Pro Kanal lassen sich zusätzliche Persona-Hinweise hinterlegen und Tabu-Themen
festlegen, die die KI nie anspricht. Außerdem kann das Feature jederzeit ein- und
ausgeschaltet werden.

**Ist die KI dieselbe wie der Moderations-Bot?**
Nein. Die Plauder-KI schreibt über eine eigene Absender-Identität, getrennt vom
Moderations-Bot, damit man Moderation und Unterhaltung auseinanderhalten kann.
