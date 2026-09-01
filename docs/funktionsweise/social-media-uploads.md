# Social-Media-Clips und Uploads

## Worum es geht

Das Social-Media-Dashboard sammelt Twitch-Clips ein und bereitet daraus fertige
9:16-Videos vor. Man kann den echten Render ansehen, Texte bearbeiten,
Plattformen wählen und einen Veröffentlichungsplan vorbereiten. Solange der
Kanal im Testbetrieb steht, wird dabei nichts an TikTok, Instagram oder YouTube
gesendet.

Die Pipeline ist absichtlich in zwei Bereiche getrennt:

- **Vorbereiten:** funktioniert ohne freigeschaltete Social-Media-Plattform.
- **Veröffentlichen:** wird erst später je Kanal bewusst eingeschaltet und
  braucht zusätzlich eine Clip-Freigabe, ein verbundenes Plattformkonto und
  die getrennte Freischaltung genau dieses Providerwegs.

## Was bereits funktioniert

- **Clips einsammeln:** Neue Twitch-Clips aktiver Partner werden mehrmals täglich
  übernommen. Manuelle Video-Uploads nutzen dieselbe Pipeline.
- **Quelle sichern:** Der Bot lädt den Twitch-Clip lokal herunter oder verwendet
  die bereits hochgeladene Datei.
- **Video aufbereiten:** Das gespeicherte Streamer-/Clip-Layout wird auf ein
  1080×1920-Video mit höchstens 60 Sekunden angewendet.
- **Echte Vorschau:** Im Dashboard lässt sich genau die MP4 prüfen und
  herunterladen, die später für den Upload vorgesehen ist.
- **Änderungen nachziehen:** Wird das Layout geändert, kann ein neuer Render
  angefordert werden. Unveränderte Ergebnisse werden nicht unnötig neu gebaut.
- **Texte vorbereiten:** Titel, Beschreibungen und Hashtags können vorgeschlagen
  und vor der Freigabe bearbeitet werden. Externe KI läuft nur mit gespeicherter
  Zustimmung über den freigegebenen zentralen KI-Pfad.
- **Freigabe und Planung:** Pro Clip lassen sich Ziele und Terminplan festlegen.
- **Upload-Strecke:** Adapter für TikTok, Instagram und YouTube, sichere
  Wiederholungen vor dem eigentlichen Plattformaufruf sowie Statusspeicherung
  sind vorhanden.
- **Aufräumen und Auswertung:** Veröffentlichte oder bewusst verworfene Clips
  werden nach der Frist aufgeräumt; verfügbare Plattformmetriken werden später
  nachgezogen.

## So läuft ein Clip durch die Strecke

1. Der Clip erscheint in der Übersicht.
2. Die Aufbereitung lädt die Quelle und rendert die Hochkant-Version.
3. Im Clip steht anschließend die echte Videovorschau bereit.
4. Titel, Beschreibung, Hashtags und Layout können geprüft oder geändert werden.
5. Mindestens eine Zielplattform wird gewählt.
6. Die Freigabe legt den Clip in den Zeitplan.
7. Im Testbetrieb bleibt er dort sicher liegen.
8. Erst im Livebetrieb, mit gültigem Plattformzugang und freigeschaltetem
   Providerweg wird er zum Termin hochgeladen.

## Testbetrieb als Standard

Jeder Kanal startet in **„Nur vorbereiten“**. Dieser Modus lässt die ganze
Aufbereitung laufen, sperrt aber den letzten Provider-Aufruf. Auch eine
automatische Clip-Freigabe oder ein bereits erreichter Termin kann diese Sperre
nicht umgehen.

Der Livebetrieb und der einzelne Providerweg werden erst nach Plattform-Audit
und kontrolliertem Test bewusst eingeschaltet. Ein echter öffentlicher Post
gehört nicht zum normalen Pipeline-Test. YouTube startet privat; Instagram wird
über ein isoliertes Testkonto geprüft. TikTok bleibt vollständig gesperrt, bis
die von TikTok verlangte Auswahl und Zustimmung pro Clip im Dashboard vorhanden
und der App-Weg freigegeben ist.

## Was im Dashboard sichtbar ist

- Quelle und Renderfortschritt;
- echte Videovorschau und Download;
- verständlicher Fehler mit erneutem Aufbereitungsversuch;
- Clip-Metadaten und bearbeitbare Texte;
- Zielplattformen und Freigabestatus;
- Kadenz und nächste Termine;
- Status verbundener Plattformkonten;
- getrennten Freigabestatus des jeweiligen Providerwegs;
- verfügbare Upload- und Aufrufdaten.

Die Oberfläche darf eine Freigabe erst annehmen, wenn die Vorschau fertig, eine
Plattform gewählt und der Test-/Livezustand sicher geladen ist.

## Verhalten bei Fehlern

- **Download oder Render fehlgeschlagen:** Der Clip zeigt die betroffene Stufe
  an und kann erneut aufbereitet werden.
- **Aufbereitung läuft bereits:** Es startet kein paralleler zweiter Lauf; der
  vorhandene Auftrag arbeitet weiter und bleibt sichtbar.
- **Plattformzugang fehlt:** Der Queue-Eintrag wird sichtbar zurückgestellt und
  nicht still verloren.
- **Fehler vor dem Plattformaufruf:** Der nächste sichere Versuch wird mit
  Abstand geplant.
- **Ergebnis nach gestartetem Plattformaufruf unklar:** Es gibt keine blinde
  Wiederholung. Die Karte zeigt „Ergebnis unklar“, damit der Clip zuerst auf
  der Plattform geprüft und danach von der Verwaltung bewusst bestätigt wird.
- **Clip verworfen:** Offene Freigaben und noch nicht gestartete Uploads werden
  beendet. War ein Provider-Aufruf bereits im Gang, meldet die Oberfläche das
  ausdrücklich.
- **Bot-Neustart:** Vorbereitungs- und Queuezustände liegen dauerhaft in der
  Datenbank und werden danach weiterverarbeitet.
- **Doppelter Worker:** Queue-Jobs werden atomar übernommen, damit derselbe Clip
  nicht durch zwei Worker gleichzeitig gepostet wird.

## Aktuelle Grenzen

- Die produktive Rust-Strecke transkribiert Clips noch nicht und erzeugt noch
  keine Untertitel.
- Der Auto-Clipper erkennt bislang überwiegend Kills; gute Fails und lustige
  Momente sind noch ein eigener Ausbau.
- TikTok-Analytics fehlen. YouTube- und Instagram-Auswertungen sind auf die
  wirklich freigegebenen API-Metriken begrenzt.
- Die Plattformadapter sind gebaut, aber ein Plattformweg gilt erst nach Audit,
  Kontofreischaltung und einem passenden privaten oder isolierten End-to-End-Test
  als vollständig bestätigt.
- TikTok kann nicht über die allgemeinen Automatikmodi freigegeben werden. Vor
  jedem Direct Post braucht es eine eigene Auswahl der Sichtbarkeit und
  Interaktionen sowie die ausdrückliche Zustimmung für genau diesen Clip.
- Ein ausgebauter Redaktionskalender und belastbare Wirkungs-KPIs sind noch Teil
  der Roadmap.

## Geplante Qualitätsstufen

Die Aufbereitung soll kontrolliert statt blind erweitert werden:

1. saubere Quelle und zuverlässiges 9:16-Layout;
2. Schnitt und richtige maximale Länge;
3. Untertitel und Deadlock-Begriffe;
4. Hook, Titel, Beschreibung und Hashtags;
5. mehrere Varianten im Test vergleichen;
6. erst nach stabilen Ergebnissen automatisch bevorzugen und veröffentlichen.

So können wir jeden Schritt mit echten Vorschauen bewerten, ohne dafür schon
einen öffentlichen Social-Media-Post riskieren zu müssen.

## Häufige Fragen

**Wird jetzt automatisch etwas veröffentlicht?**

Nein. Der Standard ist „Nur vorbereiten“. Für einen Provider-Aufruf braucht es
zusätzlich Livebetrieb, Freigabe, Zielplattform, erreichten Termin und gültigen
Zugang. Außerdem muss der jeweilige Providerweg separat freigeschaltet sein.

**Kann die Pipeline schon ohne Plattformfreischaltung getestet werden?**

Ja. Einsammeln, Download, Render, Vorschau, Textbearbeitung, Freigabe und
Zeitplanung sind davon getrennt.

**Ist die Vorschau nur ein Thumbnail?**

Nein. Sie zeigt das gespeicherte Render-MP4, das der Upload-Worker später
wiederverwendet.

**Gibt es schon automatische Untertitel?**

Nein. Transkription und Untertitel stehen offen und werden erst als vorhanden
beschrieben, wenn sie im produktiven Rust-Pfad verdrahtet und getestet sind.

**Was fehlt bis zum ersten echten Release?**

Die jeweilige Plattformfreischaltung, ein passender privater oder isolierter
End-to-End-Test, eine Sichtprüfung des Ergebnisses und die bewusste Umschaltung
des betroffenen Kanals auf Livebetrieb. TikTok braucht vorher zusätzlich seine
Auswahl- und Zustimmungsoberfläche pro Clip.
