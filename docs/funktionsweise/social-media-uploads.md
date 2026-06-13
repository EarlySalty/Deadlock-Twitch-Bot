# Social-Media-Clips & Uploads

## Worum es geht

Der Bot nimmt Highlight-Clips von Twitch und macht daraus fertige Kurzvideos für TikTok, Instagram Reels und YouTube Shorts. Er holt die Clips, schreibt automatisch passende Titel, Beschreibungen und Hashtags, bringt das Video ins Hochkant-Format und lädt es nach Freigabe auf die verbundenen Konten hoch. Ziel ist, die Reichweite von Clips über Twitch hinaus zu verlängern, ohne dass jemand jeden Clip von Hand schneiden und posten muss.

## Was der Bot tut

- **Clips einsammeln:** Er zieht regelmäßig neue Twitch-Clips der aktiven Partner-Streamer ein und übernimmt zusätzlich Clips, die der Highlight-Clipper als gute Momente erkannt hat.
- **Transkribieren:** Er hört den Ton des Clips ab und wandelt ihn in Text um. Dabei korrigiert er Deadlock-spezifische Begriffe (Heldennamen, Items), die normale Spracherkennung oft verschreibt — damit Untertitel und Beschreibungen stimmen.
- **Anreichern:** Aus dem Clip-Inhalt erzeugt er pro Plattform einen eigenen Vorschlag für Titel, Beschreibung und Hashtags. Jede Plattform bekommt eine zugeschnittene Variante, weil sich Format und Publikum unterscheiden.
- **Zur Freigabe vorlegen:** Bevor irgendetwas online geht, legt der Bot den Clip mit den fertigen Texten zur Review vor (siehe unten).
- **Plattformgerecht rendern:** Er bringt das Video ins richtige Seitenverhältnis und in die erlaubte Länge für die jeweilige Plattform und kann Untertitel einblenden.
- **Hochladen:** Nach Freigabe lädt er das Video auf die freigegebenen Plattformen hoch und übernimmt dabei den vorbereiteten Titel, die Beschreibung und die Hashtags.
- **Auswertung einsammeln:** Nach der Veröffentlichung holt er sich in Abständen die Plattform-Statistiken (z. B. Aufrufe) und erstellt daraus Reports.
- **Aufräumen:** Alte Clips und Dateien werden nach einer Aufbewahrungsfrist automatisch entfernt, wenn sie nicht mehr gebraucht werden.

## Wann es passiert

- **Automatisches Einsammeln:** Der Bot prüft in regelmäßigen Abständen (mehrmals täglich) auf neue Clips der letzten Tage. Pro Streamer wird dabei nur eine begrenzte Zahl der neuesten Clips eingelesen. Berücksichtigt werden ausschließlich aktive, verifizierte Partner — reine Beobachtungs-Kanäle und abgemeldete Streamer bleiben außen vor.
- **Verarbeitung:** Sobald ein Clip eingelesen ist, durchläuft er die Schritte Transkription, Korrektur und Texterzeugung von selbst im Hintergrund.
- **Freigabe-Anfrage:** Erst wenn die Texte fertig sind, geht der Clip in den Freigabe-Zustand und es wird eine Freigabe-Nachricht ausgelöst.
- **Upload:** Der Upload startet erst nach der Freigabe — entweder durch eine manuelle Entscheidung oder, falls aktiviert, automatisch für die voreingestellten Plattformen.
- **Statistik-Abruf:** Die Auswertung wird nach der Veröffentlichung wiederholt nachgezogen, weil Aufrufzahlen erst über die Zeit wachsen.

## Was Streamer/Viewer sehen

- **Im Chat/öffentlich:** Während der Verarbeitung ist nichts sichtbar. Sichtbar wird erst das fertige Kurzvideo auf TikTok, Instagram oder YouTube, nachdem es freigegeben und hochgeladen wurde.
- **Im Freigabe-Schritt:** Die zuständige Person bekommt eine Nachricht mit einer Vorschau des Clips — Titel, Streamer, Aufrufzahl und Thumbnail — sowie die fertig vorgeschlagenen Titel und Hashtags je Plattform (YouTube, TikTok, Instagram).
- **Auswahl-Buttons:** Zu jeder Freigabe gehören eine Plattform-Auswahl (YouTube Shorts, TikTok, Instagram Reels) und drei Aktionen: **Posten**, **Bearbeiten** und **Skip**. Nach einer Entscheidung wechselt die Anzeige die Farbe (grün = freigegeben, rot = übersprungen, gelb = in Bearbeitung) und zeigt, wer wann entschieden hat.
- **Im Admin-Dashboard:** Es gibt eine Social-Media-Übersicht mit der Clip-Liste, dem aktuellen Status jedes Clips, den Freigabe-Optionen und der Verknüpfung der Plattform-Konten.

## Was Streamer einstellen können

- **Plattform-Konten verbinden:** TikTok, Instagram und YouTube werden über einen Anmelde-/Verbindungs-Flow mit dem Bot gekoppelt. Nur verbundene und aktive Plattformen kommen als Upload-Ziel infrage.
- **Plattformen pro Clip wählen:** Im Freigabe-Schritt lässt sich einzeln auswählen, auf welche der drei Plattformen ein bestimmter Clip gehen soll — auch nur auf eine Teilmenge.
- **Texte bearbeiten:** Über **Bearbeiten** bzw. das Dashboard lassen sich die automatisch erzeugten Titel, Beschreibungen und Hashtags vor dem Upload anpassen. Manuelle Änderungen überschreiben den Bot-Vorschlag.
- **Automatische Freigabe je Plattform:** Pro Plattform kann hinterlegt werden, dass Clips ohne manuelle Bestätigung dorthin gehen. Ist das für eine Plattform aktiv, wird sie bei einer Freigabe automatisch mit aufgenommen. Standardmäßig ist diese Auto-Freigabe aus, d. h. der Default ist die manuelle Review.
- **Externe-KI-Zustimmung:** Es gibt einen ausdrücklichen Schalter dafür, ob Clip-Inhalte zur Texterzeugung an einen externen KI-Dienst gehen dürfen. Ohne diese Zustimmung wird nichts nach außen geschickt; der Bot nutzt dann nur eine lokale Variante oder lässt die automatischen Texte aus, sodass sie manuell ergänzt werden müssen.

## Grenzen & Sonderfälle

- **Freigabe ist Pflicht (Default):** Ohne Freigabe wird nichts veröffentlicht. Automatischer Upload passiert nur dort, wo die Auto-Freigabe bewusst eingeschaltet wurde.
- **Nur verbundene Plattformen:** Eine Plattform, deren Konto nicht verbunden oder deaktiviert ist, wird beim Upload nicht bedient — auch wenn sie im Freigabe-Dialog angehakt wurde.
- **Konto-Verbindung kann ablaufen:** Die Anmeldung an den Plattformen muss im Hintergrund gültig gehalten werden. Läuft sie ab, schlagen Uploads mit Anmeldefehlern fehl, obwohl das Konto im Dashboard noch als „verbunden" angezeigt wird — dann ist eine erneute Verbindung nötig.
- **Format- und Längenregeln je Plattform:** Jede Plattform hat eigene Vorgaben zu Seitenverhältnis und Videolänge. Clips, die sich nicht plattformgerecht aufbereiten lassen, werden vor dem Upload aussortiert statt fehlerhaft hochgeladen.
- **Stecken gebliebene Clips:** Die Verarbeitung läuft in einer festen Reihenfolge von Stufen. Ein Clip, der „nicht weitergeht", hängt in der Regel in genau einer Stufe fest (z. B. Transkription oder Texterzeugung) und nicht an einem einzelnen verlorenen Versuch.
- **Ohne Tonspur/Datei:** Liegt für einen Clip keine verwertbare Videodatei vor, wird die Transkription übersprungen; der Clip kann trotzdem mit manuell ergänzten Texten weiterlaufen.
- **Skip ist endgültig für diesen Clip:** Wird ein Clip übersprungen, geht er nicht online; er bleibt aber als übersprungen vermerkt.
- **Mehrfach-Upload wird verhindert:** Ist ein Clip auf einer Plattform bereits hochgeladen oder schon eingereiht, wird er dort nicht erneut gepostet.

## Häufige Fragen

**F: Lädt der Bot meine Clips automatisch und ungefragt hoch?**
A: Nein. Standardmäßig muss jeder Clip vor der Veröffentlichung freigegeben werden. Automatisch hochgeladen wird nur, wenn die automatische Freigabe für eine Plattform ausdrücklich eingeschaltet wurde.

**F: Auf welche Plattformen kann der Bot posten?**
A: Auf TikTok, Instagram Reels und YouTube Shorts. Für jeden Clip kann einzeln gewählt werden, welche dieser Plattformen bedient werden sollen.

**F: Woher kommen Titel und Hashtags?**
A: Der Bot erzeugt sie automatisch aus dem Inhalt des Clips — pro Plattform eine eigene Variante. Diese Vorschläge lassen sich vor dem Upload von Hand anpassen.

**F: Kann ich die vorgeschlagenen Texte ändern?**
A: Ja. Über die Bearbeiten-Aktion bzw. das Dashboard lassen sich Titel, Beschreibung und Hashtags vor dem Upload überschreiben. Die manuelle Version hat dann Vorrang.

**F: Welche Clips werden überhaupt eingesammelt?**
A: Neue Twitch-Clips aktiver, verifizierter Partner-Streamer aus den letzten Tagen sowie Clips, die der Highlight-Clipper als gute Momente erkannt hat. Reine Beobachtungs-Kanäle werden nicht einbezogen.

**F: Warum ist mein Clip nicht online gegangen?**
A: Mögliche Gründe: Er wurde noch nicht freigegeben oder beim Skip übersprungen, das Plattform-Konto ist nicht verbunden bzw. die Anmeldung ist abgelaufen, oder der Clip erfüllt die Format-/Längenvorgaben der Plattform nicht und wurde aussortiert.

**F: Werden meine Clips zur Texterzeugung an einen externen Dienst geschickt?**
A: Nur wenn die ausdrückliche Zustimmung für externe KI gesetzt ist. Ohne diese Zustimmung verlässt nichts den Bot; die Texte werden dann lokal oder gar nicht erzeugt und können manuell ergänzt werden.

**F: Wie sieht das fertige Video aus?**
A: Hochkant im Plattformformat, in erlaubter Länge, optional mit eingeblendeten Untertiteln — also als typisches Short/Reel/TikTok, nicht als roher Twitch-Clip.

**F: Bekomme ich mit, wie die Videos laufen?**
A: Ja. Der Bot holt nach der Veröffentlichung wiederholt die Plattform-Statistiken (z. B. Aufrufe) und fasst sie in Reports zusammen.
