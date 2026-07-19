## #393 — Deadlock-Clips laufen automatisch in OBS-Pausen

**Problem:** Stream-Pausen mussten bisher manuell mit passenden Videos oder Clips bestückt werden; vorhandene Deadlock-Clips aktiver Partner ließen sich nicht als gemeinsamer, aktueller Loop nutzen.

**Änderung:** Eine öffentliche OBS-Seite lädt ausschließlich bestehende Twitch-Clips aktiver, nicht ausgeschlossener Partner, filtert sie auf Deadlock und hält den Pool kurzzeitig im Speicher. Der Player mischt jeden Durchlauf und spielt jeden Clip genau einmal, bevor ein neuer Durchlauf beginnt.

**Aktuelles Verhalten:** Die Pause-Loop-URL kann direkt als OBS-Browser-Quelle genutzt werden; der Clip-Pool aktualisiert sich automatisch, ohne neue Clips oder Videos zu erzeugen.
## #392 — VOD-Export für dach_lock läuft automatisch

**Problem:** VODs mussten von Hand heruntergeladen und manuell weitergegeben werden — Aufwand pro Stream, und ein öffentlicher Upload wäre datenschutzmäßig unnötig.

**Änderung:** Nach jedem Stream-Ende von dach_lock lädt der Bot das komplette VOD herunter, legt es verschlüsselt in einem privaten Speicher ab und schickt eine befristete Freigabe direkt per Discord-DM.

**Aktuelles Verhalten:** Der Link ist 7 Tage gültig, danach nicht mehr erreichbar. Kein manueller Schritt mehr nötig.
## #391 — Streamer können das Netzwerk fair vergleichen

**Problem:** Die gesammelten Stream- und Raid-Daten waren nur einzeln im Dashboard sichtbar; dadurch ließ sich schwer erkennen, welche Kanäle Momentum haben und wo Raids wirklich ankommen.

**Änderung:** Eine öffentliche Vergleichsseite stellt aktive Partner mit gleicher Messung, Mindeststichprobe, Raid-Effekt und einem begründeten nächsten Test gegenüber. Private Daten wie Einnahmen, Abos, einzelne Zuschauer und Account-Verknüpfungen bleiben ausgeschlossen.

**Aktuelles Verhalten:** Unter der Streamer-Seite lassen sich 7, 30 oder 90 Tage vergleichen, Kanäle suchen und Raid-Wirkung nach Datenqualität einordnen.

## #390 — Stream-Ankündigungen bleiben eindeutig

**Problem:** Bei einem echten Stream-Neustart entstand manchmal ein zweiter Discord-Post, während der erste dauerhaft als LIVE stehen blieb.

**Änderung:** Vor einem frischen Go-Live-Post wird die bisherige Nachricht jetzt nachweislich beendet. Schlägt das vorübergehend fehl, wird kein paralleler Post erzeugt und der Abschluss erneut versucht.

**Aktuelles Verhalten:** Kurze Aussetzer verwenden weiter den bestehenden Post. Erkennt Twitch einen echten neuen Stream, wird zuerst der alte Post beendet und erst danach der neue veröffentlicht.

## #389 — edoeasy nur noch als Raid-Fallback

**Problem:** edoeasy konnte als externer Deadlock-Streamer wie jedes andere Fallback-Ziel gewählt werden, obwohl er nachrangig behandelt werden soll.

**Änderung:** Die Zielauswahl stellt edoeasy im Nicht-Partner-Fallback hinter alle anderen zulässigen Kandidaten; die harte Blacklist bleibt unberührt.

**Aktuelles Verhalten:** Gibt es irgendein anderes zulässiges Raid-Ziel, wird dieses gewählt. Nur wenn edoeasy der einzige Kandidat ist, kann der Raid weiterhin zu ihm gehen.

## #388 — Ein Schutzfall bleibt intern vollständig prüfbar

**Problem:** Hinweise zu einem auffälligen Account lagen bisher getrennt in Chat, Stream und Moderationskontext; Entwürfe ließen sich dadurch nicht zusammenhängend prüfen, und Discord-Kopien hatten keine eigene Löschfrist.

**Änderung:** Schreibt exakt der hinterlegte Twitch-Account, sammelt ein interner Shadow-Lauf die neue Nachricht, kurzzeitig transkribierte Stream-Reaktionen und beleggebundene Antwortentwürfe in einer eigenen Review-Ablage. Jeder Zyklus wird zusätzlich in den internen Discord-Kanal gespiegelt; Datenbankeinträge und die zugehörigen Discord-Nachrichten werden nach sechs Monaten anhand ihres gespeicherten Ursprungskanals einzeln entfernt.

**Aktuelles Verhalten:** Der Lauf dient ausschließlich der internen Prüfung und sendet selbst nichts auf Twitch. Roh-Audio wird nie gespeichert, und bei einem Providerfehler bleibt der Bot still.

## #387 — !lurk im Dashboard abschaltbar

**Problem:** Der Chat-Command !lurk lief immer mit fester Antwort, ohne dass Streamer das für ihren eigenen Kanal ausschalten konnten.

**Änderung:** Im Verwaltungs-Dashboard gibt's jetzt einen Schalter für !lurk. Aus lässt den Befehl technisch bestehen, der Bot antwortet dann einfach nicht mehr darauf.

**Aktuelles Verhalten:** Standardmäßig bleibt !lurk wie bisher aktiv; wer will, schaltet es im Dashboard für den eigenen Kanal ab.

## #385 — Globale Ban-Liste im Dashboard, pro Kanal abschaltbar

**Problem:** Die server-weite Ban-Liste ließ sich nur über die interne Schnittstelle pflegen und griff in jedem betreuten Kanal, ohne Möglichkeit, das für einzelne Kanäle abzuschalten.

**Änderung:** Die globale Ban-Liste ist jetzt im Verwaltungs-Dashboard einseh- und pflegbar. Zusätzlich lässt sich pro Kanal festlegen, ob die globalen Bans dort angewendet werden.

**Aktuelles Verhalten:** Standardmäßig greifen die globalen Bans in jedem Kanal; wer möchte, schaltet sie für einen Kanal im Dashboard ab.

## #386 — Raid-Erinnerung nur noch bei wirklich fehlender Begrüßung

**Problem:** Nach einem Raid bekam der raidende Streamer die Erinnerung, im Zielkanal Hallo zu sagen, oft auch dann, wenn er längst geschrieben hatte. Der Bot konnte fremde Zielkanäle gar nicht mitlesen und hat die Begrüßung deshalb schlicht nicht mitbekommen.

**Änderung:** Der Bot liest den Zielkanal nach dem Raid kurz anonym mit und prüft, ob der Streamer dort wirklich schreibt. Nur wenn die Begrüßung nachweislich ausbleibt, kommt die Erinnerung. Kann er nicht sicher mithören, verschickt er lieber nichts.

**Aktuelles Verhalten:** Die Erinnerung kommt nur, wenn der Bot durchgehend mitgelesen hat und der Streamer im Zielkanal wirklich stumm blieb.

## #384 — Fremde Discord-Werbung im Chat gibt jetzt Timeout

**Problem:** Postet ein fremder Account einen Discord-Invite im Chat, wurde das bisher nur intern gemeldet, aber nicht moderiert.

**Änderung:** Fremde Discord-Invite-Links lösen jetzt automatisch einen Timeout aus. Der eigene Community-Discord, Mods, der Streamer selbst und Stammzuschauer bleiben ausgenommen.

**Aktuelles Verhalten:** In betreuten Kanälen bekommt ein fremder Discord-Link 10 Minuten Timeout.

## #383 — Bot merkt jetzt selbst, wenn er in einem Kanal gebannt ist

**Problem:** Wurde der Bot in einem Partner-Kanal gebannt, lief die Live-Ankündigung auf Discord trotzdem weiter — wir haben also für Kanäle geworben, in denen wir gar nicht mehr erwünscht waren. Der Bot hat den Ban stündlich korrekt erkannt, das Wissen aber nur ins Protokoll geschrieben und danach verworfen. Zusätzlich hätte selbst eine gesetzte Pause nicht gehalten: Sie wurde wieder aufgehoben, sobald der Zugriff des Streamers gültig war — und der hat mit einem Ban nichts zu tun.

**Änderung:** Der erkannte Ban löst jetzt die schon vorhandene Reaktion aus: Werbung und Bot-Funktionen für diesen Kanal pausieren, der Streamer bekommt einmalig eine Hinweis-Nachricht. Die Rückkehr hängt nicht mehr am Zugriffsstatus, sondern am echten Ban: Der Bot prüft, ob er im Kanal wieder arbeiten kann, und hebt die Pause nur dann auf. Ist die Lage unklar, bleibt sie bestehen. Jede dieser Entscheidungen wird protokolliert, auch die ablehnenden. Streamer, die dauerhaft ausgeschlossen sind oder sich abgemeldet haben, werden jetzt ebenfalls zuverlässig von der Werbung ausgenommen.

**Aktuelles Verhalten:** Ein Ban pausiert die Werbung für den betroffenen Kanal von allein. Hebt der Streamer den Ban auf, kehrt der Bot ohne Zutun zurück. Zwei Kanäle waren betroffen und sind jetzt sauber pausiert.

## #382 — Jede Nachricht zählt nach einem Raid

**Problem:** Die Erinnerung nach einem Raid kam auch dann, wenn man im Zielchat längst geschrieben hatte. Der Bot zählte nur ein paar feste Grußwörter, ein "gg wp" oder "Hallöchen" fiel durch.

**Änderung:** Jetzt zählt jede Nachricht im Zielchat, egal was drinsteht. Statt 5 Minuten bleiben dafür 20 Minuten Zeit.

**Aktuelles Verhalten:** Wer sich nach dem Raid im Zielchat kurz meldet, bekommt keine Erinnerung mehr. Sie kommt nur noch, wenn dort wirklich nichts kam.

## #381 — !commands schickt nur noch den Link

**Problem:** Die Antwort auf !commands war eine Textwand aus rund 20 Befehlsnamen, die im Chat niemand lesen konnte und die trotzdem nicht erklärte, was die Befehle tun.

**Änderung:** Die Aufzählung im Chat entfällt. Es kommt nur noch der Link auf die Befehlsseite, wo jeder Befehl mit Erklärung steht.

**Aktuelles Verhalten:** !commands antwortet mit einer Zeile und dem Link. Die Befehlsseite bleibt die vollständige Übersicht.

## #380 — Analyse-Dashboard rechnet wieder mit echten Zahlen

**Problem:** Mehrere Ansichten zeigten Unsinn: Millionen Watchtime-Stunden im Juni, Wochentage mit über 100.000 Stunden Durchschnittsdauer, Raid-Retention bis 600%, internationale Streamer im deutschen Markt-Vergleich, kaputte Sonderzeichen in der Tag-Karte, eine dauerhafte Roh-Chat-Warnung und Ad-Verluste, die wie Zuwächse aussahen.

**Änderung:** Fehlerhafte Altdaten aus einem früheren Zeitstempel-Bug wurden repariert und alle Watchtime-Auswertungen dagegen abgesichert. Raid-Retention zählt jetzt nur noch echte Raid-Ankömmlinge und ist bei 100% gedeckelt, der Markt-Vergleich filtert auf deutschsprachige Streams, das Labor sammelt wieder Viewer-Daten pro Spiel. Die Ad-Analyse zeigt Verluste und Zuwächse mit klaren Vorzeichen samt echten Viewer-Zahlen, und der Tag-Trend wird jetzt wirklich berechnet statt immer 0% zu melden.

**Aktuelles Verhalten:** Trends, Planung, Raids, Markt, Labor, Chat-Tiefe und Monetarisierung zeigen plausible Werte. Wo Chat-Daten erst seit Kurzem erfasst werden, steht ein Hinweis mit dem Startdatum statt einer Dauerwarnung.

## #379 — Feature-Vergleich liegt jetzt auf einer Schriftrolle

**Problem:** Der Feature-Vergleich auf der Preisseite lag zwar auf Pergament, wirkte durch Rahmen und runde Ecken aber wie ein gewöhnliches Blatt Papier.

**Änderung:** Die Papierbahn hängt jetzt zwischen zwei goldenen Wickelstäben mit Endknäufen. Sie hat gerade Schnittkanten, läuft oben und unten in die Rollen hinein und wölbt sich leicht zu den Seiten.

**Aktuelles Verhalten:** Der Feature-Vergleich liest sich als ausgerollte Schriftrolle. Auf schmalen Bildschirmen bleibt die Rolle vollständig im Bild, die Tabelle selbst scrollt wie bisher seitlich.

## #378 — Clips aus dem Dashboard auf Social Media planen

**Problem:** Clips ließen sich im Dashboard ansehen, aber nicht gezielt für Social Media auswählen und zeitlich geplant posten. Alles Weitere war Handarbeit.

**Änderung:** Im Clip-Dashboard hat jeder Clip jetzt einen Aktivieren-Knopf. Dort wählst du die Ziele (TikTok, YouTube Shorts, Instagram sowie drei Deadlock-Montage-Kanäle) und den Zeitpunkt: sofort, automatisch auf den nächsten freien Slot oder ein fester Termin. Der Bot übernimmt das Einreihen und Posten.

**Aktuelles Verhalten:** Aktivierte Clips wandern in die Plan-Warteschlange und gehen zur gewählten Zeit raus. Für die eigenen Kanäle müssen die Plattform-Konten einmalig verbunden werden; die Einreichung bei den Montage-Kanälen ist getrennt zuschaltbar.

## #377 — Persönliche Bestwerte und Vergleich zum letzten Stream

**Problem:** Die Übersicht zeigte den letzten Stream und den Wochenvergleich, aber nirgends deine Rekorde, keinen direkten Vergleich zum Stream davor und keine Zuschauerkurve der letzten Session.

**Änderung:** Neuer Block "Persönliche Bestwerte" mit deinen Rekorden (Peak, Ø-Viewer, meiste neue Follower und Chatter in einem Stream, längster Stream). Dazu ein Vergleich zum vorherigen Stream mit grünen und roten Prozentwerten und eine Zuschauerkurve der letzten Session. Die großen Zahlen und die Gesundheitsbalken wurden optisch aufgeräumt.

**Aktuelles Verhalten:** Die Übersicht zeigt jetzt deine Bestwerte, wie der letzte Stream im Vergleich zum vorigen lief und den Zuschauerverlauf. Ein Rekord bekommt ein Neuer-Rekord-Zeichen, wenn der letzte Stream ihn geknackt hat. Die Bestwerte zählen ab dem Zeitpunkt, seit dem der Bot mitschreibt.

## #376 — Lurk-Befehl im Chat

**Problem:** Wer nur zuschauen und gerade nichts schreiben wollte, hatte keine kurze Art, sich im Chat abzumelden.

**Änderung:** Der Bot kennt jetzt !lurk. Wer das tippt, bekommt eine kurze Ansage, dass er in den Lurk geht.

**Aktuelles Verhalten:** !lurk läuft in jedem Kanal des Bots, egal ob gerade Deadlock gestreamt wird oder nicht.

## #375 — Kacheln vereinheitlicht, echtes Tab-Logo

**Problem:** Die vier Wochen-Kacheln auf der Übersicht waren deutlich größer als die Kacheln direkt darüber und mit auffälligen grünen Trendkurven versehen. Und im Browser-Tab klebte noch ein Platzhalter-Zeichen statt dem Community-Logo.

**Änderung:** Die Wochen-Kacheln sind jetzt genauso kompakt wie die vier Kacheln darüber und zeigen nur Wert und Bezeichnung, ohne Kurve. Der Tab zeigt jetzt das D-Logo der Community, dasselbe wie auf den anderen Seiten.

**Aktuelles Verhalten:** Die Übersicht wirkt aufgeräumter und einheitlicher, und das Tab-Logo passt zum Rest der Website.

## #374 — Übersicht aufgeräumt, Chat-Wert wieder da

**Problem:** Im Browser-Tab stand ein fremdes Platzhalter-Logo. Auf der Übersicht lagen die vier Wochen-Kacheln als eigene Reihe unter der Zusammenfassung des letzten Streams und kosteten viel Platz. Und die Kachel "Chat-Aktivität" zeigte dauerhaft nur einen Strich, obwohl Chatzahlen vorliegen.

**Änderung:** Der Tab zeigt jetzt das Community-Logo. Die vier Wochen-Kacheln sind in die Box der letzten Stream-Zusammenfassung gewandert, wodurch die Seite kompakter wird und der Rest nach oben rückt. Die Chat-Aktivität rechnet jetzt die Nachrichten pro Streamstunde aus und füllt Wert, Vergleich zur Vorwoche und Verlaufslinie.

**Aktuelles Verhalten:** Die Übersicht ist aufgeräumter, das Tab-Logo stimmt, und die Chat-Aktivität zeigt echte Zahlen samt Trend statt einem Strich. Wurde im Zeitraum nicht gestreamt, bleibt der Wert leer, weil "pro Stunde" ohne Streamstunden nicht definiert ist.

## #373 — FAQ-Box auf der Streamer-Seite antwortet wieder

**Problem:** Die Frage-Box auf der Streamer-Seite gab auf jede Frage nur die Ausweichantwort "das kann ich dir hier nicht sicher sagen", egal was man fragte. Echte Antworten kamen gar nicht mehr durch.

**Änderung:** Zwei Ursachen behoben. Das Sprachmodell dahinter war auf eine Variante umgestellt, die intern nur nachdachte statt zu antworten. Zusätzlich lief die fertige Antwort durch eine Längengrenze, die eigentlich für kurze Chat-Nachrichten gedacht ist und alles über zwei Sätze verwarf.

**Aktuelles Verhalten:** Die Box beantwortet Fragen zum Bot wieder mit echten, ausführlichen Antworten samt Quellen. Deckt die Doku eine Frage nicht ab, sagt sie das ehrlich, statt einfach zu schweigen.

## #372 — Nach der Anmeldung sofort einsatzbereit

**Problem:** Wer den Raid-Bot mitten im Stream freigeschaltet hat, musste teils bis zu einer halben Stunde warten, bis der Bot im Chat wirklich zuhörte. In der Zeit passierte auf Befehle wie !raid einfach nichts, obwohl die Anmeldung längst durch war.

**Änderung:** Sobald die Freischaltung durch ist, klinkt sich der Bot jetzt sofort in den Chat ein, statt auf das nächste turnusmäßige Fenster zu warten.

**Aktuelles Verhalten:** Direkt nach der Autorisierung reagiert der Bot auf Chat-Befehle. Der automatische Raid beim Offlinegehen lief schon immer, jetzt zieht der manuelle Teil sofort nach.

## #371 — Logo in der Leiste, Rand-Blitzer weg

**Problem:** Oben in der Navigationsleiste stand nur der Schriftzug, das Community-Logo fehlte. Außerdem blitzte beim Scrollen ein heller Rand um die Leiste auf, sobald sie ihren Glas-Hintergrund bekam.

**Änderung:** Das D-Logo der Community steht jetzt links neben dem Schriftzug, dasselbe wie auf den anderen Seiten. Die eingescrollte Leiste trennt sich vom Inhalt nur noch über einen weichen Schatten, der helle Rahmen ist entfernt. Alte Platzhalter-Logos wurden aus dem Bestand entfernt.

**Aktuelles Verhalten:** Beim Scrollen legt sich die Leiste sauber als mattes Glas über die Seite, ohne aufblitzende Kante, und die Marke ist oben immer sichtbar.

## #370 — Grün heißt wieder grün, rot heißt wieder rot

**Problem:** Die Statusfarben auf der Streamer-Seite waren zu erdig gewählt. Das gedämpfte Grün der Flow-Schritte und das Terracotta der Banned-Marken im Live-Feed verschmolzen mit dem warmen Braun der Seite, Status war kaum noch als Status erkennbar.

**Änderung:** Grün und Rot sind jetzt kräftige, leuchtende Töne, und die türkisen Rahmen und Icon-Flächen in den Info-Boxen wurden deutlich angehoben. Die goldene Grundstimmung der Seite bleibt unverändert, nur Statusinformationen dürfen jetzt herausstechen.

**Aktuelles Verhalten:** Im Live-Ban-Feed springen die Banned-Marken sofort ins Auge, in der Raid-Übersicht sind aktive und erledigte Schritte auf einen Blick unterscheidbar.

## #369 — Rauchglas mit goldenem Hinterlicht

**Problem:** Der Hintergrund der Streamer-Seite wirkte flach. Viele kleine, übereinander gelegte Leuchtflecken addierten sich zu einem gleichmäßigen Schleier, und die Karten lagen als fast deckende Flächen darauf.

**Änderung:** Der Grund ist jetzt fast schwarz und wird nur noch von zwei großen, sehr weichen goldenen Lichtkreisen hinterleuchtet. Die Karten und Stream-Fenster sind transparenter geworden und zeichnen den Hintergrund weich, wie mattiertes Glas.

**Aktuelles Verhalten:** Die Seite wirkt tiefer und edler. Inhalte scheinen auf Rauchglas zu liegen, das von hinten sanft golden angeleuchtet wird, und die Texte bleiben dabei besser lesbar als vorher.

## #368 — Weniger Goldplatten, mehr Ruhe auf der Streamer-Seite

**Problem:** Jede Icon-Kachel in den Bereichen Features, Community und Affiliate war eine voll leuchtende Goldfläche. Bei sechs Kacheln nebeneinander wirkte das erdrückend, und die weißen Icons darauf waren kaum lesbar. Die Discord-Box darunter fiel mit ihrer lila Vollfläche komplett aus dem Look der Seite.

**Änderung:** Icon-Kacheln liegen jetzt auf einer gedeckten Goldfläche mit goldenem Icon, voll leuchtendes Gold gibt es nur noch auf den großen Knöpfen. Die Discord-Box nutzt die normale Kartenoberfläche der Seite, die Discord-Farbe bleibt auf dem Logo und dem Beitreten-Knopf erhalten.

**Aktuelles Verhalten:** Die Abschnitte wirken ruhiger und aufgeräumter, das Auge landet zuerst auf den Überschriften und Knöpfen. Die Discord-Box fügt sich ins Design ein und bleibt trotzdem sofort als Discord erkennbar.

## #367 — Die Raid-Kette zeigt, wo sie gerade steht

**Problem:** Auf der Streamer-Seite sahen alle vier Schritte der Raid-Kette gleich wichtig aus, jeder hatte denselben goldenen Rahmen. Das Auge fand den aktuellen Schritt nicht auf Anhieb, und die beiden Knöpfe darunter konkurrierten ebenfalls um Aufmerksamkeit.

**Änderung:** Inaktive Schritte treten jetzt sichtbar zurück, sie werden blasser und verlieren ihren Rahmen. Nur der gerade laufende Schritt trägt Farbe. Der Knopf zum Mehr-Erfahren hat seinen Goldrahmen abgegeben, damit der Partner-Knopf die einzige goldene Fläche bleibt.

**Aktuelles Verhalten:** Wer die Raid-Demo anschaut, sieht sofort, welcher Schritt gerade läuft, und der Blick landet danach direkt auf dem Partner-Knopf.

## #366 — Der Portier zeigt sich am Empfang

**Problem:** Der Empfang (die FAQ-Seite) hatte zwar schon Tresen und Hausbuch, aber vom Portier selbst war nichts zu sehen. Die Halle wirkte unbesetzt. Außerdem war das Vorschaubild, das beim Teilen der Streamer-Seite in Discord und Co. erscheinen sollte, seit jeher kaputt und zeigte einfach nichts.

**Änderung:** Der Concierge steht jetzt mit seinem goldenen Zimmerschlüssel hinter dem Tresen, in der leeren Halle hängt ein gerahmtes Gemälde von ihm, und die Halle hat eine Art-Deco-Tapete samt goldenem Türbogen bekommen. Das Vorschaubild für geteilte Links gibt es jetzt wirklich, im gleichen Look.

**Aktuelles Verhalten:** Wer den Empfang betritt, wird vom Portier begrüßt; sobald das Gespräch beginnt, macht das Gemälde Platz für die Antworten. Geteilte Links zur Streamer-Seite zeigen ein goldenes Vorschaubild statt einer leeren Fläche.

## #365 — Der Ban-Knopf hinterlässt jetzt Beweise

**Problem:** Nachrichten, die zu einem Ban oder einer Löschung führten, landeten nie im Chat-Archiv. Die Pipeline hat sie vorher aussortiert. Damit fehlte ausgerechnet von den Fällen, die man später nachvollziehen will, jede Spur, und im Ban-Protokoll stand zwar der Text, aber nie der Grund.

**Änderung:** Jede Nachricht wird gesichert, bevor irgendein Filter sie anfasst. Was danach mit ihr passiert, steht als Aktion und konkreter Grund an ihr dran. Auch die Fälle, in denen die Safe-List jemanden vor einem Fehlurteil bewahrt hat, sind jetzt sichtbar. Und wer den Bot fragt, ob er für ein Spam-Wort gebannt wird, bekommt eine Antwort statt Stille.

**Aktuelles Verhalten:** Jede Moderationsentscheidung ist im Nachhinein prüfbar. Echte Werbung wird weiter gebannt, auch wenn jemand eine harmlos klingende Frage anhängt. Der lockere Spruch kommt nur, wenn ohnehin nichts moderiert wird.

## #364 — Der Empfang hat aufgemacht

**Problem:** Der Link „FAQ & Hilfe" im Dashboard führte auf eine Seite, die es nie gegeben hat. Wer draufgeklickt hat, landete auf einer Fehlermeldung. Fragen zum Bot konnte man zwar schon immer im kleinen Chat-Fenster unten rechts auf der Webseite stellen, aber das kannte kaum jemand, und einen richtigen Platz dafür gab es nicht.

**Änderung:** Der Concierge hat jetzt einen eigenen Raum. Unter „FAQ & Hilfe" öffnet sich ein Empfang, an dem du ihm einfach deine Frage stellst: Einrichtung, Chat-Befehle, Auto-Raid, Overlay, Pläne, Datenschutz. Er antwortet nur mit dem, was tatsächlich über den Bot hinterlegt ist, und schreibt dir dazu, wo er es nachgeschlagen hat. Was er nicht weiß, erfindet er nicht, sondern schickt dich in den Discord.

**Aktuelles Verhalten:** Der Link führt jetzt zum Empfang statt in eine Fehlermeldung. Ein paar Startfragen liegen bereit, falls dir gerade keine einfällt. Der Concierge steht hinter seinem Tresen, hinter ihm hängt das Schlüsselbrett, und seine Antworten kommen als Blatt aus dem Hausbuch.

## #363 — Der Analytics-Block fliegt von der Streamer-Seite

**Problem:** Die Streamer-Seite hatte einen großen Abschnitt, der das Analyse-Dashboard mit 13 Tabs und Vorschau-Kacheln vorgeführt hat. Der Block hat viel Platz gefressen und wenig überzeugt.

**Änderung:** Der Abschnitt ist raus, zusammen mit den Sprungmarken in Kopf- und Fußzeile, die sonst ins Leere gezeigt hätten.

**Aktuelles Verhalten:** Die Streamer-Seite läuft ohne den Analytics-Abschnitt. Die Live-Demo bleibt erreichbar, der Link dorthin steht weiterhin in der Fußzeile.

## #362 — Ruhigeres Dashboard, echtes Profilbild

**Problem:** Das Dashboard war so dunkel, dass Kacheln und Hintergrund zu einem schwarzen Block verschmolzen. Gleichzeitig knallte ein neonblauer Ton mitten ins Gold, am deutlichsten beim Knopf zum Analyse-Dashboard und in den Schleiern über der Startseite. Und oben links stand statt deines Profilbilds nur ein Kreis mit dem ersten Buchstaben deines Namens.

**Änderung:** Der Grund ist eine Stufe heller, damit man die Kanten der Kacheln wieder sieht. Blau und Grün leuchten jetzt ausschließlich da, wo sie etwas bedeuten, also in Statusanzeigen und in den Diagrammen. Alles andere, was man anfasst oder anschaut, ist Gold und Messing. Dein Twitch-Profilbild wird direkt von Twitch geladen und oben links angezeigt.

**Aktuelles Verhalten:** Das Dashboard liest sich ruhiger, die Farbe führt dich zu dem, was gerade passiert. Lädt dein Profilbild einmal nicht, springt die Anzeige zurück auf den Buchstaben, statt ein leeres Kästchen zu zeigen.

## #361 — Die Dashboards sehen jetzt nach Schmiede aus

**Problem:** Das Analyse-Dashboard hatte zwar schon den Gold-Look, wirkte aber flach. Das Admin-Dashboard war beim letzten Rebrand komplett übersehen worden und leuchtete weiter in Petrol und Orange. Statusanzeigen zogen sich ihre Farben aus einer bunten Standardpalette, die mit der Marke nichts zu tun hatte.

**Änderung:** Beide Dashboards laufen jetzt auf demselben Look: tiefes Schwarz-Braun als Grund, Kacheln aus gebürstetem Gusseisen mit Goldkante und Nieten in den Ecken, dazu eine ruhige Körnung. Grün und Blau leuchten nur noch da, wo wirklich etwas läuft oder verbunden ist. Der Feature-Vergleich liegt auf Pergament, mit dunkler Tinte statt Neon, damit er lesbar bleibt.

**Aktuelles Verhalten:** Analyse- und Admin-Dashboard sehen aus wie aus einem Guss. Ein pulsierender Punkt bedeutet, dass gerade etwas aktiv ist, und nicht bloß Dekoration.

## #360 — Die Raid-Erinnerung klingt jetzt nach Mensch

**Problem:** Wer nach einem Raid im anderen Chat nichts gesagt hat, bekam eine Erinnerung im Behördenton. Die Rede war vom "Zielchat" und davon, dass ein Hallo "dem Netzwerk hilft". Das las sich wie eine Ermahnung und nicht wie ein Tipp unter Streamern. Obendrein stand ein @-Mention in einer Direktnachricht, die ohnehin nur an eine Person geht.

**Änderung:** Die Nachricht ist neu geschrieben, freundlich und in normalem Deutsch. Sie erklärt jetzt auch, warum sich das lohnt, nämlich dass man im Kopf bleibt und die Connection zu anderen Streamern stärkt. Das doppelte Mention ist raus.

**Aktuelles Verhalten:** Wer den Gruß vergisst, bekommt weiterhin genau eine Erinnerung per Whisper, nur eben als netter Reminder statt als Rüffel.

## #359 — Scam-Wächter erkennt die Anmach-Masche jetzt am Netzwerk

**Problem:** Ein Scam-Account lief in zwei Partner-Kanälen dieselbe englische Anmach-Masche, und der Wächter hat sie sogar erkannt. Gehandelt hat er trotzdem nicht, weil er auf den offensichtlichen Betrugssatz gewartet hat. Den gibt es aber nie, der Betrug passiert später in den Direktnachrichten. Gemeldet hat er den Verdacht auch niemandem.

**Änderung:** Der Wächter sieht jetzt über den einzelnen Kanal hinaus. Taucht derselbe frische Account binnen einer Stunde bei mehreren Streamern neu im Chat auf und fährt überall dasselbe leere Skript, reicht das als Beweis. Außerdem landet ab sofort jede einzelne Entscheidung im Aufsichts-Channel, auch ein Freispruch und auch ein bloßer Verdacht.

**Aktuelles Verhalten:** Die Masche fliegt schon bei der Begrüßung im zweiten Kanal auf. Flüssiges Umgangsdeutsch und echter Spielbezug bleiben die stärksten Freispruch-Signale, und ohne gesichertes Account-Alter wird nicht mehr gebannt, sondern nur stummgeschaltet.

## #358 — Streamer-Seite und Dashboard im neuen Community-Look

**Problem:** Die Streamer-Website und die Dashboards liefen noch im alten Cyan-Lila-Design, während Hauptseite und Coaching längst im Gold-Look der Community sind. Das sah nach zwei verschiedenen Projekten aus.

**Änderung:** Beide laufen jetzt auf denselben Marken-Farben, warmes Dunkel mit Gold und Teal. Die alte Vorschau-Adresse unter /streamer/v2 fällt weg, weil die normale Seite jetzt so aussieht.

**Aktuelles Verhalten:** Streamer-Seite, Analyse-Dashboard und Verwaltung wirken wie ein zusammenhängendes Produkt. Inhalte, Zahlen und Funktionen bleiben unverändert, nur die Optik ist neu.

## #357 — Ein Login gilt in beiden Admin-Dashboards

**Problem:** Discord- und Twitch-Admin-Dashboard überschrieben sich gegenseitig mit getrennten Sitzungen; im Twitch-Dashboard fehlte dadurch zusätzlich der CSRF-Token.

**Änderung:** Beide Dashboards prüfen und widerrufen jetzt dieselbe zentrale Admin-Sitzung, während Twitch den dazugehörigen CSRF-Token direkt aus dieser Sitzung liefert.

**Aktuelles Verhalten:** Ein Login funktioniert in beiden Admin-Dashboards, ein Logout beendet beide, und Admin-Aktionen benötigen keine versteckte alte HTML-Seite mehr.

## #356 — Admin-Daten zeigen wieder den echten Betriebszustand

**Problem:** Bei Streamern fehlte der Zeitpunkt ihrer ersten Bot-Autorisierung, die Research-Seite lieferte keine Onboarding-Ideen und EventSub sowie Audit Log wirkten trotz laufendem System leer oder veraltet.

**Änderung:** Die Streamer-Tabelle zeigt und filtert jetzt „Partner seit“ anhand der ersten Autorisierung, Research schlägt passende noch nicht onboardete Deadlock-Streamer vor, EventSub wertet die aktuellen Webhook-Snapshots aus und erfolgreiche Admin-Änderungen werden dauerhaft protokolliert.

**Aktuelles Verhalten:** Neue Kandidaten lassen sich direkt analysieren, der EventSub-Status unterscheidet aktuelle und veraltete Snapshots, und das Audit Log füllt sich ab jetzt automatisch mit Zeitpunkt, Akteur, Aktion und Ziel.

## #355 — Raid-Hinweis verlinkt den Zielkanal wieder richtig

**Problem:** Im Hinweis nach einem Raid stand direkt hinter dem Namen ein Punkt. Twitch zieht das Satzzeichen mit in den Namen und macht aus der Erwähnung damit einen kaputten Link.

**Änderung:** Hinter der Erwähnung steht jetzt kein Satzzeichen mehr.

**Aktuelles Verhalten:** Der Zielkanal im Raid-Hinweis ist wieder anklickbar.

## #354 — Titelgenerator zuverlässiger und konkreter

**Problem:** Wenn der Titel-Dienst kurz drosselte, brach die Generierung sofort ab oder lieferte kommentarlos gar nichts. Und wenn doch ein Titel kam, war er oft austauschbar statt auf den Stream bezogen.

**Änderung:** Bei kurzzeitiger Drosselung probiert der Bot es jetzt automatisch bis zu drei Mal, bevor er aufgibt. Eine leere Antwort gilt als Fehler und wird gemeldet statt still verschluckt. Außerdem bekommt die KI mehr Kontext und klare Vorgaben: konkreter Aufhänger aus Keywords, Rang oder Anlass, 45 bis 100 Zeichen, keine Allerwelts-Titel wie „Ranked Grind".

**Aktuelles Verhalten:** Titel kommen auch bei kurzen Drosselphasen zuverlässig an. Schlägt es wirklich fehl, gibt es eine sichtbare Fehlermeldung statt Stille. Die Vorschläge orientieren sich an euren besten bisherigen Titeln und nennen einen konkreten Aufhänger.

## #353 — Die Spam-Abwehr denkt jetzt selbst mit und lässt sich korrigieren

**Problem:** Ein automatisch gelerntes „harmlos"-Muster hatte still dafür gesorgt, dass eindeutige Viewer-Bot-Werbung nur gemeldet statt entfernt wurde. Gleichzeitig konnte niemand sehen, was die KI eigentlich entschieden hatte.

**Änderung:** Bei verdächtigen Nachrichten urteilt jetzt zuerst eine KI (mit stärkerem Modell und Ersatz-Modell bei Ausfällen). Bestätigt sie Spam, wird die Nachricht gelöscht und die Person bekommt 24 Stunden Timeout, bewusst umkehrbar statt Ban. Jede Entscheidung, auch „harmlos", Fehler oder übersprungen, landet sichtbar im Mod-Kanal. Harmlos-Muster werden gar nicht mehr gelernt, und neue Spam-Muster nur noch, wenn sie einen echten Dienst- oder Domainnamen enthalten, nie bloß Allerweltswörter wie „viewer".

**Aktuelles Verhalten:** Bekannte Spam-Phrasen führen weiter sofort zum Bann. Im Graubereich entscheidet die KI, handelt umkehrbar und zeigt euch Urteil, Begründung und Aktion. Liegt sie daneben, nehmt ihr das gelernte Muster per Korrektur-Button im Alert zurück; einen bereits laufenden Timeout hebt ihr wie gewohnt im Twitch-Mod-Bereich auf.

---

## #352 — Der Bot weist Mitspieler-Suchende auf den Discord

**Problem:** Wer im Chat aktiv nach Mitspielern für Deadlock gesucht hat („suche noch zwei für Ranked", „wer hat Bock zu zocken"), bekam keinen Hinweis auf die Community, in der genau solche Leute zusammenfinden.

**Änderung:** Läuft gerade Deadlock, erkennt der Bot eine echte Mitspieler-Suche und weist die Person einmal freundlich auf den Discord hin. Eine KI prüft vorher, ob wirklich nach Mitspielern gefragt ist; bei Zweifel oder Fehler bleibt der Bot still.

**Aktuelles Verhalten:** Höchstens ein Hinweis pro Person mit langem Cooldown, Streamer im werbefreien Plan sind ausgenommen, und wenn schon eine Zugangs-Antwort rausging, schweigt der Hinweis, damit nie zwei Nachrichten auf einmal kommen.

## #351 — Beenden beendet auch die Discord-Admin-Ansicht

**Problem:** Wer im öffentlichen Twitch-Dashboard über eine Discord-Admin-Session erkannt wurde, sah den Admin-Modus dauerhaft. Der Beenden-Button wurde vom Server mit 403 abgewiesen und wirkte deshalb ohne sichtbare Reaktion.

**Änderung:** Der öffentliche Dashboard-Pfad behandelt jetzt auch diese Admin-Session als reine Berechtigung. Der Vollzugriff hängt dort ausschließlich am ausdrücklich gesetzten Admin-Modus.

**Aktuelles Verhalten:** „Beenden“ entfernt den Vollzugriff sofort und zeigt ausschließlich den Owner-Kanal mit seinem echten Plan. Fremde Kanäle bleiben in dieser Nutzeransicht gesperrt; Admin-Host und interne Aufrufe bleiben unverändert administrativ.

## #350 — Admin-Modus bleibt wirklich Opt-in

**Problem:** Im Twitch-Dashboard konnte die Admin-Ansicht nach dem Beenden oder über einen vorgelagerten Admin-Kontext wieder aktiv wirken. Dadurch waren Inhalte entsperrt, obwohl die echte Nutzeransicht gebraucht wurde.

**Änderung:** Die Status-Antwort des Dashboards prüft den Admin-Modus jetzt selbst noch einmal gegen das Session-Cookie. Ohne aktives Opt-in wird ein Twitch-Admin als normaler Partner angezeigt.

**Aktuelles Verhalten:** Der Admin-Modus ist nur aktiv, wenn er in der laufenden Browser-Sitzung ausdrücklich eingeschaltet wurde. „Beenden" fällt auf die echte Kanalansicht zurück und ein fehlendes oder gelöschtes Cookie reicht nicht mehr für Admin-Entsperrung.

## #349 — Der Bot erkennt Fragen nach Deadlock-Zugang wieder

**Problem:** Wer im Chat gefragt hat, wie man das Spiel überhaupt spielen kann oder wie man an eine Einladung kommt, bekam keine Antwort. Die frühere automatische Erkennung sprang zu oft auf harmlose Sätze an und wurde deshalb abgeschaltet. Ersetzt wurde sie durch einen Befehl, den neue Zuschauer naturgemäß nicht kennen.

**Änderung:** Solche Fragen werden wieder erkannt, aber nicht mehr allein per Stichwort. Ein grober Vorfilter sortiert vor, danach entscheidet die KI des Bots. Bei einem klaren Ja kommt der Weg zum Invite, bei Unsicherheit fragt der Bot einmal kurz nach, sonst bleibt er still.

**Aktuelles Verhalten:** Neue Zuschauer mit einer echten Zugangsfrage bekommen den Hinweis auf den Discord und den Tipp, ihren Steam-Freundescode gleich mitzuschicken. Das greift nur in Kanälen, die live Deadlock streamen. Nach einer Antwort hält sich der Bot zurück, mindestens zwei Minuten im Kanal und eine Stunde bei derselben Person. Nur wer direkt auf seine Rückfrage mit ja antwortet, bekommt den Invite sofort. Antwortet die KI gar nicht, sagt der Bot lieber nichts, und Streamer mit abgeschalteter Bot-Werbung bekommen den Hinweis nie.
## #348 — Befehle im Blick, und nur wenn Deadlock läuft

**Problem:** `!commands` hat bloß einen Link gepostet, statt zu zeigen, was der Bot kann. Und die Deadlock-Befehle liefen in jeder Kategorie, auch wenn gerade gar nicht Deadlock lief.

**Änderung:** `!commands` listet die Befehle jetzt direkt im Chat, mit Link zur vollen Übersicht. `!discord` ist neu und postet den Einladungslink. Jeder Befehl entscheidet außerdem selbst, ob er eine laufende Deadlock-Kategorie braucht.

**Aktuelles Verhalten:** Stats-Befehle wie `!rank` oder `!lastmatch` antworten nur, wenn der Kanal gerade Deadlock streamt, sonst bleibt der Bot still. `!commands`, `!help` und `!ping` gehen immer, ebenso die Mod-Befehle und das Abmelden vom Engagement-Tracking.

## #347 — Neue Promo-Texte für den Community-Discord

**Problem:** Die Discord-Hinweise im Chat zogen aus einem recht kleinen Satz an Formulierungen. Wer öfter im Stream war, kannte sie irgendwann alle auswendig.

**Änderung:** Drei neue Texte kommen dazu. Sie spielen darauf an, dass die Community auch dann weiterläuft, wenn der Stream längst offline ist.

**Aktuelles Verhalten:** Der Bot nimmt sie in die normale Rotation auf und sendet weiterhin nie zweimal hintereinander denselben Text. Wer einen eigenen Promo-Text hinterlegt hat, merkt von der Änderung nichts.

## #346 — Streamer-Seite: Die v2-Vorschau trägt jetzt den Community-Look

**Problem:** Die v2-Vorschau der Streamer-Seite war ein eigener Entwurf und driftete optisch von Hauptseite und Coaching-Bereich weg. Daneben lagen noch mehrere alte Testseiten herum.

**Änderung:** Die v2-Vorschau zeigt jetzt exakt die bekannte Streamer-Seite, nur in den Gold- und Teal-Tönen der Hauptseite. Die alten Vorschau-Unterseiten und Testseiten sind gelöscht.

**Aktuelles Verhalten:** Die Live-Seite bleibt unverändert. Unter der v2-Adresse lässt sich der neue Look ansehen, bis er gut genug für die Übernahme ist.

## #345 — Clip-Test nutzt den Gaming-Editor-Prompt

**Problem:** Der Clip-Test lieferte zwar Titel und Tags, aber noch nicht genug Material, um mehrere Clips sinnvoll zu bewerten oder direkt Social-Posts daraus abzuleiten.

**Änderung:** Die Vorschläge enthalten jetzt Hauptmoment, Content-Angle, zehn Titelideen, Captions, Hashtag-Gruppen, Pin-Kommentare, Handlungsaufrufe und Video-Hooks. Titel werden zusätzlich von Hashtags, Emoji und übertriebenen Hype-Wörtern bereinigt.

**Aktuelles Verhalten:** Mehrere bestehende Clips können in einem Testlauf ausgewertet werden. Die Ergebnisdateien zeigen pro Clip eine Editor-Auswertung plus Plattformvorschläge.

## #344 — Clip-Vorschläge liefern bessere Titelvarianten

**Problem:** Die ersten Clip-Vorschläge waren technisch korrekt, aber zu generisch: Titel enthielten teils Hashtags oder Emoji, Beschreibungen wiederholten Tags und pro Plattform gab es kaum Auswahl.

**Änderung:** Die Vorgaben für Social-Vorschläge sind enger: Titel bleiben sauber, pro Plattform entstehen mehrere Varianten, Beschreibungen bleiben frei von Tag-Blöcken und breite Füll-Tags werden begrenzt.

**Aktuelles Verhalten:** Ein Clip-Test zeigt jetzt pro Plattform einen Haupttitel plus mehrere auswählbare Alternativen. Tags stehen separat und die Texte sind näher am gesprochenen Clip-Moment.

## #343 — Dashboard liest Kennzahlen wieder aus den richtigen Spalten

**Problem:** Einige Dashboard-Abfragen benannten ihre Ergebnis-Spalten so, dass die Anwendung die Werte nicht unter dem erwarteten Namen fand. Dadurch konnten Monatswerte, Kategorievergleich und Raw-Chat-Status trotz vorhandener Daten auf Null oder „offline" zurückfallen.

**Änderung:** Die betroffenen Abfragen verwenden jetzt dieselben Spaltennamen, die beim Auslesen erwartet werden. Der Monats-Test prüft zusätzlich die echten Kennzahlen statt nur eine erfolgreiche Antwort.

**Aktuelles Verhalten:** Monatsstatistik, Kategorievergleich und Raw-Chat-Health übernehmen die berechneten Werte wieder direkt aus der Datenbank statt still auf Defaults zu fallen.

## #342 — Clip-Probe nutzt Fireworks für DeepSeek

**Problem:** Der Clip-Test erwartete einen direkten DeepSeek-Zugang, obwohl der vorhandene KI-Zugang im Bot über Fireworks läuft.

**Änderung:** Der Vorschlags-Schritt nutzt jetzt den vorhandenen Fireworks-Zugang und bleibt sonst beim gleichen Ablauf: Clip holen, transkribieren, Titel und Tags erzeugen.

**Aktuelles Verhalten:** Ein vorhandener Fireworks-Key reicht für den Titel- und Tag-Test aus. Ohne eigenen DeepSeek-Key muss nichts zusätzlich konfiguriert werden.

## #341 — Twitch-Dashboard zählt Chat und Raw-Chat wieder korrekt

**Problem:** Einige Dashboard-Auswertungen nutzten aufsummierte Session-Werte statt echte aktive Chatter. Raw-Chat konnte beim Streamstart bis zu einer Minute Nachrichten überspringen, wenn die Session noch nicht angelegt war. Außerdem liefen automatische KI-Stream-Reports ohne explizites Opt-in an.

**Änderung:** Die Dashboard-Zahlen lesen aktive Chatteilnehmer aus den Detaildaten und filtern bekannte Bots. Raw-Chat cached fehlende Sessions nicht mehr und die Health-Anzeige nutzt den gemessenen Ingest-Lag statt alte Chat-Stille. KI-Reports starten nur noch, wenn sie ausdrücklich aktiviert sind.

**Aktuelles Verhalten:** Monatswerte, Kategorievergleich und Session-Tabellen zeigen konsistent echte Chatwerte. Raw-Chat verliert beim Start keine Nachrichten mehr durch den Session-Cache, stille Chats erzeugen keinen falschen Lag-Alarm, und KI-Stream-Reports bleiben standardmäßig aus.

## #340 — Clip-Probe erstellt Transcript und Social-Vorschläge

**Problem:** Für bestehende Twitch-Clips fehlte ein schneller Testlauf, der zeigt, ob aus einem einzelnen Clip genug verwertbarer Kontext für Social-Titel und Tags entsteht.

**Änderung:** Ein interner Probe-Lauf kann den neuesten Clip eines Kanals holen, per Whisper transkribieren und die Ergebnisse für Social-Vorschläge vorbereiten. Fehlt der DeepSeek-Zugang, bleibt das Transcript trotzdem erhalten.

**Aktuelles Verhalten:** Für einen Clip entsteht eine auswertbare Datei mit Quelle, Transcript und späteren Titel-/Tag-Vorschlägen. Der Test kann ohne Dashboard und ohne Upload laufen.

## #339 — Raid-Hinweis erinnert ans Hallo im Zielchat

**Problem:** Nach Auto-Raids kamen einige Streamer im Zielchat nicht sichtbar an oder sagten dort nicht kurz Hallo. Dadurch wirkte der Zuschauer-Übergang unpersönlich.

**Änderung:** Der Bot schreibt beim Raid einen kurzen Hinweis in den Quellchat und merkt sich den Raid für ein kurzes Zielchat-Fenster. Sagt der raidende Streamer dort nicht Hallo, folgt eine freundliche Whisper-Erinnerung.

**Aktuelles Verhalten:** Bei Bot-Raids sehen Streamer und Chat sofort das Ziel und den Hallo-/Tschüss-Hinweis. Bleibt die Begrüßung im Zielchat aus, erinnert der Bot den Streamer privat.

## #338 — !rank zeigt die Unterstufe mit an

**Problem:** `!rank` zeigte nur den Hauptrang, obwohl der Steam-Dienst die Unterstufe kennt. Aus „Phantom 1" wurde dadurch im Chat nur „Phantom".

**Änderung:** Die Chat-Antwort liest die Unterstufe aus der Steam-Rangantwort und hängt sie an den Rang an.

**Aktuelles Verhalten:** `!rank` zeigt jetzt den exakten Rang, zum Beispiel „Phantom 1".

## #337 — Neue Streams bekommen wieder einen frischen Live-Post

**Problem:** Wenn ein Streamer am Vortag schon angekündigt wurde, konnte ein neuer Stream nur den alten Discord-Post aktualisieren. Dadurch erschien im Kanal kein neuer sichtbarer Go-Live-Post.

**Änderung:** Beim neuen Online-Signal wird ein alter Ankündigungsstatus verworfen, sobald Twitch eine neue Stream-Sitzung meldet oder derselbe Stream erst nach mehr als fünf Minuten zurückkommt.

**Aktuelles Verhalten:** Ein echter neuer Stream bekommt wieder einen neuen Discord-Post. Kurze Aussetzer desselben Streams bleiben bis fünf Minuten beim bestehenden Post.

## #336 — Offline-Posts zeigen VOD-Bilder wieder korrekt

**Problem:** Manche Stream-Ende-Posts bekamen von Twitch eine Aufzeichnungs-Vorschau mit Platzhaltern. Discord konnte diese URL nicht laden und zeigte deshalb ein leeres Bildfeld statt der VOD-Vorschau.

**Änderung:** Die Aufzeichnungs-Vorschau wird jetzt vor dem Senden vollständig aufgelöst, auch wenn Twitch die Platzhalter in einer anderen Schreibweise liefert.

**Aktuelles Verhalten:** Offline-Posts zeigen wieder ein echtes VOD-Bild oder fallen wie bisher auf das Kanalbild zurück.

## #335 — Live-Post: Offline-Bild, frische Vorschau, kein Doppel-Post bei kurzem Ausfall

**Problem:** Ging ein Stream offline, blieb das Bild im Post oft leer. Die Vorschau im Live-Post fror auf dem ersten Moment ein und aktualisierte sich nie. Und ein kurzer Verbindungsabbruch führte zu einem komplett neuen Post, statt den bestehenden weiterzuverwenden.

**Änderung:** Fehlt direkt nach Stream-Ende noch das Aufzeichnungs-Bild, zeigt der Post jetzt das Kanalbild statt nichts. Die Vorschau frischt sich alle paar Minuten mit dem aktuellen Stand auf. Kommt derselbe Stream nach einem kurzen Ausfall zurück, wird der bestehende Post von Offline wieder auf Live gestellt statt neu gepostet.

**Aktuelles Verhalten:** Ein Live-Post bleibt über den ganzen Stream dieselbe, aktuell gehaltene Nachricht — nur ein echter Neustart bekommt weiterhin einen frischen Post.

## #334 — Verdächtige Twitch-Spam-Alerts können manuell lernen

**Problem:** Verdächtige Twitch-Spam-Nachrichten wurden im Discord gemeldet, hatten aber keinen direkten Weg für menschliches Feedback. Positiv/negativ lernen gab es nur in einem anderen Scam-Pfad, nicht global für normale Spam-Alerts.

**Änderung:** Spam-Alerts liefern jetzt Lern-Metadaten mit und die interne Twitch-Schnittstelle kann ein Muster als Spam oder als harmlos speichern. Gespeichert wird in den vorhandenen Lernlisten des Spam-Filters.

**Verhalten jetzt:** Ein gemeldeter Fall kann nachträglich als „Spam" oder „harmlos" bestätigt werden. Dadurch landen auch neue Scam-/Viewerbot-Maschen in denselben Lernlisten statt nur im Alert-Verlauf.

## #333 — Streamer-Unban stoppt sofortige Global-Ban-Wiederholung

**Problem:** Wenn ein Streamer einen global gebannten Nutzer im eigenen Kanal wieder entbannt hat, konnte der Bot ihn bei der nächsten Chat-Nachricht direkt erneut bannen. Dadurch wirkte die Entscheidung des Streamers wirkungslos und die Bot-Nachricht wurde mehrfach wiederholt.

**Änderung:** Ein Kanal-Unban wird jetzt als bewusste Streamer-Entscheidung gespeichert. Danach greift der direkte Chat-Sofortban nicht mehr; der Eintrag wird erst wieder von der späteren Sweep-Welle geprüft.

**Verhalten jetzt:** Entbannt der Streamer jemanden im Kanal, lässt der Bot diese Person im laufenden Chat erst einmal in Ruhe. Die netzwerkweite Bannliste bleibt bestehen und wird beim nächsten Sweep wieder angewendet.

## #332 — Twitch-Live- und Offline-Post im frischen Deadlock-Look

**Ausgangslage:** Wenn jemand live ging, sah der Discord-Post aus wie bei jedem beliebigen Twitch-Bot — Twitch-Lila, die Infos als kleine Tabelle, und das Profilbild des Streamers fehlte komplett.

**Änderung:** Live- und Offline-Post sind neu aufgebaut: Deadlock-Gold statt Lila, eine klare Kopfzeile mit 🔴 „live" bzw. 💤 „Stream beendet", eine kompakte Info-Zeile (Zuschauer · Uptime · Sprache, offline: Kategorie · Laufzeit), die Tags des Streams und endlich das echte Twitch-Profilbild. Der Offline-Post wirkt bewusst ruhiger.

**Verhalten jetzt:** Der Button „Auf Twitch ansehen" bzw. „VOD anschauen" bleibt inklusive Klick-Zählung erhalten. Alle anderen Bot-Nachrichten sind unverändert.

## #331 — Go-Live-Ankündigung pingt keine Rolle mehr

**Problem:** Wenn ein Streamer live ging, hat der Ankündigungs-Post im Discord eine Rolle angepingt. Das war als Benachrichtigung gedacht, wurde aber als unnötiges Rauschen empfunden.

**Änderung:** Der Go-Live-Post erwähnt und pingt jetzt keine Rolle mehr — weder eine feste, noch eine pro Streamer, noch eine automatisch angelegte. Die eigentliche Ankündigung (Embed mit Titel, Vorschau und Link) bleibt unverändert.

**Verhalten jetzt:** Live-Posts erscheinen wie gewohnt im Kanal, nur ohne Rollen-Ping. Es wird niemand mehr per Ping über einen Go-Live benachrichtigt.

## #330 — Affiliate-Portal zeigt wieder das Affiliate-Portal statt der Analytics-Ansicht

**Problem:** Wer das Affiliate-Portal öffnete, landete auf der Analytics-Dashboard-Oberfläche statt auf der eigentlichen Affiliate-Ansicht mit Login, Streamer-Claims, Provisionen und Stripe-Auszahlungen — die Portal-Adresse lieferte schlicht das falsche Bundle.

**Änderung:** Das Affiliate-Portal liefert jetzt seine eigene, dedizierte Oberfläche aus. Verwaiste Affiliate-Altkopien in der Analytics-Ansicht wurden entfernt, damit beide Bereiche sauber getrennt bleiben.

**Verhalten jetzt:** Das Affiliate-Portal öffnet die richtige Affiliate-Ansicht; das Analytics-Dashboard bleibt unberührt. Das Programm bleibt bis zur Freischaltung inaktiv.

## #329 — Verdächtige Twitch-Spam-Nachrichten gehen wieder ins AI-Learning

**Ausgangslage:** Eine verdächtige Twitch-Nachricht mit Zuschauer-Kauf-Muster wurde zwar gemeldet, aber nicht an das Lernmodul weitergegeben, weil intern zusätzlich eine bereits bekannte Spam-Domain verlangt wurde. Genau dadurch konnten neue oder absichtlich verschleierte Schreibweisen nicht gelernt werden.

**Änderung:** Jeder Treffer mit positivem Spam-Score darf jetzt in den AI-Review. Das Lernmodul entscheidet danach selbst, ob daraus ein neues Spam-Muster oder ein Safe-Muster wird.

**Ergebnis:** Nachrichten wie der obfuskierte `PeakPy. c0m`-Fall landen künftig im Lernpfad, auch wenn die Domain noch nicht bekannt ist. Die eigentliche Spam-Erkennung bleibt unverändert; entfernt wurde nur das zu enge Vorschalt-Gate vor dem Lernen.

## #328 — Twitch-Analyse zeigt wieder echte Session-Dauern

**Problem:** Im Twitch-Analyse-Dashboard konnten beendete Streams plötzlich Laufzeiten von mehreren hunderttausend Stunden anzeigen. Ursache war ein Zeitformat, das beim Speichern nicht sauber verstanden wurde und dadurch wie ein Start im Jahr 1970 behandelt wurde.

**Änderung:** Stream-Zeiten werden jetzt auch in den Datenbankformaten korrekt erkannt. Die Übersicht berechnet beendete Sessions zusätzlich aus Start- und Endzeit, damit bereits gespeicherte kaputte Dauerwerte nicht weiter die Anzeige verfälschen.

**Verhalten jetzt:** Neue Sessions bekommen keine absurden Laufzeiten mehr. Alte betroffene Sessions werden im Dashboard wieder mit ihrer tatsächlichen Dauer angezeigt.

## #327 — Bekannte Twitch-Nebenpfade melden nicht mehr als Warnung

**Problem:** Bekannte Sonderfälle tauchten nach einem Neustart weiter als Warn-/Fehlerrauschen auf: gebannte Kanäle wurden im Folgepfad erneut für Moderator-Events versucht, der reauth-abhängige Moderator-Guard meldete erwartbare 403er als Warnung, und der optionale IRC-Reader meldete sein bewusstes Nichtstarten als Task-Fehler.

**Änderung:** Blockierte Chat-Kanäle werden jetzt auch beim Moderator- und First-Message-Abgleich übersprungen. Der optionale IRC-Reader bleibt bei leerer Konfiguration still aktiv statt als beendeter Hintergrundtask zu erscheinen.

**Verhalten jetzt:** Bekannte Bot-Bans und Moderator-Guard-403er bleiben sichtbar, aber nicht mehr als Warnwelle. Ein leerer IRC-Reader ist kein Fehler mehr.

## #326 — Twitch-403-Wellen bei Moderator-Telemetrie beruhigt

**Problem:** Kanäle, in denen der Bot nicht als Moderator nutzbar war oder sogar gebannt ist, haben regelmäßig ganze Wellen von Twitch-403-Meldungen ausgelöst. Das war meist kein akuter Ausfall, sah im Audit aber wie eine Störung aus.

**Änderung:** Der Bot versucht die zusätzliche Moderator-Telemetrie nur noch über seinen eigenen Token, wenn der Moderator-Guard für den Kanal wirklich steht. Ist der Bot im Kanal gebannt, wird das als eigener Zustand behandelt und nicht mehr im kurzen Re-Mod-Kreis wiederholt.

**Verhalten jetzt:** Echte Chat- und Core-Subscriptions heilen weiter automatisch. Erwartbare Reauth-/Mod-Lücken bleiben intern sichtbar, erzeugen aber keine Warnwelle mehr.

## #325 — Affiliate-Anmeldung läuft über dieselbe Twitch-Anmeldung

**Ausgangslage:** Für die Affiliate-Anmeldung gab es bisher eine eigene, zweite Twitch-Rückleitung — getrennt von der normalen Twitch-Anmeldung.

**Änderung:** Beide laufen jetzt über genau eine Twitch-Rückleitung. Das Backend erkennt beim Zurückkommen automatisch, ob sich jemand fürs Dashboard oder fürs Affiliate-Programm anmeldet, und leitet an die richtige Stelle weiter.

**Ergebnis:** Nur noch eine Anmelde-Adresse statt zwei; am Ablauf ändert sich für dich nichts. Das Programm bleibt bis zur Freischaltung inaktiv.

## #324 — Affiliate-Claims sind jetzt zeitlich gebundene Reservierungen

**Ausgangslage:** Bisher konnte ein Affiliate jeden noch nicht beanspruchten Streamer beanspruchen — zeitlich unbegrenzt und egal, ob er ihn wirklich neu angeworben hat.

**Änderung:** Ein Claim ist jetzt eine Reservierung rund um den Moment, in dem der geworbene Streamer bei uns aktiv wird: bis 4 Tage im Voraus reservierbar, bis 24 Stunden nach der Aktivierung nachholbar. Läuft die Reservierung ohne Aktivierung ab, wird der Streamer wieder frei. Provision fließt nur, wenn der Claim in diesem Fenster liegt.

**Ergebnis:** Belohnt werden echte Neu-Anwerbungen statt Abgriffe auf längst etablierte Kanäle. Das Programm bleibt bis zur Freischaltung inaktiv.

## #323 — Affiliate-/Partner-Abrechnung läuft jetzt auf der neuen Rust-Basis

**Ausgangslage:** Das Affiliate-System — Anmeldung, Provisionen, monatliche Gutschriften und die interne Verwaltung — lief noch auf der alten Python-Oberfläche und fehlte in der neuen Dashboard-Basis komplett.

**Änderung:** Das gesamte System ist jetzt in der neuen Basis nachgebaut: Anbindung an den Zahlungsdienstleister, verschlüsselte Stammdaten, monatliche Gutschriften mit Steuerausweis, automatische Erzeugung und die Verwaltungsansicht.

**Ergebnis:** Die Grundlage steht vollständig auf der neuen Basis. Das Programm bleibt bis zur bewussten Freischaltung inaktiv — es zahlt nichts aus, solange nichts konfiguriert und niemand verknüpft ist.

## #322 — Stream-Vorschaubild ist zurück in den Live-Ankündigungen

**Problem:** Seit der Umstellung auf die neue Bot-Version fehlte in den Live-Ankündigungen das große Vorschaubild vom Stream — das Embed war nur noch Text. Die Bildinfo kam von Twitch zwar an, wurde intern aber schlicht nie bis ins Embed weitergereicht.

**Änderung:** Die Kette ist geflickt: Live-Ankündigungen zeigen wieder das aktuelle Stream-Vorschaubild in voller Breite, genau wie früher. Damit Discord nicht tagelang ein altes gecachtes Bild anzeigt, bekommt jede Ankündigung ihre eigene Bild-Adresse. Auch die ping-freie Wiederankündigung nach kurzen Stream-Aussetzern bekommt das Bild.

**Ergebnis:** Live-Posts sehen wieder nach was aus — Vorschaubild drin, Offline-Embeds bleiben wie gehabt.

## #321 — Dashboard akzeptiert wieder seinen konfigurierten Port

**Ausgangslage:** Die neue Start-Absicherung des Dashboards verglich den Port gegen einen fest verdrahteten Standardwert statt gegen den tatsächlich konfigurierten. Auf dem regulären Live-Port hätte eine scharf geschaltete Absicherung den Start abgelehnt.

**Änderung:** Die Absicherung akzeptiert jetzt jeden konfigurierten Port und blockiert nur noch den fürs Master-Backend reservierten Port.

**Ergebnis:** Die Start-Absicherung lässt sich gefahrlos aktivieren, ohne den laufenden Betrieb auf dem regulären Port zu behindern.

## #320 — Stream-Ankündigungen: keine Doppel-Posts mehr, keine Pings bei Kurz-Aussetzern

**Problem:** Ging ein Stream kurz offline oder meldete Twitch das Spiel kurzzeitig falsch, hielt der Bot das für einen neuen Stream und postete die Live-Ankündigung doppelt — inklusive erneutem Rollen-Ping. Dazu zeigten Ankündigungen direkt nach Stream-Start „0 Zuschauer", was billiger aussieht als es ist.

**Änderung:** Der Bot erkennt jetzt am Stream selbst, ob es wirklich ein Neustart ist oder nur ein Flackern der Twitch-Daten — beim Flackern passiert nichts mehr. Startet ein Stream innerhalb von 15 Minuten nach dem Ende wirklich neu, kommt die Ankündigung ohne Rollen-Ping; das 15-Minuten-Fenster zählt dabei ab dem jeweils letzten Stream-Ende. Und solange Twitch noch keine Zuschauerzahl liefert, wird das Feld einfach weggelassen.

**Ergebnis:** Eine Live-Ankündigung pro Stream, Pings nur wenn es sich lohnt, und keine peinliche Null mehr im Embed.
## #319 — Etliche Rand-Unterschiede zwischen alter und neuer Bot-Version aufgeräumt

**Ausgangslage:** Beim systematischen Vergleich der alten Python- mit der neuen Rust-Version sind viele kleine Abweichungen aufgefallen — Stellen, an denen die neue Version in Randfällen anders reagierte als gewollt: verschluckte Datenbankfehler, falsch gerundete Werte, fehlende Chat-Befehle und ein paar zu lockere Zugriffsprüfungen.

**Änderung:** Die eindeutigen Bugs davon haben wir behoben — u.a. die AI-Engagement-Befehle (an/aus/status) wieder verfügbar gemacht, die Neu-Autorisierungs-Sperre für privilegierte Befehle nachgezogen, fehlerhafte Webhook-Antworten korrigiert und mehrere Stellen, die Fehler still verschluckten, zum Melden gebracht. Unklare oder bewusst gewollte Abweichungen haben wir stehen lassen.

**Verhalten jetzt:** In diesen Randfällen verhält sich die neue Version wieder wie die alte.

## #318 — MiniMax-Ledger wird getrennt gegen SQLite geprüft

**Ausgangslage:** Das gemeinsame MiniMax-Verbrauchsledger nutzt eine SQLite-Datei, während die übrige SQLx-Prüfung gegen Postgres läuft. Seine Abfragen waren noch reine Laufzeit-SQL und durften nicht in den Postgres-Prüflauf geraten.

**Änderung:** Die Ledger-Abfragen werden jetzt beim Bauen gegen einen eigenen SQLite-Cache geprüft. Die CI baut dafür eine separate SQLite-Prüfdatenbank; der bestehende Postgres-Job lässt diese Spur bewusst aus.

**Ergebnis:** Eine abweichende Ledger-Spalte fällt künftig beim Prüflauf auf, ohne den Postgres-Cache zu vermischen. Lokal bestätigt: Tests bleiben unverändert grün, Offline-Build funktioniert, und ein absichtlich kaputtes SQLite-Schema scheitert beim Prüfen.

## #317 — Siebte und letzte Welle: die Auswertungs-Engine wird beim Bauen geprüft

**Ausgangslage:** Das Herzstück der Auswertung — Streamer- und Partner-Status, Affiliate- und Provisionsrechnung, Watch-Time, Raid-Historie, Post-Stream-Berichte, Stripe-Abgleich, Chat-Statistik — führte seine statischen Datenbank-Abfragen bisher als reine Textbausteine aus. Es ist der größte Brocken dieser Umstellung und der letzte.

**Änderung:** 258 statische Abfragen werden jetzt schon beim Bauen gegen ein frisch aus dem Bauplan erzeugtes Schema geprüft. Drei Stellen bleiben bewusst ungeprüft und sind markiert: zwei lesen aus einer Fehler-Log-Tabelle, die im Bauplan absichtlich nicht geführt wird (der Code verträgt ihr Fehlen seit jeher), eine zählt über einen erst zur Laufzeit bekannten Tabellennamen. Dabei wurden mehrere Werte wieder passgenau geführt (Ja/Nein-Felder, Zahlbreiten), und ein stiller Fehler wurde gefangen: eine Abfrage für den Post-Stream-Bericht hätte nach dem Umbau leere bzw. unklassifizierte Chat-Zeilen mitgezählt, die vorher bewusst aussortiert waren — auf das alte Verhalten zurückgedreht.

**Ergebnis:** Verschwindet oder ändert sich in der Auswertungs-Engine eine Spalte, scheitert künftig der Bau statt der laufende Dienst. An einer nachgestellten Kopie bewiesen: Bauen ohne Datenbank läuft, eine künstlich umbenannte Spalte lässt den Bau sofort scheitern, und die volle Testsuite ist Zeile für Zeile gleich grün wie vorher (359 Tests). Damit ist die gesamte Umstellung abgeschlossen. Für den Betrieb ändert sich nichts.

## #316 — Sechste Welle: das Streamer-Dashboard-Backend wird beim Bauen geprüft

**Ausgangslage:** Die Schicht hinter dem Dashboard — Übersichten, Zuschauer- und Raid-Auswertungen, Abrechnungs- und Affiliate-Seiten, Clip-Verwaltung und Admin-Werkzeuge — führte 172 ihrer statischen Datenbank-Abfragen bisher als reine Textbausteine aus. Eine Schema-Änderung wäre erst zur Laufzeit aufgefallen, wenn der betroffene Endpunkt schon hängt.

**Änderung:** 171 davon werden jetzt schon beim Bauen gegen ein frisch aus dem Bauplan erzeugtes Schema geprüft. Eine einzelne Abfrage bleibt bewusst ungeprüft und ist als solche markiert: Sie liest einen Anzeigenamen aus einer Tabelle, die diese Spalte gar nicht (mehr) führt, und fällt seit jeher still auf den Login-Namen zurück — als eigener Punkt notiert. Beim Umbau wurden mehrere Werte wieder passgenau geführt (Ja/Nein-Felder als solche statt als Zahlen, Kennungen in der Breite, die die Spalte vorgibt), damit das Verhalten exakt gleich bleibt.

**Ergebnis:** Verschwindet oder ändert sich im Dashboard-Backend eine Spalte, scheitert künftig der Bau statt der laufende Dienst. An einer nachgestellten Kopie bewiesen: Bauen ohne Datenbank läuft, eine künstlich umbenannte Spalte lässt den Bau sofort scheitern, und die volle Testsuite ist Zeile für Zeile gleich grün wie vorher (687 Tests, kein einziger Unterschied im Verhalten). Für den Betrieb ändert sich nichts.

## #315 — Fünfte Welle: die Social-Media-Clip-Pipeline beim Bauen geprüft — und mehrere stille Altlasten behoben

**Ausgangslage:** Der Teil, der Clips einsammelt, freigibt und automatisch auf die Plattformen hochlädt, führte seine 79 statischen Datenbank-Abfragen bisher als reine Textbausteine aus — Schema-Brüche fielen erst im laufenden Betrieb auf.

**Änderung:** Alle 79 werden jetzt schon beim Bauen gegen ein frisch aus dem Bauplan erzeugtes Schema geprüft. Dabei kamen mehrere echte Altlasten ans Licht, die im Betrieb stille Fehler verursacht hätten: ein paar Ja/Nein-Felder wurden noch wie Zahlen behandelt, das Datum eines erfolgreichen Uploads wurde im falschen Format geschrieben — was die Fertig-Markierung hätte scheitern lassen — und einzelne Abfragen hätten bei einem leeren Wert oder einer ungewöhnlich großen Kennung still ausgesetzt, statt sauber weiterzulaufen. Alle behoben.

**Ergebnis:** Schema-Brüche fliegen künftig beim Bauen auf statt im Betrieb, und die Upload-Pipeline markiert fertige Uploads wieder zuverlässig — ohne dass ein einzelner Ausreißer den ganzen Lauf blockiert. Für den normalen Betrieb ändert sich nichts.

## #314 — Vierte Welle, zweiter Teil: das Raid-, Partner- und Token-Innenleben wird beim Bauen geprüft

**Ausgangslage:** Der größte einzelne Brocken dieser Umstellung ist der Maschinenraum hinter den Auto-Raids: Token-Verwaltung und -Erneuerung, Partner-Bewertung und -Liste, die Sperrlisten der Raid-Ziele, Anwerbung und Ankunfts-Erkennung. 107 Datenbank-Abfragen, bisher erst zur Laufzeit auf Passung geprüft — ausgerechnet in dem Teil, der unbeaufsichtigt im Hintergrund läuft.

**Änderung:** Alle 107 werden ab jetzt schon beim Bauen gegen ein frisch aus dem Bauplan erzeugtes Schema geprüft. Dabei fielen drei Stellen auf, an denen ein Wert breiter geführt wurde, als die Spalte ihn überhaupt fassen kann — verlustfrei zurechtgerückt, weil die Spalte den Wertebereich ohnehin begrenzt. Fünf dynamisch zusammengesetzte Abfragen bleiben bewusst ungeprüft und sind als solche markiert.

**Ergebnis:** Verschwindet oder ändert sich in diesem Hintergrund-Bereich eine Spalte, scheitert künftig der Bau statt erst der unbeaufsichtigte Betrieb. An einer nachgestellten Kopie bewiesen: Bauen ohne Datenbank läuft, eine künstlich umbenannte Spalte lässt den Bau sofort scheitern, und eine zweite, unabhängige Prüfung bestätigte, dass keine „darf nicht leer sein"-Festlegung zur Laufzeit umkippt. Für den Betrieb ändert sich nichts.

## #313 — Auto-Raid-Sperrquelle bereinigt

**Ausgangslage:** Der Auto-Raid-Filter las gebannte Ziele noch aus zwei Quellen, obwohl die globale Chatter-Ban-Liste inzwischen die maßgebliche Wahrheit für gebannte Accounts ist.

**Änderung:** Die Auswahl lädt weiter die explizite Raid-Blacklist und die globale Chatter-Ban-Liste. Der alte `twitch_exclusions kind='banned'`-Zweig ist entfernt.

**Ergebnis:** Das Verhalten bleibt gleich streng, aber die Sperrquelle ist klarer: Raid-Blacklist für reine Raid-Sperren, globale Ban-Liste für gebannte Accounts.

## #312 — Vierte Welle: interne Streamer-API compile-fest, plus ein Altlast-Fehler bei der Discord-Zuordnung

Die interne API rund um Streamer-Status, Analytics und Partner-Lebenszyklus hat ihre Datenbankabfragen bisher als reine Textbausteine ausgeführt — eine Schema-Änderung wäre erst zur Laufzeit aufgefallen, wenn der betroffene Endpunkt schon hängt. Wir haben die 51 statischen Abfragen dieser Schicht auf zur Bauzeit geprüfte Varianten umgestellt: Passt eine Abfrage nicht mehr zum echten Schema, scheitert schon der Build statt später der laufende Dienst.

Beim Umbau fiel ein Altlast-Fehler auf: Eine Abfrage las die Discord-Zuordnung eines Streamers noch aus einer Tabelle, die diese Felder beim großen Umzug längst abgegeben hatte — sie lieferte still nichts zurück. Jetzt wird die Zuordnung wieder über den Login gesucht, so wie ursprünglich gedacht.

Unterm Strich: Schema-Brüche fliegen beim Bauen auf statt im Betrieb, und ein Streamer mit hinterlegter Discord-Verknüpfung wird wieder zuverlässig erkannt — inklusive korrekter Partner-Rolle und Discord-Markierung.

## #311 — Dritte Welle: Stream-Überwachung und Chat-Aktionen — und vier Beinahe-Abstürze abgefangen

**Ausgangslage:** Die größte Umstellung bisher nimmt sich zwei Brocken auf einmal vor: den Teil, der live mitschaut (Stream-Status, Sessions, Telemetrie, die interne Aufgaben-Warteschlange) und den Teil, der im Chat handelt (Moderation, Promos, Scam-Schutz, Titel-Generator). Zusammen 197 Datenbank-Abfragen, bisher erst zur Laufzeit auf Passung geprüft.

**Änderung:** Alle 197 werden ab jetzt schon beim Bauen gegen ein frisch aus dem Bauplan erzeugtes Schema geprüft. Dabei fielen vier Stellen auf, an denen eine Abfrage einen Wert als „immer gefüllt" behandelte, obwohl die Spalte leer sein kann — genau die Sorte stiller Fehler, die sonst erst im Betrieb als Absturz hochkommt. Sie sind jetzt abgesichert: ein leerer Wert wird zum sinnvollen Standard, statt das Programm umzuwerfen.

**Ergebnis:** Verschwindet oder ändert sich in einem dieser großen Bereiche eine Spalte, scheitert künftig der Bau statt der Betrieb. An einer nachgestellten Kopie bewiesen: Bauen ohne Datenbank läuft, eine künstlich umbenannte Spalte lässt den Bau sofort scheitern, und zwei unabhängige Prüfungen bestätigten, dass keine „darf nicht leer sein"-Festlegung zur Laufzeit umkippt. Für den Betrieb ändert sich nichts.

## #310 — Zweite Welle: das ganze Chat-Gehirn wird jetzt beim Bauen geprüft

**Ausgangslage:** Nach dem Mini-Auftakt aus #308 kommt mit Abstand der größte Brocken: der Teil, der entscheidet, wann und wie der Bot sich überhaupt am Chat beteiligt — also Gesprächsgedächtnis, Persönlichkeit, Stimmungslage, Lauerer-Signale, Stream-Mitschriften und die Absender-Anmeldung. Diese 47 Datenbank-Abfragen wurden bisher erst zur Laufzeit auf Passung geprüft.

**Änderung:** Alle 47 werden ab jetzt schon beim Bauen gegen ein frisch aus dem Bauplan erzeugtes Schema geprüft. Für jede Spalte ist dabei sauber hinterlegt, ob sie leer sein darf oder nicht — exakt so, wie der Code den Wert danach weiterverwendet, ohne Verhaltensänderung. Eine einzige Sammel-Abfrage, die drei feste Status-Aktualisierungen bündelt, bleibt bewusst ungeprüft und ist als solche markiert.

**Ergebnis:** Verschwindet oder ändert sich in diesem großen Bereich eine Spalte, scheitert künftig der Bau statt erst der Betrieb. An einer nachgestellten Kopie bewiesen: Bauen ohne Datenbank läuft, eine künstlich umbenannte Spalte lässt den Bau sofort scheitern, und eine zweite, unabhängige Prüfung bestätigte, dass keine der „darf nicht leer sein"-Festlegungen zur Laufzeit umkippen kann. Für den Betrieb ändert sich nichts.

## #309 — Auto-Raids meiden jetzt zuverlässig gesperrte Kanäle

**Ausgangslage:** Beim automatischen Raid am Stream-Ende wählte der Bot sein Ziel nur anhand einer einzigen Sperrliste. Kanäle, die über andere Wege gesperrt waren — global gebannte Accounts und kanalweite Hard-Bans — standen auf keiner der geprüften Listen und konnten trotzdem als Raid-Ziel landen. Beinahe wäre ein hart gesperrter Kanal angeraidet worden.

**Änderung:** Die Zielauswahl prüft jetzt vor jedem Raid alle Sperrquellen gemeinsam: die klassische Raid-Sperrliste, global gebannte Accounts und aktive kanalweite Bans. Auch Sperren, die nur per Account-ID ohne hinterlegten Kanalnamen vorliegen, greifen jetzt. Lädt eine der Listen nicht, bricht der Raid sicher ab, statt ungefiltert weiterzulaufen.

**Ergebnis:** Ein gesperrter Kanal kann nicht mehr versehentlich angeraidet werden — egal über welchen Weg er gesperrt wurde. Die Auswahl überspringt ihn und nimmt das nächste erlaubte Ziel.

## #308 — Erste Welle: Partner-Abfrage des Clip-Sammlers beim Bauen geprüft

**Ausgangslage:** Mit dem Prüf-Verfahren aus #307 als Fundament geht es jetzt Bereich für Bereich an die eigentliche Umstellung. Den Auftakt macht der kleinste Baustein: die Abfrage, mit der der Highlight-Clip-Sammler die aktiven Partner-Streamer aus der Datenbank holt.

**Änderung:** Diese Abfrage wird ab jetzt schon beim Bauen gegen ein frisch aus dem Bauplan erzeugtes Schema geprüft — inklusive der Feinheit, dass zwei eigentlich als „darf leer sein" geführte Spalten hier bewusst als gefüllt behandelt werden, weil die Abfrage genau das schon sicherstellt. Reine Test-Abfragen, die sich ihr eigenes Wegwerf-Schema bauen, bleiben absichtlich ungeprüft.

**Ergebnis:** Verschwindet oder ändert sich diese Spalte im echten Schema, scheitert künftig der Bau statt erst der Betrieb. An einer nachgestellten Kopie bewiesen: Bauen ohne Datenbank läuft, eine künstlich umbenannte Spalte lässt den Bau sofort scheitern, die echte Abfrage prüft sauber durch. Für den Betrieb ändert sich nichts.
## #307 — Datenbank-Abfragen werden ab jetzt schon beim Bauen geprüft (Pilot + Fundament)

**Ausgangslage:** Ob eine Abfrage zur Datenbank passt — gibt es die Spalte, stimmt der Typ — stellte sich bisher erst zur Laufzeit heraus. Liefen Bauplan und Datenbank auseinander, fiel ein Fehler im schlimmsten Fall erst im Betrieb auf, als Absturz statt als klare Meldung. Genau diese Klasse von Brüchen hat die letzten Schema-Aufräumungen ausgelöst.

**Änderung:** Wir stellen die Abfragen schrittweise auf eine Form um, die beim Bauen gegen ein frisch aus dem Bauplan erzeugtes Schema geprüft wird. Das Prüfergebnis wird als Zwischenstand mitversioniert, sodass das Bauen weiterhin ganz ohne Datenbank-Zugriff auskommt — wichtig für Auslieferung und nächtliche Abläufe. Ein neues Prüf-Tor vergleicht diesen Zwischenstand zusätzlich gegen das echte Schema. Den Anfang macht ein kleiner, vollständig umgestellter Baustein als Blaupause; die übrigen Bereiche folgen in Wellen.

**Ergebnis:** Eine fehlende oder umbenannte Spalte ist jetzt ein Bau-Fehler statt eines Laufzeit-Absturzes. An einer nachgestellten Kopie bewiesen: das Bauen ohne Datenbank läuft, eine absichtlich gebrochene Abfrage lässt den Bau sofort scheitern, und die geprüften Abfragen laufen sauber gegen das echte Schema. Für den Betrieb ändert sich nichts.

## #306 — Schema-Aufräumung: beim Start angelegte Tabellen in den Bauplan überführt

**Ausgangslage:** Elf Tabellen existierten in der laufenden Datenbank nur, weil der Programmcode sie beim Start selbst anlegte — im Bauplan fehlten sie. Eine frisch aus dem Bauplan aufgesetzte Datenbank hatte sie deshalb gar nicht, und das Schema ließ sich nicht allein aus dem Bauplan reproduzieren. Dieselbe Tabelle an zwei Stellen zu beschreiben (Lücke im Bauplan, Anlage zur Laufzeit) birgt die Gefahr, dass beide mit der Zeit auseinanderlaufen.

**Änderung:** Die elf Tabellen sind jetzt echte Bauplan-Schritte — Struktur, Schlüssel und Indizes exakt aus dem Live-Stand übernommen, in vier thematischen Gruppen. Jeder Schritt ist so abgesichert, dass er auf der laufenden Datenbank nichts anfasst (die Tabellen sind dort längst da) und nur eine neu aufgebaute Datenbank angleicht; mehrfaches Anwenden bleibt folgenlos. Wo der Code eine Tabelle bisher beim Start selbst anlegte, ist diese doppelte Anlage entfernt — der Bauplan ist nun die einzige Quelle.

**Ergebnis:** Das Schema lässt sich vollständig aus dem Bauplan reproduzieren; eine frisch aufgesetzte Datenbank gleicht der Produktion exakt. An einer nachgestellten Kopie geprüft: Spalten, Schlüssel und Indizes stimmen Tabelle für Tabelle überein, und der erneute Lauf auf einer bereits bestehenden Datenbank bleibt folgenlos. Für den Betrieb ändert sich nichts.

## #305 — Schema-Aufräumung: Streamer-Tabelle durchgängig über den Login verschlüsselt

**Ausgangslage:** Die Streamer-Tabelle trug in der laufenden Datenbank noch eine alte, nirgends genutzte Zahlen-ID als Primärschlüssel mit — ein Überbleibsel aus früheren Zeiten. Der eigentliche Schlüssel ist längst der Twitch-Login. Eine frisch aufgesetzte Datenbank war bereits auf den Login umgestellt, die laufende noch nicht — wieder eine Stelle, an der Bauplan und Realität auseinanderliefen.

**Änderung:** Die tote ID-Spalte samt Zähler wird entfernt und der Primärschlüssel auf den Twitch-Login gelegt. Der einzige Verweis aus einer anderen Tabelle wird dabei sauber umgehängt. Der Schritt fasst nur die laufende Datenbank an (eine frische ist schon umgestellt → folgenlos) und bleibt auch bei mehrfacher Anwendung folgenlos.

**Ergebnis:** Streamer werden überall einheitlich über ihren Login geführt; die doppelte Schlüssel-Buchführung entfällt. An einer nachgestellten Kopie des Produktions-Zustands geprüft, dass die Umstellung sauber durchläuft und alle Verknüpfungen intakt bleiben.

## #304 — Schema-Aufräumung: tote Tabellen raus, Zeitreihen-Tabellen wie in Produktion

**Ausgangslage:** Eine frisch aus dem Bauplan aufgesetzte Datenbank wich noch von der laufenden ab. Sie schleppte vier alte Einmal-Sicherungskopien aus früheren Datenkorrekturen sowie eine längst ungenutzte Tabelle mit, und 19 große Ereignis- und Statistik-Tabellen waren dort nicht als komprimierte Zeitreihen-Tabellen angelegt, wie es die echte Datenbank längst tut. Auffallen würde das erst beim nächsten Neuaufbau (Test, neue Umgebung): andere Struktur, kein automatisches Wegkomprimieren alter Daten.

**Änderung:** Die toten Tabellen werden entfernt. Die 19 Tabellen werden in Zeitreihen-Tabellen umgewandelt — mit exakt denselben Zeitfenstern und Kompressions-Einstellungen wie in Produktion, Tabelle für Tabelle aus dem Live-Stand abgeleitet. Jeder Schritt ist so abgesichert, dass er auf der laufenden Datenbank nichts anfasst (sie ist bereits im Zielzustand) und ausschließlich eine neu aufgebaute Datenbank angleicht; mehrfaches Anwenden bleibt folgenlos.

**Ergebnis:** Bauplan und laufende Datenbank sind in diesem Bereich wieder deckungsgleich. Eine frisch aufgesetzte Umgebung bekommt automatisch dieselbe Struktur und dasselbe Speicher- und Kompressionsverhalten. Gegen die echte Datenbank Spalte für Spalte und Tabelle für Tabelle gegengeprüft.

## #303 — Demo: Streams-Tabelle, Kategorie-Ranking und Coaching jetzt voll gefüllt

**Ausgangslage:** Nach der letzten Demo-Reparatur liefen alle Tabs fehlerfrei, drei Bereiche blieben aber nur halb gefüllt: Die Stream-Tabelle ließ mehrere Spalten leer, und Kategorie-Ranking wie Coaching-Bereich wirkten dünn — ihre Beispieldaten enthielten nur einen Bruchteil dessen, was das Dashboard dort eigentlich darstellt.

**Änderung:** Wir haben die Beispieldaten dieser drei Bereiche auf die volle Form gebracht. Jeder Stream in der Tabelle trägt jetzt sämtliche Kennzahlen — Zuschauerverlauf, Retention an mehreren Zeitmarken, Chatter-Aufschlüsselung und Follower-Stand. Das Kategorie-Ranking zeigt eine komplette Rangliste mit dem Beispiel-Profil auf Platz 12 von 58. Und der Coaching-Bereich ist von ein paar Stichpunkten zur vollen Auswertung gewachsen: Effizienz, Titel- und Tag-Analyse, Sendeplan, Retention-Kurve, Raid-Netzwerk, Peer-Vergleich und fünf konkrete Empfehlungen, deren Begründung sich jeweils direkt aus den Beispielzahlen ableitet.

**Ergebnis:** Die Demo zeigt jetzt auch auf Streams, Markt und Coaching ein vollständig gefülltes Dashboard statt halbleerer Kacheln — so, wie es echte Streamer mit eigenen Daten sehen. Ein Klick-Test durch alle Tabs bestätigt, dass nichts abbricht und die Inhalte erscheinen.

## #302 — Live-Demo: alle Tabs zeigen wieder vollständige Beispieldaten

**Ausgangslage:** Die öffentliche Demo des Analytics-Dashboards (über „Live Demo" auf der Seite erreichbar) brach auf mehreren Tabs mit einem „Dashboard-Fehler" ab — Übersicht, Publikum, Wachstum, Planung, Zuschauer und Monetization zeigten statt Auswertungen eine Fehlermeldung. Hintergrund: Die Demo läuft auf fest hinterlegten Beispieldaten statt auf echten Zahlen. Diese Beispieldaten waren nur grob angelegt und passten an vielen Stellen nicht mehr zu dem, was das Dashboard inzwischen erwartet — mal kam eine Liste, wo ein einzelner Block gebraucht wurde, mal fehlten Felder, mal hatte ein ganzer Bereich überhaupt keine Beispieldaten. Beim Aufbau der Seite lief das ins Leere.

**Änderung:** Wir haben die Beispieldaten für jeden betroffenen Bereich exakt an das angeglichen, was das Dashboard tatsächlich anzeigt, und die bis dahin fehlenden Demo-Bereiche ergänzt — durchgängig mit realistischen, in sich stimmigen Werten zum Beispiel-Profil (Zuschauerschnitt, Follower-Wachstum, Watch-Time, Funnel, Monetarisierung, Raids, Heatmaps). Ein automatischer Test sichert die Form dieser Beispieldaten dauerhaft ab; ein zweiter, unabhängiger Durchgang und ein kompletter Klick-Test durch alle Tabs haben bestätigt, dass nichts mehr abbricht.

**Ergebnis:** Die Demo zeigt jetzt auf allen Tabs vollständige, glaubwürdige Beispiel-Auswertungen — von der Übersicht über Publikum, Wachstum, Planung und Zuschauer bis Monetization — statt einer Fehlermeldung. Wer die Demo öffnet, sieht das Dashboard so, wie es echte Streamer im Alltag erleben.

## #301 — Schema-Bauplan wieder deckungsgleich mit der echten Datenbank

**Ausgangslage:** Die laufende Datenbank ist korrekt — aber die Migrationen, also der Bauplan, aus dem eine *neue* Datenbank entsteht, beschrieben für 83 Spalten noch die alten Typen (Text statt Zeitstempel, 32- statt 64-Bit-Zahlen, Zahl statt Wahr/Falsch). Auffallen würde das erst, wenn jemand eine frische Datenbank aufsetzt (Test, neue Umgebung, Wiederaufbau): die liefe sofort in dieselben Datentyp-Fehler wie zuletzt im Dashboard.

**Änderung:** Wir haben das echte Schema der laufenden Datenbank ausgelesen, eine frische Datenbank allein aus den Migrationen gebaut und beide Spalte für Spalte verglichen. Die 83 abweichenden Spalten (plus 10 Pflichtfeld-Marker) zieht jetzt eine neue, rückwärtskompatible Korrektur nach — strikt abgesichert, sodass sie auf der bestehenden Datenbank nichts anfasst und ausschließlich eine frisch gebaute angleicht.

**Ergebnis:** Eine frisch aus den Migrationen gebaute Datenbank ist jetzt Spalte für Spalte identisch mit der echten. Ein erweiterter Test baut diese Prüfung dauerhaft ein und fängt künftige Abweichungen automatisch ab.

## #300 — Admin-Dashboard durchgehärtet: alle Bereiche gegen das echte Schema abgeglichen

**Ausgangslage:** Nachdem ein Datentyp-Stolperstein die Streamer-Liste blockierte (#299), war klar: derselbe Bruch konnte überall im Admin-Dashboard lauern, weil quer durch die Auswertungen Felder im Code anders gelesen wurden, als sie in der Datenbank wirklich liegen — ein Erbe aus dem Umstieg, bei dem die Schema-Beschreibung von der laufenden Datenbank abgedriftet ist.

**Änderung:** Statt jeden Fehler einzeln abzuwarten, haben wir das **tatsächliche** Datenbank-Schema ausgelesen und alle Auswertungen des Dashboards (Streamer, Monetarisierung, Übersicht, Affiliate, OAuth/System, Startseite) in einem Rutsch dagegen abgeglichen — Wahr/Falsch-, Text-, Datums- und Zahlenfelder. Ein zweiter, unabhängiger Durchgang hat geprüft, dass dabei keine Bedeutung verfälscht wurde.

**Ergebnis:** Die Admin-Bereiche laden ihre Daten sauber, ohne reihum auf Serverfehler zu laufen.

## #299 — Admin-Dashboard: Streamer-Liste lädt wirklich (Datentyp-Stolperstein behoben)

**Ausgangslage:** Nachdem die Anmeldung an der Daten-Schnittstelle gefixt war (siehe #298), kam man zwar durch, aber die Streamer-Liste warf jetzt einen „internal server error". Ursache: Ein Wahr/Falsch-Feld in der Datenbank (braucht-Neu-Anmeldung) wurde im Code noch als Zahl gelesen — ein Überbleibsel aus der alten Generation, das beim Umstieg auf echte Wahr/Falsch-Felder nie mitgezogen wurde. Beim ersten echten Zugriff (vorher durch die Anmelde-Sperre verdeckt) brach das Auslesen ab.

**Änderung:** Die betroffenen Felder werden jetzt korrekt gelesen — abgeglichen mit dem tatsächlichen Datenbank-Schema. Das betraf zwei Spielarten desselben Stolpersteins: Wahr/Falsch-Felder, die noch als Zahl gelesen wurden, und mehrere Datums-/Zeit-Felder, die in der Datenbank als Text liegen, im Code aber als Zeitstempel erwartet wurden. Alle von der Streamer-Abfrage gelesenen Spalten wurden einmal komplett gegen das Schema abgeglichen (statt Fehler für Fehler), und eine zweite Stelle (OAuth-Scope-Übersicht) mit gemischten Typen ist ebenfalls bereinigt.

**Ergebnis:** Die Streamer-Liste und die OAuth-Scope-Übersicht laden im Admin-Dashboard sauber durch.

## #298 — Admin-Dashboard: Daten laden wieder, auch wenn man eingeloggt ist

**Ausgangslage:** Im Admin-Dashboard lud die Seite zwar, aber die eigentlichen Daten — Streamer-Liste, System-Status, Konfiguration, Roadmap — blieben leer mit „konnten nicht geladen werden", obwohl man klar als Admin angemeldet war. Hintergrund: Seite und Daten-Schnittstelle prüften die Anmeldung über zwei verschiedene Wege. Die Seite akzeptierte die Admin-Anmeldung, die Daten-Schnittstelle dahinter aber nur einen internen Server-Schlüssel — eine normale Browser-Anmeldung kannte sie nicht und wies sie ab.

**Änderung:** Eine gültige Admin-Anmeldung wird jetzt serverseitig auf den Weg übersetzt, den die Daten-Schnittstellen erwarten — einmal sauber an einer Stelle, für alle Admin-Bereiche (Streamer, System, Konfiguration, Roadmap). Der interne Server-Schlüssel funktioniert unverändert weiter, und eine Anmeldung lässt sich von außen nicht fälschen.

**Ergebnis:** Wer als Admin eingeloggt ist, sieht seine Daten wieder — Streamer-Liste und Co. laden, statt auf die Anmeldung zurückzuwerfen.

## #297 — Logout im Admin-Dashboard landet nicht mehr auf „Not Found"

**Ausgangslage:** Wer sich im Admin-Dashboard ausgeloggt hat, landete auf einer „Not Found"-Seite. Grund: Der Logout schickte den Browser auf einen Pfad, der nur auf der öffentlichen Seite existiert — auf der Admin-Subdomain ist genau dieser Pfad bewusst gesperrt. Der Redirect war noch aus der alten Generation als reiner Relativpfad übernommen, ohne zu unterscheiden, von welchem Host der Logout kam.

**Änderung:** Der Logout merkt jetzt, ob er vom Admin-Host kommt, und schickt in dem Fall zurück zur Admin-Login-Seite — genau dorthin, wo man sich neu anmeldet — statt auf das öffentliche Analyse-Dashboard, das im Admin-Bereich nichts zu suchen hat. Vom regulären Host aus bleibt alles wie bisher. Zusätzlich ist der Host-/Pfad-Vertrag (welche Seite auf welcher Subdomain lebt und warum) dauerhaft festgehalten, damit diese Art Fehlleitung nicht wiederkehrt.

**Ergebnis:** Logout aus dem Admin-Bereich führt sauber zur Admin-Anmeldung zurück statt in einen toten 404.

## #296 — Streamer-Ansprache: aus der Einmal-Nachricht wird eine wachsende Beziehung

**Ausgangslage:** Wenn der Bot einen passenden deutschen Deadlock-Streamer entdeckt hat, bekam der genau eine, immer gleiche Nachricht — danach lange Funkstille. Das wirkte wie ein kalter Wurf: zu wenig, um zu erklären wer wir sind, schnell überlesen und leicht als Spam oder Scam abgetan. Eine echte Vorstellung der Community fand nie statt.

**Änderung:** Die Ansprache ist keine kalte Einzelnachricht mehr, sondern reitet auf den Support-Raids und erzählt sich über die Zeit Stück für Stück. Beim ersten Raid kommt eine freundliche Vorstellung samt entwaffnendem „keine Sorge, kein Scam"; mit jedem weiteren Raid ein bisschen mehr — wer wir sind, dass hier echte Leute und Streamer dahinterstehen, was die Community real macht (Turniere, Coaching, Events) und eine unaufdringliche Einladung Richtung Website und Discord. Der Druck bleibt durchgehend niedrig, der Ton wird über viele Kontakte vertrauter statt fordernder. Wer den Bot gebannt oder ein Opt-out gesetzt hat, wird zuverlässig nie wieder angeschrieben — mehrfach abgesichert, im Fehlerfall wird blockiert statt gesendet — und die täglichen Sende- und Sicherheitslimits bleiben unangetastet.

**Ergebnis:** Statt eines einmaligen Kaltkontakts entsteht eine über Wochen wachsende, menschlich klingende Begleitung, die einen Streamer Schritt für Schritt von „wer sind die?" zu „coole Community, schau ich mir an" mitnimmt — ohne aufdringlich oder bothaft zu wirken.

## #295 — „Mein Abo": Rechnungen und Verwaltung jetzt direkt auffindbar

**Ausgangslage:** Wer ein Abo hatte, kam an seine Rechnungen praktisch nicht heran — der Weg ins Zahlungsportal war zwar hinterlegt, aber nirgends sichtbar verlinkt. Auf der Preisseite gab es höchstens einen winzigen Punkt am aktuellen Plan; einen klaren Einstieg „hier sind deine Rechnungen" suchte man vergeblich. Und wer doch mal über einen alten Link auf der Preisseite landete, sah nur ein kryptisches Kürzel in der Adresszeile statt einer Erklärung.

**Änderung:** Abonnenten sehen auf der Preisseite jetzt ganz oben einen eigenen „Mein Abo"-Bereich mit dem laufenden Plan und einem deutlichen Button zu Rechnungen, Zahlungsdaten und Kündigung — alles gebündelt im Kundenportal. Zusätzlich übersetzen wir die bisher kryptischen Rückmeldungen aus dem Bezahlvorgang (etwa wenn noch kein Zahlungskonto hinterlegt ist oder das Portal kurz nicht erreichbar war) in verständliche Hinweise direkt auf der Seite.

**Ergebnis:** Wer ein Abo hat, findet seine Rechnungen und die Verwaltung jetzt auf Anhieb, und unklare Status-Meldungen erklären sich von selbst statt als nackter Code in der Adresszeile.

## #294 — Stille Parität-Lücken geschlossen + Schema-Migration hypertable-sicher

**Ausgangslage:** Eine systematische Re-Verifikation hat mehrere kleine, lange unbemerkte Lücken zwischen der alten und der neuen Bot-Generation zutage gefördert — nichts, das akut etwas kaputt machte, aber Stellen, an denen sich das neue System nicht exakt wie das alte verhielt. Dazu ein konkreter Stolperstein: die jüngste Datenbank-Migration scheiterte beim Start still (und wurde abgefangen), weil sie auf den komprimierten Verlaufstabellen eine Operation versuchte, die dort gar nicht erlaubt ist — obwohl an diesen Tabellen längst nichts mehr zu korrigieren war.

**Änderung:** Die Migration fasst die komprimierten Tabellen jetzt nicht mehr an, wenn dort nichts zu tun ist, und korrigiert nur noch gezielt die eine Spalte, die wirklich vom alten Schema abwich. Darüber hinaus: ein Opt-out vom Bot wird jetzt durchgängig respektiert — auch bei automatischen Hinweisen und Eskalationen, nicht mehr nur im Hauptpfad; eine Eskalations-Chatnachricht, die bisher nur aufgebaut, aber nie gesendet wurde, geht jetzt tatsächlich raus; beworbene Rechnungs-Links zeigen auf die echte Rechnungsübersicht statt ins Leere; und mehrere Auswertungs-Antworten liefern wieder genau das Format, das das Dashboard erwartet. Im Hintergrund zusätzlich angeglichen: Datenbank-Timeouts, Health-Checks, eine nicht erreichbare Admin-Datensicht und diverse weitere Detail-Angleichungen.

**Ergebnis:** Das neue System verhält sich an diesen Stellen wieder genau wie das alte, der stille Migrations-Fehler beim Start ist weg (gegen die echte Live-Datenbank geprüft), und ein Bot-Opt-out gilt jetzt wirklich überall.

## #293 — Abo-Erkennung gehärtet + Go-Live-Ankündigung einheitlich Standard

**Ausgangslage:** Zwei Dinge liefen still schief. Bei der Abo-Prüfung konnte ein laufendes, bezahltes Abo unter bestimmten Umständen als „kein Plan" gewertet und danach von einer automatischen Test-Phase überdeckt werden — weil die Bezahl-Referenz inzwischen am Login hängt, die Prüfung aber noch ausschließlich an der alten Nutzer-ID gesucht hat. Und bei der Go-Live-Ankündigung steckten im Code noch Reste des längst abgeschafften Embed-Designers; teils griffen gespeicherte Alt-Anpassungen unzuverlässig rein.

**Änderung:** Die Abo-Erkennung gleicht jetzt sowohl über den Login als auch über die Nutzer-ID ab und matcht keine leere Referenz mehr — ein bezahltes Abo wird damit zuverlässig erkannt. Die Go-Live-Ankündigung läuft jetzt durchgängig über das Standard-Design: Der automatische Post und der Rollen-Ping bleiben unverändert, nur das individuell anpassbare Embed ist endgültig raus (so entschieden). Im Hintergrund zusätzlich: eine Raid-Statistik zählt wieder exakt (sie hatte Zuschauer mitgezählt, die erst nach dem Raid auftauchten), die Admin-Datensicht fürs Raid-Netzwerk ist wieder erreichbar, und das Datenbank-Schema baut sich auch bei einem kompletten Neuaufbau mit den korrekten Spaltentypen auf — abgesichert durch neue Tests.

**Ergebnis:** Zahlende Abos werden korrekt erkannt, die Go-Live-Ankündigung ist für alle einheitlich das Standard-Design, und mehrere lange unbemerkte Ungenauigkeiten im Hintergrund sind ausgeräumt.

## #292 — Go-Live-Tipps vorerst abgeschaltet

**Ausgangslage:** Wenn du mit Deadlock live gegangen bist, hat der Bot als erste Chat-Zeile einen wechselnden „Tipp" gepostet. In der Praxis war das meist ein Hinweis, den man als Streamer ohnehin kennt — also eher Rauschen als echte Hilfe.

**Änderung:** Wir haben diese automatischen Go-Live-Tipps erstmal komplett abgeschaltet, bis wir sie inhaltlich überarbeitet haben. Alles drumherum bleibt unangetastet — der Versand pausiert einfach.

**Ergebnis:** Beim Stream-Start kommt keine Tipp-Zeile mehr. Sie kommen überarbeitet zurück, sobald sie wirklich was bringen.

## #291 — Scam-Schutz überreagiert nicht mehr auf normalen deutschen Chat

**Ausgangslage:** Der automatische Scam-Schutz schaut bei wildfremden Erstschreibern, ob da jemand die typische Betrugsmasche aufzieht — erst künstlich Nähe aufbauen, dann das Gespräch von Twitch wegziehen. Zuletzt hat er aber einen ganz normalen deutschsprachigen Zuschauer erwischt und getimeoutet, nur weil der beiläufig meinte, man könne ja mal im Discord schreiben. Das Modell hatte sich über mehrere harmlose Nachrichten in einen Verdacht hineingesteigert und beim Wort „Discord" zugeschlagen — obwohl der Kanal seinen eigenen Discord selbst aktiv bewirbt.

**Änderung:** Die Bewertung hängt jetzt klar an der Sprache. Die echte Masche läuft praktisch immer auf Englisch oder in sichtbar maschinell übersetztem Deutsch nach festem Skript. Flüssiges, lockeres Alltagsdeutsch ist umgekehrt ein starkes Zeichen für einen echten Zuschauer und zählt mehr als eine oberflächliche Skript-Ähnlichkeit. Ein Hinweis auf Discord oder „woanders weiterreden" ist für sich allein kein Verdacht mehr, sondern nur noch zusammen mit den übrigen Skript-Merkmalen in fremder Sprache. Echter Gesprächskontext — gemeinsame Vorgeschichte, Freundlichkeit, ein Sub-Versprechen — gilt ausdrücklich als normal, und die bloße Erwähnung von „Discord" startet die Prüfung nicht mehr von selbst.

**Ergebnis:** Ein natürlich deutschsprachiger Zuschauer wird zuverlässig durchgelassen, auch wenn er den Discord erwähnt. Die englischsprachigen Maschen — gespielte Freundschaft, Wachstums-Pitch und Ausreden mit sofortigem Plattform-Wechsel — fängt der Schutz unverändert ab. Direkt am realen Vorfall und an echten Betrugsverläufen gegengeprüft.

## #290 — Token-Budget-Zählung korrigiert (intern)

**Ausgangslage:** Die rollierende 5-Stunden-Budget-Zählung für das günstige Sprachmodell (MiniMax) soll ausschließlich dessen Verbrauch erfassen — so wie es früher gehandhabt wurde. Tatsächlich wurden auch die Tokens des Premium-Modells (Anthropic) in dasselbe Budget-Konto geschrieben und verfälschten die Zählung nach oben.

**Änderung:** Der Premium-Pfad schreibt nicht mehr in das MiniMax-Budget-Konto — die beiden Modelle werden sauber getrennt verbucht. Die Aufrufe selbst und die je Antwort ausgewiesenen Token-Zahlen bleiben unverändert.

**Ergebnis:** Das 5-Stunden-Budget spiegelt wieder exakt den MiniMax-Verbrauch. Reiner interner Genauigkeits-Fix ohne sichtbare Funktionsänderung.

## #289 — Breitere Zuschauer-Erfassung + stille Fehler behoben

**Ausgangslage:** Anwesenheitsdaten (wer gerade im Chat ist) konnten bisher nur von Kanälen erfasst werden, die den Bot autorisiert oder zum Moderator gemacht haben. Alle übrigen deutschen Deadlock-Streamer — Nicht-Partner und ehemalige Partner — blieben außen vor, obwohl diese Information öffentlich verfügbar ist. Parallel hat eine systematische Prüfung mehrere Fehler aufgedeckt, die nie eine Fehlermeldung erzeugten und deshalb monatelang unbemerkt blieben.

**Änderung:** Der Bot liest jetzt zusätzlich anonym mit. Für jeden live entdeckten deutschen Deadlock-Kanal verbindet er sich mit dem öffentlichen Chat-Protokoll und erfasst die Zuschauer-Anwesenheit — auch ohne jede Freigabe des Streamers. Die Verbindungen sind anonym und schonend gestaffelt, deutlich unter den Twitch-Grenzen, sodass keine Drosselung entsteht; die Kanalliste aktualisiert sich laufend mit dem Live-Status. Zusätzlich behoben: In der Zuschauer-Zeitleiste stand die Chat-Nachrichten-Zahl pro Sitzung immer auf 0 statt auf dem echten Wert. Bei kurzen Datenbank-Aussetzern konnte eine vom Auto-Chat abgemeldete Person trotzdem eine Antwort bekommen — jetzt gilt im Zweifel die Abmeldung. Verstummte der Chat-Assistent wegen eines kurzen internen Aussetzers, geschah das lautlos und blieb minutenlang hängen — jetzt wird es protokolliert und nach der Erholung sofort fortgesetzt. Und beim automatischen Clip-Upload in soziale Netzwerke konnte im Fehlerfall derselbe Clip ein zweites Mal öffentlich gepostet werden — das ist nun ausgeschlossen.

**Ergebnis:** Spürbar breitere Abdeckung der Streamer-Landschaft in den Statistiken, korrekte Chat-Zahlen im Dashboard, respektierte Abmeldungen, keine doppelten Social-Media-Posts. Jede Korrektur ist mit neuen Tests abgesichert, die genau diese stillen Fehlerfälle abdecken.

## #288 — Live-Zähler korrigiert + internes Aufräumen

**Problem:** Die „Live"-Anzeige im Analyse-Bereich zählte zuletzt deutlich zu hoch — sie führte auch längst beendete Streams als „live", weil alte Live-Markierungen nicht zuverlässig zurückgesetzt wurden. Zusätzlich schleppten interne Datensätze veraltete Verifikations-Felder mit, und eine interne Verknüpfungs-Abfrage lief gelegentlich auf einen Datenbankfehler.

**Änderung:** Ein neuer Aufräum-Mechanismus setzt verwaiste „live"-Markierungen automatisch zurück (und hat die Altlasten einmalig bereinigt) — die Live-Zahl spiegelt jetzt die tatsächlich laufenden Streams. Die veralteten Verifikations-Felder wurden durch ein einzelnes, sauberes Feld ersetzt (gleiche Anzeige, weniger Ballast). Die fehlerhafte Verknüpfungs-Abfrage wurde korrigiert und mit einem Test abgesichert, der genau diese Klasse von Schema-Fehlern künftig fängt.

**Ergebnis:** Korrekte Live-Zahlen, ein schlankeres Datenmodell und eine stabilere interne Verknüpfung — alles ohne Funktionsänderung für dich.

## #287 — Twitch-System vollständig auf eine moderne Plattform umgestellt

**Problem:** Hinter den Kulissen lief der Twitch-Bot zuletzt zweigleisig — ein Teil bereits auf der neuen, schnelleren Plattform, ein anderer noch auf der alten. Zwei parallele Systeme bedeuten doppelte Wege, mehr Fehlerquellen und Mehraufwand bei jeder Änderung. Zusätzlich wurden die stillen Mitschauer („Lurker") zuletzt nur noch für einen Bruchteil der Kanäle erfasst, weil die Erfassung versehentlich an die falsche Einstellung gekoppelt war.

**Änderung:** Alles läuft jetzt einheitlich auf der neuen Plattform — Chat-Moderation, Live-Erkennung, Raids, Statistiken, Dashboard und Anmeldung. Die alte Parallel-Version wurde abgeschaltet. Die Lurker-Erfassung wurde repariert: Sie greift jetzt für jeden Kanal, der dem Bot die nötige Chat-Leseberechtigung erteilt hat (statt nur für einen Teil). Einige selten genutzte Alt-Adressen leiten nun sauber weiter, statt ins Leere zu laufen.

**Ergebnis:** Ein einziges, durchgängiges System — verlässlicher, schneller und einfacher zu pflegen. Für dich als Streamer bleibt alles wie gewohnt, nur runder. Wer dem Bot die Chat-Leseberechtigung gibt, bekommt seine Lurker-Statistik wieder vollständig.

## #286 — Go-Live-Designer entfernt, Dashboard-Seiten direkt nativ

**Problem:** Der „Discord Announcement Designer" — der Baukasten, mit dem man sein Go-Live-Embed selbst gestalten konnte — war aufwändig und kaum genutzt. Zugleich liefen mehrere Dashboard-Seiten (Übersicht, Verwaltung, Preise und einige Alt-Adressen) noch über eine Zwischenschicht statt direkt über die aktuelle Plattform.

**Änderung:** Der Designer wurde entfernt; die alte Seite leitet jetzt aufs Dashboard weiter. Die automatische Go-Live-Ankündigung samt Rollen-Ping läuft unverändert weiter (mit Standard-Gestaltung). Die genannten Dashboard-Seiten werden nun direkt ausgeliefert; die Preis-Seite lädt wieder auch ohne Login vollständig.

**Ergebnis:** Weniger Ballast, die Go-Live-Ankündigung funktioniert wie gewohnt, und das Dashboard läuft ein Stück eigenständiger.

## #285 — Steam direkt im Dashboard verknüpfen, erzwungenes Onboarding entfernt

**Problem:** Das Verwaltungs-Dashboard drängte dich bei jedem Besuch in einen Einrichtungs-Assistenten, solange dieser nicht als „abgeschlossen" markiert war — auch wenn längst alles eingerichtet war. Dessen Steam-Schritt führte dabei ins Leere: er erklärte die Verknüpfung nur, bot aber keinen funktionierenden Knopf. Eine Steam-Verknüpfung ließ sich im Dashboard gar nicht anstoßen.

**Änderung:** Der aufgezwungene Einrichtungs-Assistent ist entfernt — das Dashboard öffnet direkt die Übersicht, und niemand wird mehr automatisch ins Onboarding geleitet. Im Verwaltungs-Bereich gibt es stattdessen neben „Discord verbinden" jetzt eine eigene Karte „Steam verbinden": Sie zeigt, ob dein Steam-Account verknüpft ist, und bietet — sofern dein Discord verknüpft ist — einen Knopf direkt in die bestehende Steam-Anmeldung. Die Verknüpfung selbst läuft unverändert über die Community-Steam-Anmeldung, gekoppelt an deine Discord-ID; es entsteht kein zweiter Speicherort. Ist Steam bereits verknüpft, steht dort nur „Verbunden". Ist Discord noch nicht verknüpft, weist ein Hinweis darauf hin, dass die Steam-Verknüpfung darüber läuft.

**Ergebnis:** Kein erzwungenes Onboarding mehr; das Dashboard startet auf der Übersicht. Steam lässt sich bequem direkt aus der Verwaltung verknüpfen — ohne Doppelung: verknüpft bleibt verknüpft, und fehlt Discord, sagt das Dashboard klar, was zuerst zu tun ist.

## #284 — Lurker-Steuer jetzt im Dashboard schaltbar

**Problem:** Die „Lurker-Steuer" — die deine ruhigsten Stammzuschauer ab und zu mit einem freundlichen Hinweis zurück in den Chat holt — ließ sich bisher nur per Chat-Befehl abschalten. Im Verwaltungs-Dashboard fehlte ein eigener Schalter, sodass man den Status dort weder sehen noch bequem ändern konnte.

**Änderung:** Im Verwaltungs-Dashboard gibt es jetzt einen eigenen Bereich „Lurker-Steuer" mit An/Aus-Schalter. Er zeigt den aktuellen Stand, speichert Änderungen direkt und bleibt mit dem Chat-Befehl synchron. Fehlt deinem Kanal die nötige Chatter-Leseberechtigung, weist ein Hinweis darauf hin, dass die Funktion sonst wirkungslos bliebe. Standardmäßig ist die Lurker-Steuer aus; jeder Partner kann sie selbst aktivieren.

**Ergebnis:** Die Lurker-Steuer lässt sich bequem im Dashboard ein- und ausschalten — ohne Chat-Befehl, mit klarer Statusanzeige und einem Hinweis, falls noch eine Berechtigung fehlt.

## #283 — Wartung: Verifikations-Feldumbau vorbereitet (zurückgestellt), interne Tests stabilisiert

**Problem:** Drei veraltete Verifikations-Datenfelder eines Streamer-Datensatzes dienen nur noch intern als Anzeige-Markierung und sollten zugunsten eines einzelnen Felds entfernt werden. Unabhängig davon konnten sich die automatisierten Anmelde-Tests bei gemeinsamem Testdatenbank-Lauf gegenseitig stören und sprangen dann fehl an.

**Änderung:** Der Feldumbau wurde vollständig vorbereitet und gegen eine 1:1-Kopie der Live-Daten als wirkungsgleich nachgewiesen, dann aber bewusst zurückgestellt: Solange die alte Programmversion parallel läuft und diese Felder noch liest und schreibt, würde ein Entfernen sie stören. Die fertige Umstellung samt Ablaufplan liegt dokumentiert bereit. Getrennt davon laufen die Anmelde-Tests jetzt jeweils in einem eigenen, isolierten Datenbank-Namensraum.

**Ergebnis:** Keine Funktionsänderung für Nutzer. Die Umstellung ist startklar, sobald die alte Programmversion abgeschaltet ist; die Testreihe ist wieder verlässlich.

## #282 — Dashboard-Schalter speichern wieder zuverlässig

**Problem:** Im Verwaltungs-Dashboard schlugen Schreib-Aktionen — Schalter umlegen, Einstellungen speichern (Stille Hinweise, Scam-Schutz, Chat-AI) — mit „csrf_failed" bzw. „HTTP 403" fehl, obwohl die Seiten normal luden. Ursache: Lag im Browser neben der gültigen Anmeldung noch ein veraltetes Cookie eines früheren Admin-Logins, prüfte der Schreib-Schutz nur dieses veraltete Cookie und wies die Aktion ab — die eigentlich gültige Anmeldung wurde dabei übersprungen.

**Änderung:** Der Schreib-Schutz prüft jetzt jede vorhandene Anmeldung unabhängig; ein veraltetes Alt-Cookie kann eine gültige Anmeldung nicht mehr verdecken. Das entspricht der Anmelde-Reihenfolge im übrigen Dashboard.

**Ergebnis:** Schalter und Einstellungen im Dashboard speichern wieder zuverlässig — auch für Konten, die früher einen separaten Admin-Login genutzt haben.

## #281 — Analyse-Daten strikt aufs eigene Konto begrenzt

**Problem:** Die Daten-Endpunkte des Verwaltungs- und Analyse-Dashboards lasen den abgefragten Kanal aus einem Anfrage-Parameter, prüften aber nicht durchgängig, dass dieser Kanal auch dem angemeldeten Konto gehört. Eine Plan- bzw. Anmeldeprüfung allein bestätigt nur, dass die eigene Sitzung berechtigt ist — nicht, wessen Kanal abgefragt wird. Dadurch hätte ein angemeldeter Streamer durch Ändern des Kanal-Parameters Auswertungen fremder Kanäle (z. B. Zuschauer-Überschneidung, Audience-, Chat- oder Raid-Auswertungen) einsehen können.

**Änderung:** Alle Endpunkte, die die privaten Auswertungen genau eines Kanals liefern, laufen jetzt über eine gemeinsame Eigentümer-Prüfung: Ein angemeldeter Streamer wird zwingend auf den eigenen Login festgelegt — ein fremder Kanal im Parameter führt zu „nicht erlaubt". Verwaltungs-Konten dürfen weiterhin jeden Kanal wählen. Bewusst kanalübergreifende, öffentliche Ansichten (Kategorie-Bestenliste, das öffentliche OBS-Overlay) bleiben unverändert offen. Plan- und Anmeldeprüfungen bleiben zusätzlich bestehen.

**Ergebnis:** Jede Kanal-Auswertung ist an das eigene Konto gebunden; ein Zugriff auf fremde Kanal-Daten über den Parameter ist nicht mehr möglich, ohne legitime kanalübergreifende Funktionen einzuschränken.
## #280 — Verwaltungs- & Analyse-Dashboard: Streamer-Standardsicht statt Dauer-Admin

**Problem:** Im Verwaltungs- und Analyse-Dashboard funktionierten viele Kacheln nicht — „Konto-Daten nicht verfügbar", Fehler beim Laden der Scam-Schutz-Fälle und beim Umschalten der Chat-AI. Ursache: Ein Konto mit Admin-Berechtigung wurde nach dem Login dauerhaft als Admin behandelt statt als normaler Streamer; dadurch passten die angezeigte Streamer-Sicht und der Datenzugriff im Hintergrund nicht zusammen. Zusätzlich scheiterten Schreib-Aktionen an einem Schutzmechanismus, den die Dashboard-Oberfläche technisch nicht erfüllen konnte.

**Änderung:** Nach dem Login giltst du jetzt standardmäßig als normaler Streamer und siehst dein eigenes Konto. Den Admin-Vollzugriff schaltest du selbst per Schalter an und jederzeit wieder aus — er ist kein Dauerzustand mehr. Schreib-Aktionen (z. B. Chat-AI ein/aus, Stille Hinweise, Scam-Schutz-Fälle bearbeiten) laufen über einen Schutz, der die eigene Herkunft der Anfrage prüft und so wieder zuverlässig durchgeht, ohne den Schutz vor fremden Seiten aufzugeben. Der Dashboard-Zugang ist nur noch mit Anmeldung möglich.

**Ergebnis:** Verwaltungs- und Analyse-Dashboard laden wieder vollständig, Einstellungen lassen sich speichern und umschalten, und der Admin-Modus ist ein bewusster, umkehrbarer Schalter statt eines festen Zustands — bei anmeldepflichtiger, klar abgegrenzter Zugriffskontrolle.
## #279 — Analyse-Dashboard: ein Plan statt drei Stufen

**Problem:** Das Analyse-Dashboard war über mehrere Pläne verstreut. Analytics ließ sich auf verschiedenen Wegen freischalten — jeder mit einem anderen Umfang an Auswertungen, Verlaufsdaten und KI-Analysen. Das war unübersichtlich und teils widersprüchlich: Selbst einzelne Auswertungen des letzten Streams waren gesperrt, obwohl sie eigentlich der Einstieg sein sollten, und es war nie ganz klar, welcher Plan was zeigt.

**Änderung:** Es gibt jetzt genau einen Analyse-Zugang. Der komplette letzte Stream ist für alle kostenlos sichtbar — inklusive Viewer-Verlauf und Chatter-Liste. Alles, was über den letzten Stream hinausgeht — der Verlauf über mehrere Streams, Trends, Vergleiche, die KI-gestützten Post-Stream-Reports, Coaching und Monetarisierung — steckt gebündelt im einen Analyse-Zugang. Raid-Boost und Werbefrei bleiben eigenständige Produkte und enthalten keine Analytics mehr.

**Ergebnis:** Ein klarer Schnitt statt drei verschachtelter Stufen: den letzten Stream sieht jeder gratis, mit dem Analyse-Zugang gibt es alles. Wer bisher einen Plan mit Analyse hatte (auch als Bundle), behält automatisch den vollen Zugang.

## #278 — Overlay-Baukasten: Vorschau bleibt nach schnellem Einstellen nicht mehr leer

**Problem:** Wer im Stream-Overlay-Baukasten schnell mehrere Einstellungen verstellte (Regler ziehen, Schalter umlegen, Modus wechseln), bei dem konnte die Vorschau dauerhaft leer und transparent bleiben — nicht nur kurz, sondern bis auf Weiteres. Hintergrund: Jede Einstellungsänderung lud die Vorschau sofort komplett neu und brach dabei die gerade laufende Datenanfrage ab. Eine interne Anfrage-Bündelung räumte sich nach einem solchen Abbruch nicht auf und blockierte danach jede weitere Anfrage für denselben Kanal — der Kanal blieb leer, bis der Dienst neu startete.

**Änderung:** Zwei Stellen. (1) Die Vorschau lädt beim Verstellen nicht mehr bei jedem einzelnen Klick oder Regler-Schritt neu, sondern gebündelt erst nach einer kurzen Pause; die angezeigte und kopierbare Overlay-URL bleibt weiterhin sofort aktuell. (2) Die interne Anfrage-Bündelung räumt sich jetzt auch dann sauber auf, wenn eine laufende Anfrage abgebrochen wird, und gibt wartende Anfragen unmittelbar frei. Läuft ein Datenabruf einmal in eine Zeitüberschreitung, zeigt das Overlay weiterhin, was vorhanden ist (etwa den Rang), statt komplett leer zu bleiben.

**Ergebnis:** Im Baukasten lässt sich jetzt beliebig schnell an Stil, Layout und Inhalten herumspielen, ohne dass die Vorschau hängenbleibt; ein dauerhaft blockierter Kanal kann nicht mehr entstehen. Das eigentliche OBS-Overlay liefert robust weiter, auch wenn einzelne Statistiken kurzzeitig nicht abrufbar sind.

## #277 — Auto-Raid heilt hängende Raid-Schalter selbst

**Problem:** Ein Streamer konnte dauerhaft ohne Auto-Raid bleiben, obwohl seine Raid-Freigabe gültig war: Hatte eine frühere Token-Störung den internen Raid-Schalter auf „aus" gestellt und war die zugehörige technische Pause später wieder verschwunden, blieb der Schalter auf „aus" hängen. Keine der bestehenden Selbstheilungen erfasste diesen Zwischenzustand — der Auto-Raid wurde still übersprungen, ohne Fehlermeldung.

**Änderung:** Ein stündlicher Abgleich schaltet den Raid-Schalter automatisch wieder ein, sobald drei Bedingungen zugleich zutreffen: aktiver Partner, nachweislich gesunder Raid-Token (gültig, keine erneute Anmeldung nötig) und keine technische Pause. Bewusste Abschaltungen (manueller Verzicht oder auf Token-Ebene deaktiviert) sowie gesperrte oder pausierte Kanäle werden dabei nie angetastet.

**Ergebnis:** Der Zustand „Token funktioniert, es wird aber trotzdem nicht geraidet" kann nicht mehr dauerhaft hängenbleiben — der Schalter gleicht sich von selbst an den tatsächlichen Token-Zustand an.

## #276 — Telemetrie-, Abrechnungs- und Raid-Robustheit

**Problem:** Mehrere interne Pfade waren unvollständig: (a) Ban-/Timeout-/Shoutout-/Follow-Telemetrie eines Kanals fiel aus, wenn der Bot dort kein Moderator war; (b) wurde der Bot selbst in einem Kanal getimeoutet, bemerkte er es nicht zuverlässig; (c) der Abrechnungs-Abgleich verließ sich blind auf hinterlegte Produkt-Kennungen, auch wenn ein Produkt zwischenzeitlich gelöscht war; (d) bei fehlgeschlagenen Follower-Abfragen im Raid-System fehlte jede Fehler-Diagnose.

**Änderung:** (a) Fehlt dem Bot der Moderator-Zugriff, springt jetzt ein Broadcaster-Token-Ersatzweg ein (ohne Doppel-Abos). (b) Ein Timeout des Bots wird über die offizielle Event-Schnittstelle erkannt und sein Selbstschutz scharfgestellt. (c) Der Abrechnungs-Abgleich prüft hinterlegte Produkte live gegen Stripe und legt gelöschte neu an, statt sie als gültig zu zählen. (d) Follower-Abfragefehler werden differenziert erfasst — erwartete Berechtigungs-Fehlschläge bleiben stumm, nur echte Fehler erscheinen in der Diagnose.

**Ergebnis:** Telemetrie und Selbstschutz greifen auch in Kanälen ohne Mod-Status, der Abrechnungs-Abgleich heilt sich selbst, und die Raid-Diagnose ist aussagekräftig — ohne Falschalarm-Rauschen.

## #275 — Datenbank-Erstaufbau repariert

**Problem:** Eine Schema-Migration brach beim vollständigen Erstaufbau einer frischen Datenbank ab — ein Trigger an einer Spalte blockierte deren Typ-Umstellung. Bestehende Installationen waren nicht betroffen (die Migration war dort längst angewandt), aber ein sauberer Neuaufbau von null scheiterte.

**Änderung:** Die Migration entfernt den blockierenden Trigger jetzt vor der Typ-Umstellung und legt ihn danach unverändert wieder an — ausschließlich im Erstaufbau-Pfad; bestehende Datenbanken bleiben unberührt.

**Ergebnis:** Eine frische Datenbank lässt sich wieder vollständig allein aus den Migrationen aufbauen, ohne Auswirkung auf den laufenden Betrieb.

## #274 — Overlay-Baukasten lädt wieder (Login statt Weißbild)

**Problem:** Die Overlay-Baukasten-Seite blieb ohne gültige Anmeldung weiß. Die Seite selbst wurde noch ausgeliefert, aber die zugehörigen Programmdateien sind anmeldepflichtig und wurden zur Login-Seite umgeleitet — im Browser kam dadurch nichts an. Anders als alle übrigen Dashboard-Seiten leitete der Baukasten nicht sauber zur Anmeldung weiter.

**Änderung:** Die Baukasten-Seite verhält sich jetzt wie die übrigen Dashboard-Seiten und leitet ohne gültige Anmeldung direkt zur Anmeldung weiter. Das öffentliche OBS-Overlay (Adresse mit Streamer-Namen) bleibt unverändert ohne Anmeldung erreichbar.

**Ergebnis:** Eingeloggte Streamer sehen den Baukasten normal; wer nicht mehr angemeldet ist, landet auf der Anmeldung statt auf einer weißen Seite.

## #273 — Zuschauer-Abfrage entlastet

**Problem:** Der neue Zuschauer-Poller fragte alle 30 Sekunden für jeden Live-Kanal die Zuschauerliste über die Twitch-Schnittstelle ab — auch für die vielen Kanäle, in denen der Bot kein Moderator ist und die Abfrage deshalb zwangsläufig fehlschlägt. Das erzeugte pro Zyklus viele vergebliche Anfragen.

**Änderung:** Kanäle, in denen der Bot nachweislich kein Moderator ist (und ein Wiederherstellen des Mod-Status nicht greift), werden für 15 Minuten vom Bot-Abfragepfad ausgenommen. Der Ersatzweg über das Streamer-Token für Raid-Kanäle bleibt unberührt, und sobald der Bot wieder Moderator wird, greift die Abfrage sofort erneut.

**Ergebnis:** Deutlich weniger vergebliche Twitch-Anfragen pro Zyklus, ohne dass erfassbare Zuschauer verloren gehen — gleiche Datenqualität, geringere Last.

## #272 — Markt-Daten-Ansicht repariert + interne Aufräumung

**Problem:** Die aggregierte Markt-Daten-Ansicht im Verwaltungsbereich lieferte seit der Datenbank-Umstellung auf größere Zahlen-Typen einen Fehler statt Daten — die Session-Kennung wurde an einer Stelle noch im alten, zu kleinen Zahlenformat gelesen, was den Abruf still abbrechen ließ. Derselbe Lesefehler betraf eine interne Chatter-Statistik. Parallel schleppte der Code eine längst abgelöste „manuell verifiziert"-Sonderbehandlung mit, die durch die neue Partner-Logik überflüssig geworden war.

**Änderung:** Die Session-Kennung wird an den betroffenen Stellen wieder im neuen, größeren Format gelesen, sodass die Markt-Daten-Ansicht und die interne Statistik wieder Werte liefern. Die abgelöste „manuell verifiziert"-Logik wurde aus dem neuen Programmteil vollständig entfernt; die zugehörigen Datenbank-Felder bleiben bewusst erhalten, bis auch der letzte Altbestand umgezogen ist — bestehende Anzeigen ändern sich dadurch nicht.

**Ergebnis:** Die Markt-Daten-Ansicht zeigt wieder Werte, interne Statistiken stimmen, und der Code trägt eine tote Sonderbehandlung weniger — ohne Auswirkung auf das, was Streamer sehen.

## #271 — Stille Zuschauer, Anwesenheit und Raid-Treue werden wieder erfasst

**Problem:** Seit der Umstellung auf die neue Bot-Generation wurden Zuschauer nur noch erfasst, wenn sie im Chat schrieben. Stille Mitschauer blieben unsichtbar — und damit liefen die Anwesenheits- und Watchtime-Verläufe sowie die Auswertung, wie viele zugeführte Zuschauer nach einem Raid hängen bleiben, ins Leere.

**Änderung:** Ein Hintergrund-Dienst fragt jetzt alle 30 Sekunden für jeden Live-Kanal die vollständige Zuschauerliste über die offizielle Twitch-Schnittstelle ab. Bevorzugt läuft das über das Bot-Konto; ist der Bot in einem Partner-Kanal kein Moderator mehr, stellt er den Status selbst wieder her (höchstens alle zehn Minuten ein erneuter Versuch). Für Raid-Kanäle springt ersatzweise das Streamer-Token ein. Alle Anwesenden — auch die stillen — werden als Sitzungs-Zuschauer, in der dauerhaften Zuschauer-Übersicht und als Anwesenheits-Tick festgehalten; bekannte Chat-Bots und das eigene Bot-Konto zählen dabei nicht mit.

**Ergebnis:** Lurker-Zahlen, Watchtime-Verläufe und Anwesenheits-Zeitleisten füllen sich wieder mit echten Daten. Zusätzlich berechnet ein stündlicher Lauf für jeden Raid der letzten sieben Tage, wie viele der zugeführten Zuschauer nach 5, 15 und 30 Minuten noch da sind, wie viele dem Quell-Kanal schon bekannt waren und wie viele für den Ziel-Kanal neu sind — die Raid-Treue ist damit wieder messbar.

## #270 — Overlay: Spielmodus-Filter (Standard / Street Brawl) + aufgeräumter Baukasten

**Problem:** Im Overlay vermischten sich alle Spielmodi — Street-Brawl-Partien verzerrten die „echte" Winrate und Serie der Standard-Matches. Außerdem hatte der Baukasten eine Positions-Auswahl, die nichts brachte (in OBS verschiebt man die Quelle ohnehin per Hand), und die Vorschau schnitt größere Overlays ab.

**Änderung:** Im Baukasten gibt es jetzt einen Spielmodus-Filter (Alle Modi, Standard, Street Brawl). Winrate, heutige Bilanz, Serie, K/D und der Match-Verlauf beziehen sich dann nur noch auf den gewählten Modus; Rang, MMR-Trend und Live-Status bleiben modusunabhängig. Die überflüssige Positions-Auswahl ist entfernt — das Overlay sitzt standardmäßig oben links, platziert wird es in OBS. Die Vorschau zeigt jetzt die komplette Karte ohne Abschneiden und nennt die passende OBS-Größe.

**Ergebnis:** Streamer können ihre Standard-Stats sauber von Street Brawl trennen, und der Baukasten ist aufgeräumter und zeigt direkt, was in OBS ankommt.

## #269 — Overlay: Hero-Bilder im Match-Verlauf + aufgeräumte Optik

**Problem:** Im überarbeiteten Overlay zeigte der Match-Verlauf nur farbige Kreise statt der Hero-Bilder — diese wurden im Browser unzuverlässig nachgeladen und fielen oft ganz aus. Außerdem konnte die Verlaufsreihe über den Kartenrand hinauslaufen, und die empfohlene OBS-Größe passte nicht zur tatsächlichen Kartengröße.

**Änderung:** Die Hero-Bilder werden jetzt serverseitig aufgelöst und direkt mitgeliefert, sodass sie zuverlässig erscheinen. Der Match-Verlauf zeigt die Heroes als abgerundete Kacheln mit dezenter Sieg/Niederlage-Markierung statt als Vollfarb-Kreise und bricht bei Bedarf sauber um. Die Karte ist etwas kompakter, und der Baukasten nennt jetzt je Layout die passende OBS-Größe.

**Ergebnis:** Das Overlay wirkt aufgeräumter und hochwertiger, der Match-Verlauf zeigt echte Hero-Bilder, und die OBS-Einrichtung passt auf Anhieb.

## #268 — Overlay-Baukasten überarbeitet: Stile, Layouts & mehr Stats

**Problem:** Der Overlay-Baukasten war funktional, aber schlicht — nur vier An/Aus-Schalter, ein einziger Look, und im Dashboard nur über einen Umweg erreichbar.

**Änderung:** Der Baukasten ist jetzt vollwertig: drei Stile (Dunkel, Hell, Akzent), zwei Layouts (Box-Karte oder schlanke Leiste) und deutlich mehr wählbare Inhalte — Rang mit Abzeichen, Winrate, heutige Bilanz, Serie, K/D, letztes Match, meistgespielter Hero, ein Match-Verlauf-Streifen mit Hero-Bildern und Sieg/Niederlage-Markierung, Live-Match und Branding. Dazu Regler für Hintergrund-Deckkraft und Verlaufslänge sowie eine Live-Vorschau, die sich sofort mitändert. Die Optik ist hochwertig (transparente Glas-Karte), und leere Werte werden ausgeblendet statt mit Platzhaltern gefüllt. Alle Zahlen kommen weiterhin direkt aus den echten Spieldaten. Erreichbar ist der Baukasten jetzt über einen eigenen Eintrag „Stream-Overlay" in der Seitenleiste.

**Ergebnis:** Streamer stellen sich in Sekunden ein Overlay zusammen, das zu ihrem Stream passt — vom dezenten Balken bis zur vollen Statuskarte — und finden es direkt in der Navigation.

## #267 — Auto-Raid, Reaktivierung und Zuschauer-Tracking repariert

**Problem:** Mehrere zusammenhängende Fehler ließen den automatischen Raid am Stream-Ende für die Mehrheit der Partner ausfallen. Die nötigen Ereignis-Abos wurden im Hintergrund alle paar Stunden gelöscht und erst verspätet neu angelegt — in der Lücke fehlte genau das „Stream offline"-Signal, das den Raid auslöst. Streamer, die ihren Bot-Zugang neu autorisierten, wurden außerdem nur dann wieder vollständig aktiviert, wenn ihr Konto mit Discord verknüpft war — ohne Verknüpfung blieb der Raid stummgeschaltet. Ein abgelaufener Zugangs-Token wurde fälschlich wie ein manuelles Abschalten behandelt, und die Karenzzeit bis zur Pausierung griff durch einen internen Zählerfehler nie. Schließlich war das Spiegeln der Zuschauer- und Chatter-Liste pro Stream still ausgefallen, sodass für viele Streams keine Chatter mehr erfasst wurden.

**Änderung:** Die Ereignis-Abos werden nicht mehr gelöscht und kurz darauf neu angelegt — beide Pflegevorgänge nutzen jetzt dieselbe Partnerliste, und bei einem leeren Ergebnis (etwa nach einem Datenbankfehler) wird gar nichts gelöscht. Zusätzlich löst jetzt auch die reguläre Stream-Überwachung den Raid am Stream-Ende aus, als zweite, unabhängige Quelle — ein Doppel-Raid wird dabei verhindert. Eine erneute Autorisierung reaktiviert den Streamer jetzt immer, unabhängig von einer Discord-Verknüpfung, und stellt den Raid-Betrieb wieder her. Ein abgelaufener Token pausiert den Streamer sauber als technischen Fehler statt als „manuell abgeschaltet" und wird bei der nächsten Autorisierung automatisch geheilt; die Karenzzeit läuft jetzt verlässlich ab. Das Zuschauer- und Chatter-Tracking pro Stream funktioniert wieder.

**Ergebnis:** Der automatische Raid feuert am Stream-Ende wieder zuverlässig — über zwei unabhängige Wege —, neu autorisierte Streamer sind sofort wieder voll aktiv, abgelaufene Tokens lösen sich bei der nächsten Anmeldung von selbst, und die Stream-Statistiken erfassen wieder alle Zuschauer.

## #266 — Chat-Befehle & automatische Werbung nach Rust-Umstellung repariert

**Problem:** Beim Umstieg auf die neue Bot-Technik hatten sich einige Verhalten verändert: `!raid` nannte den Zielkanal nicht mehr und schwieg, wenn Raids für den Kanal gar nicht eingerichtet waren; die Lurker-Erinnerung kam als normale Chat-Nachricht statt als hervorgehobene Ankündigung; der `!invite`-Cooldown von einer Stunde wurde auch dann verbraucht, wenn gar nichts gesendet wurde; und mehrere Feinheiten der automatischen Werbung (Partner-Abschaltung, Kanalauswahl) griffen nicht mehr wie vorher.

**Änderung:** `!raid` bestätigt wieder mit Zielkanal („Raid auf … gestartet"), und wenn Raids für den Kanal nicht aktiviert sind, kommt eine klare Meldung statt Stille. Die Lurker-Erinnerung wird wieder als orange Ankündigung gesendet. Der `!invite`-Cooldown wird nur noch bei erfolgreichem Versand gesetzt — ein Fehlversuch blockiert nicht mehr eine Stunde; ein erfolgreicher `!invite` verschiebt zudem die nächste automatische Werbung. Die automatische Werbung respektiert wieder Partner-Abschaltungen und die korrekte Kanalauswahl, und der passende Werbetext wird anhand des Gesprächskontexts gewählt. `!uban` findet den letzten automatischen Bann jetzt auch nach einem Neustart des Bots wieder. Das An- und Abschalten des KI-Engagements läuft nur noch über das Verwaltungs-Dashboard; die Chat-Befehle dafür wurden entfernt.

**Ergebnis:** Chat-Befehle und automatische Werbung verhalten sich wieder wie vor der Umstellung, der `!invite`-Cooldown ist fair, und die Engagement-Steuerung ist im Dashboard gebündelt.

## #265 — Overlay-Baukasten als eigene Seite + öffentlich erreichbar

**Problem:** Der Overlay-Baukasten steckte mitten im Verwaltungs-Dashboard zwischen allen anderen Einstellungen — schwer auffindbar und nicht direkt teilbar. Zudem war die Overlay-Adresse über die öffentliche Domain gar nicht erreichbar: Der vorgelagerte Reverse-Proxy kannte den Pfad nicht und beantwortete ihn mit „nicht gefunden", obwohl der Dienst dahinter die Seite längst auslieferte.

**Änderung:** Der Baukasten ist jetzt eine eigene Seite unter `…/twitch/overlay`, von der Verwaltung aus verlinkt. Dieselbe Adresse erfüllt zwei Zwecke: ohne Streamer-Namen zeigt sie den Baukasten mit Live-Vorschau und URL-Generator, mit Streamer-Namen liefert sie das reine OBS-Overlay aus. Der Proxy leitet den Pfad jetzt korrekt an den Dienst weiter und erlaubt dem Overlay die nötigen Spielgrafiken (Rang-Abzeichen, Hero-Bilder) sowie die eingebettete Vorschau auf der eigenen Seite.

**Ergebnis:** Der Baukasten hat eine eigene, teilbare Adresse, und das in OBS einzutragende Overlay funktioniert jetzt auch über die öffentliche Domain — nicht nur intern.

## #264 — Overlay-Baukasten im Dashboard + Hilfe-Bereich

**Problem:** Das Stream-Overlay gab es nur als feste URL — ohne Möglichkeit auszuwählen, was angezeigt wird, und ohne Anleitung. In der Hilfe fehlten die Stat-Befehle und das Overlay komplett.

**Änderung:** Im Verwaltungs-Dashboard gibt es jetzt einen Overlay-Baukasten: Du schaltest einzeln ein, was angezeigt werden soll (Rang, Winrate, Serie, Live-Match), wählst die Ecke im Bild, siehst eine Live-Vorschau und bekommst die fertige OBS-URL zum Kopieren — samt Schritt-für-Schritt-Anleitung für OBS. Das Overlay liest diese Auswahl aus der URL, du brauchst also nichts zu speichern. Zusätzlich erklärt der Hilfe-Bereich jetzt die komplette Stat-Familie (`!rank`, `!wins`, `!winrate`, `!lastmatch`, `!streak`, `!mostplayed`, `!mmr`, `!live`) und das Overlay.

**Ergebnis:** Streamer stellen sich ihr Overlay in wenigen Klicks selbst zusammen und finden die nötige Anleitung direkt daneben und im Hilfe-Bereich.

## #263 — Stream-Overlay für OBS mit Live-Stats

**Problem:** Die neuen Stat-Daten (Rang, Winrate, Serie, Live-Match) gab es nur als Chat-Befehle — nicht sichtbar im Stream selbst.

**Änderung:** Es gibt jetzt ein einblendbares Stream-Overlay. Wer in OBS eine Browser-Quelle mit der Adresse `…/twitch/overlay?streamer=DEIN_TWITCH_NAME` hinzufügt, bekommt eine dezente, transparente Karte mit Rang (inkl. Rang-Abzeichen), Winrate und aktueller Serie — und sobald man in einem Match ist, eine Live-Zeile mit gespieltem Hero. Die Anzeige aktualisiert sich automatisch und zieht ihre Zahlen aus denselben verlässlichen Spieldaten wie die Chat-Befehle. Rang-Abzeichen und Hero-Bilder sind die offiziellen Deadlock-Spielgrafiken.

**Ergebnis:** Streamer können ihre aktuellen Deadlock-Stats direkt im Stream zeigen, ohne dass jemand einen Befehl tippen muss — ein Einrichtungsschritt in OBS genügt.

## #262 — Partner-Status bleibt bei Inaktivität und pausiertem Raid erhalten

**Problem:** Zwei Mechanismen entzogen Partnern zu Unrecht ihren Status. Erstens wurde ein Partner, der eine Weile nicht Deadlock streamte, automatisch komplett deaktiviert — und verlor damit auf einen Schlag Stats, Leaderboard, Live-Erkennung und Auto-Raid. Zweitens war der Partner-Status an den Raid-Schalter gekoppelt: Wer den Raid-Bot pausierte oder einen abgelaufenen Zugriff hatte, verlor ebenfalls das gesamte Stream-Tracking. In Summe standen rund 35 eigentlich aktive Partner fälschlich auf „archiviert".

**Änderung:** Inaktivität deaktiviert einen Partner nicht mehr — sie wird nur noch als interner Hinweis vermerkt, der Partner bleibt aktiv. Der aktive Partner-Status wird ausschließlich aus echtem Status, bewusstem Opt-out und technischer Pause bestimmt und ist vom Raid-Schalter entkoppelt: Ein pausierter Raid-Bot oder ein Zugriffsfehler nehmen Stats, Leaderboard und Tracking nicht länger weg. Die fälschlich deaktivierten Partner wurden wieder aktiv geschaltet.

**Ergebnis:** Aktive Partner behalten ihre Funktionen durchgehend — auch bei längeren Streampausen oder pausiertem Raid-Bot. Der Auto-Raid am Stream-Ende und die Live-Auswertungen greifen für die betroffenen Kanäle wieder.

## #261 — Stream-Sitzungen werden wieder zuverlässig aufgezeichnet

**Problem:** Beim Start einer Stream-Sitzung (und beim Abschluss) brach das Speichern mit einem Datenbankfehler ab — der Zeitstempel wurde im falschen Format übergeben. Für gerade live gegangene Streamer wurde dadurch keine neue Sitzung angelegt, was die Auswertungen dieser Streams lückenhaft machte.

**Änderung:** Der Start- und der Abschluss-Zeitstempel einer Sitzung werden beim Speichern jetzt korrekt als Zeitstempel-Wert behandelt (vorher als reiner Text, was die Datenbankspalte ablehnte). Der Fix ist verträglich mit beiden Spalten-Varianten.

**Ergebnis:** Stream-Sitzungen werden wieder zuverlässig angelegt und abgeschlossen — die Auswertungen erfassen wieder jeden Stream lückenlos.

## #260 — Zwei weitere Stat-Befehle: !mmr und !live

**Problem:** Es fehlten noch ein Rang-Verlauf („wie läuft mein Climb?") und eine schnelle Live-Auskunft im Chat.

**Änderung:** Zwei neue Befehle, beide aus den Steam-Daten des verknüpften Accounts:
- **`!mmr`** (auch `!climb`) — aktueller Rang plus Trend der letzten Tage (Stufen hoch, runter oder stabil). Der Verlauf baut sich ab jetzt auf, je länger der Account verknüpft ist.
- **`!live`** — zeigt, ob du gerade in einem laufenden Deadlock-Match bist, inklusive Hero und Spielminute.

Beide laufen wie die übrigen Stat-Befehle über den eigenen Spiel-Datendienst (kein externer Dienst). Ohne verknüpften Steam-Account weist der Bot freundlich darauf hin; beide stehen in der Befehls-Übersicht.

**Ergebnis:** Die Stat-Familie im Chat ist komplett — `!rank`, `!wins`, `!winrate`, `!lastmatch`, `!streak`, `!mostplayed`, `!mmr` und `!live`, alle mit echten Zahlen aus den Matches.

## #259 — Vier neue Stat-Befehle: !winrate, !lastmatch, !streak, !mostplayed

**Problem:** Bisher konnte der Chat nur Rang (`!rank`) und Karriere-Siege (`!wins`) zeigen. Für Zuschauer interessante Live-Werte wie die aktuelle Form, das letzte Spiel oder der Lieblings-Hero fehlten.

**Änderung:** Vier neue Befehle, alle gespeist aus der echten Match-Historie des verknüpften Steam-Accounts:
- **`!winrate`** — Siegquote über die letzten gewerteten Spiele (Siege/Niederlagen).
- **`!lastmatch`** — letztes Spiel: Sieg oder Niederlage, gespielter Hero, KDA.
- **`!streak`** — aktuelle Siegesserie oder Pechsträhne.
- **`!mostplayed`** — meistgespielter Hero der letzten Spiele.

Die Zahlen kommen aus den tatsächlichen Match-Ergebnissen (ungewertete/abgebrochene Spiele werden für die Quote ausgeklammert) — keine geschätzten Werte. Voraussetzung ist ein über den Discord verknüpfter Steam-Account; ohne Verknüpfung weist der Bot freundlich darauf hin. Alle vier stehen auch in der Befehls-Übersicht.

**Ergebnis:** Streamer können ihre aktuelle Form, ihr letztes Spiel und ihren Main-Hero auf Zuruf zeigen — mit echten Zahlen direkt aus den Matches.

## #258 — Aussagekräftige Diagnose für Raid- und Follower-Abläufe

**Problem:** Das in #255 angelegte Beobachtungs-Subsystem hatte noch keine Zulieferer: Raid-Abläufe und die Follower-Abfrage hinterließen nur freien Logtext, aber keine strukturierten, über einen Ablauf hinweg zusammenführbaren Diagnose-Ereignisse — der Diagnosespeicher blieb leer. Außerdem wurden ausgelöste Raids, deren Ankunft am Ziel nie bestätigt wurde, nicht regelmäßig aufgeräumt und konnten sich ansammeln.

**Änderung:** Raid-Abläufe melden jetzt an jedem Schritt ein strukturiertes Diagnose-Ereignis mit gemeinsamer Ablauf-Kennung — vom Start über die Zielauswahl (inklusive Auswahl- und Ausführungsdauer) bis zu Erfolg oder Fehlschlag — und führen Zähler (gestartete Raids sowie Ankunfts-Chat-Hinweise ohne zugehöriges Raid-Ereignis). Die Follower-Abfrage protokolliert ihre Abschlussentscheidung strukturiert (Erfolg, HTTP-Fehler oder fehlgeschlagene Anfrage). Diese Ereignisse fließen gebündelt und asynchron in den Diagnosespeicher aus #255. Zusätzlich räumt ein periodischer Lauf (alle fünf Minuten) ausgelöste Raids weg, deren Ankunft nach fünf Minuten nicht bestätigt wurde.

**Ergebnis:** Für Raids und Follower-Abfragen liegen jetzt zusammenführbare Ablauf-Diagnosen und Zähler vor statt verstreuter Logzeilen; der Diagnosespeicher füllt sich mit echten Daten, und liegengebliebene, nie bestätigte Raids sammeln sich nicht mehr an. Damit sind weitere Umbau-Reste geschlossen.

## #257 — Zuverlässigere Ereignis-Verarbeitung und genauere Live-Erkennung

**Problem:** Nach dem Sprachumbau fehlten in der Verarbeitung der Twitch-Ereignisse (EventSub) noch mehrere Robustheits-Details der alten Version. Bei manchen Ereignissen wurde der Kanal nicht zuverlässig erkannt, eingehende Raids konnten bei einem kurzen Verarbeitungsfehler verloren gehen, unbekannte Ereignistypen verschwanden stillschweigend, und kurzzeitige Datenbankaussetzer beim Schreiben führten sofort zum Abbruch. Außerdem ordnete die Zuschauer-Verlaufskurve Werbepausen nicht zu und die Zuordnung von Streamern zu ihrer Twitch-Kennung war in Randfällen ungenau.

**Änderung:** Die Ereignis-Verarbeitung ist jetzt robuster: Der zugehörige Kanal wird über eine Kette von Ersatzfeldern aufgelöst, eingehende Raids laufen über eine wiederholbare Verarbeitungs-Warteschlange statt verloren zu gehen, unbekannte Ereignistypen werden nach mehreren Fehlversuchen sauber zur Seite gelegt statt verworfen, und Schreibvorgänge versuchen es bei vorübergehenden Datenbankfehlern automatisch erneut. Die Streamer-Erkennung nutzt zusätzliche Ersatzquellen, Werbepausen erscheinen wieder korrekt in der Zuschauer-Verlaufskurve, und der Schutz vor versehentlichen Massen-Erwähnungen (@everyone/@here und Rollen) in Ankündigungen wurde gehärtet. Im Hintergrund wurden zudem mehrere Datenbankspalten sauber auf einheitliche Typen umgestellt.

**Ergebnis:** Der Bot verarbeitet Twitch-Ereignisse verlässlicher und verliert bei kurzen Störungen keine Raids oder Schreibvorgänge mehr; Auswertungen und Live-Erkennung sind genauer. Weitere Umbau-Reste sind damit geschlossen.

## #256 — !wins zeigt deine Deadlock-Siege im Chat

**Problem:** Im Chat ließ sich der Rang zeigen (`!rank`), aber nicht die eigene Erfolgsbilanz.

**Änderung:** Neuer Befehl `!wins`: Wer seinen Steam-Account (über den Discord) verknüpft hat, bekommt seine Deadlock-Karriere-Siege im Chat angezeigt. Der Befehl taucht auch in der Befehls-Übersicht auf. Bewusst nur die Siege: Die verfügbare Datenquelle liefert über diesen Weg keine verlässliche Gesamt-Match-Zahl, deshalb gibt es keine erfundene Niederlagen- oder Winrate-Anzeige.

**Ergebnis:** Streamer können ihre Siege auf Zuruf im Chat zeigen — mit korrekten Zahlen statt geschätzter.

## #255 — Robustere Twitch-Anbindung, genauere Auswertungen und gehärtete Anmeldung

**Problem:** Nach dem Sprachumbau fehlten noch mehrere Robustheits- und Genauigkeits-Details der alten Version. Kurzzeitige Twitch-API-Aussetzer führten sofort zu Fehlern statt zu einem zweiten Versuch; einige Auswertungsseiten antworteten bei Datenbankproblemen mit einem harten Serverfehler statt mit einer leeren, gekennzeichneten Ansicht; Werbepausen flossen nicht in die Zuschauer-Verlaufskurve ein; und an der Admin-Anmeldung fehlte eine Geräte-/IP-Bindung.

**Änderung:** Die Twitch-Anbindung versucht transiente Fehler (5xx, Verbindungsabbrüche) jetzt bis zu dreimal mit Backoff und nutzt ein großzügigeres Zeitlimit; wiederholte ungültige-Client-Fehler werden für 15 Minuten ausgebremst, statt Twitch weiter zu hämmern. Auswertungen rechnen genauer und robuster: Werbepausen werden in der Verlaufskurve als solche erkannt, Zuschauer-Kennzahlen berücksichtigen wieder fehlende Zeitstempel korrekt, Admin-Bewertungen werden dem richtigen Konto zugeordnet, und Analyse-Seiten liefern bei Datenbankproblemen eine saubere „keine Daten"-Ansicht statt eines Serverfehlers. Die Admin-Anmeldung bindet die Sitzung an Gerät und Herkunft, und abgelehnte Zugriffe liefern eine klare, maschinenlesbare Begründung. Im Hintergrund wurde zudem ein Beobachtungs-Subsystem für Raid- und Analyse-Abläufe angelegt (Grundlage für spätere Diagnose-Ansichten).

**Ergebnis:** Der Bot übersteht kurze Twitch-Störungen ohne sofortigen Abbruch, die Dashboards zeigen genauere Zahlen und fallen bei Datenproblemen weich zurück, und die Admin-Anmeldung ist sicherer. Damit sind weitere Umbau-Reste geschlossen.

## #254 — Einrichtungs-Assistent jetzt direkt im Verwaltungs-Dashboard

**Problem:** Der geführte Einrichtungs-Assistent (Steam- und Discord-Verknüpfung, Go-Live-Posts an/aus, Schritt-Checkliste) war nur als Reiter im Analyse-Dashboard erreichbar. Wer das Verwaltungs-Dashboard öffnete — die naheliegende Stelle zum Einrichten — fand die neuen Optionen dort gar nicht.

**Änderung:** Der Assistent wird jetzt direkt im Verwaltungs-Dashboard angezeigt, gleich unter der Konto-Übersicht. Steam und Discord verknüpfen, die automatischen Go-Live-Posts steuern und die Einrichtungsschritte abhaken laufen damit an einer Stelle, ohne ins Analyse-Dashboard wechseln zu müssen.

**Ergebnis:** Alle Einrichtungs-Optionen sitzen dort, wo man sie sucht: im Verwaltungs-Dashboard. Der bisherige Zugang im Analyse-Dashboard bleibt zusätzlich bestehen.

## #253 — FAQ in den zentralen Hilfe-Bereich zusammengeführt

**Problem:** Die FAQ lebte als eigene Seite getrennt vom restlichen Bot-Wissen — zwei Quellen, doppelter Pflegeaufwand. Außerdem schlug im Hintergrund das Einsammeln von Werbe- und Abo-Daten still fehl.

**Änderung:** Alle FAQ-Inhalte sind jetzt Teil derselben gepflegten Wissensbasis und erscheinen im aufgeräumten Hilfe-Bereich (nach Themen gruppiert, mit Inhaltsverzeichnis). Die alte separate FAQ-Seite leitet dorthin weiter. Zusätzlich behoben: das Einsammeln von Werbe-Zeitplan- und Abo-Schnappschüssen schrieb Zeitstempel im falschen Format und schlug dadurch fehl — jetzt korrekt.

**Ergebnis:** Eine einzige Wissensquelle für Hilfe, FAQ und Chat (nichts doppelt), und die Werbe-/Abo-Daten landen wieder zuverlässig in den Auswertungen.

## #252 — Streamer-Hilfe, Go-Live-Tipps, !rank und geführtes Onboarding

**Problem:** Viele Streamer wussten kaum, was der Bot alles kann — es fehlten eine zentrale Hilfe, Hinweise direkt im Stream und ein einfacher Einstieg. Und es gab keinen Weg, den eigenen Deadlock-Rang im Chat zu zeigen.

**Änderung:** Mehrere zusammenhängende Bausteine sind dazugekommen, die alle aus derselben gepflegten Wissensbasis schöpfen:
- **Hilfeseite & Befehls-Übersicht:** Es gibt jetzt eine Hilfeseite mit erklärtem Bot-Wissen und eine gruppierte Übersicht aller Chat-Befehle. Im Chat führen `!commands` (Link zur Übersicht) und `!help <thema>` (Kurzerklärung mit Link) dorthin.
- **Tipp beim Live-Gehen:** Gehst du mit Deadlock live, postet der Bot eine kurze, wechselnde Tipp-Nachricht als erste Chat-Zeile — er wählt klug (Unbenutztes und lange Vergessenes zuerst), hält mindestens 12 Stunden Abstand und ist im Dashboard abschaltbar.
- **`!rank`:** Hast du deinen Steam-Account (über den Discord) verknüpft, zeigt `!rank` deinen aktuellen Deadlock-Rang im Chat.
- **Onboarding-Wizard:** Neue Streamer werden im Dashboard Schritt für Schritt durch die Funktionen, die Discord- und Steam-Verknüpfung und die Go-Live-Tipps geführt — resumierbar und mit sichtbarem Fortschritt.

**Ergebnis:** Streamer verstehen schneller, was der Bot kann, bekommen Hinweise im richtigen Moment und können ihren Rang zeigen. Alles speist sich aus einer einzigen Wissensquelle, sodass nichts doppelt gepflegt werden muss.

## #251 — Chat-Schutz erkennt die „Headset-kaputt, schreib mir privat"-Masche

**Problem:** Eine verbreitete Betrugsmasche rutschte durch den Chat-Schutz: Ein Erstschreiber täuscht ohne Anlass ein technisches Problem vor („mein Headset geht nicht, antworte mir im Chat") und versucht so, das Gespräch sofort von Twitch weg ins Private zu ziehen — der erste Schritt vieler Scams. Weil diese erste Nachricht kurz war und kein bisher bekanntes Stichwort enthielt, nahm der Wächter sie gar nicht erst unter die Lupe.

**Änderung:** Der Chat-Wächter kennt diese Ausreden-und-Sofort-Pivot-Masche jetzt ausdrücklich und zieht typische Einstiege wie „reply me" oder „dm me" sofort zur Prüfung heran, statt erst mehrere Nachrichten abzuwarten. Bestätigt sich der Betrugsversuch bei einem Erstschreiber, richtet sich die Reaktion nach dem Alter des Kontos: brandneue — oder nicht eindeutig alte — Konten werden gebannt, während nachweislich ältere Konten (über drei Monate) nur die Nachricht gelöscht und einen Timeout bekommen, da hinter ihnen ein gekapertes Konto eines echten Zuschauers stecken könnte.

**Ergebnis:** Die Masche wird zuverlässig erkannt und je nach Risiko angemessen geahndet — frische Wegwerf-Konten fliegen sofort raus, echte und womöglich übernommene Konten behalten die Chance auf Wiederherstellung. Der Schutz greift unverändert nur bei Erstschreibern; Stammzuschauer sind nie betroffen.

## #250 — Hilfe-Chat auf der Website lernt aus einer pflegbaren Wissensbasis

**Problem:** Der Frage-Chat auf der Website beantwortete Fragen zum Bot bisher nur aus einem fest eingebauten Kurz-Steckbrief. Dadurch konnte er nur Grundlegendes, keine tiefergehenden Fragen, und er nannte keine Quelle — und es ließ sich nicht leicht erweitern, ohne am Bot selbst zu schrauben.

**Änderung:** Der Chat liest jetzt aus einer gepflegten Sammlung einzelner Wissens-Dokumente. Pro Frage wählt er über einen Stichwort-Abgleich die passenden Dokumente aus, antwortet ausschließlich auf deren Grundlage und gibt die genutzte Quelle an. Findet sich zu einer Frage kein passendes Dokument, sagt er ehrlich, dass er das (noch) nicht weiß, und verweist auf die Seite bzw. den Discord — statt etwas zu erfinden.

**Ergebnis:** Antworten sind genauer und belegt, die Wissensbasis lässt sich jederzeit erweitern oder korrigieren, ohne den Bot anzufassen, und es gibt keine geratenen Auskünfte mehr. Damit ist die Grundlage gelegt, denselben Wissensstand künftig auch auf einer Hilfeseite und im Twitch-Chat auszuspielen.

## #249 — Analyse-Genauigkeit, Abo-Boni und Raid-Verlauf nach dem Umbau nachgezogen

**Problem:** Nach dem Sprachumbau fehlten an mehreren Stellen Daten und Feinheiten der alten Version. Auswertungen ließen anonyme Zuschauer und einzelne Kennzahlen aus, Follower-Gesamtzahlen blieben teils leer, Abonnenten- und Werbe-Daten wurden nicht mehr regelmäßig erfasst, der Bonus beim Jahres-Abo wurde nicht gutgeschrieben, und der Raid-Verlauf war nicht abrufbar.

**Änderung:** Die Auswertungen rechnen wieder vollständig: anonyme Chatter ohne Login werden mitgezählt, Bot-Konten unabhängig von Groß-/Kleinschreibung herausgefiltert, verfälschte Follower-Sprünge ausgeklammert, Vergleichswerte (Peer-Benchmark und Tier-Einstufung) wieder berechnet, und die Bestenliste gibt fremde Discord-Kennungen nicht mehr preis; tritt ein Datenbankfehler auf, meldet die Demografie ehrlich einen Fehler statt geschönter Nullwerte. Follower-Gesamtzahlen werden wieder real abgerufen (Bot-Zugang mit Streamer-Zugang als Rückfall). Abonnenten- und Werbe-Snapshots werden regelmäßig eingesammelt und bilden die Datengrundlage der Dashboards. Beim Jahres-Abo werden die Bonus-Monate wieder gutgeschrieben, und nach jeder Abo- oder Plan-Änderung wird die Raid-Einstufung sofort neu berechnet. Der Raid-Verlauf ist über eine eigene, filterbare Verlaufsseite wieder abrufbar. Schließlich werden raid-fähige Kanäle wieder zuverlässig betreten, fehlende Einladungen beim Start nachgezogen und eingehende Raids robuster bestätigt (offene Session als Rückfall, korrekte Zuschauerzahl).

**Ergebnis:** Die Dashboards zeigen wieder die vollständigen, genauen Zahlen wie vor dem Umbau, der Jahres-Abo-Bonus stimmt, und sowohl Raid-Verlauf als auch Raid-Erkennung sind zurück. Ergänzt wurden zudem technische Härtungen (z. B. Schutz vor doppelt ausgeführten Anfragen und sicherere Token-Behandlung) ohne sichtbare Auswirkung für Streamer.

## #248 — Großer Dashboard-Nachzug: Markt-Recherche, Bezahl-Vorschau, Demo-Inhalte, sicherere Anmeldung

**Problem:** Nach dem Sprachumbau fehlten im Dashboard noch zahlreiche Seiten und Verhaltensweisen der alten Version. Einige Kacheln blieben leer, ganze Seiten liefen ins Leere, und an Anmelde- sowie Auswertungspfaden fehlten Feinheiten, die vor dem Umbau selbstverständlich waren.

**Änderung:** Dieser Nachzug schließt die größten Lücken auf einmal. Die Markt-Recherche-Seite mit DACH-Überblick ist samt Datenquelle zurück (beobachtete Kanäle, Zuschauer-Verlauf über 24 Stunden, Chat-Stimmung, Zuschauer-Überschneidung). Vor dem Bezahlvorgang gibt es wieder eine Vorschau mit Plan-Prüfung, Bereitschaftsstatus und nächsten Schritten, und der Checkout weist auf die AGB sowie das Erlöschen des Widerrufsrechts mit Leistungsbeginn hin. Die Demo-Ansicht zeigt wieder vollständige Beispielinhalte (Beispiel-Analyse, Coaching-Hinweise, Monats-, Wochen- und Heatmap-Kacheln). Die Anmeldung wurde robuster und sicherer: ein zuvor pausierter oder entkoppelter Partner wird beim erneuten Login automatisch reaktiviert, die Partner-Sitzung ist an das Gerät gebunden, Anmelde- und Wechsel-Routen sind gegen Überlastung gedrosselt, und alle Antworten tragen jetzt Standard-Sicherheitsheader. Raid-Einladungslinks aus Discord führen wieder zu nativen Seiten; abgelaufene Links zeigen eine verständliche Hinweisseite. In den Auswertungen werden anonyme Chatter wieder korrekt mitgezählt, die Bestenliste gibt fremde Discord-Kennungen nicht mehr preis, Titelvorschläge berücksichtigen wieder Rang und Live-Status, und verfälschte Follower-Differenzen werden herausgerechnet.

**Ergebnis:** Das Dashboard ist wieder weitgehend vollständig — fehlende Seiten und Kacheln sind zurück, die Anmeldung ist sicherer und stabiler, und mehrere Auswertungen rechnen wieder so genau wie vor dem Umbau. Ergänzt wurden zudem admin-interne Werkzeuge ohne sichtbare Auswirkung für Streamer.

## #247 — Admin-Streamer-Verwaltung läuft wieder nativ statt ins Leere

**Problem:** Im Admin-Bereich des Dashboards führten die zentralen Streamer-Aktionen — einen Streamer hinzufügen (per Name, Login oder Twitch-Link), entfernen, eine Discord-Verknüpfung samt Mitglieds-Markierung setzen, sowie eine manuelle Chat-Aktion in einen Partner-Kanal senden — ins Leere. Sie wurden noch an den beim Sprachumbau abgeschalteten Alt-Dienst weitergereicht und endeten dort mit einem Server-Fehler.

**Änderung:** Diese Aktionen laufen jetzt direkt im neuen Dashboard-Backend gegen dieselbe gemeinsame Datenbankschicht, die auch die interne Schnittstelle des Bots nutzt — kein Umweg mehr über den toten Alt-Dienst. Das Hinzufügen legt den Streamer an und trägt die Twitch-Kennung nach, sobald sie ermittelbar ist; das Entfernen löst die Partnerschaft; die Discord-Verknüpfung speichert Profil und Mitglieds-Markierung. Die manuelle Chat-Aktion ist auf den freigeschalteten Owner beschränkt, prüft Modus, Farbe und eine Längenobergrenze, schließt abgemeldete oder archivierte Partner aus und wird zum tatsächlichen Senden an den Bot übergeben. Alle Aktionen sind weiterhin durch Admin-Anmeldung und Formular-Schutz abgesichert.

**Ergebnis:** Die Streamer-Verwaltung im Admin-Bereich funktioniert wieder vollständig; die Server-Fehler an diesen Stellen entfallen. Es handelt sich um Admin-Funktionen ohne sichtbare Auswirkung für Streamer.

## #246 — Nach dem Umbau wieder lückenlos: Werbe-Abweisungen, Raid-Erkennung und Event-Abos

**Problem:** Beim Sprachumbau des Bots waren einige Verhaltensweisen der alten Version noch nicht wieder angeschlossen. Wies ein Kanal eine automatische Nachricht des Bots ab — Werbung, Streamer-Anwerbung oder Partner-Raid-Gruß —, merkte sich der Bot das nicht und versuchte es beim nächsten Anlass erneut im selben Kanal. Widerrief Twitch ein Ereignis-Abo, über das der Bot Live-Starts und Raids erfährt, blieb das bis zum nächsten Neustart unbemerkt. Und bei der Bestätigung einer eingehenden Raid-Welle floss die Vorab-Information, ob ein Partner raidet, nicht mehr in die Einordnung ein.

**Änderung:** Diese Pfade sind jetzt zentral wieder verdrahtet. Eine abgewiesene Bot-Nachricht wird quellabhängig gesperrt — sieben Tage für Werbung und Anwerbung, drei Tage für Partner-Raid-Grüße —, sodass der Bot denselben ablehnenden Kanal in dieser Zeit nicht erneut anschreibt. Ein widerrufenes Ereignis-Abo wird sofort zur Laufzeit als inaktiv vermerkt und beim nächsten regelmäßigen Abgleich automatisch neu eingerichtet, statt auf einen Neustart zu warten. Die Raid-Ankunft berücksichtigt bei der Bestätigung wieder die Erwartung, ob ein Partner raidet, und ordnet Partner- und Fremd-Raids entsprechend ein. Fehlt dem Bot in einem Kanal die Moderatorenrolle — was die Abos dort scheitern lässt —, trägt er sich automatisch wieder ein; und die Follower-Gesamtzahl wird über den Bot-Zugang abgerufen, wenn der Streamer-Zugang die nötige Berechtigung nicht mitbringt.

**Ergebnis:** Der Bot belästigt Kanäle nicht länger mit wiederholten, ohnehin abgewiesenen Nachrichten, verliert nach einem Abo-Widerruf keine Live- und Raid-Ereignisse mehr bis zum nächsten Neustart und erkennt eingehende Partner-Raids wieder korrekt. Diese Lücken waren Reste des Umbaus und sind damit geschlossen.

## #245 — Datenbank-Tests der Streamer-Übersicht laufen wieder verlässlich durch

**Problem:** Mehrere Datenbank-Tests rund um die Streamer-/Partner-Übersicht und den Admin-Login liefen faktisch nie mit: Ohne konfigurierte Test-Datenbank übersprangen sie sich still. Einmal echt ausgeführt, brachen sie ab, weil die Test-Vorlagen das Produktionsschema nur unvollständig abbildeten — eine benötigte Tabelle sowie einige Spalten fehlten, die die zugrunde liegenden Abfragen voraussetzen.

**Änderung:** Die Test-Vorlagen bilden das Produktionsschema jetzt vollständig ab (fehlende Tabelle und Spalten ergänzt, eine doppelte Definition zur einzigen Quelle zusammengeführt). Gegen eine echte Datenbank ausgeführt laufen die betroffenen Übersicht- und Login-Tests damit durch.

**Ergebnis:** Streamer-Übersicht und Admin-Login sind wieder durch lauffähige Datenbank-Tests abgesichert. Abweichungen zwischen Test- und Produktionsschema fallen künftig sofort im Test auf, statt erst im Betrieb sichtbar zu werden.

## #244 — Automatische Scam-Bans im Dashboard sichtbar und mit einem Klick rücknehmbar

**Problem:** Der Scam-Schutz konnte verdächtige Erstschreiber bei hoher Sicherheit automatisch bannen oder timeouten. Diese automatischen Eingriffe tauchten im Dashboard aber nirgends auf — die Fall-Liste zeigte nur manuell zu prüfende Vorschläge. Ein versehentlich getroffener Zuschauer ließ sich darum nicht bequem über das Dashboard zurückholen.

**Änderung:** Die Fall-Liste zeigt jetzt auch automatisch gebannte und getimeoutete Fälle, jeweils klar gekennzeichnet (Auto-gebannt bzw. Auto-Timeout). Zu jedem dieser Fälle gibt es eine Rücknahme-Schaltfläche, die den Bann oder Timeout mit einem Klick aufhebt. Die Entbannung wird dabei echt im Kanal ausgeführt — über den Bot, der den dafür nötigen, laufend erneuerten Zugang besitzt — und nicht bloß im Dashboard vermerkt.

**Ergebnis:** Streamer sehen alle Eingriffe des Scam-Schutzes an einer Stelle und können einen Fehlalarm sofort und vollständig korrigieren, ohne in die Twitch-Einstellungen wechseln zu müssen.

## #243 — Bestenlisten und Statistiken brechen nicht mehr am Partner-Kennzeichen ab

**Problem:** Die Auswertung der Bestenlisten und Streamer-Statistiken verglich das Partner-Kennzeichen — in der Datenbank ein Ja/Nein-Wert — fälschlich mit einer Zahl. In der produktiven Datenbank ist dieser Vergleich ungültig, sodass die betroffene Abfrage mit einem Serverfehler abbrechen konnte. Die automatischen Tests bemerkten das nie, weil ihre Test-Datenbank dieselbe Spalte als Zahl anlegte und damit das echte Schema gar nicht abbildete.

**Änderung:** Die Auswertung nutzt jetzt durchgängig echte Ja/Nein-Logik — so, wie es eine bereits zuvor korrigierte Stelle vormacht. Zusätzlich legen die Test-Vorlagen das Kennzeichen jetzt selbst als Ja/Nein-Wert an und spiegeln damit das Produktionsschema. Dadurch wird genau dieser Typ-Konflikt im Test reproduziert, statt ihn zu verstecken.

**Ergebnis:** Bestenlisten und Statistiken laufen stabil durch, ohne am Partner-Kennzeichen abzubrechen. Und weil die Tests nun das echte Schema verwenden, kann dieselbe Fehlerklasse nicht unbemerkt zurückkehren.

## #242 — Partner-Zugriffsstatus lädt wieder zuverlässig

**Problem:** Beim Ermitteln des Partner-Zugriffsstatus wurde ein Datenbank-Kennzeichen im falschen Werttyp gelesen. In der produktiven Datenbank führte das zu einem Laufzeitfehler, der den Abruf dieses Status abbrechen ließ.

**Änderung:** Das Kennzeichen wird jetzt im korrekten Typ eingelesen — identisch zu einer anderen Stelle, die dasselbe Feld bereits richtig auswertet — und der Ja/Nein-Zustand daraus abgeleitet.

**Ergebnis:** Der Partner-Zugriffsstatus wird wieder zuverlässig geladen; der Laufzeitfehler kann an dieser Stelle nicht mehr auftreten.

## #241 — Bot läuft auch ohne internen Zusatz-Token weiter

**Problem:** Fehlte dem Bot die interne Zusatz-Berechtigung für den Versand von Chat-Begrüßungen, ließ sich der Begrüßungs-Dienst nicht aufbauen — und damit schaltete der Bot bisher den **gesamten** Nachlauf einer Streamer-Anmeldung ab: Partner-Abgleich, Rollenvergabe und Moderatoren-Einrichtung entfielen mit. Eine einzige fehlende Berechtigung legte also weit mehr lahm als nötig.

**Änderung:** Fehlt die Berechtigung jetzt, springt ein stiller Ersatz-Begrüßer ein. Die eigentliche Chat-Begrüßung wird in diesem Fall übersprungen und nur vermerkt; der gesamte übrige Anmelde-Nachlauf läuft unverändert weiter.

**Ergebnis:** Partner-Verknüpfung, Rollen und Moderatoren-Setup greifen auch in Umgebungen ohne den internen Zusatz-Token. Lediglich die Begrüßungsnachricht im Chat entfällt dort — alles andere bleibt vollständig funktionsfähig.

## #240 — Bot-Anmeldung übersteht Neustarts und Token-Wechsel zuverlässig

**Problem:** Nach jeder Token-Erneuerung behielt der Chat-Bot seinen frischen Zugangs-Token nur im Arbeitsspeicher; der zentrale Geheimnis-Speicher blieb auf einem veralteten Stand. Bei jedem Neustart prüfte der Bot deshalb erst den alten Token, bekam eine Ablehnung und heilte sich still über den Erneuerungs-Token — harmlos, aber Lärm in den Protokollen. Gefährlicher war der versteckte Fall: Tauscht der Anbieter beim Erneuern auch den langlebigen Erneuerungs-Token aus, ging dessen neue Fassung verloren. Hätte der Anbieter den alten je für ungültig erklärt, wäre der Bot nach einem Neustart ausgesperrt gewesen.

**Änderung:** Der Bot schreibt seinen erneuerten Zugangs-Token — und bei einem Wechsel auch den Erneuerungs-Token — jetzt zurück in den zentralen Geheimnis-Speicher. Das geschieht nach besten Kräften: Schlägt das Zurückschreiben fehl, läuft der Chat mit dem im Speicher gültigen Token unbeeinträchtigt weiter, es wird nur ein Fehler vermerkt. Fehlt die dafür nötige, eng begrenzte Schreib-Berechtigung, verhält sich der Bot exakt wie bisher. Token-Werte landen dabei zu keinem Zeitpunkt in Protokollen.

**Ergebnis:** Der nächste Start findet einen gültigen Token vor, die wiederkehrende Start-Warnung verschwindet, und ein Wechsel des Erneuerungs-Tokens kann den Bot nicht mehr aussperren. Ein dauerhafter Kontrolltest koppelt zwei aufeinanderfolgende Starts und stellt sicher, dass genau diese Fehlerklasse künftig sofort auffällt, statt sich hinter der stillen Selbstheilung zu verstecken.

## #239 — Twitch-Dashboards laden schneller und robuster

**Problem:** Die Dashboard-Seiten luden viele statische Dateien ohne brauchbare Browser-Zwischenspeicherung und ohne HTTP-Kompression. Zusätzlich startete die Chat-Auswertung direkt eine Detailabfrage, obwohl noch keine Session ausgewählt war; fehlerhafte Detailantworten wurden unnötig erneut versucht. Ein Kategorien-Ranking konnte zudem durch einen Datenbank-Typfehler mit Serverfehler abbrechen.

**Änderung:** Statische Dashboard-Dateien werden jetzt mit langfristigem Cache ausgeliefert und Antworten des Rust-Dashboards können komprimiert werden. Die Chat-Hype-Zeitlinie startet erst nach einer echten Session-Auswahl, Client-Fehler werden nicht mehr blind wiederholt und das Kategorien-Ranking nutzt den korrekten Bool-Typ der gespeicherten Partnerdaten.

**Ergebnis:** Wiederholte Dashboard-Aufrufe müssen große Assets nicht mehr neu laden, unnötige Hintergrund-Requests fallen weg und das Ranking bricht nicht mehr wegen des Partner-Flags ab. Das reduziert Wartezeit im Browser und vermeidet vermeidbare Last im Backend.

## #238 — Admin-Modus vollständig zwischen Nutzer- und Admin-Ansicht umschaltbar

**Problem:** Die ersten beiden Umsetzungen waren jeweils nur teilweise konsistent: Zuerst wechselte die Statusanzeige in die Nutzeransicht, während die Startseite weiterhin einen Admin-Parameter verlangte und dadurch eine Login-Schleife auslöste. Danach wurde der Admin-Status fest erzwungen, wodurch „Beenden“ wirkungslos blieb. Ein weiterer Versuch ließ Status- und Startseitenabfragen gleichzeitig wechseln und konnte einen leeren Bildschirm erzeugen.

**Änderung:** Die eigene Twitch-Identität gilt nun in beiden Darstellungen als stabile Grundlage. Beim Umschalten wird zuerst der neue Status bestätigt und erst danach werden die davon abhängigen Startseiten- und Partnerdaten neu geladen. Während des Wechsels bleiben alte Abfragen angehalten, damit kein Request mit gemischtem Admin-/Nutzerzustand entsteht. Alte Sitzungsschalter aus den fehlerhaften Vorversionen werden nicht übernommen; ein Vollzugriff muss nach diesem Update bewusst neu aktiviert werden.

**Ergebnis:** Das Dashboard startet in der echten Nutzeransicht des eigenen Kanals. „Admin-Modus aktivieren“ schaltet den Vollzugriff für die aktuelle Browser-Sitzung ein; „Beenden“ kehrt ohne Umleitung, Login-Schleife oder leeren Bildschirm zur Nutzeransicht zurück.

## #236 — Admin-Modus vorübergehend zurückgenommen (Lade-Schleife behoben)

**Problem:** Der in #235 eingeführte Admin-Modus führte beim Admin-Login zu einer Lade-Schleife: Das Dashboard sprang ununterbrochen zwischen Ladeanzeige und Anmeldung hin und her und ließ sich nicht öffnen. Ursache war eine Inkonsistenz — die Startseite forderte für die zugrunde liegende Sitzung zwingend die Angabe eines Kanals, die in der neuen Standard-Ansicht nicht mitgeschickt wurde, was wiederholt zu einer Neuanmeldung führte.

**Änderung:** Die automatische Umschaltung auf die Nutzer-Ansicht wurde vorerst deaktiviert; ein Admin sieht wieder die vollständige Admin-Ansicht wie zuvor. Zusätzlich wurde ein versteckter Datenbank-Typfehler beim Laden des Partner-Zugangsstatus korrigiert.

**Ergebnis:** Das Dashboard öffnet wieder normal. Der dedizierte Admin-Modus wird überarbeitet und kommt sauber verdrahtet zurück.

## #235 — Admin sieht das Dashboard jetzt standardmäßig wie ein normaler Nutzer

**Ausgangslage:** Wer als Administrator eingeloggt war, bekam im Streamer-Dashboard automatisch alles freigeschaltet — den höchsten Tarif und jede Funktion, unabhängig vom tatsächlichen Plan. Das war bequem, verdeckte aber die echte Nutzersicht: War für reguläre Nutzer etwas gesperrt, leer oder kaputt, fiel das nicht auf, weil die Admin-Ansicht alles überschrieb.

**Was geändert wurde:** Das eigene Kanal-Dashboard zeigt einem Admin jetzt standardmäßig genau das, was ein normaler Nutzer sieht — den echten Tarif samt der zugehörigen Sperren und Hinweise. Der volle Admin-Zugriff lässt sich bei Bedarf über einen neuen Schalter „Admin-Modus" in der Seitenleiste gezielt einschalten; ist er aktiv, erinnert ein Hinweisband oben daran. Der Modus gilt nur für die laufende Sitzung und schaltet sich nach dem Abmelden oder Schließen des Browsers von selbst wieder ab. Die separaten Admin-Werkzeuge bleiben unverändert erreichbar.

**Wie es jetzt läuft:** Nach dem Login landet ein Admin in der normalen Nutzeransicht und erkennt damit sofort, wenn für echte Nutzer etwas nicht stimmt. Ein Klick auf „Admin-Modus aktivieren" schaltet den vollen Zugriff frei, „Admin-Modus beenden" oder das Hinweisband führen zurück. So bleibt der Blick auf die tatsächliche Nutzererfahrung erhalten, ohne dass Admin-Rechte verloren gehen.

## #234 — Rust-Cutover stabilisiert und fehlerhafte Hintergrundpfade gestoppt

**Problem:** Nach der Umstellung des Twitch-Bots liefen mehrere Hintergrundfunktionen entgegen der getroffenen Entscheidungen automatisch weiter. Gleichzeitig scheiterten Event-Daten und Clip-Metadaten an abweichenden Datenbanktypen, Diagnose-Endpunkte lieferten nur Scheinantworten und dauerhaft blockierte Chat-Abonnements wurden bei jedem Abgleich erneut als Fehler gezählt.

**Änderung:** Highlight-Erstellung, Clip-Abruf und Stream-Transkription sind jetzt standardmäßig aus und benötigen ein ausdrückliches Opt-in. Die betroffenen Datenbanktypen und Schreibpfade wurden vereinheitlicht, Dead-Letter-Einträge können tatsächlich erneut eingeplant werden und Diagnoseansichten lesen den echten persistenten Zustand. Dauerhaft blockierte Chat-Abonnements werden separat ausgewiesen. Die Live-Rollenlogik verwendet vorhandene Rollen wieder und unterdrückt wiederholte fehlgeschlagene Anlageversuche.

**Ergebnis:** Der Bot startet mit den vereinbarten deaktivierten Funktionen, erzeugt keine wiederkehrenden Typfehler mehr und liefert bei Diagnose und Wiederanlauf belastbare Ergebnisse. Die reparierten Clip-Pfade bleiben vorhanden und getestet, werden aber bis zu einer bewussten Freigabe nicht ausgeführt.

## #233 — Scam-Schutz jetzt im Dashboard steuerbar

**Ausgangslage:** Der KI-Scam-Wächter prüft Erstschreiber im Chat auf aufgesetzte Betrugsmaschen (etwa Beziehungs- oder Wachstums-Pitches), die einfache Wortfilter durchrutschen. Er lief bereits im Hintergrund und konnte je nach Vorgabe automatisch bannen, Fälle zur Sichtung sammeln oder nur melden — aber Partner hatten keine Oberfläche, um dieses Verhalten einzustellen oder gemeldete Fälle nachzusehen. Steuern ließ sich das nur über Chat-Befehle und interne Schalter.

**Was geändert wurde:** In der Verwaltung gibt es jetzt einen Bereich „Scam-Schutz". Dort lässt sich der Schutz an- und abschalten, das Verhalten bei sehr hoher Sicherheit wählen (automatisch bannen, automatisch Timeout oder nur melden) und über zwei Schwellen festlegen, ab welcher eingeschätzten Sicherheit automatisch gehandelt und ab welcher ein Fall überhaupt zur Sichtung vorgeschlagen wird. Darunter listet eine Queue die offenen Verdachtsfälle.

**Wie es läuft:** Jeder Verdachtsfall zeigt den betroffenen Account, die eingeschätzte Sicherheit, die Kategorie und die Begründung des Wächters; auf Wunsch klappt der zugrunde liegende Chat-Auszug auf. Pro Fall kann der Partner direkt bannen oder ihn als harmlos abhaken; ein versehentlicher oder automatischer Bann lässt sich mit einem Klick zurücknehmen — das entsperrt den Account wieder und fließt dem Wächter als Fehlalarm ins Lernen ein. Die Vorschlagsschwelle ist an die Auto-Schwelle gekoppelt und kann nie über ihr liegen.
## #232 — Bestehende Twitch-Admin-Sessions automatisch ins gemeinsame SSO übernommen

**Problem:** Bereits vor der SSO-Korrektur ausgestellte Twitch-Admin-Cookies waren im Python-Dashboard weiterhin gültig, im zentralen Auth-Dienst aber unbekannt. Dadurch blieb Twitch geöffnet, während das Discord-Admin-Dashboard trotz identischem Cookie erneut zur Anmeldung leitete.

**Änderung:** Beim Twitch-Admin-Login wird eine vorhandene lokale Admin-Session jetzt synchron in den zentralen Session-Store übernommen. Gleichzeitig wird derselbe Cookie-Wert erneut für die gemeinsame Domain ausgestellt.

**Ergebnis:** Bestehende funktionierende Twitch-Admin-Sessions schalten ohne erneuten Discord-Login auch das Discord-Admin-Dashboard frei. Neue Sessions verwenden weiterhin direkt den zentralen 14-Tage-Store.

## #231 — Twitch-Admin an die gemeinsame langlebige Admin-Session gebunden

**Problem:** Das Twitch-Admin-Dashboard verwendete denselben Cookie-Namen wie das zentrale Discord-Admin-Dashboard, erzeugte die Session aber zunächst nur in seinem eigenen Store und kopierte sie anschließend unverbindlich im Hintergrund. Bei Neustarts oder einem fehlgeschlagenen Kopiervorgang zeigte derselbe Cookie deshalb je nach Dashboard auf eine unbekannte Session und der Login begann erneut.

**Änderung:** Das Twitch-Admin-Dashboard registriert eine neue Session jetzt synchron beim zentralen Auth-Dienst, bevor es das gemeinsame Cookie setzt. Das Cookie gilt für die gesamte Community-Domain. Existiert bereits eine zentrale Session, wird sie beim Twitch-Login direkt wiederverwendet statt einen zweiten Discord-OAuth-Lauf zu starten.

**Ergebnis:** Ein Login entsperrt beide Admin-Dashboards für 14 Tage. Die Session bleibt nach Neustarts gültig; kann der zentrale Store eine neue Session nicht speichern, wird kein halbgültiges Cookie mehr ausgegeben.

## #230 — Channel Intelligence: Freemium-Zugang für Partner entsperrt

**Problem:** Der Übersicht-Endpoint (`/twitch/api/v2/overview`) warf für alle Partner-Sessions 401 „unauthorized" zurück, obwohl die Freemium-Paywall-Logik im Code bereits vollständig implementiert war. Ursache: Der Handler verwendete `AuthLevel` (versteht nur den internen `X-Internal-Token`) statt `DashboardAuthLevel` (versteht Partner-Cookie, Admin-Cookie, Localhost). Partner kamen nie bis zur Plan-Prüfung — sie wurden am Eingang abgeblockt.

**Änderung:** Handler-Auth auf `DashboardAuthLevel` umgestellt. Gate von `is_privileged()` auf `is_authenticated()` gesenkt — Partner dürfen jetzt rein. Partner-Security-Fence eingebaut: Partner sehen nur ihre eigenen Daten (Login wird aus der Session erzwungen, nicht aus dem Query-Param gelesen). Admins/Localhost bleiben uneingeschränkt. Tests auf Loopback-Auth umgestellt.

**Ergebnis:** Free-Partner sehen die Übersicht mit dem letzten Stream als Zeitfenster (keine Trends, keine Zeitraumauswahl). Paid-Partner (`analytics.basic`/`analytics.extended`) bekommen das volle Rolling-Window mit Trends — Freemium-Paywall greift wie geplant.

## #229 — Raid-OAuth: Partner-Promotion jetzt zuverlässig + korrekte Block-Guards

**Problem (1) — Stille Promotion-Blockade:** `sync_partner_state_after_auth` führte Partner-Promotion und Stats-Backfill in einer einzigen Postgres-Transaktion durch. Schlug der Backfill fehl (z. B. Constraint-Konflikt oder Tabellensperre), rollte die gesamte Transaktion zurück — die neu angelegte `twitch_partners`-Zeile verschwand lautlos, ohne dass der Streamer davon erfuhr. Erkennbares Symptom: `twitch_raid_auth`-Eintrag vorhanden (Token gespeichert), aber kein `twitch_partners`-Eintrag.

**Änderung (1):** Promotion läuft jetzt in einer eigenen Transaktion (commit bevor Backfill startet). Der Backfill nutzt einen best-effort-Wrapper mit eigener Mini-Transaktion — Fehler werden geloggt, rollen die Promotion aber nie zurück.

**Problem (2) — Guard-Lücke bei ausgeschiedenen/gesperrten Partnern:** Der bestehende Hard-Pause-Guard prüfte nur die *aktive* Partner-Zeile auf `technical_pause_reason ∈ {blocked, bot_banned}`. Hatte ein Partner keine aktive Zeile mehr (departnered, archiviert), griff der Guard nicht — ein Re-OAuth reaktivierte ihn unbeabsichtigt.

**Änderung (2):** Vor dem aktiven-Zeile-Guard prüft `promote_streamer_to_partner` jetzt zusätzlich alle nicht-aktiven Zeilen. Block-Bedingungen: `technical_pause_reason ∈ {blocked, bot_banned}` **oder** `admin_archived_at IS NOT NULL` (ausgeschieden). Trifft eine zu, wird die Promotion mit `reactivated=false` und dem passenden Grund abgebrochen — kein Schreibzugriff.

**Ergebnis:** Erstmalige Raid-OAuth-Autorisierungen erzeugen zuverlässig einen `twitch_partners`-Eintrag. Banned und ausgeschiedene Partner werden via Re-OAuth nicht reaktiviert.

## #228 — Admin-Ansicht zeigte "Kein Partner auswählbar" trotz aktiver Partner

**Problem:** Der Endpoint `/twitch/api/v2/streamers`, der dem Dashboard die Liste aktiver Partner für die Admin-Partnerauswahl liefert, prüfte die Berechtigung nur anhand des internen Service-Tokens (`X-Internal-Token`). Browser schicken diesen Header nicht — die Anfrage wurde mit 401 abgelehnt, das Frontend erhielt eine leere Liste und zeigte den Fehler-Bildschirm.

**Änderung:** Der Handler nutzt jetzt `DashboardAuthLevel` statt `AuthLevel`. Damit erkennt er sowohl Localhost-Zugriffe als auch Twitch-Admin-Sessions (Cookie-basiert), genauso wie alle anderen v2-Endpunkte. Die Tests wurden entsprechend auf Localhost-Simulation umgestellt.

**Ergebnis:** Admins, die sich per Twitch-OAuth einloggen, können die Partnerliste laden und zwischen Partnern wechseln. Der `earlysalty`-Account wird korrekt zu `DashboardAuthLevel::Admin` promoted und hat Zugriff.

## #227 — Geteilten Twitch-Callback für Dashboard und Raid wiederhergestellt

**Problem:** Nach der Umstellung des Streamer-Dashboards nutzte der Login eine Rücksprung-Adresse, die bei Twitch nicht registriert ist. Twitch brach die Anmeldung deshalb vor dem eigentlichen Login ab. Gleichzeitig darf die registrierte Adresse nicht einfach exklusiv dem Dashboard gehören, weil auch die Raid-Autorisierung denselben öffentlichen Rücksprung und dieselbe Twitch-Anwendung nutzt.

**Änderung:** Der Dashboard-Login verwendet wieder die registrierte Rücksprung-Adresse. Der gemeinsame Rücksprung wird jetzt im neuen Dashboard angenommen und anhand des gespeicherten OAuth-States entschieden: Gehört der State zum Dashboard, läuft der normale Dashboard-Login; gehört er zur Raid-Autorisierung, wird die bestehende interne Raid-Verarbeitung aufgerufen.

**Ergebnis:** Der Streamer-Dashboard-Login passt wieder zur Twitch-Konfiguration, ohne die Raid-Autorisierung umzuhängen oder die Anbieter-Konsole zu ändern. Die alte Dashboard-Rücksprung-Adresse bleibt als Kompatibilitätspfad erhalten, während die gemeinsame Adresse künftig über den neuen Dashboard-Dienst laufen kann.

## #226 — Streamer-Dashboard-Login nach Cutover repariert (OAuth war stumm deaktiviert)

**Ausgangslage:** Seit der Umstellung des Streamer-Dashboards auf die neue Plattform brach der Twitch-Login mit der Meldung „Twitch OAuth ist aktuell nicht konfiguriert" ab — niemand kam mehr ins Dashboard. Ursache: Der Login braucht drei Angaben (Client-ID, Client-Secret und die öffentliche Rücksprung-Adresse). Die ersten beiden lagen wie gewohnt im Secret-Speicher, die Rücksprung-Adresse wurde in der alten Version aber aus einem eingebauten Standardwert abgeleitet. Die neue Version verlangt sie explizit und schaltet den Login bewusst komplett ab, sobald eine der drei Angaben fehlt — statt mit halber Konfiguration zu raten. Beim Umzug ist diese eine Adresse nirgends mehr gesetzt worden.

**Was geändert wurde:** Die öffentliche Rücksprung-Adresse des Logins wird beim Dienststart wieder fest gesetzt (kein Geheimnis, eine reine Web-Adresse). Sie zeigt auf die kanonische Domain und entspricht exakt der bei Twitch hinterlegten Adresse, damit Twitch den Rücksprung akzeptiert. Liegt der Wert später einmal im zentralen Secret-Speicher, hat dieser Vorrang.

**Wie es jetzt läuft:** Der native Twitch-Login ist beim Start wieder aktiv; das Dashboard leitet Streamer korrekt zur Twitch-Anmeldung und nach erfolgreicher Freigabe zurück ins Dashboard. Fehlt die Adresse erneut, bleibt das fail-closed-Verhalten erhalten — lieber sauber deaktiviert als mit kaputter Anmeldung.

## #225 — Schema-Cleanup: twitch_streamers auf reine Identitätstabelle reduziert + is_partner BOOLEAN-Fix

**Ausgangslage:** `twitch_streamers` enthielt seit der SQLite-Migration mehrere Felder die dort konzeptionell nicht hingehörten: `is_monitored_only` als Flag für "in Streamers aber kein Partner", `discord_user_id` obwohl das bereits vollständig in `twitch_streamer_identities` lag, und `archived_at` als Dashboard-Flag für Partner. Das führte immer wieder zu der falschen Annahme, `twitch_streamers ≈ twitch_partners` — zuletzt konkret in den Stats-Logging-Queries, die `is_partner` als `INTEGER` (0/1) statt als `BOOLEAN` behandelten und damit 6 Fehler/Minute in Prod produzierten. Dazu fehlte eine saubere Abbildung für Opt-out-Streamer und hard-gebannte Kanäle.

**Was geändert wurde:** `twitch_streamers` ist jetzt eine reine Identitätstabelle mit genau drei Feldern: `twitch_login`, `twitch_user_id`, `created_at`. Alle abgeleiteten Informationen kommen aus den richtigen Tabellen. Neu dazu kommt `twitch_exclusions` für Opt-out- und Ban-Fälle. Alle Stats-Queries, Analytics und der gesamte Monitoring-Code wurden auf die neue Schemastruktur umgestellt. Der `is_partner`-BOOLEAN-Fehler in Stats, Market-Queries und Test-Fixtures ist behoben.

**Wie es jetzt läuft:** Ob ein Streamer "nur Monitored" ist ergibt sich nicht mehr aus einem Flag sondern aus der Abwesenheit eines Eintrags in `twitch_partners` — das ist strukturell korrekt und kann nicht mehr aus dem Takt geraten. Opt-out (`fr4gm1nt`, `snaqeu`) und Ban (`skifahrertv`) sind in `twitch_exclusions` migriert: Opt-out ist reversibel (Reaktivierung setzt `reactivated_at`), Ban ist permanent. Die Discord-ID kommt in allen Pfaden ausschließlich aus `twitch_streamer_identities` und bleibt dort auch erhalten wenn ein Partner opt-out geht.

## #224 — Migrationen 20260616–20260617 nachgeführt (Baseline-Konflikt behoben)

**Ausgangslage:** Die Baseline-Migration enthielt ein `ALTER TABLE` auf einer komprimierten TimescaleDB-Hypertable, das auf Prod mit SQLSTATE 0A000 fehlschlug und alle nachfolgenden 7 Migrationen blockierte — sie blieben im Tracking unregistriert und wurden nicht angewendet.

**Was geändert wurde:** Baseline und Observability-Migration wurden manuell in `_sqlx_migrations` mit korrektem SHA-384-Checksum eingetragen. Alle 7 ausstehenden Migrationen (20260616–20260617) liefen danach sauber durch.

**Wie es jetzt läuft:** Prod hat 10 Migrationen registriert, keine Ausstehenden.

## #223 — Bot-Ban im Kanal: automatische Pause, Anleitung per DM, Selbstheilung

**Ausgangslage:** Wird der Bot in einem Partner-Kanal gebannt oder als Moderator entfernt, kann er dort nichts mehr tun — Auto-Raid, Chat-Schutz und Analytics laufen ins Leere. In der alten Python-Version erkannte der Bot diesen Fall, schaltete den Kanal sauber auf Pause, schickte dem Streamer eine DM mit konkreter Anleitung zur Behebung und hob die Pause automatisch wieder auf, sobald der Bot zurück war. Beim Umbau auf das neue System (Rust-Cutover) fehlte diese komplette Reaktion: Ein Kanal-Ban blieb unbemerkt, der Streamer bekam keinen Hinweis, und selbst nach einer Entsperrung blieb die Pause hängen.

**Was geändert wurde:** Der Bot erkennt einen Kanal-seitigen Ban jetzt wieder selbst und reagiert in drei Schritten — er pausiert die Bot-Funktionen für genau diesen Kanal, schickt dem Streamer (sofern ein Discord-Konto verknüpft ist) eine DM mit den zwei Befehlen, die das Problem beheben, und hebt die Pause von allein wieder auf, sobald der Kanal wieder gesund ist. Kein manuelles Eingreifen mehr nötig.

**Wie es funktioniert:** Meldet Twitch beim Senden einer Bot-Nachricht „Absender ist gebannt" (oder einen passenden Ban-Fehler), löst der Bot die Bann-Reaktion aus: Der Kanal wird auf der Raid-Sperrliste vermerkt, Raid wird deaktiviert und der Partner-Eintrag auf den technischen Grund „bot_banned" gesetzt. Damit das pro Vorfall nur einmal passiert, prüft der Bot vorab, ob der Kanal bereits so markiert ist (Doppel-Reaktionen und Doppel-DMs werden so vermieden). Die Wiederherstellungs-DM läuft — wie alle Discord-Nachrichten des Twitch-Bots — über den zentralen Discord-Bot, da der Twitch-Bot selbst keinen Discord-Zugang hat; ist der nicht erreichbar, bleibt die DM aus, der Rest der Reaktion greift trotzdem. Ein stündlicher Durchlauf prüft alle so pausierten Kanäle: Ist der Bot dort wieder verbunden (Re-Auth gültig), wird die „bot_banned"-Pause automatisch aufgehoben — dauerhafte Sperren (manuell „blockiert") bleiben davon unberührt.

## #222 — Live-Ping-Rolle wird beim Go-Live automatisch angelegt

**Ausgangslage:** Hat ein Partner den Live-Ping aktiviert, aber noch keine Ping-Rolle hinterlegt, fiel der Rollen-Ping bisher still aus — seit dem letzten Stand wurde der Ausfall immerhin protokolliert (#219), die Rolle musste aber von Hand im Dashboard nachgetragen werden. In der alten Python-Version legte der Bot diese Rolle beim ersten Go-Live noch selbst an; dieser Schritt fehlte im neuen System.

**Was geändert wurde:** Geht ein Partner mit aktivem Live-Ping, aber ohne hinterlegte Rolle live, legt der Bot die Discord-Ping-Rolle jetzt automatisch an, merkt sie sich und pingt damit sofort beim selben Stream-Start. Kein manuelles Nachtragen mehr.

**Wie es funktioniert:** Der Twitch-Bot hat selbst keinen Discord-Zugang — er beauftragt den zentralen Discord-Bot, die Rolle anzulegen (erwähnbar, benannt nach dem Streamer). Vorhandene Rollen gleichen Namens werden wiederverwendet statt doppelt erzeugt. Die zurückgelieferte Rollen-ID wird dauerhaft beim Partner gespeichert, sodass die Anlage nur einmal passiert und jeder weitere Stream die gespeicherte Rolle nutzt. Frisch angelegt fließt die Rolle direkt in die Ping-Berechtigung und in den Ankündigungstext. Scheitert die Anlage ausnahmsweise (z. B. fehlende Rechte), bleibt es beim sichtbaren Protokoll-Hinweis aus #219 statt einem kaputten Ping — der Stream-Start selbst läuft unverändert weiter.

## #221 — Doppelte „ist live"-Pings gestoppt

**Ausgangslage:** Ging ein Partner live, kam die „ist LIVE in Deadlock!"-Benachrichtigung im Discord teils doppelt an — derselbe Stream, dieselbe Startzeit, nur ein, zwei Zuschauer Unterschied zwischen den Posts. Der Bot hat keinen eigenen Discord-Zugang, sondern lässt jede Nachricht über den zentralen Verbund-Dienst posten und bekommt als Bestätigung die ID der erstellten Nachricht zurück. Diese ID merkt er sich als „für diesen Stream schon gepingt". Der Verbund-Dienst wurde frisch von Python auf die neue Version umgestellt und liefert die Nachrichten-ID seither als Zahl statt als Text. Der Bot akzeptierte strikt nur Text, verwarf dadurch die gesamte Antwort als „fehlgeschlagen" — obwohl die Nachricht in Wahrheit rausging — und speicherte folglich nie eine ID. Beim nächsten Poll-Durchlauf (alle paar Sekunden) galt der Stream weiter als „noch nicht gepingt" und wurde erneut gepostet, bis eine Schutzschicht des Verbund-Dienstes weitere identische Posts abfing.

**Was geändert wurde:** Der Bot liest die zurückgelieferte Nachrichten-ID jetzt in beiden Formen — egal ob Text oder Zahl. Parallel ist der Verbund-Dienst selbst wieder auf das ursprüngliche Text-Format zurückgesetzt, sodass beide Seiten ohnehin zusammenpassen.

**Wie es funktioniert:** Nach einem erfolgreichen Post wird die ID wieder korrekt ausgelesen und als „dieser Stream ist erledigt" gespeichert. Der nächste Poll sieht die gespeicherte ID, erkennt den Stream als bereits angekündigt und postet nicht erneut. Geht der Stream offline, wird der Eintrag aufgelöst, sodass der nächste echte Stream wieder genau einen Ping bekommt. Abgesichert ist das zusätzlich durch einen Test, der genau die fehlerhafte Antwortform (ID als Zahl) durchspielt.

## #220 — Dashboard-Startseite läuft wieder (nativ statt über Python)

**Ausgangslage:** Die interne Dashboard-Startseite (mit Status-Übersicht, letzten Streams, Bot-Aktivität und der „Was gibt's Neues"-Liste) wurde noch von einem alten Python-Dienst geliefert, während das übrige Dashboard schon auf dem neuen System läuft. Beim Abschalten des Python-Dienstes im Zuge der Umstellung fiel diese eine Seite aus — sie lieferte nur noch einen Fehler (502), weil das neue System sie mangels eigener Umsetzung bislang nur an das nun tote Python weiterreichte. Damit war auch das Einspielen neuer Changelog-Einträge blockiert.

**Was geändert wurde:** Die komplette Startseite samt Changelog-Verwaltung ist jetzt direkt im neuen System umgesetzt — kein Umweg über den alten Python-Dienst mehr.

**Wie es funktioniert:** Die Seite baut ihre Antwort jetzt selbst aus den vorhandenen Daten zusammen: Profil/Status, Kennzahlen und letzte Streams aus den Session-Daten, OAuth-/Partner-Status, Live-Status, sowie eine Aktivitätsliste aus Ban-/Raid-Verlauf und den lokalen Auto-Ban-/Service-Warn-Logs (zusammengeführt, nach Zeit sortiert, gekappt). Jeder Datenblock ist einzeln fehlertolerant — fällt eine Quelle aus, bleibt der Block leer statt die ganze Seite zu kippen. Die Rechte greifen wie zuvor: nur Admin/lokaler Zugriff sehen interne IDs und dürfen Changelog-Einträge anlegen; ein normaler Streamer sieht nur seinen eigenen Kanal. Weil die Seite jetzt nativ registriert ist, übernimmt sie automatisch — der bisherige Weiterleitungs-Umweg greift nur noch für noch nicht umgestellte Pfade.

## #219 — Sammelfix: weitere Regressionen aus dem Rust-Umbau

**Ausgangslage:** Beim Umbau der Bot-Logik von Python auf Rust wurde mehrfach die eigentliche Funktion korrekt übernommen, aber eine vorgelagerte Schutzbedingung ging verloren — dieselbe Fehlerklasse wie beim Re-Auth-Hinweis (#218). Ein Stück-für-Stück-Abgleich gegen die alte Python-Version hat sieben solcher Fälle aufgedeckt und bestätigt. Behoben:

- **Raid-Schutz auf gesperrte Ziele war ausgefallen:** Der Bot soll einen manuell gestarteten Raid abbrechen, wenn das Ziel auf der Sperrliste steht. Dafür muss er die Moderations-Ereignisse des Kanals mithören — genau dieses Abonnement legte die Rust-Version nie an, also kamen nie Ereignisse an und der Abbruch lief ins Leere. Jetzt abonniert der Bot die Moderations-Ereignisse wieder pro Partner-Kanal (mit seinem Bot-Account als Moderator), und der Abbruch greift wieder.
- **Dankesnachricht bei eingehenden Raids fehlte oft:** Die Begrüßung bei einem eingehenden Raid hängt daran, dass der Bot Raid-Ereignisse für den Ziel-Kanal abonniert hat. Rust tat das nur kurz rund um einen eigenen ausgehenden Raid; kam ein Raid „von außen" ohne vorherigen eigenen Raid, war kein Abo aktiv und die Nachricht blieb aus. Jetzt abonniert der Bot Raid-Ziele dauerhaft pro Partner.
- **Werbefrei-Hinweis nach Stream-Start:** Der einmalige Hinweis nach einem Bot-Timeout ging direkt raus — ohne die Stummschaltung zu prüfen und ohne den Werbe-Cooldown zu belegen. Folge: Er konnte in einem stummgeschalteten Kanal erscheinen, und unmittelbar danach durfte eine reguläre Werbung folgen (doppelte Werbung). Jetzt läuft er über denselben Weg wie normale Werbung: stumm = kein Hinweis, und der Cooldown ist danach belegt.
- **Lurker-Steuer auf falschen Plänen:** Die Prüfung, ob ein Plan die namentliche „Lurker-Steuer" erlaubt, war als Ausschlussliste gebaut — alles Unbekannte galt als erlaubt. Ein Plan-Name außerhalb der Liste konnte so fälschlich eine öffentliche namentliche Erwähnung auslösen. Jetzt eine echte Positivliste der berechtigten Pläne; unbekannte Pläne lösen nichts mehr aus.
- **!invite nur noch für Partner:** Der Befehl konnte über einen Sonderpfad für erlaubte Fremd-Bots auch auf reinen Beobachtungs-Kanälen eine Antwort auslösen. Jetzt prüft er zuerst den Partner-Status.
- **Partner-Autorisierung läuft nicht mehr fälschlich sofort ab:** Lieferte Twitch beim Token-Tausch ausnahmsweise keine Gültigkeitsdauer mit, setzte Rust sie auf „jetzt abgelaufen". Jetzt gilt — wie früher — ein Standardwert, plus eine Untergrenze gegen Null-Werte.
- **Fehlende Live-Ping-Rolle wird sichtbar:** Hat ein Partner den Live-Ping aktiviert, aber keine Ping-Rolle hinterlegt, fiel der Ping bisher still aus. Das wird jetzt protokolliert, damit die Rolle nachgetragen werden kann. Die automatische Anlage der Rolle wie früher folgt als eigener Schritt.

## #218 — Falsche Re-Autorisierungs-Erinnerung an fremde Kanäle gestoppt

**Ausgangslage:** Der Bot beobachtet nicht nur seine echten Partner, sondern auch fremde Twitch-Kanäle rein zur Statistik- und Raid-Ziel-Erkennung — die findet ein automatischer „Scout" und hängt sie an dieselbe Live-Erkennung wie Partner. Geht ein Kanal live, postet der Bot bei Partnern mit abgelaufenem Zugang eine Chat-Erinnerung („Für den Raid-/Stats-Bot fehlt die neue Twitch-Autorisierung, bitte neu verbinden"). Diese Erinnerung ging fälschlich auch an die nur beobachteten Fremd-Kanäle raus — an Streamer, die nie etwas mit dem Bot zu tun hatten und sich folglich nie autorisiert hatten. In deren Chat sah es so aus, als fordere der Bot grundlos auf, etwas „neu" zu verbinden, das nie verbunden war.

**Was geändert wurde:** Die Erinnerung geht jetzt nur noch an Kanäle, die tatsächlich schon einmal autorisiert waren und deren Zugang erneuert werden muss. Rein beobachtete Fremd-Kanäle bekommen sie nicht mehr.

**Wie es funktioniert:** Es ist eine *Neu*-Autorisierungs-Erinnerung — sinnvoll nur für jemanden, der schon einmal Zugriff erteilt hat. Bisher prüfte der Bot beim Live-Gehen nur „ist der Zugang voll gültig?", und ein komplett fehlender Autorisierungs-Eintrag zählte dabei als „nicht gültig". Damit warf er zwei völlig verschiedene Fälle in einen Topf: „Token abgelaufen, bitte erneuern" und „noch nie verbunden" — beide lösten die Nachricht aus. Jetzt kommt ein Schritt davor: Gibt es überhaupt keinen Autorisierungs-Eintrag, ist nichts zu erneuern → keine Nachricht. Auslöser (Live-Gehen) und die Sperre von einer Erinnerung pro Stream-Start bleiben unverändert; verschärft wurde nur, wer sie überhaupt bekommt. Diese Schutzbedingung war in der Vorgänger-Version durch den Aufbau der internen Aufrufe automatisch gegeben und ging beim Umbau auf das neue System verloren.

## #217 — Warnung vor fremden Discord-Servern entfernt

**Ausgangslage:** Der Bot hatte zwei feste Warn-Ansagen, die im Chat vor zwei fremden, nicht zu uns gehörenden Discord-Servern warnten („das könnte Fake/Scam sein, unser einziger offizieller Discord ist …"). Diese Warnungen liefen im selben Takt wie die normalen Discord-Promos: Nach Stream-Start gab es eine Anlaufzeit, danach wurde die Warnung im fälligen Promo-Slot bevorzugt vor einer normalen Promo gepostet und danach für rund zwei Stunden gesperrt, bevor sie erneut kommen durfte.

**Was geändert wurde:** Beide Warntexte und die komplette Logik dahinter sind raus — der Bot postet diese Server-Warnungen nicht mehr.

**Wie es funktioniert:** Im periodischen Promo-Slot belegte die Warnung bisher den ersten Platz: War sie fällig, ging sie raus und die normale Promo wurde für diesen Durchlauf übersprungen. Dieser Vorrang-Schritt fällt jetzt weg, dadurch greifen in einem fälligen Slot direkt wieder die regulären Discord-Promos. Der zugehörige eigene Sperr-/Anlauf-Timer und sein Speichern in der Datenbank entfallen ebenfalls; alte gespeicherte Warn-Sperren werden beim Start einfach ignoriert. Die übrige Promo- und Moderationslogik (inkl. der davon unabhängigen Scam-Pitch-Erkennung gegen Account-Übernahmen) bleibt unverändert.

## #216 — Stille Ban-/Raid-Hinweise jetzt auch im Dashboard schaltbar

**Ausgangslage:** Ob der Bot eine Chat-Notiz postet, wenn er jemanden automatisch bannt oder einen Raid auslöst, ließ sich bisher nur per Chat-Befehl (`!silentban` / `!silentraid`) umschalten. Wer das lieber in Ruhe im Dashboard einstellen wollte, hatte dort keine Möglichkeit.

**Was geändert wurde:** Im Streamer-Dashboard unter „Verwaltung" gibt es jetzt zwei Schalter — „Auto-Ban-Hinweise stummschalten" und „Raid-Hinweise stummschalten". Sie sind eins zu eins mit den Chat-Befehlen verbunden: Was du im Dashboard umlegst, gilt sofort auch für den Chat-Befehl und umgekehrt — es ist dieselbe Einstellung.

**Wie es funktioniert:** Die Schalter lesen und schreiben genau denselben Schaltzustand am Partner-Datensatz, den auch `!silentban`/`!silentraid` umschalten — daher gibt es keine zweite, abweichende Einstellung, sondern eine einzige Quelle. Beim Öffnen der Seite wird der aktuelle Stand geladen; ein Umlegen speichert sofort (und rollt bei einem Fehler sichtbar zurück). Die Einstellung gilt immer für deinen eigenen Kanal — erkannt über deine Dashboard-Anmeldung, ohne dass du etwas eingeben musst.

## #215 — Interne Diagnose-Schnittstelle für den Support-Bot

**Ausgangslage:** Meldet ein Streamer im Discord-Support ein Problem mit der Twitch-Anbindung („der Bot kommt nicht in meinen Stream", „ich habe autorisiert, aber es steht auf inaktiv"), konnte der Support-Bot bisher nur allgemeine Hinweise geben — er kannte den echten Autorisierungs-Status des Fragenden nicht.

**Was wurde geändert:** Es gibt eine neue, rein lesende interne Schnittstelle, die zu einer Discord-ID den Twitch-Status des betreffenden Streamers liefert: ist der Account verbunden, fehlen Berechtigungen, ist eine Neu-Autorisierung nötig, ist er aktiv. Sie gibt keine Tokens oder Geheimnisse aus, ändert nichts und ist nur über den abgesicherten internen Zugang (Loopback + interner Token) erreichbar.

**Wie es funktioniert:** Die Schnittstelle schlägt die Discord-ID auf den verknüpften Twitch-Account nach und stellt den Status aus den vorhandenen Partner-/OAuth-Daten zusammen — dieselbe Logik, die auch die Verwaltungsseite nutzt (vorhandene/fehlende Scopes, Neu-Autorisierung, Partner-/Live-Status). Der Discord-Support-Bot kann damit einem Streamer konkret sagen, was bei seiner Anbindung klemmt, statt zu raten.

## #214 — Texte: echte Umlaute statt Ersatzschreibung

**Ausgangslage:** In einigen nutzersichtbaren Texten standen Umlaut-Ersatzschreibungen (ae/oe/ue/ss) statt echtem ä/ö/ü/ß — entstanden, weil Texte mal mit, mal ohne echte Umlaute getippt wurden. Betroffen waren u. a. das Affiliate-Portal (Login-Hinweise, Formularfelder, Steuer- und Auszahlungstexte), die Dashboard-Vorschau auf der Startseite und die Twitch-Ansage, die bei einem Raid im Chat erscheint („Auf Twitch ansehen fuer mehr Action!").

**Was geändert wurde:** Alle betroffenen Anzeigetexte nutzen jetzt echte Umlaute. Dasselbe gilt für die internen Admin-Bereiche (Dialoge, Hinweise, Schaltflächen-Beschriftungen).

**Wie es funktioniert:** Reine Schreibweisen-Korrektur ohne Logik- oder Ablaufänderung. Die Umlaute wurden gezielt pro Wort im jeweiligen Anzeigetext ersetzt, damit englische Begriffe, technische Bezeichner und Befehlsnamen unangetastet bleiben. Die Korrekturen auf der Website werden mit dem nächsten Frontend-Build sichtbar, die Raid-Ansage mit dem nächsten Neustart des Bots.

**Betroffen:** Affiliates im Partner-Portal, Besucher der Startseite und Zuschauer, die eine Raid-Ansage im Chat sehen.

## #213 — Mod-Befehle antworten wieder, wenn jemand ohne Rechte sie tippt

**Ausgangslage:** Mehrere Chat-Befehle sind nur für Broadcaster und Mods gedacht — `!raid`, `!raid_enable`, `!uban`, `!silentban`, `!silentraid`. Tippte sie jemand ohne Berechtigung, passierte beim Umzug auf das neue System einfach gar nichts: keine Aktion, aber auch keine Rückmeldung. Für den Nutzer wirkte das, als wäre der Bot kaputt oder der Befehl nicht vorhanden.

**Was geändert wurde:** Tippt jemand ohne Mod-/Broadcaster-Rechte einen dieser Befehle, antwortet der Bot jetzt wieder mit einem klaren Hinweis („Nur der Broadcaster oder Mods können …") — wie in der Vorgänger-Version.

**Wie es funktioniert:** Bei jedem dieser Befehle prüft der Bot zuerst die Rechte des Absenders (Mod- oder Broadcaster-Abzeichen). Fehlen sie, wird die Aktion nicht ausgeführt und stattdessen eine kurze Ablehnung in den Chat geschrieben, adressiert an die Person, die den Befehl getippt hat. Die eigentliche Funktion bleibt unverändert geschützt.

## #212 — Discord-Profil setzen vergibt wieder die Streamer-Rolle (Admin-Aktion)

**Ausgangslage (Admin-Aktion):** Wenn im Admin-Bereich das Discord-Profil eines Streamers gesetzt wird (Discord-ID + Anzeigename), passierten in der Vorgänger-Version zwei Dinge mehr, die beim Umzug verloren gingen: die Twitch-User-ID des Streamers wurde aufgelöst und mitgespeichert, und der Streamer bekam automatisch die Discord-Streamer-Rolle. Der neue Handler schrieb nur die Discord-Felder in die Datenbank — keine Rollen-Vergabe, keine ID-Auflösung.

**Was geändert wurde:** Beim Setzen des Discord-Profils löst der Bot die Twitch-User-ID jetzt wieder auf und trägt sie nach (falls noch keine hinterlegt war), und er vergibt dem Streamer automatisch die Discord-Streamer-Rolle.

**Wie es funktioniert:** Die User-ID wird wie früher in zwei Stufen ermittelt — zuerst aus dem vorhandenen Autorisierungs-Datensatz des Streamers, und falls dort nichts steht, über eine Twitch-API-Abfrage anhand des Logins. Diese ID wird auf der Streamer-Zeile ergänzt, sofern dort noch keine stand (bestehende IDs bleiben unangetastet). Ist eine Discord-ID angegeben, weist der Bot anschließend über den Discord-Broker die Streamer-Rolle zu — als Nebeneffekt, der den eigentlichen Speichervorgang nie blockiert (schlägt die Rollen-Vergabe fehl, wird das nur protokolliert). Ohne erreichbaren Broker bleibt der Speichervorgang trotzdem erfolgreich.

## #211 — Admin-Statistik zeigt wieder die Live-EventSub-Auslastung

**Ausgangslage (rein intern/Admin):** Die `/stats`-Übersicht im Admin-Bereich hat eine EventSub-Sektion (wie viele Twitch-Abos der Bot aktuell hält, aufgeschlüsselt nach Typ und Kanal). Beim Umzug war diese Live-Sektion nicht mehr an die neue Abo-Verwaltung angeschlossen — sie blieb leer, nur die historischen Kapazitäts-Werte aus der Datenbank waren da.

**Was geändert wurde:** Die EventSub-Sektion wird wieder live aus der aktiven Abo-Verwaltung gefüllt: aktuelle Abo-Anzahl, belegte Slots, plus Aufschlüsselung nach Abo-Typ und nach Kanal (mit Login).

**Wie es funktioniert:** Der Bot betreibt die Twitch-Abos im Webhook-Modus (keine WebSocket-Listener — die entsprechenden Felder bleiben daher bewusst 0). Für die Live-Sektion zählt er das aktuell getrackte Abo-Set aus, gruppiert es nach Typ und nach Broadcaster, löst die Kanal-Logins per Sammel-Abfrage auf und liefert das in genau der Form, die das Admin-Dashboard erwartet. Ohne aktive Abo-Verwaltung (z. B. ohne Twitch-Anbindung) bleibt es beim bisherigen DB-Block.

## #210 — Kostenlose „Tagesform": dein letzter Stream gratis, Verlauf & Coaching im Plan

**Ausgangslage:** Bisher sah jeder Streamer das komplette Analyse-Dashboard mit voller Historie kostenlos — der eigentliche Mehrwert (Entwicklung über Zeit, Coaching) war damit gratis, und es gab kaum einen Grund, einen Plan zu buchen. Gleichzeitig ist eine einzelne Tageszahl ohne Vergleich wenig wert: „heute 23 Ø Zuschauer" sagt nichts, solange man nicht weiß, ob das über oder unter dem eigenen Schnitt liegt.

**Was geändert wurde:**

- **Kostenlos** gibt es jetzt die **„Tagesform"**: die ehrlichen Kennzahlen deines letzten Streams (Ø-/Peak-Zuschauer, neue Follower, Retention, Watchtime, Chatter) — ein sauberer Schnappschuss nach jedem Stream.
- **Im Plan** (mit 30 Tagen Gratis-Test) liegt die **Entwicklung über Zeit**: Wachstumskurve über alle Streams, Stammzuschauer-Verlauf, Retention-Trend, Wochen-/Monatsvergleich — plus Coaching und Post-Stream-Bericht.
- Kein Abo-Zwang, jederzeit kündbar, und dein Verlauf wird ab Tag 1 mitgezeichnet — beim Freischalten ist deine komplette Historie sofort da.

**Wie es funktioniert:** Die Analyse-Endpunkte erkennen serverseitig, ob ein Plan den vollen Verlauf freischaltet. Ohne Plan wird das Zeitfenster auf den letzten Stream begrenzt — statt alles zu sperren —, und das Dashboard zeigt die Tagesform genau dieses einen Streams plus eine Vorschau auf den gesperrten Verlauf-Bereich. Die Verlauf- und Coaching-Auswertungen bleiben dem Plan vorbehalten. Bezahlmodell und Preise sind unverändert; es verschiebt sich nur, was kostenlos sichtbar ist.

## #209 — Chat ab Stream-Start wird vollständig erfasst (Go-Live-Lücke geschlossen)

**Ausgangslage:** Wenn ein Streamer live ging, begann die Chat- und Zuschauer-Erfassung erst, sobald der reguläre Status-Abruf den Kanal das nächste Mal einsammelte — bis zu rund 15 Sekunden später, plus ein kurzes Nachwirken eines Zwischenspeichers. Genau in diesem Fenster direkt nach Stream-Start wurden Chat-Nachrichten still verworfen, weil noch keine „offene Session" existierte, der sie zugeordnet werden konnten. Folge: Die allerersten Chatter eines Streams fehlten in den Zahlen — die Community wirkte etwas kleiner oder später aktiv, als sie tatsächlich war.

**Was geändert wurde:**

- Die Stream-Session wird jetzt **sofort beim Go-Live-Signal** eröffnet, nicht erst beim nächsten Status-Tick. Damit hat jede Nachricht ab der ersten Sekunde eine Session, der sie zugeordnet wird.
- Eine frisch eröffnete Session ohne Messwerte zeigt im Dashboard jetzt „noch keine Daten" statt einer irreführenden 0 (z. B. 0 % Retention), bis die ersten Messpunkte vorliegen.

**Wie es funktioniert:** Das Go-Live-Ereignis löst — neben dem schon bestehenden Setzen des Live-Status — direkt das Anlegen der Session aus. Eine Doppel-Anlage gegen den parallel laufenden Status-Abruf ist zweifach verhindert: ein Einmal-pro-Ereignis-Riegel und eine Sperre pro Kanal, die bei bereits offener Session keine zweite anlegt, sondern die vorhandene weiterführt. Fehlende Felder (Titel, Spiel, Zuschauerzahl) trägt der erste reguläre Abruf nach. Im Dashboard werden leere Messwerte als „kein Wert" statt als 0 durchgereicht, sodass eine echte 0 von „noch nichts gemessen" unterscheidbar bleibt.

## #208 — Analyse: „Erste Nachricht je"-Markierung wird wieder gesetzt

**Ausgangslage:** Wenn ein Zuschauer zum allerersten Mal in einem Stream etwas schreibt, hält der Bot das doppelt fest: als eigenes Ereignis und als Markierung am Zuschauer der laufenden Session (Grundlage für „neue vs. wiederkehrende Chatter" in der Auswertung). Beim Umzug auf das neue System ging der zweite Teil verloren — das Ereignis wurde geschrieben, die Session-Markierung aber nicht. Still, ohne Fehler.

**Was geändert wurde:** Beide Schreibvorgänge laufen wieder gemeinsam und unteilbar (in einer Transaktion): der Ereignis-Eintrag plus das Flag „erste Nachricht je bestätigt" am passenden Session-Zuschauer.

**Wie es funktioniert:** Trifft das `user_first_message`-Event ein, schreibt der Bot den Eintrag und setzt in derselben Transaktion die Markierung für den Zuschauer der aktuell offenen Session des Streamers. Die Session wird dabei direkt in der Update-Abfrage ermittelt; gibt es gerade keine offene Session, bleibt es beim reinen Ereignis-Eintrag (kein Fehler, keine falsche Markierung).

## #207 — Multi-Stream / Shared Chat: Bot arbeitet jetzt im richtigen Kanal

**Ausgangslage:** Bei Twitch „Shared Chat" teilen sich mehrere Streamer einen gemeinsamen Chat (typisch bei Multi-Streams/Kollabs — eine Nachricht erscheint dann in allen beteiligten Kanälen). Twitch markiert jede solche Nachricht mit ihrem echten Herkunfts-Kanal. Beim Umzug auf das neue System wurde dieses Feld komplett ignoriert: Der Bot hielt jede Nachricht für eine aus dem Kanal, auf den er gerade lauscht (den Host), statt aus dem Kanal, in dem sie wirklich geschrieben wurde. In der Vorgänger-Version war das korrekt — die Funktion ist beim Port verloren gegangen.

**Was geändert wurde:** Der Bot liest jetzt den Quell-Kanal jeder Nachricht und behandelt sie konsequent in dessen Kontext — wie früher. Moderation, Chat-Statistik, Commands und Werbung laufen damit wieder im richtigen Kanal. Wer kein Shared Chat nutzt, merkt nichts: Für normale Nachrichten ändert sich nichts.

**Wie es funktioniert:** Sobald Twitch eine Nachricht als „stammt aus einem anderen Kanal der Session" kennzeichnet, normalisiert der Bot sie ganz am Anfang der Verarbeitung einmalig auf diesen Quell-Kanal — Kanal-ID, Kanal-Name und die Nachrichten-ID für Mod-Aktionen. Ab da arbeiten alle weiteren Schritte (Klassifizierung, Ban-Liste, Scam-/Spam-Prüfung, Zählung, Commands, Werbung) automatisch im richtigen Kanal, ohne dass jede Stelle das einzeln wissen muss. Was vorher schiefging: Ein Ban oder Timeout hätte den falschen Kanal getroffen (mit einer Nachrichten-ID, die es dort gar nicht gibt → ging ins Leere oder traf den Falschen), fremde Zuschauer wären als eigene Chatter gezählt worden, und Commands wie `!clip`/`!raid` sowie die Werbe-Logik wären im Host- statt im Quell-Kanal gelaufen.

## #206 — Analyse-Dashboard rechnet ehrlicher: Unique-Zahlen entdoppelt, keine erfundenen Werte mehr, Geschätztes als geschätzt markiert

**Ausgangslage:** Beim genauen Nachrechnen der Analyse-Werte fielen mehrere Stellen auf, an denen Zahlen entweder falsch gerechnet, im Fehlerfall durch Beispieldaten ersetzt oder geschätzt-aber-als-gemessen dargestellt wurden:

- Die "Unique Chatter" über einen Zeitraum wurden aus den einzelnen Streams aufsummiert — wer an mehreren Streams teilnahm, zählte mehrfach. Im Streamer-Detail kam die Zahl zudem aus einer Gesamt-Historie ohne Zeitfilter, also unabhängig vom gewählten Zeitraum.
- Der Health-Score-Teilwert "Monetarisierung" teilte intern immer durch 1 statt durch die echte Stream-Anzahl und war dadurch bei mehreren Streams zu hoch.
- Die Lurker-Auswertung fiel bei einem internen Fehler still auf Demo-Beispieldaten zurück — man sah dann plausible, aber erfundene Zahlen, ohne es zu merken.
- Der Follower-Trichter zeigte "durchschnittliche Zeit bis Follow" und die Aufteilung "organisch vs. Raid" als harte Werte, obwohl beide nur geschätzt sind.
- Ein kurzer Mess-Aussetzer beim Follower-Stand (die API liefert zwischendurch 0) verfälschte an zwei weiteren Stellen den Follower-Vergleich nach unten.
- Eine ungültige Zeitraum-Angabe in der Raid-Auswertung führte zu einem harten Serverfehler statt einer sauberen Rückmeldung.

**Was geändert wurde:**

- Unique-Chatter werden jetzt echt entdoppelt: eindeutig über alle Streams des Zeitraums, plus der gespeicherte Alt-Bestand für ältere Streams ohne Einzeldaten. Kein Mehrfachzählen wiederkehrender Zuschauer mehr, und es zählt nur noch der gewählte Zeitraum.
- Der Monetarisierungs-Score wird durch die tatsächliche Anzahl der Streams normiert.
- Die Lurker-Auswertung zeigt im Fehlerfall einen ehrlichen Leerzustand ("momentan nicht verfügbar") statt Beispieldaten.
- Geschätzte Felder (Zeit bis Follow, Quellen-Split) sind jetzt als geschätzt markiert, damit die Oberfläche sie entsprechend kennzeichnen kann.
- Der Follower-Aussetzer-Filter, der an anderen Stellen längst greift, wird jetzt auch im Stream-Vergleich und im Stream-Bericht angewandt.
- Ungültige Zeitraum-Angaben liefern eine klare Rückmeldung statt eines Serverfehlers.

**Wie es funktioniert:** Für die Unique-Chatter zählt das System die eindeutigen Schreiber über alle Streams des Zeitraums in einem Durchgang (jeder Mensch genau einmal) und addiert für Alt-Streams, zu denen keine Einzel-Chatter-Daten mehr existieren, deren gespeicherte Gesamtzahl dazu — so geht weder Historie verloren noch wird doppelt gezählt. Der Monetarisierungs-Score teilt die gewichteten Ereignisse (Subs, Bits, Hype-Trains) durch die echte Stream-Anzahl im Zeitraum statt wie bisher durch eine feste 1. Geschätzte Werte tragen jetzt eine Markierung, aus der die Oberfläche ein "≈/geschätzt" machen kann; gemessene Werte bleiben unmarkiert. Der Follower-Aussetzer (Endstand 0 bei vorher positivem Stand = kurzer API-Hänger) wird beim Mitteln und Summieren als unbekannt behandelt und zieht den Vergleich nicht mehr als großer negativer Ausschlag nach unten.

## #205 — Bezahlschranke ließ zahlende Kunden nicht durch, !clip-Token läuft jetzt nach, Events-Zeitleiste wieder sichtbar

**Ausgangslage:** Drei Nachwehen aus der Rust-Umstellung, die erst beim genauen Gegenprüfen auffielen:

- Die in #200 wiederhergestellte Bezahlschranke prüfte intern gegen erfundene Plan-Namen (`analytics_pro`/`analytics_extended`) statt gegen die echten verkauften Pläne (`analysis_dashboard` und die Bundles). Folge: Genau die zahlenden Kunden — und das 30-Tage-Onboarding-Geschenk — wurden ausgesperrt, während nur der manuell gestartete Trial durchkam. Die Schranke wirkte also praktisch umgekehrt.
- Der native `!clip` (#203) nutzte den Broadcaster-Token nur roh: War er abgelaufen, scheiterte der Clip mit Fehler, obwohl er sich hätte erneuern lassen. Zusätzlich war der Token-Zugriff an „Raids aktiviert" gekoppelt — ein Streamer mit abgeschalteten Raids bekam fälschlich „OAuth fehlt", obwohl er autorisiert war.
- Im Analyse-Dashboard blieb die Ereignis-Zeitleiste einer Stream-Session (Titel-/Kategorie-Wechsel, Raids, Follower-Verlauf) leer, weil die Daten unter anderen Schlüsseln geliefert wurden, als die Oberfläche sie erwartete.

**Was geändert wurde:**

- Die Bezahlschranke fragt jetzt den echten Plan-Katalog ab — dieselbe Quelle, die auch die Features freischaltet. Wer einen erweiterten Plan oder einen laufenden Trial hat, kommt durch; alle anderen sehen die klare „Plan nötig"-Antwort. Die Ablaufprüfung akzeptiert zusätzlich reine Datumsangaben und Zeiten ohne Zeitzonen-Angabe, statt sie als ungültig zu verwerfen.
- `!clip` holt den Token jetzt über den regulären Weg mit automatischer Erneuerung bei Ablauf und ohne Raid-Kopplung — wie schon in der Vorgänger-Version. Streamer mit deaktivierten Raids können wieder clippen, abgelaufene Tokens werden vor dem Clip aufgefrischt.
- Die Ereignis-Zeitleiste liefert ihre Felder wieder unter den Namen, die die Oberfläche ausliest — Titel-/Spiel-Wechsel, Raids und Follower-pro-Minute erscheinen wieder.

**Wie es funktioniert:** Die Schranke ruft pro erweiterte Seite dieselbe Katalog-Prüfung auf, die der Rest des Systems für Plan-Rechte nutzt — eine einzige Wahrheitsquelle statt einer zweiten, von Hand gepflegten Liste, die mit der Zeit auseinanderlief. Beim Ablaufdatum wird ein reines Datum als Tagesende (UTC) gewertet, ein „Z" bzw. ein fehlender Zeitzonen-Teil sauber als UTC interpretiert. Für `!clip` entschlüsselt der Bot den Broadcaster-Zugang, prüft die Restlaufzeit (5-Minuten-Puffer) und erneuert den Token bei Bedarf serialisiert über ein DB-Lock, bevor er die Clip-Erstellung aufruft; die Raid-an/aus-Einstellung steuert nur noch Raids, nicht mehr den Clip-Zugriff. Die Zeitleiste benennt ihre Datenfelder jetzt exakt so, wie die Auswertungs-Oberfläche sie liest.

## #204 — Lurker-Erinnerung pingt jeden ruhigen Zuschauer nur noch einmal pro Stream

**Ausgangslage:** Die Lurker-Steuer-Erinnerung (der freundliche @-Ping an ruhige Stammzuschauer mit Discord-Hinweis) konnte denselben Zuschauer im Lauf eines Streams mehrfach anpingen — das wirkte wie Spam.

**Was geändert wurde:** Pro Stream merkt sich der Bot jetzt, wen er schon erwähnt hat, und überspringt diese Zuschauer beim nächsten Mal. Stattdessen rücken die nächsten ruhigen Zuschauer nach. Beim nächsten Stream (neue Session) ist die Liste wieder leer.

**Wie es funktioniert:** Beim Senden wird die Erinnerung an die jeweils ranghöchsten noch-nie-in-diesem-Stream-erwähnten Lurker geschickt (maximal zwei pro Erinnerung). Der Bot holt dafür mehr Kandidaten als er erwähnt, filtert die bereits Erwähnten heraus und kappt erst dann. Die Merkliste wird nur bei einer tatsächlich erfolgreich gesendeten Erinnerung ergänzt und beim Stream-Wechsel automatisch zurückgesetzt.

## #203 — !clip funktioniert wieder nativ

**Ausgangslage:** Seit der Umstellung auf das neue System war der Chat-Command `!clip` kaputt — jeder Aufruf endete mit einer Fehlermeldung statt einem Clip. Die eigentliche Clip-Erstellung war beim Umzug noch nicht nachgebaut.

**Was geändert wurde:** Der komplette Clip-Ablauf läuft jetzt nativ. Tippt jemand `!clip` (optional mit einem Titel), erstellt der Bot über die Twitch-API einen Clip aus dem aktuellen Stream-Buffer (ungefähr die letzten Sekunden) und postet den fertigen Clip-Link in den Chat.

**Wie es funktioniert:** Für die Clip-Erstellung braucht Twitch eine Autorisierung mit Clip-Recht – die kommt vom Broadcaster selbst (über die bestehende Bot-Verbindung, die der Streamer per `!raid_enable` eingerichtet hat). Der Bot entschlüsselt diesen Zugang, ruft damit die Clip-Erstellung auf und gibt den Link zurück. Ist der Streamer (noch) nicht verbunden, kommt ein klarer Hinweis, sich einmal per `!raid_enable` zu verbinden, statt einer nichtssagenden Fehlermeldung. Titel/Länge legt Twitch beim Buffer-Clip selbst fest; der angegebene Titel erscheint in der Chat-Antwort.

## #202 — Nachgebaute Features: Werbetexte, Personality-Panel, Ad-Drop, Discord-Scope

**Ausgangslage:** Ein paar Punkte aus dem Audit waren keine schnellen Korrekturen, sondern echte Nachbauten. Die abgeschlossenen:

- **Discord-Werbetexte wieder korrekt.** Die im Hintergrund vorgeschlagenen Promo-Texte (die der Bot passend zum Chat-Kontext auswählt) wichen vom hinterlegten Set ab — falsche Texte, falsche Stichwörter für die Auswahl. Jetzt 1:1 wie vorgesehen, mit den richtigen Schlagwörtern.
- **Personality-Panel im Zuschauer-Detail zurück.** Die Einordnung, worüber ein Zuschauer typischerweise schreibt (Hype, Frage, Feedback, Game-Talk, …), fehlte komplett. Jetzt werden bis zu 2000 Nachrichten des Viewers ausgewertet und der häufigste Typ samt Verteilung angezeigt.
- **Werbe-Drop-Auswertung mit richtigem Vorzeichen.** Die Kennzahl „Viewer-Verlust durch Werbung" verglich vorher einen Einzelpunkt mit dem Tief während der Ad und kam mit umgekehrtem Vorzeichen heraus. Jetzt: 5-Minuten-Mittel vor der Werbung gegen 5-Minuten-Mittel nach der Werbung, korrekt vorzeichenrichtig — und die „schlimmsten Ads" sind richtig sortiert.
- **Discord-Aktions-Berechtigung nachgezogen (intern).** Zwei interne Routen (Discord-Flag/Profil setzen) prüften eine konfigurierbare Berechtigungs-Allowlist nicht; jetzt tun sie es, wie die übrigen Routen.

**Wie es jetzt funktioniert:** Diese Punkte entsprechen wieder der Vorgänger-Version. Ein paar größere Nachbauten (u. a. die native Clip-Erstellung und das automatische Verifizieren neuer Streamer) sind bewusst als eigene, sorgfältig zu bauende Einheiten zurückgestellt — intern als Backlog dokumentiert, damit nichts gehetzt mit neuen Fehlern reinkommt.

## #201 — Port-Audit: Restliche Abweichungen aus dem Backlog behoben

**Ausgangslage:** Nach den großen Fehlern (#199) blieb aus dem Port-Audit ein Backlog kleinerer Abweichungen — einzeln meist unauffällig, in Summe aber spürbar in Analyse-Zahlen, Moderation und internen Abläufen. Behoben, gruppiert nach Bereich:

**Analyse-Dashboard:**
- **Plan-Berechtigungen wieder korrekt.** Die Zuordnung „welcher Plan schaltet welches Feature frei" wich beim Umzug ab: zahlende Erweitert-/Analyse-Kunden wurden beim KI-Modell heruntergestuft, bei einem Bundle wurde die Werbung trotz bezahltem Werbefrei-Anteil nicht abgeschaltet, und ein paar Pläne bekamen einen Raid-Boost ohne Berechtigung. Jetzt 1:1 wie vorgesehen. Außerdem: ein manueller Admin-Downgrade auf den Gratis-Plan wird wieder respektiert, statt von einem noch laufenden Abo überschrieben zu werden.
- **Kategorie-Rang exakt.** Der angezeigte Rang im Kategorie-Vergleich nutzte eine Näherung und stimmte nur, wenn der eigene Schnitt zufällig genau einem anderen Wert entsprach. Jetzt exakt nach der ursprünglichen Formel.
- **Viewer-Detail ohne Chat ehrlich.** Hatte ein Zuschauer nie geschrieben, zeigte das Profil Schein-Werte (Stunden 0–2, „Sonntag") statt leerer Aktivität. Jetzt: keine Stunden, „N/A".
- **Retention-Kurve:** „verwendete Sessions" zählte fälschlich Minuten-Zeilen statt Sessions; die durchschnittliche Watch-Dauer übersprang die allererste Minute.
- **Admin-Streamerliste:** Status-Anzeige hat jetzt die richtige Reihenfolge (gesperrt/abgemeldet vor „live"), Anzeigename fällt auf den Discord-Namen zurück, Notiz zeigt die Admin-Notiz statt der Werbenachricht.

**Moderation & Chat:**
- Ein bereits gebannter Spammer, der erneut auffällt, erzeugt keinen doppelten Ban-Eintrag in der öffentlichen Statistik und keine wiederholte Chat-Notice mehr.
- Eine ungünstig kodierte Verdachts-Nachricht kann den Moderations-Pfad nicht mehr zum Absturz bringen (Zeichen- statt Byte-Kürzung im Log).
- Die Lurker-Erinnerung berechnet die „stille Zeit" robust (keine negativen Werte mehr, die die Schwelle verfälschen).
- `!clip` antwortet ehrlich („wird gerade umgestellt") statt „in 10 Sekunden nochmal" — die Clip-Erstellung wird noch nativ nachgezogen.

**Intern (robuster, ohne sichtbaren Effekt):** Channel-Points-Einlösungen werden wieder als Telemetrie gespeichert; Live-Ankündigungen nutzen einen über Neustarts stabilen Idempotenz-Schlüssel (keine Doppel-Postings mehr im Neustart-Fenster); bestätigte Raids auf nicht-Deadlock-Ziele werden korrekt sofort aufgelöst; User-Abfragen an Twitch werden auf 100er-Pakete aufgeteilt (API-Limit).

**Wie es jetzt funktioniert:** Alle genannten Stellen entsprechen wieder der ausgereiften Vorgänger-Version. Die wenigen verbliebenen Punkte (größere Feature-Nachbauten wie die volle Clip-Erstellung sowie bewusst belassene Verbesserungen gegenüber dem Original) sind intern als Backlog dokumentiert.

## #200 — 30-Tage-Analyse-Trial + Bezahlschranke für erweiterte Auswertungen

**Ausgangslage:** Die erweiterten Analyse-Seiten (Zuschauer-Demografie, Insights, Kategorie-Vergleich, Bestenliste, Timings, Viewer-Profile, Audience-Sharing, Follower-Trichter) waren im umgestellten Dashboard versehentlich für jeden eingeloggten Streamer offen — die Bezahlschranke, die es vorher gab, war beim Port verloren gegangen. Gleichzeitig fehlte ein einfacher Weg, die erweiterten Auswertungen unverbindlich zu testen.

**Was geändert wurde:**

- **Bezahlschranke greift wieder.** Die erweiterten Auswertungen brauchen einen passenden Plan oder einen laufenden Trial. Ohne beides kommt eine klare „Plan nötig"-Antwort statt der vollen Daten. Admins und lokale Zugriffe sind ausgenommen.
- **30-Tage-Trial zum Selbst-Freischalten.** Jeder Streamer kann sich im Dashboard auf der Preis-/Plan-Seite einmalig einen 30-Tage-Gratis-Test der erweiterten Analyse holen. Während der 30 Tage sind alle erweiterten Seiten offen, danach greift wieder die Schranke.
- **Mitbringsel beim Onboarding.** Neue Partner bekommen den 30-Tage-Trial automatisch beim ersten Verbinden — als Willkommens-Geschenk, ohne etwas tun zu müssen.
- **Einmalig pro Streamer.** Der Trial lässt sich pro Streamer genau einmal aktivieren — egal ob automatisch beim Onboarding oder per Button. Wer bereits einen bezahlten Plan hat, bekommt keinen Trial aufgedrängt.

**Wie es funktioniert:** Beim Klick auf „Trial starten" — oder automatisch beim Onboarding — wird ein 30-Tage-Plan mit Ablaufdatum gesetzt und ein unveränderlicher Merker „Trial schon vergeben" gespeichert; der verhindert Mehrfach-Nutzung. Vor jeder erweiterten Seite prüft die Schranke, ob ein gültiger Plan oder ein noch laufender Trial vorliegt — ist das Ablaufdatum überschritten, ist die Seite wieder gesperrt. Die Trial-Logik selbst gab es schon in der alten Version (inkl. Ausschluss bei bestehendem Bezahlplan); neu ist, dass der Trial neuen Partnern direkt beim Onboarding geschenkt wird, und dass die Schranke jetzt auch im umgestellten Dashboard wirkt. Der „Trial starten"-Button im Dashboard spricht jetzt direkt den nativen Endpoint an.

## #199 — Port-Audit: Schwung stiller Fehler aus der Rust-Umstellung behoben

**Ausgangslage:** Ein systematischer Audit hat den auf Rust umgestellten Bot Funktion für Funktion gegen die alte Python-Version gestellt. Dabei kamen mehrere Fehler ans Licht, die seit der Umstellung still mitliefen — nichts ist abgestürzt, deshalb fielen sie im Alltag kaum auf, aber Zahlen und Verhalten wichen ab.

**Analyse-Dashboard zeigte teils Nullen.** Auf mehreren Auswertungsseiten (Kategorie-Vergleich, Bestenliste, Zuschauer-Demografie, Raid-Auswertung, Ranglisten nach Wachstum, Follower-Trichter, Monatsstatistik) standen Kennzahlen dauerhaft auf 0 oder zeigten für jeden dieselbe „Starter"-Stufe — obwohl die Daten in der Datenbank lagen. Ursache: Beim Umzug wurden Ganzzahl-Werte (Zuschauerzahlen, Peaks, Summen) intern im falschen Zahlenformat ausgelesen; die Datenbank lieferte einen Lesefehler, der still verschluckt und durch eine 0 ersetzt wurde. Statt das zu kaschieren, werden die Werte jetzt im korrekten Format gelesen — die Seiten zeigen wieder echte Zahlen und Ranglisten-Stufen.

**Bots wurden als Zuschauer mitgezählt.** Im Zuschauer-Verzeichnis und den Cross-Channel-Auswertungen sollte eine feste Liste bekannter Chat-Bots herausgefiltert werden. Durch einen Logikfehler in der Filterbedingung griff der Ausschluss nie — Bots tauchten als normale Zuschauer auf und verfälschten die Zahlen. Der Filter schließt sie jetzt wieder zuverlässig aus.

**Events-Tab einer Stream-Session war leer.** In der Detailansicht einer einzelnen Session blieben die Listen „Raids" und „Follows" immer leer, weil sie beim Port nie befüllt wurden. Jetzt werden eingehende und ausgehende Raids sowie die Follows pro Minute innerhalb des Stream-Fensters wieder angezeigt.

**Scam-/Service-Pitch-Warnung war zu aggressiv.** Beim ersten Treffer hat der Bot die Nachricht sofort gelöscht und einen 10-Minuten-Timeout gesetzt. Das konnte gelegentlich legitime Nutzer treffen, die knapp über der Schwelle lagen. Jetzt gilt wie vorgesehen: Der erste Treffer ergibt nur eine öffentliche Warnung — kein Löschen, kein Timeout. Ein Timeout kommt erst, wenn jemand trotz laufender Warn-Sperre erneut einen Pitch nachlegt. Gelöscht wird in keinem Fall.

**Große Kanäle bekamen fälschlich Scam-Warnungen.** Die Warnung soll etablierte Kanäle ab einer gewissen Follower-Zahl ausnehmen. Diese Ausnahme war praktisch deaktiviert (die Follower-Zahl wurde nie nachgeschlagen, jeder Kanal galt als „klein"). Jetzt wird die letzte bekannte Follower-Zahl wieder herangezogen; große Kanäle sind ausgenommen. Der Nachschlag ist abgesichert, damit ein Datenbank-Hänger den Chat nie ausbremst.

**Streamer blieb nach abgelaufener Gnadenfrist ausgesperrt.** Nach einem Twitch-Anmelde-Fehler gibt es eine Gnadenfrist, in der der Analyse-Zugang eingeschränkt ist. Eine längst abgelaufene Frist wurde weiter als „aktiv" behandelt — der Streamer kam dauerhaft nicht mehr ins Analyse-Dashboard, obwohl er längst hätte freigegeben sein müssen. Jetzt wird geprüft, ob die Frist wirklich noch in der Zukunft liegt; danach ist der Zugang wieder offen.

**Token-Sperre nach Autorisierungs-Entzug.** Wenn die Twitch-Autorisierung eines Streamers widerrufen ist, nimmt der Bot den toten Token jetzt sofort aus dem Betrieb und markiert „Neu-Verbinden nötig", statt ihn weiter erfolglos zu erneuern.

**Kategorie-Statistik wieder deutschsprachig.** Das kategorieweite Sampling über alle Deadlock-Streams lief ohne Sprachfilter und mischte nicht-deutsche Streams in die Statistik. Der Filter ist wieder fest auf die deutschsprachigen Varianten gesetzt.

**So funktioniert es jetzt:** Alle genannten Stellen verhalten sich wieder wie in der ausgereiften Python-Version — gleiche Funktionalität, aber an einer Stelle (Follower-Nachschlag) zusätzlich gegen Datenbank-Hänger abgesichert. Die Zahlen im Dashboard stimmen wieder, die Moderation greift zurückhaltender und nur dort, wo sie soll.

## #198 — Tote Refactoring-Relikte entfernt: promos.rs, scam_pitch.rs, auto_raid.rs

**Ausgangslage:** Drei weitere Reste aus früheren Umbauphasen hatten sich im Code gehalten — toter Code, der mit `let _ =` vor Compiler-Warnungen kaschiert wurde.

**Was wurde geändert:**

- `promos.rs`: `invite_opt` aus dem periodischen Promo-Loop entfernt. Die zugehörige `cached_invite_or_none()`-Methode gab immer `None` zurück (Invite-Feature nie implementiert) — der Rückgabewert wurde daher direkt ignoriert. Methode und Tuple-Feld vollständig gelöscht.
- `scam_pitch.rs`: Tote `now`-Variable vor dem `tokio::spawn`-Block entfernt. Zeitstempel innerhalb des Spawns werden über eine eigene `epoch.elapsed()`-Berechnung ermittelt — die äußere `now`-Bindung wurde nie an den Spawn übergeben.
- `auto_raid.rs` (`source_skip_reason`): Toter `target_game_lower`-Parameter bereinigt und gleichzeitig Logging-Granularität auf Python-Niveau gebracht. Vorher: alle nicht-eligiblen Spiele → `last_game_not_eligible`. Jetzt: leeres Spiel → `missing_current_game`, falsches Spiel → `source_category_mismatch` (entspricht Python `raid_data_sources.py`). Parameter war tot, weil der `target_game == current_game`-Fall (= eligible) nie diese Funktion erreicht.

**Ergebnis:** Kein Verhaltensunterschied für Endnutzer, aber sauberere Logs beim Auto/Manual-Raid-Skipping — künftig ist im Tracing-Log erkennbar, ob ein Streamer kein Spiel gesetzt hatte oder ein anderes gespielt hat.

## #197 — Chatter-Tracking: Partner-Gate entfernt + Refactoring-Relikt in GlobalBanSweep bereinigt

**Ausgangslage:** Chatter-Daten (Chat-Nachrichten, Session-Chatters, Rollup) wurden im Rust-Bot nur für aktive Partner-Kanäle gespeichert — alle anderen Kanäle wurden still ignoriert. Gleichzeitig gab es in der Global-Ban-Sweep-Logik einen toten API-Call, der aus einem unvollständigen Refactor stammte.

**Was wurde geändert:**

- Chatter-Tracking schreibt jetzt für **alle Kanäle**, in denen der Bot aktiv ist — kein Partner-Gate mehr beim Speichern. Die Trennung Partner/Nicht-Partner erfolgt beim Abfragen der Daten, nicht beim Schreiben.
- Pipeline: das Legacy-`is_monitored_only`-Flag entfernt; Non-Partner-Kanäle werden jetzt einheitlich behandelt (Datensammlung, keine Moderation/Promos).
- GlobalBanSweep: doppelten `bot_user_id()`-Call entfernt. Die `moderator_id` für Helix-Ban-Calls wird intern in `ban_user()` gesetzt — der externe Vorab-Abruf war ein Überbleibsel aus einer früheren API-Schicht, nie verwendet, mit `let _ = bot_id` kaschiert.

**Wie es jetzt funktioniert:** Jede Chat-Nachricht in einem Bot-bekannten Kanal landet in `twitch_chat_messages`, `twitch_session_chatters` und `twitch_chatter_rollup` — sofern eine offene Stream-Session existiert und Deadlock live ist. GlobalBanSweep ruft `bot_user_id()` genau einmal pro Ban-Call auf (intern in der Helix-Schicht), nicht mehr doppelt.

## #196 — Lurker-Tax und Promo-Engine: 5 Python/Rust-Portfehler behoben

**Ausgangslage:** Beim Python→Rust-Port der Chat-Promo-Engine haben sich 5 semantische Bugs eingeschlichen, die alle stumm blieben — kein Crash, kein Test-Fail, aber falsches Verhalten.

**Was wurde geändert:**

- **Lurker-Tax ohne Bezahlplan** (kritisch): `maybe_send_lurker_tax_reminder` prüfte in Rust nur das Feature-Flag `lurker_tax_enabled`, nicht ob der Streamer überhaupt das passende Abo hat. Jetzt wird geprüft: Plan-Entitlement `chat.lurker_tax` (nur in `raid_boost` und höher) + Scope `moderator:read:chatters` im Auth-Store. Ohne beides wird die Erinnerung übersprungen.

- **Lurker-Tax für aktive Chatter** (mittel): Die SQL-Abfrage für Live-Kandidaten filterte in Rust nicht auf `messages = 0`. Chatter, die in der laufenden Session bereits geschrieben hatten, konnten als Lurker getaggt werden. Jetzt wird die Live-Kandidaten-CTE wie in Python auf echte Lurker eingeschränkt (`messages = 0` + `seen_via_chatters_api = TRUE`).

- **Promo-Text Wiederholung** (mittel): Die Anti-Repeat-Logik wählte in jeder Runde aus dem gefilterten Pool, schrieb den gewählten Text aber nie zurück. Folge: der gleiche Text wurde immer wieder gesendet. Nach der Auswahl wird der Template-String jetzt in `last_promo_text` gespeichert.

- **Cooldown-Verbrauch bei Send-Fehler** (mittel): Wenn `send_announcement` fehlschlug, wurde der Promo-Cooldown trotzdem belegt — nächster Versuch erst nach der vollen Cooldown-Zeit. Python gibt bei `not ok` sofort `False` zurück, ohne `_mark_promo_sent` aufzurufen. Rust tut das jetzt auch.

- **Targeted-Promo Timeout-Fallback immer erster Preset** (low): Bei MiniMax-Timeout wählte Rust immer `presets[0]` statt zufällig. Das `choose()`-Ergebnis wurde mit `.map(|_| ())` weggeworfen. Jetzt: `presets.choose(&mut rng).unwrap_or(&presets[0])`.

**Wie es jetzt funktioniert:** Lurker-Tax feuert nur noch für Streamer mit aktivem Abo und richtigen Scopes. Promo-Texte werden korrekt rotiert. Fehlgeschlagene Sends verbrauchen keinen Cooldown-Slot mehr. Targeted-Promos bei KI-Timeout sind jetzt gleichmäßig verteilt.

## #195 — Streamer-Link-Matcher: Rust-Implementierung im tb-bot

**Ausgangslage:** Der automatische Twitch↔Discord-Abgleich lief als Python-Cog im Discord-Bot (StreamerLinkMatcher) und sendete alle 6h ein Discord-Embed — auch wenn keine neuen unverknüpften Streamer vorhanden waren.

**Was wurde geändert:** Neues Modul `streamer_link` im tb-bot (Rust). Der Matcher läuft als Tokio-Hintergrund-Task alle 6h, ist aber komplett still wenn keine neuen Partner-Streamer ohne Discord-Verknüpfung in der DB vorhanden sind. Für das Matching wird der neue Broker-Endpoint `GET /discord/members` genutzt (2315 Guild-Member). Namens-Normalisierung: Unicode-NFKD, Leetspeak-Ersatz, Stream-Affixe entfernen, Jaro-Winkler-Ähnlichkeit (strsim). State-Datei bleibt kompatibel mit dem Python-Cog (gleicher JSON-Pfad, gleiche Felder) — kein Re-Scan beim Umstieg. Schwellen wie bisher: Score ≥ 90 → Auto-Link + Rolle, Score 70–89 → Manual-Prompt-Embed, darunter → still verworfen. Den Broker-Aufruf für `add-role` und `send-rich-message` nutzt der Task über die bestehende BrokerRelay-Infra.

**Wie es jetzt funktioniert:** Neuer aktiver Partner ohne discord_user_id → erscheint beim nächsten 6h-Tick im Scan → Member-Index wird aus allen Guild-Membern gebaut → Match-Score berechnet → Auto-Link oder Prompt. Kein Embed wenn nichts zu tun ist.

## #194 — Viewer-Spike-Promo: Silence-Check bei fehlendem Chat-History-Eintrag korrigiert

**Problem:** Wenn ein Channel noch nie eine Chat-Nachricht hatte (kein `last_raw_chat_message_ts`-Eintrag im In-Memory-State), hat der Viewer-Spike-Promo-Guard in Rust den Channel als „nicht still genug" eingestuft und die Promo geblockt. In Python gilt: kein Chat-Aktivitäts-Timestamp = kein aktiver Chat = Silence gilt als erfüllt (die Bedingung prüft explizit `if activity_age_sec is not None`).

**Ursache:** Rust's `is_some_and(pred)` liefert bei `None` immer `false` — „kein Timestamp vorhanden" wurde als „Bedingung nicht erfüllt" gewertet. Python's `if x is not None and x < threshold` entspricht semantisch `map_or(true, |x| x < threshold)` — also: kein Wert → Guard durchgelassen.

**Fix:** Ein-Zeilen-Änderung in `maybe_send_viewer_spike_promo`: `is_some_and(|t| age >= threshold)` → `map_or(true, |t| age >= threshold)`. Betrifft nur den Silence-Guard für den Viewer-Spike-Pfad — alle anderen Promo-Pfade nutzen andere Guards ohne dieses Muster.

## #193 — Bugfixes: category-comparison + audience-demographics

**Was war kaputt:**

**category-comparison:**
- Q5 (Kategorie-Durchschnitt für Retention + Chat-Health) lief doppelt — erster Durchlauf wurde komplett verworfen. Jeder Request löste damit eine extra DB-Query aus die nichts beitrug.
- `categoryRank` (Rang des Streamers) wurde über eine Integer-Division-Näherung berechnet: `total - percentile*total/100`. Bei mehreren Streamern mit gleichem Durchschnitt wich das deutlich ab. Jetzt exakt: `partition_point` zählt Streamer mit Avg ≤ deinem Wert, Rang = total − dieser Zahl + 1.

**audience-demographics:**
- Leere Schleife über `time_rows` (Stunden-Daten) mit `let _ = row` — tat buchstäblich nichts.
- Drei `.max(0)` auf `usize`-Feldern (können nie negativ sein) — entfernt.
- Überflüssiges `format!()` ohne Platzhalter als SQL-String — jetzt direktes Literal.

## #192 — Python-Bot abgeschaltet: Rust übernimmt alle verbleibenden API-Routen nativ

**Ausgangslage:** Trotz vollständiger Chat-, Monitoring- und Raid-Übernahme durch Rust lief der Python-Prozess noch weiter — einzig wegen 5 interner API-Routen die kein Rust-Pendant hatten und über den Legacy-Proxy an Python auf Port 8779 weitergeleitet wurden. Das hieß: Python brauchte Speicher, startete voll durch und hätte einen API-Fehler auf 8779 produziert sobald die Route gerufen wird.

**Was umgestellt wurde:** Alle 5 noch proxied Routen haben jetzt native Rust-Handler:
- `GET /debug/observability` und `GET /debug/chatters/:login`: liefern leere JSON-Antworten (diese Routen zeigten Python-in-Process-State der ohnehin nicht mehr relevant ist)
- `POST /eventsub/processing/requeue`: antwortet mit `ok: true` (Rust verarbeitet Events direkt in der Pipeline, kein manuelles Requeue nötig)
- `POST /streamers/:login/chat-action`: gibt 503 zurück mit Erklärung — der Bot-OAuth-Token liegt in tb-bot und ist aus tb-internal-api nicht erreichbar; die Route ist bis zur Bot-Token-Bridge bewusst offen
- `POST /raid/requirements` (Login im JSON-Body): gibt 503 zurück — Discord-DM-Versand braucht den Discord-Bot der nicht mehr in Python läuft; zu implementieren via Master-Broker (8770)

Danach: `deadlock-twitch-bot.service` (Python) gestoppt und deaktiviert. Port 8779 ist geschlossen.

**Ergebnis:** Nur noch ein Bot-Prozess (`deadlock-twitch-bot-rust.service` auf 8776). Unbekannte Routen gehen nicht mehr an Python, sondern liefern jetzt einen sauberen 404 statt 502 Gateway-Fehler. Zwei Routen (chat-action, raid-requirements) sind bewusst 503 bis die Bot-Token-Bridge bzw. Discord-DM via Broker gebaut ist.

## #191 — Werbefrei-Plan und Entitlement-Check in Rust nachgezogen

**Problem:** Streamer mit einem bezahlten „Werbefrei"-Plan (z. B. `chat_quiet`, `bundle_werbefrei_analyse` usw.) haben in Rust trotzdem Promos bekommen, wenn der Dashboard-Toggle `promo_disabled` nicht explizit aktiviert war. Python prüft zwei Bedingungen: die `promo_disabled`-Spalte UND ob der aktive Plan das Entitlement `chat.promos.disable` trägt. Rust hat nur die Spalte geprüft.

**Warum das passiert:** `promo_disabled` ist ein manueller Dashboard-Toggle, der erst nach dem Kauf aktiv gesetzt werden muss. Wer das nie angefasst hat, hatte `promo_disabled=0` — aber der Werbefrei-Plan schützte ihn trotzdem in Python, weil das Entitlement unabhängig vom Toggle geprüft wird. In Rust fehlte dieser zweite Pfad.

**Fix:** Rust liest jetzt zusätzlich `manual_plan_id` (kanonisch) und `plan_name` (Legacy) aus `streamer_plans`. Beide Spalten werden gegen alle Plan-IDs gecheckt, die `chat.promos.disable` tragen (aus `catalog.py` abgeleitet: `chat_quiet`, `bundle_chat_quiet_raid_boost`, `bundle_analysis_raid_boost`, `bundle_werbefrei_analyse`, `bundle_komplett` plus Legacy-Namen wie `werbefrei`/`quiet`). Bei DB-Fehler bleibt das Fail-open-Verhalten.

Außerdem: beim Start bereinigt die Promo-Engine jetzt die `twitch_promo_cooldowns`-Tabelle von Einträgen die älter als 24 Stunden sind — wie Python es in `_restore_promo_cooldowns` macht (war bisher nicht aufgerufen).

## #190 — Zwei weitere Promo-Port-Bugs in Rust behoben

**Problem 1 — Scam-Warnung vergaß ihren Seed-Timer nach Neustart:** Die Scam-Warnung benutzt einen Verzögerungs-Mechanismus: beim ersten Auftauchen eines Channels wird der Timer auf "vor 100 Minuten" gesetzt, damit die Warnung frühestens nach 20 Minuten kommen kann (statt sofort nach dem Neustart). Im Rust-Port wurde dieser Seed-Wert zwar in den In-Memory-State geschrieben, aber nicht in die DB persistiert — nach jedem Neustart fehlte der Eintrag, der Timer startete wieder von null, und die 20-Minuten-Initialverzögerung lief jedes Mal erneut ab. Python schreibt den Seed via `_persist_scam_warning_ts` sofort weg. Fix: Rust tut dasselbe — Seed wird direkt nach dem Setzen per `save_promo_cooldown` in `twitch_promo_cooldowns` geschrieben, und die DashMap-Ref wird vorher freigegeben (Rust-Async-Constraint: kein Shard-Lock über einen `.await`-Punkt halten).

**Problem 2 — Scam+Targeted-Block ignorierte Aktivitätsschwellen:** Der `send_promo_if_due`-Block, der Scam-Warnung und Targeted-Promo feuern kann, hat zwar `overall_ready` geprüft (globaler Cooldown + Mindest-Roh-Nachrichten), aber nicht `activity_ready` — also die zweite Schwelle: mindestens 3 Nachrichten im 8-Minuten-Fenster und ≥2 neue Chatter. Python prüft `activity_ready` als Pflicht-Gate vor beiden Typen (Zeile 1466 in promos.py). Im Rust-Port konnte Scam-/Targeted-Promo also auch in toten Chats feuern. Fix: `activity_ready` wird jetzt zusammen mit `overall_ready` berechnet und als AND-Bedingung vor dem Block geprüft.

**Ergebnis:** Scam-Warnung überlebt Bot-Neustarts ohne Timing-Reset. Scam- und Targeted-Promos erscheinen nur noch in aktiven Chats — identisches Verhalten wie Python.

## #189 — Targeted-Promo belegt jetzt den globalen Promo-Cooldown (Rust)

**Problem:** Nach einer Targeted-Promo (personalisierter Discord-Pitch an einen Chatter) hat der Rust-Bot unmittelbar danach noch eine normale Chat-Promo gesendet. Im schlimmsten Fall zwei Ankündigungen innerhalb von Sekunden.

**Ursache:** Der Rust-Port von `maybe_send_targeted_promo` hat den Promo-Slot nicht markiert. Python ruft nach jeder Targeted-Promo `_mark_promo_sent` auf — das setzt `last_promo_sent`, resettet den Roh-Nachrichten-Zähler auf 0 und persistiert den Cooldown in die DB. Im Rust-Port fehlte dieser Aufruf komplett: `last_promo_sent` blieb `None`, also konnte der nächste Loop-Tick sofort wieder eine Activity- oder Viewer-Spike-Promo rausschicken.

**Fix:** Beide Pfade in `maybe_send_targeted_promo` (User-targeted und Global-Preset) rufen jetzt nach dem erfolgreichen Send `mark_promo_sent` auf — identisches Verhalten wie Python.

## #188 — Werbung kommt frühestens 10 Minuten nach Stream-Start

**Problem:** Der Bot hat Discord-Promos teilweise direkt beim Go-Live gepostet — als erste Nachricht im Chat, bevor überhaupt jemand geschrieben hat. Das wirkt roboterhaft und ist schlechtes Timing.

**Was geändert wurde:** Beide Promo-Engines (Python und Rust) prüfen jetzt vor jeder Werbung, wie lange der Stream bereits live ist. Dazu wird `last_started_at` aus `twitch_live_state` gelesen — der Zeitstempel, den Twitch beim Online-Event schickt. Ist der Stream noch keine 10 Minuten alt, blockiert das Gate alle Promo-Pfade: Chat-Aktivitäts-Promos, Viewer-Spike-Promos, Targeted-Promos und die Scam-Warnung. Die Schwelle gilt für beide Engines identisch. Bei DB-Fehler oder fehlendem Eintrag läuft der Bot wie bisher weiter (fail-open), damit ein Infrastruktur-Problem nicht dauerhaft alle Werbung unterbindet.

**Ergebnis:** Die erste Promo in einem Stream-Channel erscheint frühestens nach 10 Minuten — und erst dann, wenn auch die bestehenden Aktivitätsschwellen erfüllt sind (mind. 16 Roh-Nachrichten, 3 Messages im Aktivitätsfenster usw.). Kein Kaltstart-Spam mehr.

## #187 — Category-Timings nativ in Rust

**Ausgangslage:** Der Category-Timings-Endpoint lief noch über den Fallback-Proxy. Python lud alle Viewer-Count-Rohdaten in den Speicher und berechnete Mediane in Python.

**Was wurde portiert:**
- `GET /twitch/api/v2/category-timings?days=30&source=category`: Outlier-resistente Stunden- und Wochentags-Verteilung der gesamten Kategorie. Methode: Median der Streamer-Mediane ("Median of Medians") + P25/P75-Konfidenzband.
- Statt alle Rohdaten zu laden: Postgres `PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY viewer_count)` direkt per `(streamer, hour)` — reduziert die übertragenen Zeilen von N×Samples auf max. 24×Streamer-Anzahl.
- P25/P75 mit Python-kompatibler "exclusive"-Quartil-Methode: virtueller Index `(len+1)*q - 1` (0-basiert). Für count < 4 Sonderfall: count=1 → gleicher Wert, count 2-3 → min/max.
- `source=tracked` liest aus `twitch_stats_tracked`, Default aus `twitch_stats_category`.

**Technisch:** 2 SQL-Queries (hour + DOW), rest reine Rust-Arithmetik. `total_streamers` aus dem Union der Ergebnis-Sets beider Queries.

## #186 — Audience-Demographics nativ in Rust

**Ausgangslage:** Der aufwändigste Audience-Endpoint lief noch über den Fallback-Proxy. Er enthält eine eigene Engagement-Berechnung, gewichtete Peak-Hour-Analyse und eine 5-Kategorie-Viewer-Klassifikation.

**Was wurde portiert:**
- `GET /twitch/api/v2/audience-demographics`: Gibt Viewer-Typen (Dedicated/Regular/Silent Regular/Casual/New Visitors), Aktivitätsmuster (weekend-heavy/weekday-focused/balanced), Primärsprache, Chat-Penetration und Peak-Aktivitätsstunden zurück.
- **`_compute_weighted_peak_hours`**: Statt alle Chat-Messages als Einzelzeilen zu laden (Python: `fetchall()`), gruppiert eine SQL-Query direkt nach `(session_id, EXTRACT(HOUR FROM message_ts AT TIME ZONE $tz))` — max. 720 Aggregat-Zeilen statt N×10.000 Einzelzeilen. Exponentielles Recency-Gewicht (`0.5^(idx/8)` per Session), Winsorisierung des p90 pro Stunde (lineare Interpolation), gewichtete Summe → Top-3 Stunden.
- **`calculate_engagement`** komplett in Rust: Chat-Penetration (aktive Chatter / tracked Accounts), Messages per 100 Viewer-Minutes, Reliability-Flag (mind. 1 passiver Viewer-Sample + ≥20% Chatters-API-Coverage).
- **Viewer-Klassifikation**: Cold-Rollup-Detection (>90% `seen_before=False` → Fallback auf session_count≥2), `is_first_time_streamer`-Flag-Logik, 5-Kategorien-Mapping.
- **Timezone-Validierung**: Timezone-String wird via `chrono-tz` validiert (IANA-Namen), bei ungültigem Wert Fallback auf UTC — kein AT-TIME-ZONE-Injection-Risiko.

**Technisch:** 8 SQL-Queries + 1 DOW-Hilfsquery (Aktivitätsmuster), `$3::text[]`-Bot-Array wird in per_user- und rollup-CTE mehrfach referenziert. `$1` (since-DateTime) wird im CASE-WHEN für `seen_before` wiederverwendet.

## #185 — Category-Comparison nativ in Rust

**Ausgangslage:** Der Category-Comparison-Endpoint lief noch über den Fallback-Proxy. Er ist der komplexeste Performance-Endpoint: 9 SQL-Queries, Python-seitige Percentile-Berechnung und eine Peer-Group-Analyse.

**Was wurde portiert:**
- `GET /twitch/api/v2/category-comparison`: Vergleicht den eigenen Kanal mit dem Category-Durchschnitt und zeigt Perzentile für alle vier Kernmetriken (Ø-Viewer, Peak, 10-Minuten-Retention, Chat-Health).
- **Percentile-Berechnung** komplett in Rust repliziert: Formel `(below + 0.5 * equal) / total * 100` — identisch zur Python-Referenz `_percentile_of` und genauer als Postgresʼ `PERCENT_RANK()` bei vielen gleichen Werten.
- **Peer-Group-Analyse** (`_get_peer_group_stats`): Stuft den eigenen Streamer in eine Tier-Klasse ein (starter/rising/established/featured/top, Grenzen: 15/50/150/500 Ø-Viewer), holt Session-Metriken aller Peers via `= ANY($1::text[])`, berechnet Median und Peer-Percentile.
- `exclude_external=1` filtert Streamer über 100 Ø-Viewern aus der Percentile-Basis heraus (per `HAVING AVG <= 100` auf die jeweiligen Queries), die Peer-Group-Berechnung bleibt davon unberührt.

**Technisch:** 9 Queries, davon Q1/Q2 deine eigenen Tracked- und Session-Stats, Q3 alle Kategorie-Avgs (Basis für Peer-Group + Percentile), Q4–Q8 die sortierten Listen für Peak/Ret/Chat-Percentile, Q9 Peer-Session-Metriken via Array-Bind. `partition_point` in Rust liefert exakte `below`/`equal`-Splits ohne vollständige Sortierung des Arrays.

## #184 — Category-Leaderboard nativ in Rust

**Ausgangslage:** Der Category-Leaderboard-Endpoint lief noch über den Fallback-Proxy.

**Was wurde portiert:**
- `GET /twitch/api/v2/category-leaderboard`: Rangliste aller Streamer aus `twitch_stats_category` nach Durchschnitts- oder Peak-Viewer. Optionaler `tier`-Filter (starter/rising/established/featured/top) wird Rust-seitig auf den avg_vc-Wert angewendet. `exclude_external=1` begrenzt auf Streamer unter 100 Ø-Viewern via `HAVING AVG <= 100`. Gibt die eigene Position immer zurück, auch wenn sie außerhalb des `limit`-Fensters liegt. `yourTier` wird aus dem avg_vc der Ergebnismenge berechnet (Fallback auf twitch_stream_sessions wenn der Streamer nicht in den Category-Daten ist).

**Technisch:** Ein SQL-Query mit bedingtem `HAVING`-Clause, Rest in Rust. `BOOL_OR(is_partner)` fasst das Partner-Flag per Streamer zusammen. Tier-Klassifikation als reine Funktion (`< 15`, `< 50`, `< 150`, `< 500`, else `top`).

## #183 — Lurker-Analysis nativ in Rust

**Ausgangslage:** Der Lurker-Analysis-Endpoint lief noch über den Fallback-Proxy.

**Was wurde portiert:**
- `GET /twitch/api/v2/lurker-analysis`: Analysiert Viewer die via Chatters-API gesehen wurden (`seen_via_chatters_api = TRUE`) aber nie geschrieben haben. Gibt Lurker-Ratio (Lurker / Seen-Sample-Viewer), Durchschnittliche Sessions der Lurker, Konversionsrate (Viewer die erst lurken und dann aktiv werden, erkennbar daran dass `first_active_seen > first_lurk_seen`) und die Top-25 reinen Lurker nach Session-Anzahl zurück.

**Technisch:** Zwei Queries mit identischer Sessions-CTE (`started_at >= since`). Die erste Query nutzt `COUNT(*) FILTER (WHERE ...)` direkt in Postgres, statt Python-seitiger Listenschleifen. Bot-Exclusion via `NOT (chatter_login = ANY($3::text[]))`.

## #182 — Viewer-Profiles, Audience-Sharing + Audience-Insights nativ in Rust

**Ausgangslage:** Drei weitere Analytics-Endpoints liefen noch über den Fallback-Proxy nach Python 8765.

**Was wurde portiert:**
- `GET /twitch/api/v2/viewer-profiles`: Für jeden Chatter der Streamer-Basis wird global gezählt, auf wie vielen Streamern er auftaucht. Daraus entstehen fünf Segmente: `exclusive` (nur dieser Streamer), `loyalMulti` (2–3 Streamer), `casual` (Restmenge), `explorer` (≥8 Streamer), `passive` (≥3 Sessions aber 0 Nachrichten). Zwei Queries: eine CTE für die globale Exklusivitäts-Verteilung, eine separate für den Passive-Count.
- `GET /twitch/api/v2/audience-sharing`: Cross-Streamer-Overlap mit Jaccard-Ähnlichkeit, Inflow (Viewer die erst nach `since_date` beim anderen Streamer auftauchten) und Outflow (Viewer zuletzt vor `since_date` gesehen). Top-5-Partner erhalten zusätzlich eine Monats-Timeline der geteilten Viewer. `days`-Parameter 7–365, Standard 30.
- `GET /twitch/api/v2/audience-insights`: Vergleicht zwei aufeinanderfolgende Zeitfenster (`days` vs. `days×2` zurück). Berechnet Watch-Time-Trend (echter Durchschnitt aus `twitch_session_chatters.last_seen_at − first_message_at`, nur gültig wenn ≥25 Samples und ≥15 % Coverage) und Return-Rate (Anteil Viewer im Fenster, die in `twitch_chatter_rollup.first_seen_at` schon vor dem Fensterstart bekannt waren).

**Technisch:** Alle drei nutzen `NOT (chatter_login = ANY($n::text[]))` für Bot-Exclusion statt N+1-Loops. Watch-Time-Distribution verwendet `= ANY($n::bigint[])` für Session-IDs. Der `$1`-Parameter in `true_return_rate` wird in derselben Query zweimal referenziert (WHERE-Bedingung und JOIN-Bedingung) — Postgres erlaubt das nativ ohne doppeltes Binden.

## #181 — Raid-Retention + Raid-Analytics nativ in Rust

**Ausgangslage:** Die beiden Raid-Analytics-Endpoints liefen noch über den Proxy. Beide nutzen `recalculate_raid_chat_metrics`, eine Bulk-Berechnung von Chatter-Metriken pro Raid via Postgres `json_to_recordset`.

**Was wurde portiert:**
- `GET /twitch/api/v2/raid-retention`: Liest bis zu 100 Outgoing-Raids aus `twitch_raid_retention` und berechnet Retention (30m-Chatter vs. gesendete Viewer) + New-Chatter-Conversion live neu. Fallback auf stored Werte wenn kein `target_session_id` vorhanden. Gibt `dataAvailable: false` wenn keine Raids im Fenster.
- `GET /twitch/api/v2/raid-analytics`: Per-Source-Aggregation (avg_viewers, avg_new_chatters, avg_retention_30m, follows_attributed), Retention-Curves für die 50 neuesten Raids, Follow-Attribution (raid vs. organic via Session-Join + pre-session-Check), Incoming-Raids aus `twitch_raid_arrival_tracking` mit Boost- und Retention-Impact (Viewer-Timeline-Differenz). Incoming-Summary mit Best-Raider (höchster avg Boost-%).

**Technisch:** `recalculate_raid_chat_metrics` übergibt die Raid-Inputs als JSON-String an Postgres via `json_to_recordset` — drei CTEs für plus5m/15m/30m, known_from_raider und new_chatters. Python-Batch-Loop entfällt, da Postgres das Array selbst verarbeitet.

## #180 — Viewer-Directory, Viewer-Detail, Viewer-Segments nativ in Rust

**Ausgangslage:** Die drei Viewer-Profil-Endpoints liefen noch über den Proxy. Sie sind die komplexesten Analyse-Endpoints: mehrere Sub-Queries, Batch-Loops, Cross-Channel-Auswertung, Churn-Erkennung und Ingestion-Gap-Diagnostik.

**Was wurde portiert:**
- `GET /twitch/api/v2/viewer-directory`: Paginiertes Viewer-Verzeichnis mit Segment, Cross-Channel-Anzahl, Top-3-anderen-Kanälen und Window-Metadata (Presence + Roh-Chat). Filter-Typen: active/lurker/exclusive/shared/new/churned. Sort nach sessions/messages/last\_seen/other\_channels/first\_seen.
- `GET /twitch/api/v2/viewer-detail`: Einzelner Viewer — Activity-Timeline pro Tag, Cross-Channel-Präsenz mit Overlap-Richtung (before/after), Chat-Patterns (Peak-Stunden, aktivster Wochentag, Trend increasing/decreasing/stable). `personality` bleibt `null` (braucht `_classify_message`, Python-only).
- `GET /twitch/api/v2/viewer-segments`: Segment-Verteilung (dedicated/regular/casual/lurker/new), Churn-Risiko-Liste mit Whereabouts der Top-20-At-Risk-Viewer, Cross-Channel-Statistik (Exklusivitäts-%), Top-Shared-Channels mit Richtungsanalyse (incoming/outgoing/bidirectional).

**Technisch:** Python-Batch-Loops durch Postgres `= ANY($n)` ersetzt. `build_raw_chat_status` und `build_viewer_window_metadata` als async Rust-Helpers inlined.

## #179 — Retention-Curve, Loyalty-Curve, Viewer-Timeline nativ in Rust

**Ausgangslage:** Drei analytisch komplexe Endpoints liefen noch über den Python-Proxy — jeder davon holte Rohdaten aus der DB und machte Berechnungen serverseitig in Python.

**Was wurde portiert:**
- `GET /twitch/api/v2/retention-curve`: Pro Minute der letzten 50 Sessions wird `viewer_count / peak_viewers` normalisiert. Postgres `PERCENTILE_CONT(0.5/0.25/0.75)` aggregiert direkt auf DB-Seite — Python holte alle Rows und rechnete Quantile im Speicher. Drop-Events (>10 % Median-Rückgang) werden in Rust berechnet.
- `GET /twitch/api/v2/loyalty-curve`: Aus `twitch_chatter_rollup` wird gezählt, wie viele Chatter genau 1×, 2×, 3×, … aufgetaucht sind — gibt One-Time-Rate und Gesamt-Verteilung.
- `GET /twitch/api/v2/:streamer/viewer-timeline` + `/profile`: Berechnet Anwesenheits-Spans aus `twitch_viewer_presence_ticks` per Postgres `LAG()`-Window-Funktion (Gap > 2 Min = neuer Span). Viewer werden mit `_classify_viewer`-Logik (new/lurker/dedicated/regular/casual) basierend auf Sessions-per-Week und Msgs-per-Session klassifiziert — identisch zu Python portiert.

**Wie es jetzt funktioniert:** Alle fünf Endpoints antworten nativ aus Rust. Kein Proxy-Hop.

## #178 — Title-Performance + Ads-Schedule nativ in Rust

**Was wurde portiert:** `GET /twitch/api/v2/title-performance` aggregiert Stream-Titel aus `twitch_stream_sessions` nach Avg-Viewers, Retention-10m, Follower-Gain und Peak — sortiert nach Avg-Viewers. Keywords werden direkt in Rust extrahiert (Stop-Wort-Filter + 3+-Zeichen-Wörter, max 5 Keywords, identisch zu Python). `peerBenchmark` wird als `null` zurückgegeben (`_get_peer_group_stats` noch nicht portiert). `GET /twitch/api/v2/ads-schedule` liest die letzten 50 Snapshots aus `twitch_ads_schedule_snapshot` und gibt aktuellen Stand + 10-Einträge-Verlauf zurück.

**Wie es jetzt funktioniert:** Beide Endpoints antworten mit 200. Kein Proxy-Hop mehr.

## #177 — Tag-Analysis + Viewer-Overlap nativ in Rust

**Ausgangslage:** `GET /twitch/api/v2/tag-analysis` und `viewer-overlap` liefen über den Proxy. `tag-analysis` war in Python schon ein leerer Stub.

**Was wurde geändert:** `tag-analysis` gibt direkt `[]` zurück (Parität zum Python-Stub). `viewer-overlap` berechnet Jaccard-Overlap via `twitch_chatter_rollup` — Python hatte N+1-Queries für die Totals der Partner-Streamer; in Rust nutzen wir eine einzige CTE die alle Totals in einem Durchgang aggregiert. Bot-Exclusion (10 bekannte Bots) läuft auf beiden JOIN-Seiten und auf dem Total-Aggregat.

**Wie es jetzt funktioniert:** Beide Endpoints antworten mit 200. Wenn die Rollup-Tabelle keine Daten hat (neuer Streamer, noch kein Rollup-Job gelaufen), kommt `[]` zurück — identisch zu Python.

## #176 — Follower-Funnel nativ in Rust (tb-dashboard-api 8769)

**Ausgangslage:** `GET /twitch/api/v2/follower-funnel` lief über den Legacy-Proxy. Der Endpoint berechnet Follower-Conversion aus mehreren verknüpften Quellen und war wegen der Komplexität noch nicht portiert.

**Was wurde geändert:** Fünf SQL-Queries laufen jetzt direkt in Postgres: (1) Session-Aggregat (Anzahl, Dauer, Follower-Delta), (2) bot-bereinigte Chatter-Distinct-Counts (10 bekannte Bots ausgeschlossen, gleiche Liste wie chatter_tracking), (3) Follow-Events die tatsächlich während aktiver Streams stattfanden (JOIN auf Session-Zeitfenster), (4) Raid-Inflow (erfolgreiche eingehende Raids), (5) alles wird zu einem Confidence-Level kombiniert. Conversion-Rate nimmt echte Follow-Events als erste Quelle, fällt auf Follower-Delta-Summe zurück wenn keine Events vorliegen.

**Wie es jetzt funktioniert:** Endpoint antwortet mit 200 und dem vollständigen Payload inkl. `dataQuality`-Block.

## #175 — Rankings + Session-Detail-Endpoints nativ in Rust

**Ausgangslage:** `GET /twitch/api/v2/rankings` (Streamer-Rangliste) und `GET /twitch/api/v2/session/{id}` + `session/{id}/events` liefen noch über den Legacy-Proxy an Python 8765.

**Was wurde geändert:** `rankings` wird jetzt direkt in Postgres abgefragt — drei SQL-Varianten je nach `?metric=viewers|retention|growth`, jeweils als separate Query statt als String-Erweiterung, damit der Compiler jeden Bind-Parameter-Typ prüfen kann. `exclude_external=1` fügt `HAVING AVG(avg_viewers) <= 100` hinzu (Threshold wie in Python). `session/{id}` liest die Session-Row aus `twitch_stream_sessions`, ergänzt bot-bereinigte Chatter-Stats (10 bekannte Bots ausgeschlossen via NOT IN), Viewer-Timeline und Top-20-Chatters. Wenn `twitch_session_chatters` keine Daten für die Session hat, wird auf die aggregierten Werte aus der Session-Row zurückgefallen. `session/{id}/events` liefert Channel-Updates im Session-Zeitfenster aus `twitch_channel_updates`.

**Wie es jetzt funktioniert:** Alle drei Endpoints antworten mit 200. Partner-Isolierung ist aktiv: ein `DashboardAuthLevel::Partner`-Cookie sieht nur eigene Sessions. Nicht-existierende IDs geben 404 statt 500.

## #174 — Performance-Analytics-Endpoints nativ in Rust (tb-dashboard-api 8769)

**Ausgangslage:** Die vier Analytics-Endpoints `monthly-stats`, `weekly-stats`, `hourly-heatmap` und `calendar-heatmap` wurden über den Legacy-Proxy an Python 8765 weitergeleitet. Ein erster Port-Versuch scheiterte mit HTTP 500: sqlx übergab den Zeitstempel als formatierten String, Postgres verweigerte den Vergleich mit der `TIMESTAMPTZ`-Spalte (`operator does not exist: timestamp with time zone >= text`).

**Was wurde geändert:** Alle vier Handler lesen jetzt direkt aus `twitch_stream_sessions` in Postgres. Der Zeitstempel wird als `chrono::DateTime<Utc>` direkt an sqlx gebunden — ohne Umweg über einen formatierten String. Die Queries folgen zeilengenau dem Python-Original (`api_performance.py`): Monatsgrupierung mit Follower-Delta-Korrektur, Wochentags-Aggregation, Stunden-DOW-Heatmap, Kalender-Heatmap mit `DATE(started_at)`-Gruppierung.

**Wie es jetzt funktioniert:** Alle vier Endpoints liefern 200 mit echten Daten aus der DB. Auth-Parität: `DashboardAuthLevel::None` → 401, alles andere erlaubt. Streamer-Filter via optionalem `?streamer=`-Parameter.

## #173 — Kompletter `/streamers`-Baum nativ in Rust (tb-internal-api 8776)

**Ausgangslage:** Alle Streamer-CRUD-Routen (`GET/POST /streamers`, `DELETE /streamers/:login`, `verify`, `archive`, `discord-flag`, `discord-profile`) wurden trotz vollständiger Handler-Implementierung noch über den Legacy-Proxy an Python 8779 weitergeleitet. Grund war ein Axum-Eigenheit: nativer GET hätte POST auf demselben Pfad mit 405 statt Proxy-Fallback beantwortet — kein Teil-Flip möglich, solange POST nicht auch nativ war.

**Was wurde geändert:** Alle sechs CRUD-Methoden sind jetzt direkt im Rust-Router 8776 registriert. GET und POST auf `/streamers` liegen auf demselben Route-Eintrag (`get(list).post(add)`), womit das 405-Problem entfällt. Die Handler lesen/schreiben direkt in die DB über `tb-analytics::streamers_crud`. `chat-action` bleibt bewusst im Fallback-Proxy, weil es den live rotierten Bot-Token des Python-Chat-Prozesses braucht.

**Wie es jetzt funktioniert:** `verify` mit `mode=permanent/temp` → DB-Update + 200. `verify` mit `mode=clear/failed` → ehrlicher 503 (departnern mit Discord-DM ist noch nicht nativ portiert, Admin sieht den Fehler). `archive`, `discord-flag`, `discord-profile` → DB-Update, kein Discord-Nebeneffekt (der Flag in der DB steuert ob der Streamer im Server ist, die eigentliche Discord-Rollen-Sync läuft noch auf Python-Seite). Python 8779 ist jetzt nur noch für `chat-action`, Debug-Routen und EventSub-Requeue zuständig.

## #172 — Analytics-Dashboard nativ in Rust: SPA-Serving für `/analyse`

**Ausgangslage:** Die Route `/analyse` (Dashboard-HTML + alle statischen Assets wie JS/CSS/Icons) lief bislang über den Legacy-Proxy an Python 8765. Jedes Dashboard-Seitenaufruf war ein Umweg durch Python, obwohl die Auth-Logik und der Auth-Status schon nativ in Rust liefen.

**Was wurde geändert:** Der Rust-Service 8769 serviert `/analyse` und `/analyse/*` jetzt vollständig selbst. Der Handler liest `bot/analytics/dashboard_v2/dist/index.html`, ersetzt den Vite-internen Asset-Prefix (`/twitch/dashboard-v2/` → `/analyse/`), injiziert ein `<script>`-Tag mit dem Runtime-Config-Objekt (`apiBase`, `demoMode`, `allowedDemoProfiles`) direkt vor `</head>` — und gibt die fertige HTML-Seite aus. Assets (JS, CSS, SVG, Fonts) werden direkt aus dem `dist/`-Verzeichnis gelesen, mit MIME-Type-Mapping pro Extension.

**Sicherheit:** Jedes Pfad-Segment wird vor dem Dateizugriff einzeln geprüft — leere Segmente, `.`, `..` und Backslashes werden abgelehnt (Path-Traversal-Schutz, identisch zur Python-Implementierung). Der Dist-Pfad ist über die Env-Variable `DASHBOARD_V2_DIST_PATH` konfigurierbar. Auth-Flow: nicht eingeloggt → Redirect zum Login; Partner ohne Analytics-Freigabe → Redirect zum Legacy-Landing; Admin/Localhost → direkter Zugriff.

## #171 — Scam-Pitch: Nachricht löschen + Timeout bei Erkennung

**Ausgangslage:** Der Scam-Pitch-Detektor erkannte verdächtige Service-Angebote im Chat (Viewer-Kauf, Design-Spam, Account-Takeover-Verdacht) und sendete eine Warn-Nachricht — aber die ursprüngliche Spam-Nachricht blieb sichtbar, und bei einem `StrongTimeout`-Signal (wiederholter oder eskalierter Pitch) wurde kein Timeout ausgelöst. Das Ergebnis: Chat-Warnung erschien, der Spammer-Text stand weiter im Chat.

**Was wurde geändert:** Die Chat-Pipeline führt nach einem `PitchDecision::StrongTimeout`- oder `PublicWarn`-Signal jetzt aktiv Aktionen aus. Beim `StrongTimeout` (z. B. ein älterer Account, der verdächtig scammt — Account-Takeover-Verdacht) wird die Nachricht gelöscht und ein 10-Minuten-Timeout verhängt. Beim `PublicWarn` wird nur die Nachricht gelöscht. In beiden Fällen kommt ein Discord-Moderations-Alert mit passendem Titel. Der Eskalations-Pfad (User bereits gewarnt, sendet erneut `STRONG`) rief `timeout_user` bereits intern auf — der zweite Timeout-Call ist idempotent, Twitch setzt den Timer nur neu.

**Wie es jetzt funktioniert:** `StrongTimeout` → Nachricht weg + 10m Timeout + Discord-Alert „🛡️ Account-Takeover erkannt — Quarantäne (reversibel)". `PublicWarn` → Nachricht weg + Discord-Alert „⚠️ Scam-Pitch erkannt — Verwarnung". Mods und Broadcaster werden nie betroffen (Pre-Check unverändert).

## #170 — Caddy-Flip: alle Dashboard-Routen gehen jetzt über Rust (Strangler-Fig)

**Ausgangslage:** Caddy leitete den gesamten Twitch-Dashboard-Traffic (`/twitch/api/v2/*`, `/analyse`, etc.) direkt an Python 8765 weiter. Der Rust-Dienst auf Port 8769 war zwar gestartet, aber nur für Legal-Seiten und drei Public-v2-Routen im Caddy verdrahtet — der Rest wurde komplett umgangen.

**Was wurde geändert:** Zwei Änderungen in einem Schritt: (1) Der `auth-status`-Handler in Rust bekam vollständige Python-Parität — bisher lieferte er nur 2 Felder, jetzt alle 20 Felder (`partnerStatus`, `plan`, `access`, `permissions`, etc.) mit echter DB-Abfrage von `twitch_partners` und `streamer_plans`/`twitch_billing_subscriptions`. Plan-Katalog (Tier/Name/Entitlements) als statische Lookup-Tabelle in Rust; Partner-Zugangsstatus inklusive Grace-Period-Logik und Blacklist-Check. (2) Caddy-Flip: der `@public_twitch`-Block zeigt jetzt auf Rust 8769 statt Python 8765. Rust beantwortet nativ portierte Routen direkt; für alle anderen proxied es transparent zu Python 8765.

**Wie es jetzt funktioniert:** Jeder Browser-Request an das Dashboard geht über Rust. Nativ portierte Routen (Legal, Public-v2-API, auth-status, overview, admin-streamers) werden direkt beantwortet. Alles andere — Analytics, Streamer-Seite, Auth-Flow, Affiliate — wandert per Strangler-Proxy zu Python. Der Rollback ist eine Caddyfile-Zeile (Backup `Caddyfile.bak-public-twitch-strangler`). Ab sofort macht jede neu portierte Route sofort live, ohne Caddy-Änderung.

## #169 — Scout-Loop als nativer Rust-Hintergrundtask gebaut (deaktiviert)

**Ausgangslage:** Der Scout-Loop lief bisher in Python (`_scout_deadlock_channels`, ~235 Zeilen) und war für das automatische Entdecken live gehender Deadlock-Streamer zuständig. Er rief Helix GET /streams auf, filterte nach Sprache "de", und registrierte neue Streamer als `is_monitored_only=1` in `twitch_streamers`. Streamer die 2 Zyklen hintereinander offline waren, wurden wieder entfernt — inklusive offener Sessions und Live-State-Löschung.

**Was wurde geändert:** Der Scout-Loop ist jetzt vollständig in Rust als `tb-monitoring::scout` portiert. Architektur: `ScoutRepository` kapselt alle DB-Zugriffe (load, upsert, session-close, live-state-delete, sicheres delete mit `is_monitored_only=1`-Guard), `ScoutTask` hält den Absent-Cycle-Counter im Arbeitsspeicher als transiente `HashMap<String, u32>`. Der Counter ist bewusst nicht persistent — verloren beim Neustart heißt höchstens 2 Zyklen länger warten, kein Datenverlust. Gestartet mit `TB_SCOUT_ENABLED=1`, 30s Anlaufverzögerung, 90s Intervall zwischen Zyklen. Mehrere `TWITCH_LANGUAGE_FILTERS` werden als separate Helix-Requests ausgeführt.

**Wie es jetzt funktioniert:** Jeder Zyklus: Game-ID über Helix auflösen → Streams je Sprache holen → neue Logins in `twitch_streamers` mit `is_monitored_only=1` eintragen → Absent-Zähler hochzählen für alle die diesmal nicht live sind → bei ≥2 aufeinanderfolgenden Fehltreffen: Sessions schließen, Live-State löschen, DB-Zeile entfernen (Safety-Guard: nur `is_monitored_only=1`-Einträge werden gelöscht, Partner bleiben immer unberührt). Der Task ist standardmäßig deaktiviert.

## #168 — Clip-Fetcher als nativer Rust-Hintergrundtask gebaut (deaktiviert)

**Ausgangslage:** Der Clip-Fetcher lief bisher ausschließlich in Python als Discord-Cog — ein Konzept, das an den Discord-Bot-Lifecycle gebunden ist und keine klare Trennung zwischen HTTP-Aufrufen, Datenbankzugriffen und Scheduling kennt. Außerdem schrieben `clip_fetcher.py` und `clip_manager.py` Streamer ohne `is_monitored_only=1` in die DB, wodurch diese versehentlich als Discord-Link-Kandidaten auftauchten (wurde separat in #166 gefixt, Python-Seite bereits korrigiert).

**Was wurde geändert:** Neuer Crate `tb-social-media` mit vier klar getrennten Schichten: `repository` (sqlx-DB-Zugriff: aktive Partner lesen, Clip registrieren, FK-Streamer sicherstellen, Verlauf schreiben), `helix` (Twitch Helix GET /clips mit automatischer Pagination), `service` (Orchestrierung eines Fetch-Laufs: User-ID holen → Clips paginiert fetchen → DB-Writes → History), `task` (Tokio-Hintergrundtask mit 60s Initial-Delay und 6h Intervall). Der Task wird in `tb-bot` eingebunden, aber nur gestartet wenn `TB_CLIP_FETCHER_ENABLED=1` gesetzt ist — ohne dieses Flag bleibt er still, auch wenn der Code in Production deployt ist.

**Wie es jetzt funktioniert:** Beim Bot-Start prüft `ClipFetchTask::start_if_enabled()` das Env-Flag. Ist es nicht gesetzt, loggt der Bot einmalig "Task deaktiviert" und tut nichts weiter. Ist es gesetzt und ein Helix-Client verfügbar, startet ein Tokio-Spawn: 60s Wartezeit, dann alle 6h ein Komplettlauf über alle aktiven Partner aus `twitch_partners`. Jeder Streamer bekommt eine Sekunde Rate-Limit-Pause. Clips gehen in `twitch_clips_social_media` mit Status `pending`, Fehler und Erfolge landen in `clip_fetch_history`. `is_monitored_only=1` wird per ON CONFLICT-Backfill sichergestellt — der Bug aus #166 ist strukturell unmöglich.

## #167 — Dashboard-Login überlebt jetzt Bot-Neustarts

**Ausgangslage:** Nach jedem Update oder Neustart des Dashboard-Dienstes waren alle eingeloggten Streamer und Admins plötzlich ausgeloggt und mussten neu durch den Login. Der Grund saß tief: Die Login-Sessions werden verschlüsselt in der Datenbank abgelegt — der Schlüssel dafür stammte aber aus einem Code-Pfad, der nur unter Windows funktioniert. Auf dem Linux-Server griff stattdessen ein Notfall-Verhalten, das bei jedem Start einen **neuen Zufallsschlüssel** erzeugte. Damit waren alle vorher gespeicherten Sessions ab dem Moment des Neustarts unlesbar — für das System sahen sie aus wie „nie eingeloggt gewesen". Aufgefallen ist das nie, weil unlesbare Sessions still als „abgelaufen" behandelt wurden.

**Was wurde geändert:** Der Dienst bezieht den Verschlüsselungs-Schlüssel jetzt zuerst aus dem zentralen Secret-Tresor, der beim Start ohnehin in die Dienst-Umgebung geladen wird. Der alte Windows-Weg bleibt nur noch als Fallback bestehen.

**Wie es jetzt funktioniert:** Der Schlüssel ist über Neustarts hinweg stabil — bestehende Logins bleiben gültig, egal wie oft der Bot dahinter aktualisiert wird. Sessions verlängern sich wie gehabt bei Aktivität automatisch (Streamer-Dashboard 6 Stunden rollierend, Admin-Bereich 14 Tage). Einmalig wurden durch die Umstellung alle aktiven Logins zurückgesetzt — das war der letzte erzwungene Re-Login dieser Art.

## #166 — Discord-Link-Meldungen nur noch für echte Partner

**Ausgangslage:** Der automatische Discord-Namens-Matcher schickte für jeden Streamer in `twitch_streamers` eine "Kein Discord-Match"-Meldung, sobald kein passender Discord-Account gefunden wurde — also auch für Kanäle, die zufällig oder per Monitoring in die Tabelle gerutscht sind, ohne je Partner zu sein.

**Was wurde geändert:** Die Datenquelle des Matchers (`list_unlinked_streamers` / `list_unlinked`) filtert jetzt per `INNER JOIN twitch_partners` auf aktive Partner (nicht departnered, nicht admin-archived). Kanäle ohne Partnereintrag erscheinen gar nicht erst in der Kandidatenliste.

**Ergebnis:** Meldungen und Auto-Link-Versuche passieren ausschließlich für Streamer, die tatsächlich aktiv als Partner geführt werden. Rein gescrapte oder beigetretene Kanäle bleiben still.

## #165 — Partnerkriterium: nur noch OAuth zählt, archivierte Kanäle raus

**Ausgangslage:** Drei Kanäle (xoralle, yorganson, yqmaa) waren in der DB längst als archiviert markiert (`admin_archived_at` gesetzt), standen aber trotzdem noch als aktive Partner drin — weil die View `twitch_partners_all_state` das Archiv-Datum bei der Berechnung von `is_partner_active` schlicht ignorierte. Resultat: der GlobalBanSweep versuchte bei jedem Zyklus, in diesen Kanälen zu bannen, bekam 403 (Bot kein Mod), schrieb nichts in den Ledger, und startete beim nächsten Lauf von vorne. Gleichzeitig wurden Kanäle, die der Bot nur per Admin-Manualverifizierung eingetragen hatte (ohne dass der Streamer den Bot selbst autorisiert hat), genauso behandelt wie echte OAuth-Partner — obwohl der Bot dort keine Handlungsfähigkeit hat.

**Was wurde geändert:** Die View `twitch_partners_all_state` berechnet `is_partner_active` jetzt mit zwei zusätzlichen Bedingungen: `admin_archived_at IS NULL` (archivierte Kanäle fliegen raus) und `raid_bot_enabled = 1` (nur Kanäle, die aktiv OAuth-autorisiert haben, gelten als Partner). Das `manual_verified`-System bleibt als Datensatz erhalten, bestimmt aber nicht mehr die Partnerschaft. Einziger Weg in den aktiven Partnerstatus: Twitch-OAuth-Flow abschließen.

**Ergebnis:** GlobalBanSweep läuft ohne 403-Rauschen. Chat-Sub-Reconcile läuft jetzt über 23 statt 45 Kanäle — alle mit echtem OAuth-Grant und `channel:bot`-Scope. Kein Retry-Spam für Kanäle, bei denen der Bot sowieso nicht handlungsfähig ist.

## #164 — EventSub: Chat-Nachrichten erreichen Rust jetzt in Echtzeit

**Ausgangslage:** Seit dem Chat-Cutover (Welle B, 12.6.) liest Rust `channel.chat.message` und `channel.chat.notification` als Webhook-Subscriptions. Die bestehende Python-Bridge (8765) proxyt Twitch-Webhooks an Rust weiter — hatte aber diese beiden Typen nicht auf der Weiterleitungsliste. Die Bridge antwortete Twitch mit 204 und meldete intern „Dispatch completed", schickte die Nachricht aber nie an Rust durch. Einzige Kompensation war der 15-Sekunden-Poll-Loop: Chat-Events kamen mit bis zu 15 Sekunden Verzögerung an.

**Was wurde geändert:** Zwei parallele Fixes. Erstens: Die Bridge leitet `channel.chat.message` und `channel.chat.notification` jetzt explizit an Rust weiter (sofortiger Fix, kein Infrastruktur-Umbau). Zweitens: Neuer nativer EventSub-Webhook-Empfänger in Rust (`WebhookReceiver`, Port 8786, gatedt via `TWITCH_WEBHOOK_SECRET`-Env) — wenn aktiv, ersetzt er den Python-Bridge-Hop komplett durch direkten In-Process-Dispatch mit HMAC-SHA256-Signaturprüfung. Der Receiver antwortet bei Dispatch-Fehler mit 503, damit Twitch die Nachricht automatisch wiederholt.

**Wie es jetzt funktioniert:** Bridge leitet chat.message/notification weiter → Rust empfängt sie im bestehenden Dispatcher-Stack in Echtzeit statt mit 15s Verzögerung. Der native Receiver ist deployed, aber erst aktiv wenn Caddy auf Port 8786 umgeleitet und das Secret gesetzt ist — Rollout ohne Breaking Change.

## #163 — EventSub: 429- und 401-Fehler stumm schalten

**Ausgangslage:** Die Fehlerbehandlung beim EventSub-Subscription-Erstellen kannte nur einen Sonderfall: HTTP 403 (Bot gebannt / Kanal gesperrt). Alle anderen HTTP-Fehler — darunter 429 (Rate-Limit) und 401 (App-Token abgelaufen) — landeten im gleichen warn!-Zweig. Da der Chat-Reconcile-Loop alle 30 Minuten für alle ~48 aktiven Partner-Kanäle läuft, würde ein temporäres Rate-Limit oder ein kurzzeitig ungültiger App-Token dieselbe Spam-Flut im Discord auslösen wie der 403-Bug, der diese Audit-Runde ausgelöst hat.

**Was wurde geändert:** In `subscriptions.rs` wurden 429 und 401 als eigene Zweige aus dem Fehler-String (`"Helix-Status 429"` / `"Helix-Status 401"`) erkannt und auf `debug!` heruntergezogen. Beide Fehler sind transient: 429 wird beim nächsten Reconcile-Zyklus automatisch erneut versucht, 401 wird vom Token-Manager durch Token-Refresh behoben. Ein `warn!` mit vollständigem Fehler-String bleibt für alle unbekannten HTTP-Status erhalten.

**Wie es jetzt funktioniert:** Der Fehler-Klassifikations-Baum lautet: 403 → `perm_failed`-Set + einmaliges warn! (kein Retry bis Neustart) · 429 → debug! (Retry nächster Zyklus) · 401 → debug! (Token-Manager refresht) · alles andere → warn! mit Fehlerdetail.

## #162 — EventSub-Wartung: Stale-Cleanup + Core-Sub-Reconcile beim Start

**Ausgangslage:** Die Rust-Migration hatte zwei stille Lücken beim EventSub-Lifecycle. Erstens: `cleanup_stale()` existierte im Code, wurde aber nie aufgerufen — veraltete Twitch-Subscriptions für Partner die ausgetreten sind blieben dauerhaft bei Twitch liegen und fraßen Subscription-Slots. Zweitens: `ensure_core_subscriptions()` (stream.online, stream.offline, channel.update) wurde ebenfalls nie aufgerufen — Kanäle die nach der Rust-Migration neu hinzukamen hatten nur einen Teil ihrer Subs, weil Python die ursprünglichen angelegt hatte und Rust das einfach voraussetzte.

**Was wurde geändert:** Neuer Background-Task `subscription_maintenance_loop` startet direkt beim Bot-Start und läuft danach alle 6 Stunden. Er lädt alle aktiven Partner-IDs aus der DB, ruft `cleanup_stale()` auf (löscht Subs für nicht mehr aktive Kanäle), und ruft `ensure_core_subscriptions()` für alle aktiven Partner auf (erstellt fehlende stream.online/offline/channel.update-Subs nach).

**Beim ersten Start** wurden sofort 2 stale Subs gelöscht und mehrere fehlende Core-Subs für neu hinzugekommene Kanäle (certifiedtoeguzzler, jckydl, xoralle) angelegt.

## #161 — Chat-EventSub-Reconcile: `channel:bot`-Scope-Gate nachgerüstet (Python-Parität)

**Ausgangslage:** Der Rust-Chat-Reconciler versuchte alle 30 Minuten, `channel.chat.message`- und `channel.chat.notification`-Subscriptions für ALLE aktiven Partner-Kanäle anzulegen — ohne zu prüfen, ob der Streamer dem Bot überhaupt die `channel:bot`-Berechtigung erteilt hat. Twitch lehnt solche Subscription-Versuche mit HTTP 403 ab. Für xoralle und berserkkoo (kein `channel:bot`-Grant) erzeugte das alle 30 min konstante Warn-Logs und löste stündliche Discord-Alerts aus.

**Was war das Python-Verhalten:** `join_partner_channels()` jointe `twitch_raid_auth` per INNER JOIN und prüfte danach `"channel:bot" in scopes` — Kanäle ohne Grant wurden still übersprungen, nie erst versucht.

**Was wurde geändert:** Die Reconcile-Query joiniert jetzt `twitch_raid_auth` und filtert auf `scopes LIKE '%channel:bot%'` sowie `needs_reauth = FALSE` — exakt die Python-Logik. Nur Kanäle, die dem Bot die Chat-Berechtigung explizit erteilt haben, werden für Chat-Subscriptions berücksichtigt. Als zweite Sicherheitsschicht trägt der `SubscriptionManager` trotzdem aufgetretene 403-Fehler in ein In-Memory-`perm_failed`-Set ein, damit auch unerwartete Randfälle (Grant zwischenzeitlich widerrufen, Bot gebannt) nicht endlos retryed werden.

**Jetzt:** Startup und jeder weitere Reconcile laufen sauber durch — `ok=45 failed=0`, kein 403-Warn, kein Alert-Spam.

## #160 — Chat-Bot komplett auf Rust umgestellt (Welle B: Cutover vollzogen)

**Ausgangslage:** Der Twitch-Chat (Moderation, Spam-Filter, Commands, Promos, Chatter-Tracking) lief als letzter großer Brocken noch im Python-Worker — inklusive der Hoheit über das Bot-Token. Der Rust-Bot bediente bereits Monitoring, Raids und die interne API, musste für jede Chat-Aktion aber den Umweg über Python nehmen.

**Was wurde geändert:**

- Die komplette Chat-Verarbeitung läuft jetzt nativ im Rust-Bot. Statt einer dauerhaften WebSocket-Verbindung wie im alten Bot abonniert der neue Bot Chat-Nachrichten als Webhook-Events über die bestehende EventSub-Strecke — „einem Kanal beitreten" heißt jetzt: eine Subscription anlegen. Beim Umschalten wurden 46 Partner-Kanäle abonniert und alle Subscriptions von Twitch bestätigt.
- Die Nachrichten-Pipeline arbeitet jede eingehende Nachricht in derselben festen Reihenfolge ab wie vorher: eigene Nachrichten verwerfen → bekannte Service-Bots nur zählen → Kanal einordnen (Partner / nur beobachtet / Deadlock live?) → globale Bannliste prüfen → Scam-Anschreiben erkennen → Spam bewerten → verdächtige Discord-Links flaggen → Spaß-Antworten → Chatter zählen → Promos → Commands.
- Der Spam-Filter eskaliert exakt wie bisher: Konto jünger als 90 Tage und Erstnachricht zählen nur dann als Verdachtspunkte, wenn bereits ein hartes Signal (bekannte Spam-Domain oder -Marke) vorliegt — eine harmlose erste Nachricht eines neuen Zuschauers kann also weiterhin nicht zum Bann führen. Ab 3 Punkten wird gebannt, bei hartem Signal darunter nur gelöscht.
- Bans, Timeouts und Lösch-Aktionen laufen über einen einzigen gemeinsamen Pfad — egal ob Spam-Filter, globale Bannliste oder Sweep sie auslösen. Dadurch greifen Chat-Hinweis (sofern nicht per !silentban stumm geschaltet), Review-Protokoll und Discord-Alert an die Moderatoren überall identisch.
- Das Chatter-Tracking schreibt weiter dieselben Daten (wer chattet wann in welcher Session, Erstkontakt-Erkennung, Roh-Nachrichten für die Analytics) — inklusive des Gesundheits-Heartbeats, den das Admin-Dashboard zur Überwachung der Chat-Erfassung liest.
- Die Bot-Token-Verwaltung ist mit umgezogen: Der Rust-Bot validiert das Token beim Start, erneuert es bei Bedarf sofort über das Refresh-Token und hält es im 30-Minuten-Takt frisch. Der Wechsel passierte atomar — erst Python-Chat aus, dann Rust-Chat an — damit sich nie zwei Prozesse um dasselbe Token streiten.
- Die Begrüßung nach einer Partner-Autorisierung sendet jetzt direkt der Rust-Bot statt über den Python-Umweg.

**Wie es jetzt funktioniert:** Twitch liefert jede Chat-Nachricht als signiertes Webhook-Event, der Bot prüft die Signatur, arbeitet die Pipeline ab und handelt bei Treffern sofort (löschen, bannen, warnen, antworten). Alle 30 Minuten gleicht er die Kanal-Abonnements mit der Partnerliste ab — neue Partner werden so automatisch „betreten". Kanäle, die den Bot nicht autorisiert haben, werden sauber übersprungen statt Fehler zu werfen. Für Streamer und Zuschauer ändert sich am Verhalten nichts — Commands, Moderation und Promos funktionieren wie gewohnt, nur schneller und aus einem Guss.

**Bewusst noch beim Alten:** !title (Titel-Generator) und die Engagement-Funktionen folgen in einer eigenen Phase.

## #159 — Sicherheitsseite: Responsible Disclosure, Meldeformular + Opus-Analyse

**Ausgangslage:** Die `/twitch/sicherheit`-Seite endete mit einem knappen Einzeiler „Hinweise willkommen, bitte vertraulich melden". Kein klarer Rahmen, wer was darf, und kein direkter Meldeweg — wer eine Lücke gefunden hat, musste selbst herausfinden, wie er sie loswird.

**Was wurde geändert:** Zwei Ergänzungen in einem Zug.

Erstens ein vollständiger *Security Testing & Responsible Disclosure*-Abschnitt: White-Hat-Testing ist jetzt ausdrücklich erlaubt. Die Regeln stehen klar da — kein Schaden, keine Datenexfiltration, kein Social Engineering gegen Nutzer, dafür Pflicht zu einem reproduzierbaren Report. Wer sich daran hält, hat keine Konsequenzen zu befürchten; DoS, Backdoors und echte Exfiltration fallen ausdrücklich nicht darunter.

Zweitens ein eingebettetes Meldeformular direkt auf der Seite: Kurztitel, detaillierter Reproduktionsweg (Pflichtfeld, mindestens 100 Zeichen), optionaler Kontakt. Die Hinweisbox macht explizit klar, dass nur echte, selbst geprüfte Lücken erwartet werden — keine KI-Halluzinationen, keine Vermutungen ohne Eigenprüfung. Nach dem Absenden sendet der Rust-Handler (`tb-dashboard`, Port 8769) direkt über die Discord-HTTP-API eine DM an den Bot-Eigentümer — kein Umweg über Python-Cogs, kein Zwischen-Service.

Drittens ein lokaler Opus-Analyse-Flow: Sobald die Eingangs-DM raus ist, startet `tb-dashboard` im Hintergrund einen `tokio::spawn`-Task, der via `spawn_blocking` das Claude-CLI (`claude -p --model opus --dangerously-skip-permissions`) headless aufruft. Opus liest den Report, prüft den relevanten Code im Repo auf die beschriebene Lücke, bewertet Schweregrad und Echtheit, und committet einen Fix falls sicher möglich. Das Ergebnis kommt als zweite Discord-DM mit dem vollständigen Analysebericht. Die HTTP-Response geht sofort zurück — der Nutzer wartet nicht auf Opus.

## #158 — Spam-Bot-Bans werden jetzt im Live-Ban-Feed sichtbar

**Ausgangslage:** Die „Spam-Schutz in Echtzeit"-Sektion zog ihre Zahlen und den Feed aus einer Tabelle, die nur Bans erfasst, die über Twitch-Benachrichtigungen (EventSub) reinkommen — und die sind nur für sehr wenige Kanäle aktiv. Die eigentliche Kernleistung, das automatische Bannen von Viewer-Bot- und Spam-Accounts im Chat, wurde dort gar nicht protokolliert (nur in eine interne Logdatei). Deshalb zeigte der Feed praktisch nichts und „Bans heute" stand auf 0, obwohl der Bot laufend Spam-Bots entfernt.

**Was wurde geändert:** Jedes Mal, wenn die Auto-Moderation einen Account wegen Spam bannt, wird dieser Ban jetzt zusätzlich in der Statistik-Tabelle protokolliert — und die gesamte Historie wurde aus der vorhandenen Moderations-Logdatei rückwirkend nachgetragen. Der öffentliche Feed filtert außerdem sauber auf echte Bans (frühere Entsperrungen tauchen nicht mehr fälschlich als „Ban" auf).

**Wie es jetzt funktioniert:** Sobald der Bot eine Spam-Nachricht erkennt und den Account bannt, schreibt er nach dem erfolgreichen Bann einen Eintrag mit Account-Name, Spam-Inhalt und Zeitpunkt in die Ban-Ereignis-Tabelle — als zusätzlicher Schritt, der den eigentlichen Bann-Vorgang nie blockiert (schlägt das Protokollieren fehl, läuft die Moderation normal weiter). Der Bot hat seine Auto-Bans schon immer in einer Logdatei mitgeschrieben; daraus wurden die zurückliegenden Bans einmalig in die Statistik-Tabelle übernommen (idempotent, ohne Duplikate), sodass der Feed sofort die echte Historie zeigt statt erst über die Zeit zu füllen. Die öffentliche Statistik-Abfrage zählt und zeigt nur noch Einträge vom Typ „Ban". Im Feed steht jetzt der tatsächliche Spam-Text der gebannten Bots (z. B. die typischen „billige Viewer"-Werbungen). Zusammen mit der bereits korrigierten Zahl der geschützten Kanäle spiegelt die Sektion damit die tatsächliche Schutzleistung wider — aktuell rund 50 entfernte Spam-Bots in den letzten 30 Tagen über alle Partner-Kanäle.

## #157 — Streamer-Seite: Sicherheits-Sektion, Discord-Beitritt, Demo-Vorschau repariert, Ban-Zahl korrigiert

**Ausgangslage:** Auf der Streamer-Landingpage fehlte ein sichtbarer Hinweis darauf, wie sicher der Bot mit den Mod-Rechten umgeht. Einen echten Discord-Beitritts-Button gab es nirgends — die Community-Sektion erwähnte Discord nur. Die eingebettete Dashboard-Demo lud nicht (leerer, kaputter Rahmen), und die „Spam-Schutz"-Sektion zeigte unglaubwürdige Zahlen, u. a. „1 geschützter Kanal", obwohl der Bot viele Partner moderiert.

**Was wurde geändert:** Die Streamer-Seite hat eine neue Sicherheits-Sektion bekommen, die kurz und ehrlich zeigt, was der Bot darf und was nicht — mit Verweis auf das vollständige Sicherheitskonzept. Discord ist jetzt an zwei prominenten Stellen direkt beitretbar (eigener Block in der Community-Sektion und Button im Schluss-Aufruf). Die Demo-Vorschau wurde repariert, die Navigationsleiste überarbeitet, und die Zahl der geschützten Kanäle zeigt jetzt den echten Wert.

**Wie es jetzt funktioniert:** Die Demo lud nicht, weil die Sicherheitsrichtlinie der Seite das Einbetten der Demo-Adresse blockierte — die Freigabe für die eigene Domain hat gefehlt; sie ist jetzt ergänzt, die Vorschau lädt wieder. Die „geschützte Kanäle"-Zahl wurde bisher falsch aus den protokollierten Ban-Ereignissen abgeleitet (daher 1); sie kommt jetzt aus der Zahl der aktiv betreuten Partner-Kanäle (aktuell 47) — das ist die korrekte Bedeutung, denn der Bot schützt jeden Partner-Kanal, unabhängig davon, ob dort zuletzt etwas gebannt wurde. Diese öffentliche Statistik-Abfrage wird seit diesem Schritt nativ von der Rust-Schicht beantwortet. In der Navigationsleiste überlappten sich bei mittlerer Fensterbreite Logo, Menü und Buttons; sie klappt jetzt früher ins kompakte Menü und hat einen klareren Demo-Button.

## #156 — Stats-Leaderboard Slow-Query-Fix

**Problem:** Das Stats-Leaderboard im Admin-Dashboard brauchte bei jedem Aufruf 1,3–3,2 Sekunden statt unter 100 ms. Betraf alle drei Teilabfragen (Top-Streamer-Ranking, Stunden-Verteilung, Wochentag-Verteilung) gleichzeitig — also 9 langsame Queries pro Dashboard-Besuch.

**Ursache:** Die Abfragen liefen über ein `UNION ALL`-CTE-Muster (`WITH source_rows AS (SELECT ... FROM twitch_stats_tracked UNION ALL SELECT ... FROM twitch_stats_category)`). TimescaleDB erkennt in einem CTE keine echten Hypertable-Referenzen — es kann deshalb die 30-Tage-Grenze nicht für Chunk-Exclusion nutzen und scannt alle Chunks beider Tabellen komplett durch. Mit ~3500–3560 Ergebniszeilen und wachsendem Datenbestand wurde das zunehmend langsamer.

**Fix:** UNION ALL CTE entfernt. Jede Abfrage fragt jetzt direkt die zuständige Tabelle an (`twitch_stats_tracked` für Partner-Daten, `twitch_stats_category` für den Rest). TimescaleDB kann so den Zeitraum-Filter auf die relevanten Chunks (ca. 5 Stück bei 7-Tage-Interval) beschränken statt alle zu scannen. Zusätzlich neue DB-Migration mit `LOWER(streamer)`-Indizes auf beiden Tabellen, die beim nächsten Start automatisch angewendet wird.

## #155 — Streamer-Analytics repariert + drei Analytics-Routen nativ in Rust

**Ausgangslage:** Die Analytics-Ansicht im Streamer-Dashboard war für jeden Streamer mit Daten kaputt — und zwar schon seit der Umstellung der Datenbank auf Postgres: Die zentrale Session-Abfrage nutzte eine SQLite-Funktion (`TIME(...)`), die Postgres nicht kennt. Jede Anfrage endete mit „Internal error", das Dashboard zeigte dauerhaft „keine Daten". Zusätzlich standen drei interne Lese-Routen (/stats, Streamer-Analytics, Session-Detail) noch auf dem Python-Umweg, weil frühere Rust-Versuche die Antwort-Struktur nicht exakt trafen.

**Was wurde geändert:** Alle drei Routen sind jetzt shape-genau nativ in Rust — gebaut aus zeilengenauen Verträgen gegen den Python-Quelltext und gegen die laufende Python-API mit echten Produktionsdaten verglichen. Die Session-Detail-Route liest wie Python dynamisch alle Spalten (künftige Felder kommen automatisch mit). Die Stats-Route liefert exakt die Python-Sektionen inklusive Monetization (Werbepausen mit Viewer-Drop-Berechnung, Hype Trains, Bits, Subs). Beim Vergleich flogen zwei weitere schlafende Python-Bugs auf: Die Sub-Geschenk-Zählung verglich ein Ja/Nein-Feld mit einer Zahl (schlug still fehl, Subs waren immer 0 — in Rust korrekt umgesetzt), und der Verifikations-Status der Partner wurde in einem früheren Rust-Entwurf durch einen Typ-Fehler still verschluckt (alle Partner erschienen als unverifiziert — vor dem Release gefixt).

**Wie es jetzt funktioniert:** Streamer öffnen ihr Dashboard → die Analytics laden wieder echte Daten (Sessions, Retention, Chat-Aktivität, Vergleich mit ähnlichen Kanälen). Verifiziert wurde mit echten Produktions-Sessions: Session-Details sind feldgenau identisch zur Python-Antwort, die Stats-Aggregate ebenso; nur die Live-Zähler unterscheiden sich um die Sekunden zwischen zwei Abfragen.

## #154 — Sicherheitskonzept: Passwortmanager als Phishing-Schutz

**Ausgangslage:** Der Abschnitt zur menschlichen Schutzebene nannte den Passwortmanager bisher nur als Speicher für lange, einzigartige Passwörter. Sein vielleicht stärkster Effekt fehlte: Er ist selbst ein Phishing-Schutz.

**Was wurde geändert:** Die Seite `/twitch/sicherheit` erklärt jetzt, dass der Passwortmanager Zugangsdaten an die echte Adresse (Domain) bindet und auf gefälschten Seiten deshalb gar nicht erst ausfüllt.

**Wie es jetzt funktioniert:** Ein Passwortmanager merkt sich zu jedem Zugang die echte Domain und füllt das Passwort nur dort automatisch ein. Landet man über einen gefälschten Link auf einer täuschend echt aussehenden Betrugsseite, bleibt das Feld leer — die Adresse ist dem Manager unbekannt. Damit dort überhaupt ein Passwort hineingerät, müsste man es bewusst von Hand eintippen oder die falsche Seite aktiv freigeben. Genau dieses ausbleibende Auto-Ausfüllen wirkt als zweite Warnstufe: Es erzwingt einen aktiven, bewussten Schritt, statt dass die Anmeldung unbemerkt durchläuft.

## #153 — Sicherheitskonzept: Tresor statt .env, Server-Praktiken, menschliche Schutzebene

**Ausgangslage:** Das Sicherheitskonzept beschrieb den Secret-Manager nur knapp und ließ drei Dinge offen, nach denen ein sicherheitsbewusster Nutzer fragt: Wie genau werden Geheimnisse verwahrt (und warum nicht der übliche, riskante Weg über Klartext-Dateien), welche gängigen Server-Schutzpraktiken laufen, und wie ist die menschliche Ebene abgesichert — also die Zugänge des Betreibers selbst, die in der Praxis das häufigste Angriffsziel sind.

**Was wurde geändert:** Drei Bereiche der öffentlichen Seite `/twitch/sicherheit` wurden ausgebaut. Der Abschnitt zu Zugangsdaten erklärt jetzt, was eine .env-Datei ist, warum dieser verbreitete Weg riskant ist und dass wir stattdessen einen verschlüsselten Tresor nutzen. Beim Server kamen gängige Praktiken dazu. Und es gibt einen neuen Abschnitt zur menschlichen Schutzebene. Außerdem wird jetzt ehrlich eingeordnet, warum bei uns die gezielte Feld-Verschlüsselung wichtiger ist als eine pauschale Festplattenverschlüsselung.

**Wie es jetzt funktioniert:** Eine .env-Datei ist eine Textdatei, in der Geheimnisse im Klartext stehen und beim Start als Umgebungsvariablen übergeben werden — riskant, weil sie lesbar auf der Platte liegt, leicht in Backups oder die Versionsverwaltung rutscht und von anderen Prozessen mitlesbar ist. Stattdessen liegen alle Geheimnisse verschlüsselt in einem zentralen Tresor; im Klartext existieren sie nur flüchtig im Arbeitsspeicher des laufenden Dienstes und werden beim Start frisch geholt. Zur Datenbank wird klargestellt: Festplattenverschlüsselung schützt nur die ausgeschaltete Platte vor physischem Diebstahl — gegen einen kopierten Datenbank-Auszug oder ein verlorenes Backup hilft sie nicht. Genau dort greift die Feld-Verschlüsselung, die die sensibelsten Werte einzeln verschlüsselt (Schlüssel außerhalb der Datenbank). Bei den Server-Praktiken werden Dienste mit minimalen Rechten in getrennten Prozessen, durchgehende TLS-Verschlüsselung, automatische Neustarts mit Überlastschutz und regelmäßige Backups genannt — unter dem Leitprinzip Verteidigung in mehreren Schichten. Die neue menschliche Ebene beschreibt: einzigartige, zufällig erzeugte Passwörter im Passwortmanager (das Bot-Konto rund 40 Zeichen), Zwei-Faktor überall, E-Mail-Benachrichtigung bei jedem Login und bewusste Wachsamkeit gegen Social Engineering. Alle Texte synchron in Python-Fallback und Rust-Live-Modul, byte-identisch verifiziert.

## #152 — Sicherheitskonzept vertieft (Token-Verschlüsselung im Detail) und doppelter Checkout-Pfad entfernt

**Ausgangslage:** Das Sicherheitskonzept erklärte die Token-Verschlüsselung nur in einem Satz. Gerade dieser Punkt ist aber zentral, weil der Bot OAuth-Tokens der Streamer hält. Außerdem gab es zwei Code-Pfade, die eine Stripe-Bezahlsession bauen konnten: den echten (über die Bezahlseite, mit Pflicht-Häkchen zur AGB- und Widerrufs-Zustimmung) und eine ungenutzte JSON-Schnittstelle, der genau dieses Häkchen fehlte. Letztere wurde von keinem Frontend aufgerufen, war aber eine wartungsgefährliche Doppelung: Hätte je ein Bedienelement darauf umgestellt, wäre der Widerrufs-Verzicht stillschweigend weggefallen.

**Was wurde geändert:** Das öffentliche Sicherheitskonzept (`/twitch/sicherheit`) erklärt jetzt im Detail, wie die OAuth-Tokens geschützt sind, und zeigt ein echtes Format-Beispiel. Neue Abschnitte zu Zahlungsdaten, zur Echtheitsprüfung eingehender Twitch-Ereignisse und zum Server-Schutz kamen dazu. Die ungenutzte JSON-Checkout-Schnittstelle samt zugehöriger Hilfsverweise wurde vollständig entfernt — es gibt jetzt nur noch genau eine Stelle im Code, die eine Bezahlsession erzeugt, und die hat die Zustimmungs-Abfrage zwingend eingebaut.

**Wie es jetzt funktioniert:** Der vertiefte Abschnitt beschreibt die fünf Eigenschaften der Feldverschlüsselung: jeder Wert bekommt einen frischen Zufallswert (gleiche Tokens sehen verschlüsselt völlig verschieden aus), trägt ein Echtheitssiegel (jede Manipulation lässt die Entschlüsselung kontrolliert fehlschlagen), ist fest an Tabelle, Spalte und Twitch-Konto-ID gebunden (ein Token von Streamer A kann technisch nie als Token von Streamer B entschlüsselt werden, ein Refresh-Token nie als Access-Token), der Schlüssel liegt getrennt vom Datenbestand, und jeder Wert trägt eine Versions-Kennung für Schlüsselwechsel. Dazu ein Format-Beispiel mit Wegwerf-Schlüssel, damit man sieht, wie so ein Wert in der Datenbank aussieht — unlesbar ohne den Master-Schlüssel. Beim Bezahlen baut nun nur noch der Bezahlseiten-Pfad die Stripe-Session, mit dem Pflicht-Häkchen „Ich stimme den AGB zu und erkenne an, dass die Leistung sofort beginnt und mein Widerrufsrecht erlischt". Alle Sicherheitstexte sind synchron in Python-Fallback und Rust-Live-Modul gelandet (byte-identisch verifiziert).

## #151 — Re-Autorisierung setzt Partner-Einstellungen nicht mehr zurück (Python-Fix)

**Ausgangslage:** Der in #149 beschriebene Bug im Python-Pfad: Bei jeder Re-Autorisierung oder erneuten Partner-Aktivierung wurden Silent-Ban, Silent-Raid, die Live-Ping-Rolle und das Discord-Link-Pflicht-Flag still auf ihre Standardwerte zurückgesetzt. Ursache war das Aufräumen der alten Streamer-Tabelle: Die dort gedroppten Spalten liefern seitdem nur noch Festwerte, und der als Sicherheitsnetz gedachte Rückgriff auf den bestehenden Partner-Datensatz konnte nie greifen, weil ein Default-Wert ihn verdeckte.

**Was wurde geändert:** Die fünf betroffenen Felder greifen bei nicht explizit übergebenen Werten jetzt direkt auf den aktiven Partner-Datensatz zurück — exakt das Verhalten, das die Rust-Seite seit #149 hat. Ein neuer Regressionstest legt einen Partner mit gesetzten Einstellungen an, re-promotet ihn und prüft, dass alles erhalten bleibt.

**Wie es jetzt funktioniert:** Streamer autorisiert den Bot erneut → Partner-Status wird aufgefrischt (verifiziert, aktiv, Raid-Bot an) → alle individuellen Einstellungen bleiben unverändert stehen. Bereits zurückgesetzte Einstellungen aus dem Bug-Zeitfenster lassen sich nicht automatisch wiederherstellen — betroffene Partner müssen sie einmalig neu setzen.

## #150 — AGB klar auf den Twitch-Bot eingegrenzt, Server-Schutz im Sicherheitskonzept

**Ausgangslage:** Die frisch erweiterten AGB sprachen pauschal von den „digitalen Diensten der Deutschen Deadlock Community" — damit war unklar, ob sie auch für die anderen Angebote (Discord-Bots, Steam-Bot, Community-Server) gelten sollen. Und im Sicherheitskonzept fehlte die Server-Ebene: Wie die Maschine selbst geschützt ist, stand dort noch nicht.

**Was wurde geändert:** § 1 der AGB grenzt den Geltungsbereich jetzt ausdrücklich ein: Sie gelten ausschließlich für den Twitch-Bot samt zugehöriger Web-Dienste (Dashboard, Statistik-Seiten, Abo-Verwaltung) — die Discord-Bots, der Steam-Bot und der Community-Discord-Server sind explizit ausgenommen. Im Sicherheitskonzept gibt es einen neuen Abschnitt „Wie der Server geschützt wird".

**Wie es jetzt funktioniert:** Der neue Abschnitt erklärt die Schutzschichten des Servers: die doppelte Firewall (Rechenzentrums-Firewall bei IONOS vor dem Server, zweite Firewall auf dem Server selbst — offen ist nur das Minimum), Fernzugriff nur über ein privates VPN-Netz, Schutz vor SQL-Injection durch ausschließlich parametrisierte Datenbank-Abfragen (Nutzereingaben werden nie Teil des SQL-Befehls), Schutz vor eingeschleustem Browser-Code durch Ausgabe-Maskierung plus Content-Security-Policy und automatische Betriebssystem-Sicherheitsupdates inklusive kontrolliertem Neustart. Beide Änderungen sind synchron in der Python- und der Rust-Implementierung gelandet (byte-identisch verifiziert), live ausgeliefert wird weiter über Rust.

## #149 — Streamer-Autorisierung: Setup-Followups in Rust + Einstellungs-Wipe-Bug entdeckt

**Ausgangslage:** Der OAuth-Callback (Streamer autorisiert den Bot) war in Rust zwar fertig portiert, durfte aber nicht live gehen: Nach dem Token-Speichern startet das System Hintergrund-Schritte — Bot als Moderator einsetzen, Begrüßungsnachrichten in den Chat, Partner-Status setzen, Discord-Rolle vergeben, Trial-Zeitstempel — und die liefen nur im Python-Prozess. Ein nativer Callback hätte den Einmal-OAuth-Code verbrannt und diese Schritte verschluckt.

**Was wurde geändert:** Die kompletten Followups laufen jetzt nativ in Rust. Bei einer Erst-Autorisierung passiert in dieser Reihenfolge: (1) Partner-Status wird gesetzt bzw. reaktiviert (inkl. Übernahme der Discord-Verknüpfung, Anzeigename kommt über den Master-Broker direkt vom Discord-Bot), (2) der Erst-Login-Zeitstempel für die Trial-Logik wird einmalig festgehalten (Doppelausführung kann ihn nicht überschreiben), (3) der Bot setzt sich mit dem frischen Streamer-Token selbst als Moderator ein (war er es schon, wird das erkannt statt als Fehler behandelt), (4) drei Begrüßungsnachrichten gehen in den Chat — interim noch über den Python-Chat-Prozess geschickt, bis der Chat selbst auf Rust umzieht. Bei einer Re-Autorisierung mit Discord-Verknüpfung läuft nur der Partner-Sync. Jeder Schritt ist fehler-isoliert: Scheitert einer, laufen die übrigen trotzdem.

**Bug dabei gefunden und in Rust gefixt:** Der bisherige Python-Pfad setzt bei jeder Re-Autorisierung still die Partner-Einstellungen zurück — Silent-Ban/Silent-Raid auf aus, die Live-Ping-Rolle auf leer. Grund: Beim Aufräumen der Streamer-Tabelle wurden die Quell-Spalten durch Festwerte ersetzt, und die als Sicherheitsnetz gedachten Rückgriffe auf den bestehenden Partner-Datensatz greifen durch einen Default-Wert nie. Die Rust-Version bewahrt die bestehenden Einstellungen; der Python-Pfad (läuft noch fürs Streamer-Dashboard) hat den Bug weiterhin und wird separat gefixt.

**Verifikation:** 13 neue Datenbank-Tests gegen das echte Produktionsschema, Live-Vergleich aller gefahrlos testbaren Callback-Fehlerpfade gegen die Python-Antworten (inhaltlich identisch), Wiederholungsschutz (Idempotenz) live bestätigt. Die sofortige Offline-Erkennung nach der Autorisierung übernimmt der Rust-Poller innerhalb von ~15 Sekunden — der bisherige Sonderpfad dafür entfällt ersatzlos.

## #148 — Richtige AGB für den Bot, öffentliches Sicherheitskonzept und Legal-Seiten in Rust

**Ausgangslage:** Die AGB deckten bisher nur den Bezahl-Shop ab (Raid Boost, Analyse-Abos, Stripe-Checkout). Die eigentliche Bot-Nutzung — automatische Moderation, die kanalübergreifende Bannliste, Auto-Raids, KI-Funktionen, das kostenlose Partnerprogramm — war nirgends geregelt, obwohl genau das der Kern des Dienstes ist. Und wie das Projekt mit Sicherheit umgeht (Tokens, Mod-Rechte, Schutzmechanismen), konnte man nirgendwo nachlesen, obwohl der Bot in jedem Partner-Kanal Moderator-Rechte trägt.

**Was wurde geändert:** Die AGB wurden zu vollständigen Nutzungsbedingungen ausgebaut, wie man sie von großen Bots (Nightbot, StreamElements) kennt — 14 Paragraphen statt 10. Neu geregelt sind: die kostenlose Basisnutzung (Chat-Bot, Raid-Netzwerk, Moderation, Statistiken), die Teilnahme am Partnerprogramm inklusive OAuth-Autorisierung und deren Widerruf, die automatische Moderation mit der kanalübergreifenden Bannliste samt Einspruchsweg (Mail oder Discord, dann prüft ein Mensch), die KI-Funktionen, die Pflichten der Nutzer und die Haftung bei kostenlosen Leistungen. Dazu gibt es eine neue, frei zugängliche Seite: das Sicherheitskonzept unter `/twitch/sicherheit`. Beides ist im Website-Footer verlinkt. Unter der Haube wurden die kompletten Legal-Seiten zusätzlich in Rust neu geschrieben und live geschaltet.

**Wie es jetzt funktioniert:** Impressum, Datenschutz und AGB bleiben hinter dem bekannten Human-Gate (Cloudflare-Prüfung, 10-Minuten-Cookie) — das Sicherheitskonzept ist bewusst ohne Gate und für Suchmaschinen freigegeben, weil es gelesen werden soll. Es erklärt auf Konzeptebene, wie der Bot selbst abgesichert ist: getrennte Berechtigungsebenen (der Bot-Account hat keine Broadcaster-Rechte, mächtigere Aktionen brauchen die ausdrückliche Streamer-Autorisierung), verschlüsselte Token-Ablage, nur lokal erreichbare interne Dienste, Schutz gegen Prompt-Injection bei den KI-Funktionen, Fehlbann-Schutz im Spam-Filter und die laufende Überwachung mit stündlicher Log-Prüfung und täglichem Audit. Die Rust-Portierung liefert dabei byte-identische Seiten wie vorher der Python-Renderer — geprüft per direktem Vergleich beider Ausgaben — und übernimmt ab sofort den Live-Traffic für alle Legal-Pfade.

## #147 — !invite-Command in Rust, Auto-Erkennung entfernt

**Ausgangslage:** Der Bot hatte eine Regex-basierte Auto-Erkennung für Deadlock-Zugangsfragen und antwortete automatisch mit dem Discord-Invite-Link. Die Logik war zu breit: Das Wort "play" reichte als Zugangs-Signal aus, sodass normaler Kommentar-Text mit "play Deadlock" und einem Fragezeichen irgendwo im Satz die Antwort auslöste. Dazu lief die gesamte Logik noch in Python.

**Was wurde geändert:** Die automatische Regex-Erkennung wurde vollständig entfernt. Stattdessen gibt es jetzt einen expliziten `!invite`-Command. Wenn jemand `!invite` in den Chat schreibt, fragt Python den neuen Rust-Endpoint `POST /internal/twitch/v1/chat/command` an. Rust prüft dort in `twitch_live_state`, ob der Kanal gerade Deadlock streamt (`is_live = 1` und `last_game` enthält "deadlock"). Ist das der Fall, wird die Discord-Invite-URL geladen — zuerst streamer-spezifisch aus `twitch_streamer_invites`, dann als Fallback aus der Env-Var `PROMO_DISCORD_INVITE` — und als fertiger Reply-Text zurückgegeben. Python schickt ihn per IRC in den Chat.

**Wie es jetzt funktioniert:** `!invite` im Chat → Python schickt Request an Rust → Rust prüft ob Deadlock läuft → bei Ja: Invite-Link zurück, Python postet Antwort. Kein Deadlock live → keine Antwort, kein Spam. Pro User und Kanal gilt 1h Cooldown. Die gesamte Entscheidungslogik liegt in Rust.

## #146 — Discord-Link-Click: Typ-Mismatch bei clicked_at behoben

**Ausgangslage:** Jedes Mal wenn ein User auf den Discord-Live-Link klickte, schlug die Datenbank-Eintragung mit einem Typfehler fehl: Die Spalte `clicked_at` in der DB ist `TIMESTAMPTZ`, aber der Code schickte den Zeitstempel als formatierten Text-String. PostgreSQL macht keine implizite Text→Timestamp-Konvertierung, also scheiterte jeder Schreibversuch — der Klick wurde nicht gespeichert, der Nutzer bekam trotzdem ein `ok: true` zurück.

**Was wurde geändert:** Der Klick-Zeitstempel wird jetzt direkt als `DateTime<Utc>` an die Datenbank übergeben, nicht mehr als formatierter String. sqlx weiß, wie es einen nativen Zeitstempel-Typ korrekt an eine `TIMESTAMPTZ`-Spalte bindet. Gleichzeitig wurde das Test-Schema mit dem Prod-Schema synchronisiert (`TEXT` → `TIMESTAMPTZ`).

**Wie es jetzt funktioniert:** Jeder Discord-Link-Click landet sauber in der DB — der Zeitstempel wird typ-korrekt als Timestamp with time zone gespeichert, ohne Konvertierungsumweg über einen String.

## #145 — OAuth-Anmeldung fertig portiert + Systemsuche nach weiteren Halb-Portierungen: vier stille Lücken im Raid-Tracking geschlossen

**Ausgangslage:** Nach der #140-Korrektur stellte sich die Frage, ob es im Rust-Teil weitere Stellen gibt, die Arbeit versprechen, aber nicht (ganz) tun. Zwei Dinge wurden angegangen: Erstens die OAuth-Anmeldung der Streamer — deren Rust-Fassung brach bisher mitten im Ablauf ab, weil nach dem Token-Tausch der Schritt fehlte, der ermittelt, *wem* das frische Token gehört (die Twitch-Token-Antwort enthält keine Identität; dafür braucht es einen zweiten Abruf mit dem frischen Token). Zweitens ein systematischer Audit (84 unabhängige Prüf-Agenten, jeder Befund dreifach gegengeprüft) über den gesamten Rust-Baum nach genau solchen Mustern: Erfolgsmeldungen ohne Wirkung, Datenbanktyp-Abweichungen, eigenerfundene Antwortformate, zu schmale Schnittstellen.

**Was wurde geändert:**

- **OAuth-Anmeldung komplett:** Die gesamte Kette ist jetzt nativ — Code-Tausch bei Twitch, Identitäts-Abruf mit dem frischen Token, Falscher-Account-Prüfung (wenn Streamer X sich anmelden sollte, aber Account Y den Link benutzt → klare Fehlermeldung statt stillem Speichern unter falschem Namen), Berechtigungs-Prüfung gegen das erwartete Profil, verschlüsseltes Speichern der Tokens und die Erfolgsseite mit Weiterleitung. Dazu der Wiederholungsschutz: Ein Netz-Retry mit demselben Schlüssel bekommt die gespeicherte Antwort, statt den nur einmal gültigen Anmelde-Code ein zweites Mal zu verbrennen. Sieben Tests decken Erfolg, beide Mismatch-Arten, fremde Berechtigungen, Twitch-Ausfall und ungültigen State ab — jeweils mit der Prüfung, dass im Fehlerfall nichts gespeichert wird. Die Route bleibt vorerst bewusst beim alten System: Python startet nach dem Speichern Folgeaktionen (Bot als Moderator einsetzen, Chat-Begrüßung, sofortige Offline-Überwachung), die noch nicht portiert sind — und der echte Anmelde-Weg der Streamer läuft ohnehin über das Dashboard, nicht über diese interne Route.

- **Vier stille Lücken im Raid-Ankunfts-Tracking geschlossen (liefen LIVE):** Erstens wurden Zweit-Signale eines bereits erkannten Raids (z. B. wenn nach der Chat-Ankündigung auch das offizielle Raid-Ereignis eintrifft) komplett verworfen — die Signal-Liste und die „Raid wurde zurückgezogen"-Markierung in der Datenbank blieben dauerhaft unvollständig. Jetzt wird der jüngste passende Ankunfts-Eintrag (innerhalb von 10 Minuten) gefunden und fortgeschrieben. Zweitens gingen Raids, die NUR über die Chat-Ankündigung sichtbar wurden (ohne vorheriges offizielles Ereignis), vollständig verloren — jetzt werden sie 15 Sekunden lang auf Korrelation gewartet und danach als eigenständige Ankunft verbucht, exakt wie im alten System. Drittens wurde das Ergebnis der Score-Buchung bei bestätigten Partner-Raids stillschweigend weggeworfen — ein Datenbankfehler wäre unsichtbar geblieben; jetzt wird er protokolliert. Viertens fehlte die regelmäßige Komplett-Neuberechnung aller Partner-Scores (alle 5 Minuten im alten System) — Rust rechnete nur bei Online/Offline-Ereignissen; Partner mit verpassten Ereignissen veralteten dauerhaft. Der neue 5-Minuten-Durchlauf hat direkt beim ersten Lauf 52 Partner-Scores aktualisiert (live verifiziert).

- **Weitere Typ-Fallen gegen die echte Datenbank entschärft:** Mehrere Stellen lasen Ganzzahl-Spalten im falschen Bit-Format oder Text-Spalten als Zeitstempel — grüne Tests, aber Absturz beim echten Aufruf, weil die Testdatenbank andere Spaltentypen hatte als die echte. Die betroffenen Abfragen wurden korrigiert und die Testdatenbanken an die echten Typen angeglichen, inklusive eines Testaufbaus, der alte Tabellen verschleppte und deshalb neue Spalten nie bemerkte.

- **Zukünftiges Dashboard (noch nicht live) abgesichert:** Die öffentliche Raid-Liste zeigte auch fehlgeschlagene Raids (Filter fehlte) und 20 statt 10 Einträge — gefixt. Die Browser-Schutzregeln (CORS) standen auf „jede fremde Website darf zugreifen" — auch für Admin-Routen; jetzt gilt das wie im alten System nur für die drei öffentlichen Routen. Das Audit-Urteil zum restlichen Dashboard-Port ist ehrlich dokumentiert: Vor einem Umstieg fehlen die Partner-Anmeldung, rund 40 Routen und mehrere Antwortformate — das ist eingeplante Arbeit für die Dashboard-Welle, kein stiller Mangel mehr.

**Wie es jetzt funktioniert:** Das Raid-Ankunfts-Tracking erfasst wieder alle drei Signalwege (offizielles Ereignis, Chat-Ankündigung als Zweitsignal, Chat-Ankündigung allein), Partner-Scores bleiben auch ohne Ereignisse maximal 5 Minuten alt, und die OAuth-Anmeldung ist als vollständiger, getesteter Baustein bereit — der Umstieg ist ein Einzeiler, sobald die Folgeaktionen portiert sind. Der Audit hat zusätzlich bestätigt, was schon sauber war: Von 19 geprüften Verdachtsmomenten der schweren Kategorie waren 6 bereits durch die Tagesarbeit erledigt und 7 betrafen ausschließlich den noch nicht aktiven Dashboard-Port.

## #144 — Markt-Dominanz: Viewer- und Kanal-Dimension getrennt + Partner-Bestand

**Ausgangslage:** Die Seite zeigte nur die Viewer-Dimension. Dass z. B. 4 von 6 live geschalteten DE-Kanälen zum Netzwerk gehören (Kanal-Dominanz 67 %), war nirgends ablesbar — genauso wenig wie der Bestand: wie viele Partner überhaupt unter Vertrag sind und wie viele davon im Zeitraum wirklich gestreamt haben.

**Was wurde geändert:** Die Kennzahlen sind jetzt in zwei beschriftete Reihen aufgeteilt. „Viewer": Marktanteil live, Netzwerk-Viewer live, Markt-Viewer gesamt, Dominanz-Zeit. „Streamer": Partner unter Vertrag (aktive, ohne Opt-out/Pause), davon im Zeitraum mit Deadlock-Stream aktiv, live jetzt (Netzwerk-/Markt-Kanäle mit Prozent), Ø Kanal-Anteil über den Zeitraum. Dazu zwei neue Verlaufs-Charts: Kanal-Anteil des Netzwerks (wie viele der gleichzeitig live geschalteten Markt-Kanäle gehören uns — unabhängig von deren Größe) und die Live-Kanal-Verteilung Netzwerk vs. Rest. Die Kanal-Sicht zeigt die Netzwerk-Stärke fairer als die Viewer-Sicht, in der ein einzelner großer externer Kanal alles dominiert.

## #143 — Markt-Dominanz: 100-%-Phasen sichtbar + neue Dominanz-Zeit-Kennzahl

**Ausgangslage:** Die Mindestmarkt-Schwelle (20 Viewer) sollte nächtliche 0↔100-%-Sprünge glätten — hat dabei aber genau die wertvollsten Momente versteckt: die Phasen, in denen ausschließlich Netzwerk-Kanäle live sind und der Marktanteil real 100 % beträgt. Diese Zeiten tauchten weder in der Linie noch im Peak auf.

**Was wurde geändert:** Die Schwelle ist komplett raus — die Anteil-Linie zeigt jetzt jeden Mess-Zeitraum mit aktivem Markt, inklusive der 100-%-Phasen; die Einordnung (wie groß der Markt dabei war) liefert das Viewer-Panel direkt darunter. Die Peak-Karte wurde zur Dominanz-Karte: Sie zeigt, in wie viel Prozent der gemessenen Zeit das Netzwerk mindestens die Hälfte aller Markt-Viewer hielt, plus den Spitzenwert mit absoluten Zahlen (z. B. „100 % — 3 von 3 Viewern"), damit klein-aber-100 % von groß-und-40 % unterscheidbar bleibt.

## #142 — Markt-Dominanz: Chart in zwei lesbare Panels aufgeteilt

**Ausgangslage:** Marktanteils-Linie und Viewer-Flächen teilten sich ein Diagramm mit zwei Y-Achsen. Ergebnis: Die Prozent-Linie zerfiel durch die nächtlichen Mindestmarkt-Lücken in abgehackte Stücke, und die Viewer-Flächen waren neben einzelnen Tages-Spitzen kaum noch ablesbar.

**Was wurde geändert:** Das Diagramm ist jetzt zweigeteilt — oben der Marktanteil als durchgehende Fläche mit eigener Prozent-Skala (Phasen unter 20 Markt-Viewern werden überbrückt statt als Loch zu erscheinen, mit Hinweistext darunter), darunter die Viewer-Verteilung Netzwerk vs. Rest mit eigener Skala. Zusätzlich sind die Zeit-Buckets gröber (24 h → 15 min, 7 Tage → 2 h, 30 Tage → 6 h), was das Zickzack glättet und die Kurven ruhiger macht.

## #141 — Markt-Dominanz: DE-Erkennung jetzt über die Stream-Sprache statt über Tags

**Ausgangslage:** Die deutschsprachige Marktsicht erkannte Streams über die frei wählbaren Stream-Tags („Deutsch"/„German"). Das war zu grob: Internationale Streamer, die mehrsprachig taggen, rutschten in den DE-Markt, obwohl sie englisch streamen — und blähten dessen Viewer-Basis künstlich auf, sodass der eigene Anteil viel kleiner wirkte als er ist.

**Was wurde geändert:** Die Kategorie-Zeitreihe speichert ab sofort pro Stream die offizielle Stream-Sprache aus der Twitch-API (neue Spalte, der Poller schreibt sie bei jedem ~17-Sekunden-Tick mit). Die DE-Markt-Definition ist jetzt exakt dieselbe wie bei der Bot-Discovery: Stream-Sprache `de` — oder Partner, die zählen immer dazu. Für ältere Datenpunkte ohne Sprachinfo bleibt ein Fallback: vor dem 10.06.2026 zählen sie komplett (die Erhebung war damals selbst schon auf Deutsch gefiltert), für die zwei Tage danach gilt näherungsweise der Tag-Filter. Die Top-Stream-Tabelle zeigt jetzt den echten Sprachcode pro Stream.

**Wie es sich auswirkt:** Der mehrsprachig getaggte englische Stream mit 400 Viewern fällt aus dem DE-Markt raus — übrig bleibt der echte deutschsprachige Markt. Direkt nach der Umstellung: 12 von 29 DE-Viewern beim Netzwerk, also rund 41 % Marktanteil statt scheinbarer 2 %.

## #140 — Korrektur zu #137: Routen waren nie aktiv — jetzt wirklich nativ, geprüft gegen das Live-System

**Ausgangslage:** Eintrag #137 behauptete, zwölf interne API-Routen liefen nativ in Rust. Das stimmte nicht: Der Code lag zwar im Repo, war aber nie in das Programm eingebunden — die neuen Module waren nicht einkompiliert und keine einzige der zwölf Routen im Router registriert. Alles lief still weiter über den Python-Umweg, ohne dass es jemand merkte, weil der Fallback-Mechanismus genau dafür gebaut ist. Ein adversarialer Voll-Review (70 unabhängige Prüf-Agenten, jeder Befund von drei Skeptikern gegengeprüft) fand zusätzlich 19 bestätigte Abweichungen vom Python-Verhalten, einige davon auf Routen, die bereits live liefen.

**Was wurde geändert:**

- **Live-Routen repariert:** Die globale Bannliste normalisiert Eingaben jetzt wie das alte System (akzeptiert `@Name` und alternative Feldnamen, weist Ungültiges mit 400 statt kommentarlos ab) und liefert wieder die vollständige Antwort inkl. Login und Begründung. Der Health-Endpunkt gab Host, Port, Datenbankname und DB-Benutzer im Klartext aus — jetzt erscheinen sie wie im alten System ausschließlich als nicht zurückrechenbare Hashes (mit dem Original abgeglichen, Hash für Hash identisch).
- **Die 19 Review-Befunde behoben,** darunter: Der OAuth-Abschluss hätte Fehler als HTTP-Status gemeldet, obwohl die Auth-Oberfläche immer Status 200 mit dem Ergebnis im Inhalt erwartet. Eine leere Scope-Schutzliste hätte den Schutz komplett abgeschaltet statt alles zu sperren (fail-open statt fail-closed). Der Zufalls-Token für den OAuth-Schutz kam aus einer schwachen Eigenbau-Mischung statt aus dem Krypto-Generator des Betriebssystems. Vier Auswertungs-Abfragen wären beim ersten echten Aufruf mit einem Datenbankfehler geplatzt (Zahl mit Text verkettet — die Testdatenbank verzieh das, die echte nicht). Am gefährlichsten: Eine fehlgeschlagene Verifizierung hätte den Streamer dauerhaft **verifiziert** statt ihn zu entfernen, weil unbekannte Modi pauschal als „dauerhaft bestätigen" behandelt wurden — jetzt mutieren unbekannte und noch nicht portierbare Modi gar nichts und antworten ehrlich.
- **Wiederholungsschutz nachgebaut:** Das alte System dedupliziert kritische Schreib-Anfragen über einen mitgeschickten Schlüssel (gleiche Anfrage zweimal = einmal ausführen, zweite bekommt die gespeicherte Antwort; gleicher Schlüssel mit anderem Inhalt = Konflikt-Fehler; parallele Doppel-Anfrage wartet aufs Ergebnis der ersten). Dieser Mechanismus existiert jetzt als ein geteilter Baustein in Rust — exakt nach dem Python-Regelwerk inkl. 15-Minuten-Gedächtnis, Obergrenze und der Regel, dass Fehler nie gespeichert werden, damit ein erneuter Versuch wirklich neu ausgeführt wird.
- **Sieben Routen wirklich aktiviert** — jede einzelne vor der Freischaltung im laufenden Betrieb gegen die Python-Antwort verglichen, bis die Antworten byte-identisch waren: die Raid-Auth-Lesestrecke (Auth-Link erzeugen, Auth-Status, Block-Status, Link-Weiterleitung), die Liste aktiver Live-Ankündigungen, das Link-Klick-Logging (mit dem neuen Wiederholungsschutz) und die Vergleichs-Auswertung.
- **Bewusst NICHT aktiviert** (laufen unverändert über Python, nichts geht verloren): der OAuth-Abschluss selbst — die native Fassung ist am letzten Schritt unfertig und hätte beim Fehlschlag den einmalig gültigen Anmelde-Code verbrannt, womit die Streamer-Anmeldung kaputt statt nur langsamer gewesen wäre; der Anforderungs-Versand (braucht den Discord-Direktnachrichten-Versand, den nur Python kann); drei Auswertungs-Routen, deren Rust-Fassung beim Live-Vergleich eine andere Antwortstruktur lieferte als das Original.
- **Beim Live-Vergleich zwei schlafende Typ-Fehler gefunden:** Zwei Datenbankspalten haben in der echten Datenbank andere Typen als in der Testdatenbank — die Tests waren grün, die echte Route stürzte ab. Beides behoben und die Testdatenbank an die echten Typen angeglichen, damit diese Fehlerklasse künftig im Test auffällt.

**Wie es jetzt funktioniert:** Sieben weitere interne Routen beantwortet der Rust-Kern direkt — nachweislich mit identischen Antworten wie das alte System, verifiziert am echten Datenbestand. Alles, was der Rust-Kern noch nicht vollständig kann, läuft automatisch weiter über Python; es gibt keine halb portierten Pfade mehr, die Erfolg melden und nichts tun. Und als Konsequenz aus dem #137-Fehler gilt ab jetzt: „nativ" heißt erst dann nativ, wenn die Route im Router registriert ist und der Live-Vergleich gegen das alte System identische Antworten zeigt.

## #139 — Markt-Dominanz: deutschsprachige Sicht repariert und als Hauptsicht ausgebaut

**Ausgangslage:** Die frisch gebaute Markt-Dominanz-Seite hatte in der deutschsprachigen Sicht drei Fehler. Erstens zeigten die Live-Kennzahlen und die Top-Stream-Tabelle trotz DE-Auswahl die globale Kategorie (internationale Streams in der Liste). Zweitens fielen Partner ohne gesetzten „Deutsch"-Tag aus dem DE-Markt heraus, und die historischen Daten vor dem 10.06.2026 — die bei der Erhebung bereits sprachgefiltert waren — wurden durch den Tag-Filter größtenteils weggeworfen, obwohl Daten bis Oktober 2025 vorliegen. Drittens sprang die Anteils-Linie nachts wild zwischen 0 und 100 %, weil bei einem fast leeren Markt (2 von 2 Viewern) jeder Anteil bedeutungslos ist.

**Was wurde geändert:** Der DE-Markt ist jetzt einheitlich definiert als „Deutsch-Tag oder Partner" — in der Zeitreihe wie im Live-Block. Daten vor dem 10.06.2026 zählen im DE-Scope komplett (sie waren schon deutschsprachig erhoben), damit reicht die Historie nutzbar bis zum Datenbeginn im Oktober 2025 zurück; dafür gibt es neue Zeitraum-Optionen 180 Tage und „Alles". Live-Karten und Top-Stream-Tabelle respektieren den gewählten Scope. Anteils-Linie und Peak gelten nur noch ab einer Mindestmarktgröße von 20 Viewern — darunter zeigt das Chart eine ehrliche Lücke statt eines 100-%-Ausschlags, und der Peak nennt keinen Nachts-Mini-Markt mehr. Der Hinweis zur unvollständigen Vor-Cutover-Basis erscheint nur noch in der globalen Sicht, wo er tatsächlich gilt.

## #138 — Markt-Dominanz-Seite im Admin-Dashboard

**Ausgangslage:** Wie viel der Deadlock-Zuschauerschaft auf Twitch tatsächlich bei unseren Partner-Streamern läuft, war bislang nirgends sichtbar — die Daten lagen zwar längst in der Kategorie-Zeitreihe (der Poller schreibt seit Oktober alle ~17 Sekunden jeden Live-Stream der Kategorie samt Viewerzahl und Partner-Flag in die Datenbank), aber es gab weder eine Auswertung noch eine Visualisierung darüber.

**Was wurde geändert:** Das Admin-Dashboard hat unter Community eine neue Seite „Markt-Dominanz". Sie zeigt als Zeitverlauf ein gestapeltes Flächendiagramm (Viewer im Partner-Netzwerk vs. Rest der Kategorie) mit überlagerter Marktanteils-Linie in Prozent, dazu Kennzahlen-Karten (Marktanteil live, Live-Streams Partner/gesamt, Kategorie-Viewer, Peak-Anteil im Zeitraum) und eine Live-Tabelle der aktuellen Top-Streams mit Partner-Badge. Zeitraum ist wählbar (24 h / 7 / 30 / 90 Tage), und ein Scope-Schalter wechselt zwischen globaler Kategorie und der deutschsprachigen Teilmenge.

**Wie es funktioniert:** Die Berechnung läuft nativ im Rust-Worker als neuer interner Endpoint. Pro Zeit-Bucket (10 Minuten bis 1 Tag, automatisch je nach Zeitraum) werden die Viewer-Summen über die Poll-Ticks gemittelt — geteilt wird durch die Anzahl der Ticks im Bucket, damit das Ergebnis unabhängig von der Abtastrate stimmt. Marktanteil = Partner-Viewer geteilt durch Gesamt-Viewer des Buckets. Die deutschsprachige Sicht filtert über die Stream-Tags („Deutsch"/„German"), weil die Zeitreihe keine Sprachspalte führt — eine Näherung, aber Partner taggen durchgängig deutsch. Das Dashboard reicht die Admin-Anfrage nur durch (Admin-Login bzw. Localhost nötig); zwei Einordnungen zeigt die Seite ehrlich an: Daten vor dem 10.06.2026 enthalten nur die damals sprachgefilterte Teilmenge der Kategorie (der Kategorie-Poll lief bis dahin gefiltert), und der Peak-Anteil nennt immer die absoluten Viewerzahlen dazu, damit ein „100 % nachts bei 3 Viewern" nicht wie Dominanz aussieht.

## #137 — Raid-OAuth, Telemetrie und Analytics nativ in Rust (12 Routen, Welle B)

**Ausgangslage:** Trotz der bisherigen Portierungswellen liefen noch zwölf interne API-Routen über den Fallback-Proxy zu Python-8779 weiter: die komplette Raid-OAuth-Strecke (sechs Schritte von der Auth-URL-Generierung bis zum fertigen OAuth-Callback), zwei Telemetrie-Routen (Liste aktiver Live-Announcements, Link-Click-Logging), vier Analytics-Routen (Bot-Statistiken, Streamer-Einzelanalyse, Vergleichsauswertung, Session-Details). Außerdem fehlten der Verify-Route die befristete und löschbare Verifizierungs-Variante, und ArchiveMode kannte keine Toggle-Block-Variante — beide boten damit nicht die volle Python-Semantik.

**Was wurde geändert:** Alle zwölf Routen laufen jetzt direkt im Rust-Kern:

- **Raid-OAuth (6 Routen):** `GET /raid/auth-url`, `/raid/auth-state`, `/raid/block-state`, `/raid/go-url`, `POST /raid/requirements`, `/raid/oauth-callback` — der OAuth-Flow-Stack (`StateStore`, `AuthWriter`, `TwitchTokenClient`) wird per Composition-Root `raid_oauth_impl.rs` verdrahtet und über den Trait `RaidOAuthPort` mit den Handlern verbunden. Discord-Scope-Guard (Allowlist-Prüfung für Guild-/Channel-/Role-IDs) ist identisch zur Python-Implementierung eingebaut. Offener Punkt: Idempotenz-Layer für `requirements` + `oauth-callback` fehlt noch (OAuth-Codes sind Single-Use; ein zweiter Aufruf mit demselben Code schlägt beim Token-Exchange fehl statt eine gecachte Antwort zu liefern).

- **Telemetrie (2 Routen):** `GET /live/active-announcements` liest aktive Discord-Announcements direkt aus DB (JOIN auf State + Config, nur Zeilen mit Message- und Tracking-Token, alphabetisch sortiert). `POST /live/link-click` schreibt Link-Klicks in `twitch_link_clicks`; ein In-Memory-Idempotenz-Cache (15 min TTL, max 2000 Einträge, cleanup on write) verhindert Doppel-Writes bei Wiederholungsanfragen — Parität zu Pythons In-Process-Cache. `GET /debug/observability` und `/debug/chatters/:login` bleiben weiterhin proxied, da sie Live-Bot-Laufzeitstatus brauchen.

- **Analytics (4 Routen):** `GET /stats` (Bot-weite Kennzahlen), `/analytics/streamer/:login` (30-Tage-Statistiken + letzte Sessions), `/analytics/comparison` (Vergleichskategorien + Top-Streamer), `/sessions/:session_id` (Session-Detail) — alles direkte DB-Abfragen, kein Proxy-Umweg.

- **Verify-Modus:** `POST /streamers/:login/verify` unterstützt jetzt `mode: permanent | temp | clear`. `temp` setzt eine 30-Tage-Frist (`manual_verified_until = NOW()+30d`), `clear` löscht die Verifizierung, alles andere (inkl. leer/fehlt) verhält sich wie bisher als `permanent`.

- **ArchiveMode:** `parse()` ist jetzt infallibel — unbekannte Werte fallen auf `Toggle` durch statt 400 zu liefern (Python-Parität: unbekannte modi landen immer auf „toggle"). Neue Varianten `ToggleBlock` und `Toggle` ergänzen `Archive`, `Unarchive`, `Block`, `Unblock`.

**Wie es jetzt funktioniert:** Die zwölf Routen werden vom Rust-Kern direkt beantwortet; für Nutzer und Streamer ändert sich nichts Sichtbares. Der Proxy-Fallback zu Python-8779 wird damit ein weiteres Stück kleiner — die verbleibenden proxied Routen sind nur noch solche, die echten Live-Bot-Laufzeitstatus (Chat-Verbindung, In-Process-State) brauchen.

## #136 — Paritäts-Härtung der nativen Routen nach Voll-Review

**Ausgangslage:** Ein adversarialer Voll-Review des Rust-Umbaus (mehrere unabhängige Prüfer, jeder Befund gegengeprüft) hat die frisch nativisierten internen Routen Feld-für-Feld gegen das alte Python-Verhalten gestellt. Drei kleine, aber echte Abweichungen kamen zum Vorschein — nichts Kaputtes, aber Verhalten, das in Randfällen vom Original abwich.

**Was wurde geändert:** Die Self-Explainer-Discord-Log-Route gibt bei einem nicht lesbaren Anfrage-Body jetzt wieder exakt die alte Fehlerform zurück (klarer „ungültiges JSON"-Fehler mit Statuscode 400) statt der generischen Framework-Antwort, und sie behandelt eine leere oder Null-Kanal-ID jetzt wie früher als ungültige Eingabe statt sie durchzulassen. Die Liste der noch nicht mit Discord verknüpften Streamer (Quelle für den automatischen Namens-Abgleich) liefert bei einem kurzzeitigen Datenbank-Schluckauf jetzt wieder eine leere Liste statt eines harten Fehlers — genau wie das alte System, damit der Abgleich-Prozess robust bleibt und nicht abbricht.

**Wie es jetzt funktioniert:** Die drei Routen verhalten sich in genau diesen Randfällen wieder byte-gleich zum Python-Original. Für Nutzer und Streamer ändert sich nichts Sichtbares; es ist reine Angleichung an das bewährte Verhalten, abgesichert mit zusätzlichen Tests für die Fehlerfälle.

## #135 — Zwei weitere interne Routen nativ in Rust: Link-Kandidaten + Self-Explainer-Log

**Ausgangslage:** Im Zuge der Rust-Umstellung liefen noch mehrere interne API-Routen über die alte Python-Schicht (Fallback-Proxy). Zwei davon: die Liste der Streamer ohne Discord-Verknüpfung (Quelle für den automatischen Discord-Namens-Abgleich) und das Weiterreichen einer Self-Explainer-Antwort als Discord-Nachricht.

**Was wurde geändert:** Beide Routen laufen jetzt nativ im Rust-Kern. Die Link-Kandidaten-Liste liest direkt aus der Datenbank — identische Abfrage wie vorher, inklusive der Feinheiten: leerer Verknüpfungs-Eintrag zählt als unverknüpft, die Identitäts-Tabelle dient als zweite Wahrheitsquelle, archivierte Streamer fallen raus, Sortierung alphabetisch. Ein Paritäts-Detail wurde dabei gleich geradegezogen: das Feld für die Twitch-User-ID erscheint jetzt — wie im alten System — immer im Ergebnis (leer = `null`) statt bei fehlendem Wert ganz zu verschwinden, damit der Discord-Matcher nicht über ein fehlendes Feld stolpert. Die Self-Explainer-Log-Route ist ein reiner Weiterleiter: sie nimmt das fertige Discord-Embed entgegen und schickt es über den zentralen Nachrichten-Broker an Discord — mit demselben Dedup-Schlüssel-Verfahren wie zuvor (kanonisches JSON → Hash), sodass echte Wiederholungen derselben Frage+Antwort nicht doppelt gepostet werden. Beide mit Vertragstests gegen das alte Verhalten abgesichert.

**Wie es jetzt funktioniert:** Der automatische Discord-Namens-Abgleich und das Posten von Self-Explainer-Antworten laufen unverändert — nur über den Rust-Kern statt über die alte Python-Schicht. Für Nutzer und Streamer ändert sich nichts Sichtbares; der Proxy wird ein weiteres Stück kleiner.

## #134 — Raid-Sperrliste läuft jetzt nativ in Rust (Python-Proxy schrumpft)

**Ausgangslage:** Bei der schrittweisen Umstellung auf Rust laufen die Verwaltungs-Routen der Raid-Sperrliste — Eintragen, Entfernen, Prüfen und Auflisten der Kanäle, die nie angeraidet werden — noch über die alte Python-Schicht. Der Rust-Kern reicht diese Anfragen bislang über einen internen Seitenport an den Python-Worker durch (Übergangs-Klempnerei aus der „Strangler-Fig"-Migration, bei der nach und nach Route für Route von Python nach Rust wandert). Damit hängt ein Stück Admin-Funktion ohne sachlichen Grund weiter an Python.

**Was wurde geändert:** Die vier Sperrlisten-Routen sind jetzt direkt im Rust-Kern implementiert und greifen selbst auf die Datenbank zu — der Umweg über Python entfällt für sie. Damit das Verhalten exakt identisch bleibt, wurde der Datenbankzugriff mit demselben SQL nachgebaut (gleiche Antwortfelder, gleiche Reihenfolge, gleiche Standard-Begründung). Die Login-Normalisierung — die aus `@Name`, einem bloßen Kanalnamen oder einer kompletten `twitch.tv/...`-Profil-URL immer denselben kanonischen Namen macht und reservierte URL-Pfade (z. B. `/videos`) sowie fremde Hosts abweist — wurde als geteilter Baustein in die Domänenschicht gezogen, sodass Streamer-, Raid- und Sperrlisten-Routen dieselbe Logik nutzen statt je eine eigene Kopie. Abgesichert mit Vertragstests, die jedes Antwortfeld, beide Eingabe-Varianten, die Groß-/Kleinschreibung und die Fehlerantwort gegen das alte Verhalten prüfen.

**Wie es jetzt funktioniert:** Eintragen, Entfernen, Prüfen und Auflisten der Raid-Sperrliste beantwortet der Rust-Kern direkt — Eingaben werden wie zuvor auf den kanonischen Kanalnamen normalisiert, ungültige Eingaben mit derselben Fehlermeldung abgewiesen, die Liste kommt neuester Eintrag zuerst. Für Nutzer und Streamer ändert sich nichts Sichtbares; der Unterschied ist allein, dass eine weitere Funktion nicht mehr über die alte Python-Schicht läuft und der Proxy ein Stück kleiner wird.

## #133 — Performance- und Robustheits-Welle: schnellerer Score-Refresh, NULL-sichere Telemetrie, ehrliche Tests

**Ausgangslage:** Mehrere Befunde aus dem Audit ohne akute Außenwirkung, aber relevant für sauberen Dauerbetrieb. Der regelmäßige Score-Refresh lud bei jedem Lauf die komplette Stream-Historie aller Partner aus der Datenbank und warf den Großteil erst danach im Code weg — bei wachsender Tabelle unnötige Last. Beim Mitschreiben von Hype-Train-Ereignissen konnte, wenn der Startzeitpunkt eines Events nicht lesbar war, statt einer aktualisierten Zeile eine verwaiste Doppel-Zeile entstehen. Die Herkunfts-Einstufung eines eingehenden Raids („kennen wir den Sender per ID oder nur per Name?") wertete versehentlich den übergebenen Parameter statt des tatsächlichen Datenbankwerts aus. Und die Testbasis war doppelt unzuverlässig: Tests konnten ohne Datenbank still als „bestanden" durchlaufen, und bei wiederverwendeter Test-Datenbank blieben alte Tabellen stehen, sodass Tests gegen ein veraltetes Schema liefen.

**Was wurde geändert:** Der Score-Refresh filtert das 45-Tage-Fenster jetzt schon in der Datenbank-Abfrage (der bisherige Code-Filter bleibt als Sicherheitsnetz, das Ergebnis ist identisch). Das Produktions-Binary wird mit Link-Time-Optimierung gebaut (kleiner und schneller). Das Hype-Train-Update vergleicht den Startzeitpunkt jetzt NULL-sicher, sodass keine verwaisten Doppel-Zeilen mehr entstehen. Die Raid-Herkunfts-Einstufung nutzt den echten Datenbankwert und entspricht damit wieder dem alten System. Die Test-Helfer legen ihr Schema vor jedem Lauf frisch an (kein Verschleppen alter Tabellen), und es gibt jetzt eine durchgehende CI, die Build, Lints und die komplette Testsuite gegen eine echte Datenbank ausführt.

**Wie es jetzt funktioniert:** Der Score-Refresh überträgt nur noch die wirklich benötigten Daten; das Binary läuft etwas schlanker. Hype-Train-Statistiken bleiben sauber, Raid-Herkunft wird korrekt klassifiziert. Und „grün" in den Tests bedeutet künftig verlässlicher auch „wirklich geprüft", weil die Testdatenbank in der CI immer steht und jeder Testlauf mit frischem Schema startet.

## #132 — !dldc / !dlde: Discord-Link-Command

**Ausgangslage:** Zuschauer fragten in mehreren Partner-Kanälen nach dem Discord-Link des Streamers — bisher keine Bot-Antwort, Streamer musste manuell tippen oder hatte es nicht im Profil hinterlegt.

**Was wurde geändert:** Neuer Chat-Command `!dldc` (Alias `!dlde`) — gibt den für den jeweiligen Streamer bereits generierten Discord-Invite-Link aus. Die Lookup-Logik wurde als nativer Rust-Endpoint in die interne API gebaut (`GET /internal/twitch/v1/streamer/:login/discord-invite`), der direkt die `twitch_streamer_invites`-Tabelle liest. Python empfängt den Chat-Command und ruft nur noch diesen Endpunkt auf — keine Datenbanklogik in Python.

**Wie es funktioniert:** Wer `!dldc` oder `!dlde` tippt, bekommt sofort den hinterlegten Discord-Invite-Link des Streamers als Chat-Antwort. Ist für diesen Kanal noch kein Invite generiert worden, antwortet der Bot mit einem kurzen Hinweis. Fehlt der Eintrag, läuft der Command lautlos durch (kein Chat-Noise).

## #131 — Härtungswelle: Token-Erneuerung race-sicher, Helix-Zeitlimits, klare HTTP-Fehler

**Ausgangslage:** Drei robustheitskritische Befunde aus dem Audit plus eine Vorab-Korrektur. Erstens: Beim Erneuern eines Raid-Tokens konnte ein Wettlauf mit dem parallel laufenden Wartungsprozess dazu führen, dass mit einem bereits verbrauchten Refresh-Token erneuert wird — Twitch entwertet den alten Token bei Benutzung, also stirbt dann die ganze Token-Kette und der Partner wird grundlos zur Neu-Autorisierung aufgefordert. Zweitens: Die Aufrufe an die Twitch-API hatten kein Zeitlimit; ein hängender Twitch-Server konnte den gesamten Überwachungs-Durchlauf einfrieren. Drittens: Bei Fehlerantworten (z. B. Rate-Limit oder ungültige Zugangsdaten) parste der Code direkt das Antwort-JSON und meldete dann ein kryptisches „Feld fehlt" statt des echten HTTP-Status. Viertens (intern, noch nicht aktiv): die künftige Dashboard-Lese-API dekodierte mehrere Datenbankspalten im falschen Typ und wäre beim Scharfschalten sofort gebrochen.

**Was wurde geändert:** Die Token-Erneuerung liest jetzt unter einer prozessübergreifenden Sperre erst den frischesten Stand aus der Datenbank neu und erneuert nur dann — mit genau diesem Token; hat ein anderer Prozess in der Zwischenzeit schon erneuert, wird übersprungen. Abgesichert mit einem Test, der genau diesen Wettlauf nachstellt. Die Twitch-Aufrufe bekommen ein 10-Sekunden-Zeitlimit. Antworten werden zuerst auf ihren HTTP-Status geprüft und erst dann geparst — über einen gemeinsamen Helfer statt des an mehreren Stellen kopierten Musters; die Fehlermeldungen tragen den Status, niemals den Token-Wert. Die Dashboard-Typen sind an das echte Datenbankschema angeglichen, und die zugehörigen Tests laufen jetzt gegen dieselben Spaltentypen wie in der Produktion.

**Wie es jetzt funktioniert:** Die Token-Erneuerung kann sich nicht mehr selbst aussperren — grundlose Neu-Autorisierungs-Aufforderungen durch diesen Wettlauf fallen weg. Ein langsamer oder hängender Twitch-Server bremst den Bot nicht mehr aus, sondern läuft nach spätestens zehn Sekunden in einen sauberen Fehler. Und Betriebsfehler stehen im Log sofort als das da, was sie sind (echter Statuscode), statt als irreführender Parsing-Fehler.

## #130 — Auto-Raid gehört fest zur Partnerschaft: Abschalt-Befehl entfernt

**Ausgangslage:** Partner konnten ihren Auto-Raid per Chat- bzw. Discord-Befehl komplett abschalten. Das lief dem Grundgedanken des Partner-Netzwerks zuwider — gegenseitiges Anraiden beim Offline-Gehen ist genau der Mechanismus, von dem alle profitieren. Ein einzelner Abschalter nimmt nicht nur dem Bot, sondern dem ganzen Netz Reichweite weg.

**Was wurde geändert:** Die Abschalt-Befehle (samt ihrer Kurzformen) sind ersatzlos entfernt — sowohl im Twitch-Chat als auch als Discord-Befehl. Der Befehl zum Autorisieren/Einschalten bleibt unverändert: Wer den Bot noch nicht autorisiert hat, bekommt darüber weiterhin den OAuth-Link, und wer ihn früher abgeschaltet hatte, kann ihn damit jederzeit wieder anschalten. Geblockte Partner und solche mit abgelaufener Autorisierung raiden weiterhin nicht — das ist eine andere Mechanik (Moderation bzw. fehlende Berechtigung), kein abschaltbarer Schalter.

**Wie es jetzt funktioniert:** Sobald ein Partner den Bot autorisiert hat, raidet er beim Offline-Gehen automatisch den passenden anderen Partner — und das bleibt so. Es gibt keinen Selbst-Abschalter mehr; wer raus will, klärt das über einen Admin.

## #129 — Audit-Nachzieher: Raid-Ankunfts-Tracking repariert, Global-Bann von der Raid-Sperrliste getrennt

**Ausgangslage:** Ein zeilengenaues Vollaudit des auf Rust umgestellten Bot-Kerns hat mehrere Altlasten aufgedeckt; drei davon mit direkter Wirkung. Erstens: Das Mitschreiben jedes Raid-Ankunftsereignisses (wer nach einem Raid tatsächlich rübergekommen ist) schlug seit dem Umzug bei jedem Versuch fehl. Die Datenbankspalte für die laufende Nummer ist ein 32-Bit-Ganzzahltyp, der neue Kern erwartete beim Zurücklesen aber 64 Bit — die Datenbank wies den Schreibvorgang strikt ab. Der Fehler wurde an dieser Stelle obendrein stillschweigend verschluckt und tauchte in keinem Log auf, während die Schwester-Funktion daneben denselben Fehler korrekt meldete. Zweitens: Wurde ein Chatter global gebannt, landete sein Kanal fälschlich auch auf der Raid-Sperrliste — obwohl „global gebannter Chatter" und „Kanal, den wir nicht anraiden" zwei verschiedene Dinge sind, die schon im alten System getrennt waren. Drittens: Ein altes Komfort-Schlupfloch aus der Anfangszeit gab Admin-Rechte allein aufgrund einer lokalen Verbindung — hinter einem vorgelagerten Proxy wäre das gefährlich geworden.

**Was wurde geändert:** Der Schreibvorgang fürs Ankunfts-Tracking liefert die laufende Nummer jetzt im passenden Format zurück und schreibt wieder zuverlässig; schlägt ein Insert doch mal fehl, wird er sichtbar geloggt statt verschluckt. Der zugehörige Test läuft jetzt gegen denselben Spaltentyp wie die echte Datenbank, damit genau dieser Fehler nicht erneut unbemerkt durchrutscht. Der Global-Bann spiegelt nicht mehr in die Raid-Sperrliste — beide Listen werden wieder getrennt geführt. Das lokale Admin-Schlupfloch ist ersatzlos entfernt; Admin-Zugriff läuft nur noch über die echte Anmeldung per Token bzw. künftig die Discord-/Twitch-Identität.

**Wie es jetzt funktioniert:** Raid-Ankünfte werden wieder lückenlos erfasst, die Statistik dahinter stimmt wieder. Ein global gebannter Chatter sperrt keinen Kanal mehr versehentlich von Raids aus. Und wer sich nicht echt authentifiziert, bekommt keine Admin-Rechte mehr — unabhängig davon, von wo die Anfrage kommt.

## #128 — Discord-Rollen-Sync beim Speichern des Discord-Profils

**Ausgangslage:** Im Admin-Dashboard kann man für einen Partner eine Discord-User-ID hinterlegen. Die Streamer-Rolle im Discord-Server wurde aber nur bei der initialen Verifizierung vergeben — wer erst nachträglich verknüpft wurde oder dessen ID später gesetzt/korrigiert wurde, bekam die Rolle nie automatisch.

**Geändert:** Sobald im Dashboard eine Discord-ID gespeichert wird, löst der Bot jetzt unmittelbar `sync_streamer_role` aus. Wenn die Person bereits im Server ist, landet die Rolle sofort. Wenn nicht, passiert nichts (kein Fehler, kein Absturz).

**Jetzt:** Discord-Profil speichern = Rolle wird in einem Zug gesetzt, ohne dass ein zweiter manueller Schritt nötig ist.

## #127 — Raid-Priorität aus Plänen zählt wieder im Scoring

**Ausgangslage:** Wer einen Plan mit Raid-Priorität hat (Raid Boost, die Bundles oder „Alles drin"), soll bei der automatischen Raid-Ziel-Auswahl bevorzugt werden. Seit dem Umzug der Score-Berechnung auf den neuen Kern wurde aber nur noch das direkte Boost-Häkchen in der Datenbank geprüft — die Zuordnung „Plan → enthält Raid-Priorität" (inklusive manuell vergebener Pläne mit Ablaufdatum) war schlicht noch nicht mit umgezogen und im Code als offene Lücke vermerkt. Partner, deren Boost allein aus ihrem Plan kommt, wurden im Scoring wie Free-Nutzer behandelt.

**Was wurde geändert:** Die Plan-Zuordnung ist jetzt im neuen Kern nachgebaut, exakt nach derselben Tabelle wie im alten System: Boost gilt, wenn das Datenbank-Häkchen gesetzt ist ODER der Plan-Name auf einen Plan mit Raid-Priorität zeigt ODER ein manuell vergebener Plan mit Raid-Priorität aktiv (nicht abgelaufen) ist. Abgesichert mit Tests, die das Verhalten des alten Systems Punkt für Punkt nachstellen — inklusive Groß-/Kleinschreibung und der verschiedenen Datumsformate beim Ablaufdatum.

**Wie es jetzt funktioniert:** Beim regelmäßigen Score-Refresh bekommen alle Partner mit Raid-Priorität — egal ob per Häkchen, Plan oder manuellem Override — wieder ihren Boost-Multiplikator in der Raid-Ziel-Auswahl. Abgelaufene manuelle Pläne verlieren ihn automatisch.

## #126 — Stream-Reports scheitern nicht mehr an unsauberem KI-Ausgabeformat

**Ausgangslage:** Die automatische Nach-Stream-Analyse (der strukturierte Report nach Stream-Ende) schlug regelmäßig fehl — 15 Mal in den letzten Tagen. Das KI-Modell liefert das Ergebnis als JSON-Datenblock, hängt aber manchmal seinen „Denkprozess" in einem eigenen Block davor oder lässt ein überzähliges Komma vor einer schließenden Klammer stehen. Beides brachte den Parser zum Abbruch: Beim Denkblock griff die Extraktion den falschen Textabschnitt, beim Komma scheiterte das Einlesen — der Streamer bekam dann nur einen leeren Ersatz-Report („Bewertung: gemischt", keine Inhalte). Auch die Wiederholungsversuche scheiterten, weil sie denselben Text durch denselben Parser schickten. Nebenbefund aus demselben Audit: Ein Fehler im Hintergrund-Token-Refresh wurde mit leerer Fehlermeldung geloggt und war damit nicht diagnostizierbar.

**Was wurde geändert:** Vor der JSON-Extraktion werden Denkblöcke des Modells entfernt. Schlägt das Einlesen trotzdem fehl, läuft ein Reparatur-Versuch, der überzählige Kommas vor schließenden Klammern entfernt und es erneut probiert — echte Syntaxfehler werfen weiterhin sauber einen Fehler, nichts wird still geschluckt. Die Härtung greift an allen drei Stellen, die KI-Antworten einlesen (Report, Wortgruppen-Analyse, Kurz-Report). Der Token-Refresh loggt Fehler jetzt mit vollem Fehlertyp und Stacktrace.

**Wie es jetzt funktioniert:** Liefert das Modell sauberes JSON, ändert sich nichts. Liefert es einen Denkblock oder ein überzähliges Komma — die beiden mit Abstand häufigsten Fehlerbilder —, wird der Report trotzdem korrekt erzeugt statt durch den leeren Ersatz-Report ersetzt zu werden.

## #125 — Nachzieher zum Kern-Umzug: Admin-Aktionen und Discord-Verknüpfung repariert

**Ausgangslage:** Nach dem heutigen Umzug auf den neuen Bot-Kern steckten neben dem Onboarding-Fehler (#124) weitere stille Brüche in den Funktionen, die der neue Kern selbst beantwortet — dort greift das Auffangnetz aus #124 nämlich nicht. Ein systematischer Abgleich von altem und neuem Verhalten hat gefunden: Der Archiv-Knopf im Admin-Dashboard schlug fehl (der neue Kern kannte den „Umschalten"-Modus nicht, den das Dashboard standardmäßig sendet). Discord-Profil-Speichern und die automatische Discord-Verknüpfung liefen ins Leere — alte Aufrufer senden Feldnamen mit Unterstrichen, der neue Kern erwartete sie in Binnengroßschreibung, dadurch kamen alle Werte als „leer" an; gespeichert wurde nichts, das Mitglieds-Häkchen wurde dabei sogar jedes Mal entfernt, und die Antwort behauptete trotzdem „ok". Der Discord-Flag-Schalter gab einen Fehler. Chat-Nachrichten und Ankündigungen aus dem Dashboard scheiterten, weil der neue Kern eine beim Start eingefrorene Kopie des Bot-Logins benutzte — das eigentliche Chat-System erneuert diesen Login aber laufend, die Kopie war also nach kurzer Zeit tot. Verifizierungs-Modi (temporär, zurücksetzen) blieben wirkungslos. Und das Dashboard meldete bei jedem Start fälschlich „verschiedene Datenbanken", weil alter und neuer Kern zwei unterschiedliche Prüfsummen-Verfahren benutzten.

**Was wurde geändert:** Alle Streamer-Verwaltungs-Schreibwege beantwortet vorerst wieder der bewährte bisherige Dienst — der neue Kern reicht sie über das Auffangnetz aus #124 durch. Jede dieser Funktionen kehrt erst dann in den neuen Kern zurück, wenn ein Vertragstest beweist, dass sich Anfrage und Antwort exakt gleich verhalten. Die Datenbank-Prüfsumme berechnet der neue Kern jetzt mit genau demselben Verfahren wie das Dashboard, abgesichert durch Referenzwerte aus dem alten System.

**Wie es jetzt funktioniert:** Archivieren, Verifizieren, Discord-Flag, Discord-Profil und Chat-Aktionen laufen wieder über den Pfad, der das seit Jahren korrekt macht — inklusive aller Modi und mit dem stets frischen Bot-Login. Die automatische Discord-Verknüpfung speichert wieder echte Werte statt nichts. Der Datenbank-Fehlalarm beim Dashboard-Start ist weg; geprüft wird weiterhin, nur vergleichen jetzt beide Seiten dasselbe.

## #124 — Raid-Bot-Autorisierung im Onboarding repariert

**Ausgangslage:** Seit dem heutigen Umzug der Bot-Innereien auf den neuen Rust-Kern war die Raid-Bot-Autorisierung kaputt: Wer auf der Streamer-Seite bzw. im Onboarding auf „Raid-Bot verbinden" klickte, bekam statt der Twitch-Anmeldung nur die Fehlermeldung „Raid bot not initialized". Grund: Beim Umzug hat der neue Kern den internen Schnittstellen-Port komplett übernommen, dort aber nur die bereits portierten Funktionen angeboten — der komplette Autorisierungs-Ablauf (Login-Link erzeugen, Twitch-Rückmeldung verarbeiten), die Raid-Sperrliste und ein paar Statistik-Abfragen liefen ins Leere. Das Dashboard interpretierte die fehlende Antwort als „Raid-Bot nicht da".

**Was wurde geändert:** Der neue Kern hat jetzt ein Auffangnetz: Jede Anfrage an eine Funktion, die er selbst (noch) nicht kennt, reicht er unverändert an den bisherigen Dienst weiter, der dafür auf einem internen Nebenkanal weiterläuft. Antwort geht denselben Weg zurück — für das Dashboard sieht alles aus wie vorher.

**Wie es jetzt funktioniert:** Klick auf „Raid-Bot verbinden" → Dashboard fragt den neuen Kern → der erkennt „kenne ich nicht", reicht an den bisherigen Dienst weiter → der erzeugt den Twitch-Login-Link wie gewohnt. Sobald eine Funktion nativ im neuen Kern nachgebaut ist, beantwortet er sie automatisch selbst — das Auffangnetz greift dann nur noch für den Rest. Verifiziert: exakt der Klick, der heute Nachmittag mit Fehler abbrach, liefert jetzt wieder die Twitch-Anmeldeseite mit den korrekten Berechtigungen.

## #123 — !raid funktioniert jetzt auch direkt nach Stream-Ende

**Ausgangslage:** `!raid` hat mit „Du musst live sein" abgebrochen, sobald der Stream offline war — also genau in dem Moment, in dem man den Befehl am dringendsten braucht: Der Stream ist gerade zu Ende, der Auto-Raid hat aus irgendeinem Grund nicht gefeuert, und manuell nachsteuern ging nicht. Dabei startet der Auto-Raid selbst seinen Raid auch erst nach dem Offline-Gehen — Twitch lässt den Raid-Start im Nachlauf des Streams zu.

**Was wurde geändert:** Die harte Live-Sperre ist raus. Wenn Twitch den Kanal als offline meldet, nimmt der Bot jetzt den letzten bekannten Stream-Zustand als Grundlage (Spiel, Zuschauerzahl, Startzeit) und versucht den Raid ganz normal. Die Deadlock-Regel bleibt: `!raid` geht weiter nur, wenn der (letzte) Stream Deadlock war oder gerade erst von Deadlock auf Just Chatting gewechselt wurde.

**Wie es jetzt funktioniert:** `!raid` direkt nach Stream-Ende läuft durch die normale Ziel-Auswahl (Partner zuerst, sonst deutsche Deadlock-Streamer) und startet den Raid. Ist das Twitch-Fenster für Raids schon zu (zu lange offline), lehnt Twitch den Versuch ab und der Bot meldet die echte Fehlermeldung im Chat — statt vorher pauschal zu blocken. Nur wer noch nie gestreamt hat, bekommt weiterhin einen Hinweis, dass es keinen Stream gibt, von dem aus geraidet werden kann.

## #122 — Admin-Chat-Aktion: Versenden funktioniert jetzt wieder vollständig

**Ausgangslage:** Nach dem Fix des 503-Fehlers (#121) schlug das Versenden einer Nachricht immer noch fehl — Fehlermeldung: „Chat-Aktion für [login] konnte nicht gesendet werden". Das Dashboard läuft als eigenständiger Python-Prozess ohne lokalen Twitch-Chat-Bot, weshalb der Fallback-Pfad über die interne Rust-API geht. Der nötige Endpoint `POST /internal/twitch/v1/streamers/{login}/chat-action` war im Rust-Bot aber noch nicht implementiert — stand zwar im HTTP-Vertrag, war aber nicht als Handler gebaut.

**Was wurde geändert:** Der fehlende Endpoint wurde im Rust-Bot (`tb-internal-api`) implementiert. Er liest den Bot-Token und die Bot-User-ID aus Umgebungsvariablen (identisch zur Python-Logik), holt die `twitch_user_id` des Ziel-Streamers direkt aus der DB und sendet die Nachricht über die Twitch Helix API — mit dem Bot-Token des Chat-Accounts.

**Wie es jetzt funktioniert:** Drei Modi werden unterstützt: normaler Chat (`mode=chat`), `/me`-Aktion (`mode=action`, sendet `/me …` als Prefix) und Announcement (`mode=announcement`, nutzt `/helix/chat/announcements` mit wählbarer Farbe). Fehlt die `twitch_user_id` für einen Streamer in der DB, kommt ein 404. Antwortet Helix mit einem Fehler, wird dieser als 502 weitergereicht. Der neue Endpoint ist per Auth-Token + Loopback-Guard gesichert wie alle anderen internen Endpoints.

## #121 — Admin-Chat-Aktion: 503 bei kurzzeitigem DB-Fehler behoben

**Ausgangslage:** Das Absenden einer manuellen Chat-Nachricht an einen Partner-Kanal über das Admin-Dashboard schlug sporadisch mit einem 503-Fehler fehl. Logs zeigten: „Dashboard internal API unavailable (degraded mode). First failure in streamers_list: internal server error" — der Endpunkt lieferte immer dann keinen Response, wenn der interne Rust-API-Call auf `GET /streamers` kurzzeitig mit DB-Fehler (500) antwortete.

**Was wurde geändert:** Die Chat-Aktion hat die komplette Streamer-Liste über die interne API geladen, nur um drei Felder eines einzigen Streamers zu prüfen (`twitch_user_id`, `archived_at`, `manual_partner_opt_out`). Das ist durch einen direkten DB-Lookup gegen die `twitch_partners_all_state`-View ersetzt — keine API-Abhängigkeit mehr für diese Validierung.

**Wie es jetzt funktioniert:** Beim Absenden einer Chat-Nachricht liest der Handler direkt aus der DB. Transiente Fehler in der internen API blockieren den Ablauf nicht mehr. Schlägt die DB selbst fehl, erscheint eine klare Fehlermeldung im Dashboard statt eines nackten 503.

## #120 — Bot war blind für Partner mit nicht-deutscher Kanal-Sprache: Auto-Raid & Werbezitate gefixt

**Ausgangslage:** Ein Partner meldete, dass der Auto-Raid bei ihm nie feuert (manuell ging es) und Werbezitate in seinem Chat komplett ausbleiben. Die Spur in Logs und Datenbank: Der Bot hat seinen Stream über Wochen kein einziges Mal live „gesehen" — keine Stream-Session, keine Kategorie, keine Zuschauerzahlen, nie ein Live-Posting. Grund: Die Live-Abfrage bei Twitch lief mit Sprachfilter Deutsch, und der Partner hat seine Twitch-Kanal-Sprache auf Englisch gestellt. Das Push-Event „Kanal ist live" kam zwar an, enthält aber weder Kategorie noch Zuschauer — und alles, was darauf aufbaut, lief ins Leere: Der Auto-Raid wird am Stream-Ende nur ausgelöst, wenn die letzte Kategorie Deadlock war; bei „Kategorie unbekannt" greift die Sicherheitsregel und überspringt (24 Mal in Folge, als einziger Partner). Werbezitate brauchen Zuschauer-Baseline bzw. Chat-Aktivitätsdaten aus genau diesem Tracking — ohne Daten kein Trigger.

**Was wurde geändert:**

- **Partnerliste ohne Sprachfilter:** Die Live-Abfrage der eigenen, kuratierten Partnerliste läuft jetzt ohne Sprachfilter — wer Partner ist, wird getrackt, egal welche Kanal-Sprache bei Twitch eingestellt ist. Der Deutsch-Filter bleibt nur dort aktiv, wo er hingehört: bei der Entdeckung fremder Deadlock-Streams in der Kategorie-Suche.
- **Kategorie sofort beim Live-Gehen:** Zusätzlich holt der Bot beim „Kanal ist live"-Event jetzt sofort Titel und Kategorie per gezieltem Einzel-Lookup, statt auf den nächsten Abfrage-Takt zu warten. Damit ist die Kategorie ab Sekunde 1 bekannt — auch bei Streams, die kürzer sind als ein Abfrage-Intervall, und unabhängig von jedem Sprachfilter.
- **Nebenfix Kanal-Sprache:** Beim Kategorie/Titel-Änderungs-Event wurde die Kanal-Sprache aus einem falschen Feld der Twitch-Nachricht gelesen und blieb deshalb in der Statistik fast immer leer (1997 von 1998 Einträgen). Jetzt wird das richtige Feld gelesen.

**Wie es jetzt funktioniert:** Geht ein Partner live, kennt der Bot binnen Sekunden Titel und Kategorie; der laufende Abfrage-Takt ergänzt Zuschauerzahlen und legt die Stream-Session an — für alle Partner, egal ob ihr Kanal auf Deutsch, Englisch oder sonstwas steht. Damit funktionieren Auto-Raid am Stream-Ende, Werbezitate, Live-Postings und Stream-Statistiken jetzt auch für Partner mit nicht-deutscher Kanal-Sprache.

**Betroffen:** Alle Partner, deren Twitch-Kanal-Sprache nicht auf Deutsch steht — bei ihnen waren Auto-Raid, Werbezitate, Live-Postings und Statistiken bisher komplett tot, ohne dass es eine Fehlermeldung gab.

## #119 — Drei latente Robustheits-Lücken im Live-Bot geschlossen

**Ausgangslage:** Beim Rust-Port des Monitorings (#110–#116) sind drei Schwachstellen im laufenden Python-Bot aufgefallen, die bisher nur unter Race-/Fehlerbedingungen zuschlagen — also genau dann, wenn man es nicht sieht. Sie wurden zunächst nur im Rust-Code gelöst; jetzt sind sie auch im Live-Bot gefixt.

**Was wurde geändert:**

- **Doppel-Abschluss-Schutz für Sessions:** Stream-Ende kann von zwei Seiten gleichzeitig erkannt werden (Abfrage-Takt und Twitch-Offline-Event). Bisher konnte der zweite Abschluss die bereits berechneten Kennzahlen (Retention, Chatter, Dauer) überschreiben. Jetzt greift der Abschluss nur noch, wenn die Session wirklich noch offen ist — der Verlierer des Rennens räumt nur seinen Zwischenspeicher auf und lässt die korrekten Werte stehen.
- **Doppel-Anlage-Schutz für Sessions:** Das Anlegen einer neuen Session war nur durch einen Zwischenspeicher im Prozess geschützt — bei einem Race konnten zwei offene Sessions für denselben Streamer entstehen. Jetzt sichert eine Datenbank-Sperre pro Streamer plus eine Prüfung in derselben Transaktion ab: Existiert schon eine offene Session, wird deren ID übernommen statt eine zweite anzulegen.
- **Event-Warteschlange übersteht Fehler:** Der Hintergrund-Arbeiter der Event-Verarbeitung konnte bei einem Datenbank-Fehler im Abhol-Schritt still sterben — Events wären ab dann liegen geblieben, ohne dass es jemand merkt. Zusätzlich hätte eine kaputte gespeicherte Nachricht beim Aussortieren einen Absturz im Absturz ausgelöst. Beides abgefangen: Der Arbeiter loggt, wartet kurz und macht weiter; kaputte Nachrichten wandern sauber in die Aussortierten-Liste.

**Nebenbei:** Zwei Tests, die der Code-Entwicklung hinterherhingen (veraltete Attrappen nach Raid-Refactoring), wurden auf den aktuellen Stand gebracht — sie waren rot und hätten echte Regressionen verdeckt.

## #118 — Doppel-Abschluss-Schutz für Stream-Sessions (Rust-Monitoring)

**Problem:** Eine Stream-Session kann theoretisch von zwei Seiten gleichzeitig abgeschlossen werden — vom Abfrage-Takt und vom Twitch-Offline-Event. Ohne Schutz überschreibt der zweite Abschluss die bereits berechneten Kennzahlen (Retention, Chatter, Dauer) mit neuen, dann falschen Werten. Im Python-Original existiert diese Lücke bis heute.

**Geändert:** Der Abschluss im Rust-Monitoring greift jetzt nur noch, wenn die Session wirklich noch offen ist (Bedingung direkt im Datenbank-Update). Kommt ein zweiter Abschluss-Versuch, läuft er ins Leere, räumt nur seinen Zwischenspeicher auf und meldet sauber „war schon zu" — die ersten, korrekten Kennzahlen bleiben unangetastet.

## #117 — Chatter-Statistiken pro Stream waren seit Monaten 0 — gefixt

**Was war das Problem:** Beim Abschluss jeder Stream-Session zählt der Bot, wie viele verschiedene Chatter im Stream geschrieben haben, wie viele davon zum ersten Mal da waren und wie viele Wiederkehrer — diese drei Werte landen in den Session-Statistiken fürs Dashboard. Genau diese Zählung war kaputt: Sie benutzte eine SQL-Summen-Funktion auf einem Wahrheitswert-Feld (`SUM` über ein boolean), und die gibt es in Postgres schlicht nicht. Seit der Umstellung dieser Spalte auf echte Wahrheitswerte schlug die Abfrage bei **jedem** Session-Abschluss fehl. Tückisch daran: Der Fehler wurde intern abgefangen und nur als unauffällige Debug-Zeile geschluckt — nach außen sah alles normal aus, aber jede abgeschlossene Session wurde mit 0 einzigartigen, 0 neuen und 0 wiederkehrenden Chattern gespeichert.

**Was wurde geändert:** Die Zählung nutzt jetzt einen Filter-Ausdruck (`COUNT` mit Bedingung) statt der Summe — das ist die korrekte Postgres-Schreibweise für „zähle nur die Zeilen, bei denen das Flag gesetzt ist". Aufgefallen ist der Bug übrigens nicht im Betrieb, sondern bei der systematischen Verifikation der Datenbank-Zugriffe gegen das echte Schema im Rahmen interner Umbauarbeiten.

**Wie es jetzt funktioniert:** Ab sofort bekommt jede neu abgeschlossene Session wieder echte Chatter-Zahlen: Gesamtzahl der Chatter aus der Session-Chatter-Tabelle, Erstschreiber über das First-Time-Flag, Wiederkehrer als Differenz. **Betroffen:** Alle Sessions seit der Boolean-Umstellung der Chatter-Flags stehen mit 0er-Werten in der Datenbank — die Chat-Nachrichten selbst sind vollständig vorhanden, nur die verdichteten Kennzahlen am Session-Datensatz fehlen. Eine Rückrechnung der Altdaten wäre aus den vorhandenen Chatter-Einträgen möglich, ist aber bewusst nicht Teil dieses Fixes.

## #116 — Monitoring-Cutover vorbereitet (Schritt 4f) — Umschalten bleibt manuell

**Ausgangslage:** Alle Monitoring-Bausteine (#110–#115) existierten als Bibliotheks-Code mit Tests, aber nichts davon war im Rust-Bot-Prozess zusammengesteckt. Für den späteren Umstieg muss der komplette Kreislauf — Twitch abfragen, Zustände schreiben, Events verarbeiten, Discord posten — startklar verdrahtet sein, ohne versehentlich loszulaufen.

**Geändert:** Der Rust-Bot setzt jetzt beim Start alles zusammen: Twitch-API-Anbindung für Streams/Kategorien/Follower/VOD-Vorschau, Discord-Versand über die interne Bridge (inklusive Rückfall auf einen einfachen Link-Button, wenn der Tracking-Button beim Broker nicht verfügbar ist), Event-Verarbeitung und Abfrage-Schleife. **Entscheidend ist der Schutzschalter:** Die Schleife startet nur, wenn sie per Umgebungsvariable ausdrücklich eingeschaltet wird — standardmäßig bleibt der Python-Bot der einzige Schreiber, und der Rust-Prozess beantwortet nur den Event-Empfang. Der Umschaltplan steht jetzt als Checkliste in der internen Doku: welche Variablen zu setzen sind, in welcher Reihenfolge Python-Monitoring aus- und Rust eingeschaltet wird, wie man den Erfolg prüft — plus eine ehrliche Liste der sechs Stellen, die beim Umschalten bewusst noch ohne Nachfolger sind (Raid-Auslösung, Statistik-Anmeldungen für Neue, Rollen-Erstellung, Auto-Archivierung, Offline-Folgeaktionen, Partner-Rekrutierung) und in welcher späteren Phase jede davon ankommt.

**Damit ist Schritt 4 (Monitoring) code-komplett:** sechs Teilschritte, alle gegen eine Wegwerf-Datenbank bzw. Mock-Server getestet, das echte Datenbank-Schema read-only verifiziert. Das tatsächliche Umschalten passiert bewusst nicht automatisch, sondern im geplanten Wartungsfenster.

## #115 — Go-Live-Embeds in Rust (Schritt 4e)

**Ausgangslage:** Der letzte fehlende Monitoring-Baustein vor dem Umschalt-Schritt: die Discord-Ankündigungen. Geht ein Partner mit Deadlock live, postet der Bot ein konfigurierbares Embed mit Ping-Rolle und Tracking-Button; endet der Stream, wird dasselbe Posting zum „ist OFFLINE"-Embed mit VOD-Button umgebaut.

**Geändert:** Das komplette Announcement-System ist nachgebaut — in zwei sauber getrennten Teilen:

- **Template-Engine** (reine Logik, ohne Netz/Datenbank): Jeder Streamer kann sein Embed per gespeicherter Konfiguration anpassen — Titel-/Beschreibungs-/Footer-Vorlagen mit Platzhaltern wie `{channel}`, `{game}`, `{viewer_count}` oder `{uptime}`, Farben (auch als Hex), eigene Felder, Bild-Modi (Stream-Vorschau, eigenes Bild, keins) und Erwähnungs-Regeln. Unbekannte Platzhalter bleiben sichtbar stehen, Discord-Längenlimits werden eingehalten, und das Vorschaubild bekommt einen stabilen Cache-Buster, damit Discord beim erneuten Senden kein veraltetes Bild zeigt.
- **Broker-Versand:** Gesendet wird wie bisher über die interne Discord-Bridge mit deterministischem Idempotenz-Schlüssel — derselbe Inhalt kann nicht doppelt gepostet werden, auch wenn der Versand wiederholt wird. Schlägt das Senden fehl, merkt sich der Bot Tracking-Token und Render-Zeitpunkt und versucht es im nächsten Durchlauf mit identischem Token erneut. Das Stream-Ende editiert das Posting zum Offline-Embed (inkl. VOD-Vorschaubild des letzten Streams, sofern abrufbar) und hängt einen einfachen VOD-Link-Button an.

**Bewusste Grenze:** Das automatische *Anlegen* der „ist live"-Ping-Rollen braucht die Discord-Verbindung des Python-Bots — Rust verwendet die bereits hinterlegte Rollen-ID. Auch das ist im Cutover-Plan als offener Punkt notiert.

## #114 — EventSub-Subscription-Verwaltung in Rust (Schritt 4d, Teil 2)

**Ausgangslage:** Empfangen konnte Rust seit #113 — aber jemand muss Twitch auch sagen, welche Events es überhaupt schicken soll. Diese Anmeldungen (Subscriptions) leben bei Twitch und müssen pro Streamer angelegt, beim Start abgeglichen und bei departnerten Streamern wieder abgeräumt werden.

**Geändert:** Rust verwaltet jetzt die Kern-Anmeldungen des Monitorings selbst — live gegangen, offline gegangen, Titel-/Kategoriewechsel. Der Ablauf wie im Original: Geht ein Raid-fähiger Partner live, wird sofort seine Offline-Anmeldung sichergestellt (damit das Stream-Ende nicht verpasst wird); antwortet Twitch „existiert schon", zählt das als Erfolg statt als Fehler; ein Speicher im Prozess merkt sich bestehende Anmeldungen (beim Start aus dem Twitch-Bestand aufgebaut), damit nicht bei jedem Event neu angelegt wird. Das Aufräumen löscht gezielt nur Anmeldungen der eigenen Callback-Adresse, deren Ziel-Streamer nicht mehr aktiv ist — fremde Anmeldungen werden nie angefasst. Jede Offline-Anmeldung schreibt außerdem einen Kapazitäts-Schnappschuss fürs Admin-Dashboard.

**Bewusste Grenze:** Die Statistik-Anmeldungen (Bits, Subs, Bans, …) brauchen die verschlüsselten Streamer-Tokens aus dem Raid-Bereich — der kommt erst in einer späteren Phase. Bestehende Anmeldungen liefern unabhängig davon weiter; nur für *neue* Partner würde im Übergangsfenster niemand Statistik-Anmeldungen anlegen. Das ist als offener Punkt im Cutover-Plan festgehalten statt halbgar mitgebaut.

## #113 — EventSub-Empfang in Rust (Schritt 4d, Teil 1) + Exists-Check-Fix

**Ausgangslage:** Twitch-Events (live gegangen, offline, Titelwechsel, Bits, Subs, Follows, …) erreichen den Bot über die Dashboard-Bridge, die sie per HTTP an die interne API auf Port 8776 zustellt. Beim Cutover übernimmt Rust diesen Port — der Empfangsweg musste also vorher vollständig stehen.

**Geändert:** Die Rust-API hat jetzt denselben Zustell-Endpoint wie Python, mit identischem Vertrag (die Bridge merkt keinen Unterschied). Dahinter steckt die volle Verarbeitungskette:

- **Annahme:** Jede Nachricht wird über den persistenten Dedup-Speicher geprüft (10 Minuten Fenster) — Doppelzustellungen werden sofort als Duplikat beantwortet. Schlägt die Annahme fehl, wird der Dedup-Eintrag wieder freigegeben und die Bridge puffert den Event durable und versucht es erneut — es geht nichts verloren.
- **Kern-Events** (live/offline/Titelwechsel) wandern in die durable Warteschlange aus #110 und werden vom Worker verarbeitet: live → minimaler Live-State sofort (der Poll-Tick füllt den Rest), offline → Session abschließen + Live-State auf offline (mit 120-Sekunden-Drossel gegen Online/Offline-Flattern und Doppel-Trigger durch Polling), Titelwechsel → Protokoll-Eintrag + Live-State-Update. Jeder fachliche Effekt ist pro Nachricht exactly-once absichert (7-Tage-Gedächtnis) — auch wenn dieselbe Nachricht mehrfach verarbeitet wird, feuert z. B. der Go-Live-Trigger nur einmal.
- **Telemetrie-Events** (Bits, Subs/Gifts/Resubs, Follows, Werbepausen, Hype-Trains, Bans, Shoutouts, Erstnachrichten) werden direkt in die Statistik-Tabellen geschrieben, inklusive Zuordnung zur laufenden Session. Hype-Train-Enden aktualisieren das passende offene Begin-Event statt zu doppeln.
- **Raid-Events** gehen an einen Andockpunkt fürs Raid-Subsystem (kommt in einer späteren Phase) statt in der Warteschlange zu verhungern.

**Nebenbei gefunden und gefixt:** Der „Streamer hinzufügen"-Endpoint der Rust-API aus Schritt 3b hatte einen Typ-Fehler im Duplikats-Check — wer einen bereits vorhandenen Streamer anlegte, bekam einen 500er statt der sauberen „existiert schon"-Antwort. Aufgefallen, weil die Tests jetzt konsequent gegen eine echte Wegwerf-Datenbank laufen statt übersprungen zu werden.

## #112 — Poll-Loop in Rust (Schritt 4c)

**Ausgangslage:** Mit Fundament (#110) und Schreibkern (#111) fehlte der Taktgeber des Monitorings: die Schleife, die alle 15 Sekunden bei Twitch abfragt, wer von den getrackten Streamern live ist, und daraus alle Zustandsübergänge ableitet.

**Geändert:** Der komplette Poll-Durchlauf ist jetzt in Rust nachgebaut, in derselben Mechanik wie das Original: getrackte Streamer plus ein Kategorie-Sample (bis 400 Streams, mit Sprachfiltern und Cursor-Pagination) werden von der Twitch-API geholt, daraus entstehen die Übergänge — frisch live (Session anlegen), offline (Session abschließen samt Kennzahlen), Stream-Neustart (alte Session zu, neue auf), Spielwechsel (Transition-Protokoll). Dazu die Pflege-Kadenzen: Statistik-Samples pro Tick, alle 10 Ticks verwaiste Sessions schließen und abgelaufene Dedup-Einträge abräumen, höchstens alle 15 Minuten die Auto-Archiv-Prüfung (Partner ohne Deadlock-Stream seit 10 Tagen). Das Abfrage-Intervall bleibt zur Laufzeit verstellbar (5–3600 Sekunden, aus der Datenbank gelesen, ungültige Werte fallen auf 15 zurück).

**Wie es zusammenhält:** Alles mit Außenwirkung — Discord-Postings, EventSub-Anmeldungen, Raid-Bewertungs-Updates, Archivierungen — läuft über definierte Andockpunkte, die aktuell auf „tue nichts" stehen. Dadurch kann der Rust-Loop heute schon vollständig gegen eine Wegwerf-Datenbank getestet werden (Transitions über mehrere Ticks, Neustart-Erkennung, Kadenzen), ohne dass er irgendetwas nach außen sendet — die Andockpunkte werden in den nächsten Schritten (EventSub, Announcements, Cutover) einzeln scharfgeschaltet. Die Twitch-API-Anbindung selbst (Streams, Kategorie-Suche mit Cache, Follower-Zahl) ist gegen einen Mock-Server getestet.

## #111 — Monitoring-Write-Core in Rust (Schritt 4b) + zwei Prod-Befunde

**Ausgangslage:** Nach dem Idempotenz-Fundament (#110) fehlte der eigentliche Schreibkern des Monitorings: Wer ist live, welche Stream-Session läuft, Viewer-Verläufe, Statistik-Zeitreihen. Das ist der Teil, der pro Poll-Tick in die Datenbank schreibt.

**Geändert:** Der komplette Write-Core ist jetzt in Rust nachgebaut — Live-State (inkl. Drift-Schutz: wechselt ein Login die Twitch-User-ID, wird die alte Zeile vorher entfernt, Konflikt-Schlüssel bleibt die User-ID), Session-Lebenszyklus (Anlegen, Viewer-Samples mit laufendem Durchschnitt/Peak, Abschluss mit Retention-Kennzahlen bei 5/10/20 Minuten, größtem Zuschauer-Einbruch, Chatter-Zählung und Follower-Differenz), Auto-Aufräumen verwaister Sessions (Scout-Reste nach 24 h, eingeschlafene Sessions 1 h nach dem letzten Viewer-Datenpunkt) und die Statistik-Zeitreihen. Die experimentellen Analytics-Tabellen werden dünn mitgeschrieben, weil AI-Reports sie lesen.

**Beim Verifizieren des echten Datenbank-Schemas (read-only) kamen zwei Dinge ans Licht:**

- **Die Tabellen-Definitionen im Python-Code sind veraltet.** Die echte Datenbank führt für Sessions längst echte Zeitstempel-, Boolean- und Bigint-Spalten statt der im Code behaupteten Text-/Integer-Typen. Der Rust-Port bindet die echten Typen direkt; ein neuer Schema-Vertrags-Test schlägt künftig an, wenn Prod davon abweicht.
- **Ein stiller Prod-Bug:** Die Chatter-Zählung beim Session-Abschluss benutzt eine SQL-Funktion, die es für Boolean-Spalten nicht gibt (`SUM` auf einem Wahrheitswert). Seit der Umstellung dieser Spalten schlägt die Abfrage bei jedem Abschluss fehl, der Fehler wird nur als Debug-Zeile geschluckt — **jede Session wurde mit 0 einzigartigen/neuen/wiederkehrenden Chattern gespeichert**. Die Rust-Version zählt korrekt über einen Filter-Ausdruck.

**Außerdem bewusst robuster als das Original:** Das Anlegen einer Session ist jetzt datenbankseitig gegen Doppel-Einträge abgesichert (Sperre pro Login + Prüfung in derselben Transaktion) — Python verlässt sich nur auf einen In-Memory-Cache, was bei einem Race zwei offene Sessions erzeugen kann. Abgesichert mit 16 Tests gegen eine Wegwerf-Datenbank plus Kennzahlen-Unit-Tests.

## #110 — Idempotenz-Fundament fürs Monitoring (Rust Schritt 4a)

**Ausgangslage:** Schritt 4 des Rust-Umbaus portiert das Monitoring — den Teil des Bots, der weiß wer live ist, Sessions und Statistiken schreibt und Go-Live-Posts auslöst. Bevor irgendein Schreibpfad portiert werden kann, braucht es das Fundament, auf dem dort alles ruht: die Mechanik, die garantiert, dass jedes Twitch-Event genau einmal wirkt, auch wenn es doppelt ankommt oder die Verarbeitung mittendrin abstürzt.

**Geändert:** Neues Rust-Modul mit den zwei Bausteinen, die Python und Rust sich während der Migration über dieselben Tabellen teilen:

- **Guard-Store** (`eventsub_guard_state`): Ein „Claim" gewinnt genau dann, wenn für den Schlüssel kein aktiver Eintrag existiert — entschieden durch einen einzigen bedingten Datenbank-Upsert, also ohne Race-Fenster zwischen Prüfen und Schreiben. Damit werden doppelte Event-Zustellungen, Online/Offline-Flapping und doppelte fachliche Effekte (z. B. zwei Go-Live-Posts für denselben Stream-Start) unterdrückt.
- **Processing-Inbox** (`twitch_eventsub_processing_inbox` + Dead-Letter): Eingehende Events landen erst durable in einer Warteschlange; ein Worker leased fällige Aufträge (Lease 30 s, Batch 20), verarbeitet sie und wiederholt Fehlschläge mit wachsendem Abstand (1 s verdoppelnd, Cap 60 s). Nach 5 Versuchen wandert der Auftrag in die Dead-Letter-Tabelle und kann von dort per Knopfdruck zurück in die Queue. Das Leasing läuft über Row-Locks mit Skip — mehrere Worker (auch der Python-Prozess parallel) stehlen sich keine Aufträge.

**Bewusst anders als das Python-Original:** Die Müllabfuhr abgelaufener Guard-Einträge läuft nicht mehr bei jedem einzelnen Claim mit (unnötiger Schreib-Traffic pro Event), sondern als periodischer Sweep. Zwei latente Python-Schwächen sind nicht mitportiert: Ein Datenbankfehler beim Lease konnte den Verarbeitungs-Worker still sterben lassen, und ein kaputtes Payload-JSON hätte beim Dead-Letter-Alarm den Worker gecrasht — der Rust-Worker loggt, wartet und macht weiter. Außerdem entschieden: Der WebSocket-Fallback-Transport wird nicht portiert (Prod läuft nachweislich über Webhook), und die experimentellen Session-Tabellen werden dünn mitgeführt, weil AI-Reports sie lesen. Abgesichert mit 9 Tests gegen eine Wegwerf-Datenbank.

## #109 — EventSub stream.offline sofort nach Auth registrieren

**Problem:** Wenn ein Streamer die Raid-Auth abschloss während sein Stream bereits lief, wartete der Bot bis zu 45 Minuten auf den nächsten Polling-Tick, bevor die `stream.offline` EventSub-Subscription angelegt wurde. In diesem Fenster konnte der Bot ein Offline-Event komplett verpassen — passiert heute bei cheazycrust: Auth um 16:42, Subscription erst 17:27, erster Offline-Event dazwischen verpasst.

**Ursache:** `complete_setup_for_streamer()` erledigte Mod-Hinzufügen und Chat-Join, rief aber nie `_handle_stream_went_live()` auf. Diese Funktion — eigentlich für den Polling-Tick zuständig — ist der einzige Ort, der die Subscription registriert.

**Fix:** `PartnerSetupService` bekommt einen optionalen `stream_went_live_fn`-Callback. Direkt nach dem Auth-Setup (Mod + Chat + Willkommensnachricht) wird dieser aufgerufen und registriert die `stream.offline`-Subscription sofort. Der Callback ist idempotent — läuft der Streamer gerade nicht live, passiert schlimmstenfalls gar nichts.

## #108 — Streamer-CRUD-Endpoints (Rust Schritt 3b)

**Ausgangslage:** Nach Schritt 3a hatte das neue `tb-bot`-Binary die Global-Ban-Endpoints, aber noch kein Äquivalent für die Streamer-Verwaltung — Add, Remove, Verify, Archive/Block und Discord-Profil-Updates fehlten komplett.

**Geändert:** 7 neue Endpoints unter `/internal/twitch/v1/streamers/`:

- **`GET /streamers`** — Liste aller nicht-archivierten Streamer.
- **`POST /streamers`** — Streamer hinzufügen: holt die Twitch-User-ID über die Helix-API, prüft ob der Login schon aktiv ist, führt danach einen Upsert in `twitch_streamers` + `twitch_streamer_identities` durch. Ohne konfigurierten Twitch-Client → 503.
- **`DELETE /streamers/{login}`** — Departnern: setzt `archived_at = NOW()` auf dem aktiven Eintrag und löscht `twitch_live_state`. War der Login noch nie aktiv, wird der Eintrag direkt gelöscht.
- **`POST /streamers/{login}/verify`** — Setzt `manual_verified_permanent = 1` in `twitch_partners` wenn ein aktiver Eintrag vorhanden ist; 404 sonst.
- **`POST /streamers/{login}/archive`** — Archivieren/Blockieren per `mode`-Feld: `archive`/`unarchive` steuert `admin_archived_at`, `block`/`unblock` steuert `technical_pause_reason` + Opt-Out-Flag.
- **`POST /streamers/{login}/discord-flag`** — Setzt `is_on_discord` (Partner-Pfad via `twitch_partners`, Fallback via `twitch_streamers`).
- **`POST /streamers/{login}/discord-profile`** — Setzt `discord_user_id` + `discord_display_name` (gleicher Dual-Pfad).

Discord-Rollen-Sync und EventSub-Callbacks sind bewusst weggelassen (kommen mit Schritt 5/6). `HelixClient` wird beim Start aus den Env-Variablen `TWITCH_CLIENT_ID`/`TWITCH_CLIENT_SECRET` gebaut und als Extension im Router eingehängt.

---

## #107 — Internes API-Binary tb-bot (Rust Schritt 3a)

**Ausgangslage:** Die Python-interne API auf Port 8776 bedient alle CRUD-Operationen des Dashboards. Für den Rust-Umbau braucht es ein neues Binary das diesen Port übernehmen kann — zunächst nur mit den einfachsten, rein datenbankbasierten Endpoints.

**Geändert:** Neues Rust-Binary `tb-bot` auf `127.0.0.1:8776` mit neuem Crate `tb-internal-api`. Implementiert sind 5 Endpoints:

- **`GET /internal/twitch/v1/healthz`** — `{"ok": true, "service": "twitch-internal-api"}` mit DB-Schema-Fingerprint (SHA-256 über `information_schema.tables`) und Connection-Metadaten (Host, Port, DB, User — aus DSN geparst).
- **`POST /internal/twitch/v1/globalban/add`** — Upsert in `twitch_chatter_global_ban` + Einbahn-Spiegel in `twitch_raid_blacklist`, beides in einer Transaktion.
- **`POST /internal/twitch/v1/globalban/remove`** — Löscht aus `twitch_chatter_global_ban` und `twitch_chatter_global_ban_applied` (Sweep-Ledger), damit ein späterer Re-Add sauber über alle Kanäle ausrollt.
- **`GET /internal/twitch/v1/globalban/check?login=...`** — Boolean-Check (Login oder Chatter-ID).
- **`GET /internal/twitch/v1/globalban`** — Alle Einträge, neueste zuerst.

Auth: `X-Internal-Token`-Header + Loopback-Guard (Defense-in-Depth, da 127.0.0.1-Binding allein schon externe Verbindungen blockiert). healthz antwortet auch bei Fingerprint-Fehler immer 200.

**Wie es jetzt funktioniert:** `tb-bot` liegt als eigenes Binary neben `tb-dashboard` im Workspace. Schritt 3b (Streamer-CRUD mit Twitch-API-Lookup) folgt als nächster Schritt.

---

## #106 — Admin-Streamer-Endpoints (Rust Schritt 2b)

**Ausgangslage:** Das Rust-Dashboard konnte nach Schritt 2a zwar den Systemzustand abfragen, hatte aber noch keinen Ersatz für die Python-seitigen Admin-Streamer-Übersichten — keine Möglichkeit, alle Partner nach Status zu filtern oder einen einzelnen Streamer vollständig abzufragen.

**Geändert:** Zwei neue Admin-only-Endpoints unter `/twitch/api/admin/streamers/`:

- **`GET /twitch/api/admin/streamers?view=<view>`** — gibt alle Streamer eines bestimmten Views zurück: `active`, `archived`, `departnered`, `blocked`, `non_partner`, `token_error` oder `all`. Ungültiger View → 400 mit Liste der erlaubten Werte.
- **`GET /twitch/api/admin/streamers/{login}`** — vollständige Detail-Ansicht eines Streamers mit Settings, OAuth-Zustand (inkl. Scope-Diff gegen die 7 Required-Scopes), Stream-Stats und letzten 10 Sessions.

Beide Endpoints berechnen den OAuth-Status (`connected/partial/reauth/missing`) und den logischen Partner-Status (`active/archived/departnered/blocked/non_partner/token_error`) lokal aus den DB-Feldern — exakt dieselbe Logik wie bisher in Python.

**Wie es jetzt funktioniert:** Die Query nutzt 5 CTEs (Billing, Partner-State mit ROW_NUMBER-Dedup, Live-State mit User-ID-bevorzugtem JOIN, OAuth mit Reauth-Flag, letzte Stream-Session). Der WHERE-Zweig für den jeweiligen View ist als statischer String im Code hinterlegt und wird per `format!` eingesetzt — kein dynamischer SQL-Wert, kein Injection-Risiko. Zusätzlich wurde `ApiError::not_found()` und `ApiError::bad_request_with_body()` in `tb-http-core` ergänzt.

---

## #105 — Admin-Session: 5-Minuten-Cap beim Cross-Dashboard-Login entfernt

**Ausgangslage:** Wenn man sich am Discord-Dashboard einloggte und dann das Twitch-Dashboard öffnete, holte der Twitch-Bot die Session per internem API-Call vom Discord-Bot — soweit korrekt. Das Problem: Die lokal gecachte Session wurde dabei mit `expires_at = now + 300` gespeichert (hardcoded 5 Minuten), obwohl die eigentliche Session im Discord-Bot noch 2 Wochen gültig war. Nach 5 Minuten galt die lokale Kopie als abgelaufen; Route-Handler sahen keine gültige Session mehr und verhielten sich inkonsistent, während Caddys Forward-Auth (der parallel re-validiert) noch 200 zurückgab.

**Geändert:** Der 300-Sekunden-Hardcode in `_fetch_discord_dashboard_session` wurde durch `_discord_admin_session_ttl` ersetzt. Die gecachte Synth-Session läuft jetzt genauso lang wie eine lokal erstellte Admin-Session.

**Wie es jetzt funktioniert:** Login am Discord-Dashboard → Cookie wird am Twitch-Dashboard erkannt → beim ersten Zugriff wird die Session vom Discord-Bot geholt und lokal mit voller TTL gespeichert. Route-Handler und Forward-Auth sehen ab dann konsistent dieselbe gültige Session.

## #104 — Admin System-Endpoints (Rust Schritt 2a)

**Ausgangslage:** Das Rust-Dashboard hatte nach Schritt 1b zwar Auth-geschützte Analytics-Routen, aber keine Möglichkeit, den Gesundheitszustand des laufenden Systems abzufragen — kein Uptime, kein Memory, kein Überblick über DB-Größe, EventSub-Zustand oder Error-Log.

**Geändert:** Vier neue Admin-only-Endpoints unter `/twitch/api/admin/system/`:

- **`/health`** — Prozess-Uptime (OnceLock-Timestamp aus `main.rs`), RSS-Memory aus `/proc/self/status`, PID, letzter DB-Tick aus `twitch_live_state`, Raw-Chat-Ingest-Lag aus `twitch_raw_chat_ingest_health`. Lag-Warning (`RAW_CHAT_LAG`) wird nur ausgelöst wenn der herangezogene Streamer gerade live ist — Offline-Fallback läuft stumm durch.
- **`/database`** — DB-Gesamtgröße + Row-Count/Size für 10 definierte Tabellen via `pg_class.reltuples` + `pg_total_relation_size`. Nicht-existierende Tabellen werden einfach weggelassen.
- **`/eventsub`** — Neuester EventSub-Snapshot aus `twitch_eventsub_capacity_snapshot` (nur Zeilen mit `listener_count > 0` und gefülltem JSON, max. 200 Einträge zurück).
- **`/errors`** — Paginierter Error-Log aus `twitch_admin_error_log` (page/page_size Query-Params, Default 1/25). Falls die Tabelle noch nicht existiert: leere Antwort statt 500.

Alle vier Routen verlangen den internen Token (`X-Internal-Token`), 401 sonst. Der Live/Fallback-JOIN in der Health-Query nutzt `LOWER()`-Vergleich — konsistent mit allen anderen JOINs im Codebase. `sqlx`-Workspace-Feature `chrono` ergänzt (war bisher nicht gesetzt).

**Wie es jetzt funktioniert:** `build_admin_system_router` wird in `build_router` per `.merge()` eingebunden. Der Uptime-Timestamp wird einmalig in `main.rs` nach dem DB-Connect gesetzt, damit er die echte Startzeit des Prozesses widerspiegelt — nicht den Zeitpunkt des ersten Health-Requests.

## #103 — Auth-Layer + Admin-Analytics-Routen (Rust Schritt 1b)

**Ausgangslage:** Der Rust-Rewrite hatte nach Schritt 1a drei öffentliche Read-only-Endpoints ohne jede Zugriffskontrolle. Alle Admin-seitigen Analytics-Routen — Streamer-Liste, Session-Übersicht — waren noch ausschließlich im Python-Backend.

**Geändert:** Rust `tb-dashboard`-Binary (Port 8767) bekommt einen vollständigen Auth-Layer und drei neue Admin-Routen:

- **Auth-Level-Extraktion** (`AuthLevel`): Jeder Request wird eingestuft — `Localhost` wenn Loopback-IP + localhost-Host-Header (kein Token nötig), `Admin` bei korrektem `X-Internal-Token`-Header (constant-time-Vergleich), `None` sonst. Der Extractor sitzt als axum-`FromRequestParts`-Impl direkt im Request-Pipeline, kein Middleware-Wrapper nötig. Partner-Session-Auth (Fernet-Cookie) bleibt deferred (ADR 0003 — kein Fernet-Nachbau, Migration erst wenn Login-Flow auf Rust wechselt).
- **IDOR-Guard** (`require_owner`): Blockiert `None`-Requests mit 401, lässt Admin/Localhost durch — vorbereitet für Partner-Level wenn es kommt.
- **Plan-Gating** (`require_extended_plan`): Liest `manual_plan_id` + `manual_plan_expires_at` aus `streamer_plans`, prüft gegen die bekannten Extended-Plan-IDs (`analytics_pro`, `analytics_extended`), respektiert Ablaufdaten. Admin/Localhost überspringen die Prüfung.
- `GET /twitch/api/v2/auth-status` — antwortet immer 200, liefert `auth_level` + `logged_in`; nützlich für Frontend-Conditional-Rendering.
- `GET /twitch/api/v2/streamers` — Admin-only; gibt alle aktiven Partner mit Live-Status + Viewer-Count zurück, sortiert nach Live-Zustand dann Viewer-Zahl.
- `GET /twitch/api/v2/overview?streamer=<login>&days=30` — Admin-only; aggregierte Session-Metriken (avg/peak Viewers, Airtime, Hours-Watched, Follower-Delta, Session-Count) für einen konfigurierbaren Zeitraum (7–365 Tage). Leerer Zeitraum liefert `{empty: true}` statt 500.

**Wie's jetzt funktioniert:** `into_make_service_with_connect_info` injiziert die echte Client-IP in jede Request-Extension — ohne diesen axum-Aufruf würde `ConnectInfo` nie befüllt und `AuthLevel::Localhost` in Produktion nie feuern. Alle SQL-Zeitvergleiche nutzen explizite `::TIMESTAMPTZ`-Casts (PostgreSQL lehnt `TEXT >= TIMESTAMPTZ` sonst ab), Aggregat-Summen explizit `::FLOAT8`/`::BIGINT` um NUMERIC-Rückgaben zu vermeiden. Jeder Test läuft in einem eigenen PostgreSQL-Schema (`SET search_path TO test_xxx`) — keine parallelen Testkonflikte möglich.

## #102 — Werbefrei deckt jetzt auch die Fake-Server-Warnung ab

**Problem:** Streamer mit Werbefrei-Abo (`chat.promos.disable` bzw. gesetztes `promo_disabled`) sollen keine automatischen Bot-Werbe-Announcements im Chat bekommen. Die regulären Promos (Chat-Aktivität, Viewer-Spike) hielten sich daran, weil sie über `_send_promo_message` laufen, das die Werbefrei-Sperre prüft. Die periodische Promo-Schleife rief aber zwei Inhalte — die Fake-Server-/Scam-Warnung und die Targeted-Promo — *direkt* auf, also am gemeinsamen Sende-Pfad und damit an der Sperre vorbei. Ergebnis: Ein Werbefrei-Kanal sah trotzdem die orange Warn-Announcement (die de facto den eigenen offiziellen Discord bewirbt) und ggf. die personalisierte Promo.

**Geändert:** Die Werbefrei-Sperre greift jetzt eine Ebene höher — direkt an der Quelle der Live-Kanal-Liste, über die die periodische Schleife iteriert. Gesperrte Kanäle werden vorab herausgefiltert, bevor irgendein Sende-Pfad sie überhaupt erreicht.

**Wie's funktioniert:** Sobald die Schleife die Liste der aktuell live-Kanäle geholt hat, läuft jeder Kanal einmal durch dieselbe Sperr-Prüfung, die der reguläre Promo-Pfad schon nutzt (DB-Lookup auf `promo_disabled` + Plan-Entitlement `chat.promos.disable`). Kanäle, die gesperrt sind, fallen aus der Liste — die nachfolgende Schleife sieht sie gar nicht mehr, weder für die Scam-Warnung noch für Targeted-, Stats- oder Viewer-Spike-Promos. Die Lurker-Steuer bleibt unberührt: Sie nutzt eine eigene Kanal-Liste und ist ein vom Streamer selbst aktiviertes Feature, kein aufgedrängter Werbeinhalt. Damit gilt „werbefrei heißt wirklich werbefrei" konsistent für alle automatischen Announcements statt nur für einen Teil der Sende-Pfade.

## #101 — Jeder Partner bekommt jetzt seinen eigenen Discord-Invite-Link

**Problem:** Der Bot sollte pro Streamer einen eindeutigen Discord-Invite erstellen, damit die Quelle jedes Joins (welcher Kanal hat jemanden reingebracht) nachvollziehbar ist. In der Praxis fiel der Bot aber regelmäßig auf den allgemeinen Fallback-Link zurück, besonders bei neu eingetragenen Partnern. Die Ursache: Der Twitch-Bot läuft als eigener Prozess mit einem eigenen Discord-Account — und dieser Account ist kein Mitglied im Deadlock-Discord-Server. `bot.guilds` war daher immer leer, `channel.create_invite()` wurde nie aufgerufen, und jeder Invite-Erstellungsversuch schlug lautlos fehl.

**Geändert:** Invite-Erstellung läuft jetzt über den internen Master-Broker (Port 8770). Der Hauptbot (der tatsächlich im Server ist) bekommt einen neuen Endpunkt `/internal/master/v1/discord/create-invite` und erstellt den Invite auf Anfrage. Der Twitch-Bot ruft diesen Endpunkt per HTTP via dem gleichen Token auf, das er ohnehin für Nachrichten-Routing nutzt.

Zusätzlich läuft beim Start ein Backfill-Task (`_ensure_partner_invites`), der alle aktiven Partner ohne Invite-Eintrag in der DB sequenziell nachversorgt — mit 0,5s Pause zwischen den Requests und einem automatischen Retry-Pass (60s Wartezeit) falls transiente Timeouts einzelne Requests beim Startup-Burst treffen.

**Wie's funktioniert:** Beim Bot-Start wartet `_ensure_partner_invites` auf das Discord-Ready-Event, fragt dann die DB nach Partnern ohne Eintrag in `twitch_streamer_invites`, und ruft für jeden `_create_streamer_invite` auf. Diese Methode baut eine HTTP-POST-Anfrage an den Broker mit `channel_id`, `reason` und einem zufälligen Idempotency-Key. Der Broker löst den Channel auf (`get_channel` / `fetch_channel`), ruft `channel.create_invite(max_age=0, max_uses=0, unique=True)` auf und gibt `invite_url`, `code`, `guild_id` und `channel_id` zurück. Der Twitch-Bot speichert das Ergebnis in `twitch_streamer_invites` (UPSERT) und cached es im Memory-Dict.

## #100 — Jahresabo: Bonusmonate automatisch + Admin-Planvergabe vollständig

**Problem:** Drei separate Baustellen. Erstens: Wer ein Jahresabo über den neuen Checkout-Flow buchte, bekam die versprochenen 2 Bonusmonate nicht — der Webhook hat sie nie in die DB geschrieben, weil das `bonus_months`-Feld nur im alten Legacy-Flow in die Stripe-Subscription-Metadata gesetzt wurde, im neuen API-Checkout-Endpunkt aber fehlte. Zweitens: Das Admin-Dashboard hatte im Dropdown zur manuellen Planvergabe nur 4 der 8 verfügbaren Pläne — `chat_quiet` (Werbefrei) und die neueren Bundles fehlten komplett, was das manuelle Setzen für diese Pläne blockierte. Drittens: Das Ablaufdatum-Feld zeigte das ISO-Format `YYYY-MM-DD`, was Tag und Monat verwechselbar macht.

**Geändert:** (1) Der neue API-Checkout-Endpunkt setzt jetzt bei `cycle_months = 12` automatisch `subscription_data.metadata.bonus_months = "2"` in die Stripe-Session — der Webhook liest das nach Zahlungseingang aus und verlängert das Ablaufdatum um 62 Tage über das Abo-Ende hinaus. (2) Das Admin-Dropdown enthält jetzt alle 8 Pläne inkl. Werbefrei, Bundle Werbefrei+Analyse und Bundle Komplett. (3) Das Ablaufdatum-Feld ist jetzt ein nativer Datums-Picker, der im Browser im deutschen Format (TT.MM.JJJJ) anzeigt.

**Wie's funktioniert:** Jahreskäufer erhalten ab sofort ihre Bonusmonate vollautomatisch: Stripe feuert `checkout.session.completed`, der Bot liest `bonus_months: 2` aus der Subscription-Metadata, addiert 62 Tage auf das `current_period_end` und schreibt das Ergebnis als `manual_plan_expires_at` in die DB — kein Admin-Eingriff mehr nötig. Checkout ohne verknüpften Login wird in beiden Flows jetzt hart geblockt: der alte Abbo-Flow leitet zurück zur Abo-Seite, der neue API-Flow gibt 401 zurück — eine Zahlung ohne zugeordnetem Account ist damit nicht mehr möglich.

## #99 — Global-Ban-Sweep nur noch für OAuth-autorisierte Kanäle

**Problem:** Der tägliche Ban-Sweep hat alle aktiven Partner-Kanäle durchgearbeitet — auch solche, die sich nie per OAuth autorisiert haben und bei denen der Bot gar kein Moderator sein kann. Das war verschwendete Arbeit und produzierte unnötige 403-Fehler.

**Geändert:** Der Sweep prüft jetzt vor dem Durchlauf, welche Streamer einen gültigen OAuth-Eintrag haben (`needs_reauth = FALSE`). Kanäle ohne Eintrag oder mit ungültigem Token werden übersprungen — invalide Token werden sowieso departnered.

**Wie's funktioniert:** Vor dem Sweep lädt der Bot einmalig alle `twitch_user_ids` mit gültigem OAuth aus der Datenbank. Nur Kanäle, die in dieser Menge enthalten sind, landen im Durchlauf. Wer kein OAuth hat, kommt gar nicht erst dran — statt mit 403 zu scheitern.

## #98 — Keine doppelte Werbung mehr

**Problem:** Der Bot hat seine Discord-Werbung manchmal doppelt gepostet — zwei Werbungen mit nur einer einzigen Chat-Nachricht dazwischen. Eigentlich gibt es eine Mindestschwelle, wie viele Nachrichten zwischen zwei Werbungen liegen müssen, aber die wurde übergangen. Grund: Über das Werben entscheiden zwei Stellen parallel — eine reagiert auf jede eingehende Chat-Nachricht, eine läuft als Timer im Hintergrund. Beide prüfen die Schwelle, senden dann aber erst nach mehreren Zwischenschritten und setzen den „zuletzt geworben"-Marker erst ganz am Ende. Trifft eine Chat-Nachricht genau in dieses Zeitfenster, sehen beide noch den alten Stand und werben gleichzeitig.

**Geändert:** Beide Werbe-Pfade werden jetzt pro Kanal gegeneinander gesperrt, sodass immer nur einer gleichzeitig entscheiden und senden darf.

**Wie's funktioniert:** Ein Schloss pro Kanal umschließt die komplette Sequenz „Schwelle prüfen → senden → Marker setzen". Während ein Pfad sendet, muss der andere warten; sobald er drankommt, sieht er den frisch gesetzten Marker und die noch nicht zurückgesetzte Nachrichtenschwelle und bricht ab. Doppelte Werbung ist damit ausgeschlossen, und die Mindestnachrichten-Schwelle greift wieder zuverlässig.

## #97 — Netzwerkweite Sperrliste bannt jetzt vorsorglich (nur wenn offline)

**Problem:** Es gibt eine netzwerkweite Sperrliste für unerwünschte Accounts (z.B. Scammer oder Leute, die gegen die Community-Richtlinien verstoßen haben). Bisher wurde so jemand erst gebannt, wenn er in einem der betreuten Kanäle tatsächlich geschrieben hat — der Ban passierte also reaktiv, mitten im laufenden Stream, oft in einem Kanal, in dem die Person vorher nie aufgetaucht war. Das war auffällig und für Zuschauer verwirrend (warum bannt der Bot jemanden, der gerade nichts getan hat?). Und wer einfach nichts schrieb, rutschte komplett durch.

**Geändert:** Zusätzlich zum reaktiven Ban gibt es jetzt einen vorsorglichen Abgleich, der gesperrte Accounts aus allen betreuten Kanälen bannt — aber bewusst nur, wenn der jeweilige Kanal gerade offline ist. Außerdem wurde die Chat-Begründung beim reaktiven Ban ehrlich gemacht.

**Wie's funktioniert:** Der Abgleich läuft zu zwei Zeitpunkten, beide an „Streamer ist offline" gekoppelt: einmal rund eine Stunde nachdem ein Stream endet (gezielt für genau diesen Kanal), und einmal täglich gegen 6 Uhr morgens als Sammeldurchlauf über alle Kanäle, die gerade nicht senden. Ist ein Kanal live, wird er übersprungen und beim nächsten Mal erneut versucht — so wird nie jemand sichtbar mitten im Stream gebannt. Damit nicht bei jedem Durchlauf dieselben Bans erneut an Twitch geschickt werden, merkt sich das System pro Kombination „Account × Kanal", wo der Ban schon gesetzt wurde. Der frühere reaktive Ban bleibt als Sicherheitsnetz — falls jemand schreibt, bevor der Offline-Abgleich den Kanal erreicht hat — sagt im Chat jetzt aber klar, dass die Person netzwerkweit gesperrt ist (Verstoß gegen die Community-Richtlinien), statt sie pauschal als Spam zu deklarieren. Wer neu auf die Sperrliste kommt, landet automatisch auch auf der Raid-Sperrliste.

## #96 — Engagement-AI: Validate+Erklär-Muster gebrochen

**Problem:** 44% aller generierten Nachrichten folgten dem Muster `[kurze Reaktion], [erklärender zweiter Teil]` — z.B. "haha genau, das ist halt typisch für den hero". Dieses Muster ist das sicherste Bot-Tell überhaupt: echte Chatter sagen entweder die Reaktion oder die Meinung — nie beides in einem Satz.

**Geändert:** Zwei Stellen.

1. **System-Prompt:** Neuer Abschnitt mit konkretem Gegenbeispiel (Show-don't-tell statt abstrakter Regel): Die KI soll bei einem Komma in ihrer Antwort alles nach dem Komma streichen und prüfen ob der erste Teil alleine steht. Dazu explizit: entweder Reaktion oder Meinung, nie beides plus Begründung.

2. **Gold-Beispiele in den Few-Shot-Vorlagen:** Die kuratierten Muster-Nachrichten wurden um mehr reine Reaktionen ergänzt ("alter bitte", "echt wild", "ngl stimmt", "no shot den", "ich auch") — kurze Fragmente ohne Erklär-Nachklapp. Diese erscheinen immer als erste im Prompt und konditionieren den Ton.

**Wie's funktioniert:** LLMs imitieren konkrete Beispiele besser als abstrakte Regeln. Wenn die ersten 4 Beispiele alle pure Reaktionsfragmente ohne Erklärung sind, schreibt das Modell automatisch im gleichen Register — statt zu erklären warum etwas "genau" ist.

## #95 — Engagement-AI: weniger Bot-isch, keine Dialekt-Imitation

**Problem:** Die AI hat sich im Chat zu oft verraten — vor allem durch einen einzigen Tell: 40% aller Nachrichten fingen mit "haja" an, weil das Modell den Ausdruck aus den Channel-Stilvorlagen übernahm und ihn auf jede Antwort klebte. Dazu kamen erfundene Compound-Wörter ("brumm-heizkessel", "konsequenz-plan"), broken Schweizerdeutsch in Channels wo Schweizer zuschauen, und gelegentlich `<3` als Antwort auf komplett unpassende Trigger.

**Geändert:** Drei Stellen gleichzeitig angepasst.

1. **Starter-Repeat-Guard in der Pipeline:** Nach dem Modell-Call wird das erste Wort der generierten Antwort mit dem ersten Wort der letzten Bot-Nachricht verglichen. Stimmen sie überein → Antwort verworfen (silent). Das ist ein harter Filter unabhängig vom Prompt — fängt auf, wenn das Modell die Prompt-Regel ignoriert.

2. **System-Prompt-Ergänzungen:** Vier neue Regeln direkt im Baseline-Prompt: (a) nie mit demselben Wort starten wie die vorherige Antwort, (b) "haja", "hmm", "naja", "danke" nicht als Opener, (c) kein Dialekt — auch wenn Schweizerdeutsch im Channel läuft, bleibt die Antwort normales Deutsch mit Chat-Slang, (d) `<3` nie als eigene Antwort senden.

3. **Style-Beispiel-Deduplication:** Die Few-Shot-Stilvorlagen, die dem Modell aus dem Channel-Chat gezogen werden, dürfen jetzt max. 2 Beispiele mit demselben Starter-Wort enthalten. Vorher konnten 6 von 8 Beispielen mit "haja" anfangen — das hat das Modell trainingsartig auf genau das konditioniert.

**Wie's funktioniert:** Prompt und Filter wirken als Doppelnetz: der Prompt verhindert, der Guard fängt auf. Die Stil-Dedup greift schon früher und sorgt dafür, dass das Modell gar nicht erst lernt, dass "haja" der Standard-Opener ist.

## #94 — Fairere Raid-Verteilung im Fallback (kein Partner online)

**Problem:** Wenn kein Partner live war, wurde der Fallback-Kanal immer nur nach wenigsten Viewern ausgewählt — ohne Rücksicht darauf, wie viele Raids jemand schon bekommen hat. Wer klein streamt und oft live ist, bekam dadurch unverhältnismäßig viele Raids.

**Geändert:** Die Fallback-Selektion (`select_fairest_candidate`) berücksichtigt jetzt `received_successful_raids_total` aus der Score-Datenbank als primären Sortierschlüssel. Viewer-Count bleibt Tiebreaker.

**Wie's funktioniert:** Vor der Auswahl wird für alle Kandidaten die bisherige Raid-Gesamtzahl aus der DB geladen. Sortiert wird dann zuerst nach wenigsten erhaltenen Raids — wer noch kaum Raids hat, kommt nach vorne. Kanäle die noch gar nicht im System sind, bekommen automatisch 0 und haben damit höchste Priorität. Viewer-Count und Follower-Count entscheiden bei Gleichstand. Das verhindert, dass ein Kanal mit wenig Viewern dauerhaft alle Fallback-Raids kassiert während andere leer ausgehen.

## #93 — Abo-Preise drastisch gesenkt + Raid-Boost-Gewichtung reduziert

**Ausgangslage:** Die Abo-Preise lagen zwischen 3,99 € und 13,99 € und wurden als zu hoch eingestuft — insbesondere weil das Analyse-Dashboard aktuell MiniMax als KI nutzt, nicht Claude.

**Geändert:** Alle sieben Abo-Stufen neu bepreist, neue Stripe-Preise angelegt und die alten deaktiviert. Zusätzlich wurde die Zusatz-Gewichtung für Raid-Boost-Abonnenten von 25 % auf 15 % gesenkt.

**Neue Preise:**
- Werbefrei: 3,99 € → **1,99 €/Monat**
- Raid Boost: 3,99 € → **1,99 €/Monat**
- Analyse Dashboard: 8,49 € → **1,99 €/Monat**
- Werbefrei + Raids: 6,99 € → **3,49 €/Monat** (spart 49 ¢ gegenüber Einzelkauf)
- Werbefrei + Analyse: 10,49 € → **3,49 €/Monat**
- Analyse + Raids: 11,49 € → **3,49 €/Monat**
- Alles drin (Komplett): 13,99 € → **4,99 €/Monat** (spart 0,98 € gegenüber Einzelkauf)

**Wie Preise in Stripe funktionieren:** Stripe-Preise sind unveränderbar — ein gesenkter Preis bedeutet immer: neuen Price-Eintrag anlegen, alten auf inaktiv setzen. Bestehende Abos laufen weiterhin auf dem alten Preis, bis sie verlängert oder migriert werden. Neue Checkout-Sessions zeigen sofort die neuen Preise. Die Raid-Boost-Gewichtung ist ein interner Score-Multiplikator: Abonnenten werden im Raid-Netzwerk weiterhin bevorzugt, aber weniger stark als vorher (15 % statt 25 % Score-Aufschlag).

## #92 — Live-Ping-Rolle: Umbenennung + Streamer pingen sich nicht mehr selbst

**Problem:** Die automatisch erstellte Discord-Ping-Rolle hieß `KANALNAME LIVE PING` (alles Großbuchstaben, englisch) — unpassend. Dazu hatte der Bot beim Erstellen der Rolle dem Streamer die Rolle direkt selbst zugewiesen: wer live geht, bekam die eigene Ping-Rolle und wurde beim nächsten Go-Live-Event angepingt. Das ist das Gegenteil von sinnvoll.

**Geändert:** Der Rollenname folgt jetzt dem Format `kanalname ist live`. Die Zuweisung der Ping-Rolle an den Streamer selbst wurde in beiden Code-Pfaden (Monitoring-Flow + Dashboard-Live-Flow) entfernt.

**Wie's funktioniert:** Die Rolle wird nach wie vor automatisch erstellt und im Live-Embed als Mention eingebunden — Zuschauer können sie sich selbst in Discord zuweisen, um Benachrichtigungen zu erhalten. Der Streamer bekommt sie nicht mehr automatisch und wird damit nicht über den eigenen Stream angepingt.

## #91 — Live-Ankündigung: Textzeile entfernt, Umlaute gefixt, Offline-Embed aufgeräumt

**Problem:** Drei Kleinigkeiten, die zusammen störten. (1) Vor jedem Live-Embed stand eine redundante Klartextzeile (`X ist live! Schau über den Button unten rein.`), die denselben Inhalt wie das Embed selbst doppelt angezeigt hat. (2) Im Footer (`fuer`) und im Content-Template (`ueber`) standen ASCII-Umlaute statt echter Zeichen — sichtbar für alle Streamer ohne eigene DB-Konfiguration, z. B. daxy. (3) Das Offline-Embed zeigte `OFFLINE` dreifach: im Embed-Titel, im Author-Label (`OFFLINE: Name`) und als eigenes Status-Feld.

**Geändert:** Content-Template auf reinen Rollen-Ping (`{rolle}`) gekürzt — die Textzeile fällt weg, der Mention bleibt. Footer-Text und Template-Fallbacks verwenden jetzt echte Umlaute. Im Offline-Embed ist das redundante Status-Feld (`OFFLINE`) entfernt und das Author-Label zeigt nur noch den Streamer-Namen ohne Präfix.

**Ergebnis:** Live-Embeds sehen kompakter aus (kein doppelter Informationstext über dem Embed). Offline-Embeds zeigen `OFFLINE` nur noch einmal im Titel. Alle Standardtexte verwenden korrekte Umlaute.

## #90 — Admin-Dashboard: Changelog-History, Raid-Historie, DB-Query, Raw-Chat-Lag-Fix, Memory-Fix

**Problem:** Sechs Baustellen im Admin-Dashboard auf einmal. (1) Die Changelog-Seite zeigte nie eine History, weil das Backend das Feld schlicht nicht lieferte. (2) Die Raids-Seite zeigte keine vergangenen Raids — gleicher Grund: Backend lieferte keine History. (3) Die EventSub-Seite zeigte bei inaktivem WebSocket 0 Subscriptions, ohne Hinweis was zuletzt registriert war. (4) Die Memory-KPI zeigte dauerhaft „0 B", weil `psutil` nicht im venv installiert war. (5) Der Raw-Chat-Lag-Warning blieb permanent aktiv für `its_raffi`, weil der Live-State seit dem EventSub-Ausfall nie auf Offline gesetzt wurde — der Bot behandelte einen 80 Tage alten Timestamp als „live". (6) Keine Möglichkeit, direkt Daten aus der DB abzulesen.

**Geändert:**
- **Changelog-History:** Der `/twitch/api/admin/config/overview`-Endpoint gibt jetzt `changelog.entries` mit den letzten 20 Einträgen aus der `internal_home_changelog`-Tabelle zurück — die History-Sektion auf der Changelog-Seite füllt sich damit automatisch.
- **Raid-Historie:** Derselbe Endpoint enthält jetzt `raids.history` mit den letzten 50 Einträgen aus `twitch_raid_history` (Streamer, Ziel, Viewer-Zahl, Zeitstempel, Erfolg). Die Raids-Seite im Dashboard zeigt sie direkt in der Tabelle.
- **EventSub Last-Known-Snapshot:** Wenn der WebSocket inaktiv ist und keine aktiven Subscriptions vorliegen, liest der EventSub-Endpoint den letzten Snapshot mit `listener_count > 0` aus `twitch_eventsub_capacity_snapshot` — inklusive `listeners_json` — und gibt ihn als `lastKnownSubscriptions` zurück. Die EventSub-Seite zeigt diesen Snapshot mit Zeitstempel als eigene Sektion.
- **Raw-Chat-Lag-Fix:** Das Live-Scope-JOIN in `_fetch_raw_chat_health_snapshot` prüft jetzt zusätzlich, ob `last_seen_at`/`last_started_at` des Live-States nicht älter als 4 Stunden ist. Eingefrorene States aus der Zeit vor dem EventSub-Ausfall gelten nicht mehr als „live" — die veraltete Warnung verschwindet beim nächsten Health-Poll.
- **Memory-Fix:** `psutil` ins venv installiert. Die RSS/Process-Memory-KPI im System Overview zeigt jetzt den echten Prozess-Speicherverbrauch.
- **DB Query:** Neuer Admin-Endpoint `GET /twitch/api/admin/system/query?sql=...` — nur SELECT erlaubt, max. 200 Rows, gefährliche Keywords (`INSERT`, `UPDATE`, `DROP` etc.) werden geblockt. Neue Seite „DB Query" in der Sidebar unter Operations: Tabellenliste zum Klicken, SQL-Textarea mit Ctrl+Enter, Ergebnistabelle.

**Wie's funktioniert jetzt:** Admin öffnet `/twitch/admin/operations/query`, klickt eine Tabelle an, der Editor füllt sich mit einem Basis-SELECT, Ctrl+Enter schickt die Abfrage ab, das Ergebnis erscheint als sortierbare Tabelle. Schreiboperationen schlägt der Server mit HTTP 400 zurück bevor eine Verbindung zur DB geht.

## #89 — Live-Ankündigungen: kein Schriftzug über dem Embed, echte Umlaute, Promos nur für aktive Partner

**Problem:** Drei separate Baustellen. Erstens stand über jedem Live-Embed ein redundanter Klartext-Satz („X ist live! Schau über den Button unten rein."), der denselben Inhalt wie das Embed doppelt zeigte. Zweitens verwendeten Embeds und Buttons ASCII-Ausweichreplacement statt echter Umlaute (`ue`, `fuer`, `ueber` statt `ü`, `für`, `über`). Drittens wurden Chat-Promos an Streamer gesendet, auch wenn deren Partner-Status inzwischen deaktiviert oder archiviert war.

**Geändert:** Der Freitext-Content der Go-Live-Nachricht enthält jetzt nur noch die Rollen-Mention (damit der Ping feuert), kein angehängter Beschreibungstext mehr — das Embed trägt alle Infos. Der Offline-Edit schreibt ebenfalls keinen Plaintext mehr über das OFFLINE-Embed. Alle `ue`/`fuer`/`ueber`-Einträge in Embed-Titeln, Feldern und Footer wurden durch echte Umlaute ersetzt. `_promo_channel_allowed` fragt jetzt live gegen `twitch_streamers_partner_state` ab: `is_partner_active = true` und `archived_at IS NULL` — wer nicht mehr aktiver Partner ist, bekommt keine Chat-Promos mehr.

**Wie's funktioniert:** Discord zeigt Mentions nur dann als echten Ping an, wenn die Mention-ID im Content-Text steht — deshalb bleibt der Content nicht leer, sondern enthält nur die `<@&role_id>`. Fehlt eine Rolle (kein Ping konfiguriert), ist Content `None` → keine sichtbare Textzeile über dem Embed. Die Partner-Prüfung passiert bei jedem `_promo_channel_allowed`-Aufruf synchron gegen die DB, da dieselbe Funktion ohnehin vor echten Netzwerk-/API-Operationen steht.

## #88 — Chat-Werbung: kein Spam zum Stream-Start, Fake-Server-Warnung wird endlich sichtbar

**Problem:** Drei zusammenhängende Macken in der automatischen Discord-Werbung im Chat. Erstens warf der Bot direkt zu Stream-Beginn eine Werbenachricht raus, obwohl noch niemand im Chat geschrieben hatte. Zweitens griff die eigentlich vorgesehene Regel „erst nach genug Chat-Aktivität werben" nicht — es wurde geworben, egal wie tot der Chat war. Und drittens tauchte die Warnung vor den gefälschten Discord-Servern („Deadlock Discord Deutschland" / „Deadlock German Competitiv HUB") praktisch nie auf.

Ursache war ein zweiter, neuerer Werbe-Weg (die zielgerichtete Promo mit KI-Vorauswahl), der parallel zum älteren lief, aber nur seinen eigenen 15-Minuten-Kanaltakt prüfte — nicht die Aktivitäts-Schwellen. Bei frischem Stream ist dieser Takt sofort „frei", also feuerte er sofort. Und weil er den Werbe-Slot dauerhaft belegte, kam der ältere Weg, an dem die Fake-Server-Warnung hängt, fast nie zum Zug — die Warnung wurde regelrecht ausgehungert, ihr Timer nicht mal gestartet.

**Geändert:** Beide Werbe-Wege hängen jetzt an derselben Aktivitäts-Schwelle, und die Fake-Server-Warnung wird eine Ebene höher entschieden — dort, wo feststeht, welcher Werbetyp den Slot überhaupt bekommt.

**Wie's funktioniert:** Bevor geworben wird — egal über welchen Weg — muss seit der letzten Werbung genug echte Chat-Aktivität zusammengekommen sein: eine Mindestzahl an Nachrichten seit der letzten Promo, mehrere verschiedene Schreiber im Zeitfenster und (nach der ersten Promo) ein paar neue Gesichter. Erst wenn das erfüllt ist, kommt eine Werbung — am toten Stream-Start also gar nichts. Die Fake-Server-Warnung wird jetzt zentral im fälligen Werbe-Slot geprüft: Ist ihr eigener Takt (Standard 45 Minuten pro Kanal) reif, kommt die Warnung statt einer normalen Promo — unabhängig davon, welcher Werbe-Weg sonst gefeuert hätte. So wechseln sich Werbung und Warnung von selbst ab, statt dass ein Weg den anderen verdrängt. Der Wortlaut bleibt bewusst vorsichtig („könnte/möglicherweise Fake"), nennt die beiden Server aber klar beim Namen.

**Betroffen:** Alle Partner-Kanäle, in denen der Bot Discord-Werbung postet. Zuschauer sehen am Stream-Anfang keine Werbung mehr ins Leere und bekommen die Warnung vor den Fake-Servern jetzt regelmäßig zu Gesicht.

## #87 — Admin-Dashboard: Logout und Direktaufrufe führen nicht mehr ins Leere

**Problem:** Zwei Ärgernisse beim Admin-Zugang. Erstens schickte der Logout-Button einen auf eine „Seite nicht gefunden", statt sauber abzumelden — er rutschte in den Anmelde-Weg der öffentlichen Streamer-Seite, dessen Ziel es auf der Admin-Adresse gar nicht gibt. Zweitens: Wer eine Admin-Unterseite direkt aufrief (z. B. einen gespeicherten Link), ohne angemeldet zu sein, bekam eine nackte Fehlerseite (401) statt zur Anmeldung geleitet zu werden — man musste erst umständlich die Startseite ansteuern und sich dort einloggen.

**Geändert:** Auf der Admin-Adresse führt der Logout jetzt direkt zur Admin-Anmeldung. Und ein Direktaufruf einer Admin-Seite ohne Anmeldung leitet automatisch zur Anmeldung weiter, statt eine Fehlerseite zu zeigen.

**Wie's funktioniert:** Der Abmelde-Vorgang erkennt jetzt, dass er auf der Admin-Adresse läuft, und nimmt den richtigen Discord-Abmelde-Weg — der die Sitzung beendet und auf die Anmeldeseite führt, statt in den Twitch-Login der Streamer-Seite zu rutschen (dessen Weiterleitungsziel auf der Admin-Adresse nicht existiert und deshalb „nicht gefunden" lieferte). Für Direktaufrufe prüft der vorgelagerte Reverse-Proxy, ob überhaupt eine Admin-Sitzung vorliegt: fehlt sie, geht es sofort zur Anmeldung; ist sie da, läuft alles unverändert weiter — Angemeldete merken nichts. Der Proxy musste dafür einmal neu gestartet werden, weil seine Konfigurationsdatei nur als Einzeldatei eingebunden war und Änderungen sonst nicht ankamen.

**Betroffen:** Admin-Login-/Logout-Komfort; für Zuschauer und Streamer nichts sichtbar.

## #86 — Chat-KI (Testphase): vom Cringe-Mitspieler zum stillen Zuschauer

**Problem:** Der KI-Stammgast, der gerade nur intern im Review getestet wird, verhielt sich wie ein aufdringlicher Mitspieler statt wie ein normaler Zuschauer. Er reagierte auf praktisch jede Chatzeile — auch auf einzelne Emotes, Begrüßungen, Reaktionswörter („gg", „easy") und Chat-Commands. Schlimmer: Lob für gute Spielzüge nahm er an, als hätte er selbst gespielt („läuft grad gut"), sprach einzelne Zuschauer wie ein Gastgeber mit Namen an und klinkte sich in Absprachen der Stammcrew ein. Dazu klangen die Antworten nach KI — zu lang, zwei ausformulierte Sätze mit Abschluss-Pointe, wo ein echter Chatter nur ein Fragment hinwirft.

**Geändert:** Eine zweistufige Brems-Logik, ein neues Selbstbild und ein an echten Chatdaten geeichter Schreibton. Außerdem hört die KI im Test nur noch echte Partnerkanäle mit.

**Wie's funktioniert:**
- Vor dem Sprachmodell sortiert ein billiger Vorfilter offensichtliches Rauschen aus, ohne das Modell überhaupt zu fragen: einzelne Emotes und Reaktionswörter, mehrfach wiederholte Emote-Ketten, Chat-Commands und Nachrichten, die mit „@name" direkt an eine bestimmte Person gehen. Das spart Rechenzeit und lässt sich nicht per Prompt „überreden".
- Das Modell weiß jetzt klar, dass es Zuschauer ist und nicht der Streamer: Lob und Zurufe zum Spielgeschehen gehören dem Streamer, nicht ihm; es grüßt, dankt und verabschiedet niemanden wie ein Gastgeber und mischt sich nicht in Pläne ein.
- Melden darf es sich nur noch bei einem echten Deadlock-Anlass — einer konkreten Frage oder Meinung zu Helden, Items, Builds oder Meta. Alles andere bleibt stumm; der Normalfall ist Schweigen.
- Der Schreibton wurde an rund 3000 echten Chatnachrichten gemessen (typisch: drei bis vier Wörter, ein einziger Satz, kaum Satzzeichen, echte Umlaute) und genau darauf geeicht — kurze Fragmente statt ausformulierter Absätze.

**Betroffen:** Intern. Die Chat-KI ist weiter in der Review-Testphase und für Zuschauer wie Streamer noch nicht aktiv.

## #85 — Admin-Dashboard: Affiliate-Abrechnung repariert + Admin-Login hält 2 Wochen

**Problem:** Zwei Dinge im Admin-Bereich. Erstens warf die Affiliate-Abrechnung beim Öffnen einen Server-Fehler (500) und lud gar nicht — die Datenbank-Abfrage fragte eine Spalte („Provisionssatz") ab, die es in der Vertriebler-Konten-Tabelle nie gegeben hat. Zweitens fiel die Admin-Anmeldung schon nach wenigen Stunden wieder raus (die Sitzung galt nur 6 Stunden), was sich anfühlte, als wäre das Dashboard ständig „tot": Oberfläche lädt, aber jede Aktion läuft ins Leere, weil man im Hintergrund längst abgemeldet war.

**Geändert:** Die nicht existierende Spalte fliegt aus der Abrechnungs-Abfrage. Die Gültigkeit der Admin-Sitzung steigt von 6 Stunden auf 2 Wochen.

**Wie's funktioniert:** Provisionen werden ohnehin als fester Betrag pro Buchung gespeichert, nicht als Satz am Konto — die Abfrage liest jetzt nur noch tatsächlich vorhandene Felder, der 500er ist weg und die Abrechnungs-Liste lädt wieder. Bei der Sitzung wurden beide beteiligten Lebensdauern angehoben: die zentrale Login-Sitzung (über die der Admin-Zugang läuft) und die davon abgeleitete Dashboard-Sitzung stehen jetzt auf 14 Tagen und verlängern sich bei jeder Nutzung automatisch — einmal anmelden reicht damit praktisch für zwei Wochen, statt mehrmals täglich neu einloggen zu müssen.

**Betroffen:** Admin-intern (Affiliate-Verwaltung und Login-Komfort), für Zuschauer und Streamer nichts sichtbar.

## #84 — Tracking-Tabelle: vestigiale Alt-Spalten entfernt

**Problem:** Auf der Tracking-Tabelle lagen noch vier Spalten aus einem alten Schema (Beschreibung des letzten Link-Checks, Link-Status, „hinzugefügt von", Zeitpunkt des letzten Checks) — partner-klingende Reste, die seit Langem von keinem Code mehr geschrieben oder gelesen wurden. Tot, aber verwirrend, weil sie auf der „nur Tracking"-Tabelle nach Partner-Daten aussehen.

**Geändert:** Diese vier toten Spalten wurden entfernt. Eine geprüfte Abhängigkeits-Analyse hat vorher bestätigt, dass kein Programmteil sie noch liest.

**Wie's funktioniert:** Ein weiterer versionierter Migrationsschritt entfernt die vier Spalten (idempotent, greift nur solange sie existieren). Ihre Daten waren ohnehin schon im Gesamt-Backup aus #83 enthalten. Die interne Identitäts-Spalte der Tabelle bleibt unangetastet. Damit enthält die Tracking-Tabelle wirklich nur noch Tracking-/Identitäts-Felder — keine partner-klingenden Altlasten mehr.

**Betroffen:** Für Zuschauer und Streamer nichts sichtbar — interne Datenhygiene.

## #83 — Partner-DB-Konsolidierung: doppelte Partner-Spalten endgültig entfernt

**Problem:** Partner-Eigenschaften (Verifizierungs-Status, Raid-Schalter, Stumm-Flags, Live-Ping-Einstellungen, Discord-Link-Pflicht) lagen jahrelang in **zwei** Tabellen gleichzeitig — einmal in der schmalen Partner-Tabelle (die eigentliche Wahrheit) und einmal als gespiegelte Kopie in der breiten Tracking-Tabelle. Zwei Pflegeorte für denselben Wert heißt: sie können auseinanderlaufen, und genau dieser Drift war die Wurzel der wiederkehrenden „mal stimmt der Partner-Status, mal nicht"-Verwirrung (siehe #80–#82).

**Geändert:** Die elf gespiegelten Spalten wurden aus der Tracking-Tabelle entfernt. Der Partner-Status lebt jetzt an genau einer Stelle.

**Wie's funktioniert:** Voraussetzung war die Vorarbeit aus #80–#82 — erst nachdem nachweislich kein Programmteil diese Spalten mehr aus der Tracking-Tabelle liest und die zentrale Schreib-Stelle sie nicht mehr dorthin spiegelt, sind sie „tot" und können weg. Ein versionierter Migrationsschritt legt beim Start einmalig ein vollständiges Backup der Tracking-Tabelle an und entfernt dann die elf Duplikat-Spalten; er greift nur solange die Alt-Spalten existieren und läuft sonst als No-op. Reine Tracking-Felder (Beobachtungs-Archivierung, „nur beobachtet"-Markierung, Identität) bleiben unangetastet. Ergebnis: eine einzige Quelle für den Partner-Status, kein Drift mehr möglich, und die ohnehin kanonische Lese-Sicht zieht ihre Werte unverändert aus der Partner-Tabelle.

**Betroffen:** Für Zuschauer und Streamer nichts sichtbar — interne Datenhygiene, Abschluss der Partner-DB-Konsolidierung.

## #82 — Partner-DB-Konsolidierung: Monitoring-Loop liest keine Partner-Config mehr aus der Tracking-Tabelle

**Problem:** Die Überwachungs-Schleife lud auch für reine Beobachtungs-Kanäle (keine Partner) Partner-Einstellungen wie „Discord-Link nötig" oder die Live-Ping-Rolle aus der breiten Tracking-Tabelle mit — Werte, die für einen Nicht-Partner nichts bedeuten. Das war der letzte offene Leser aus der Aufräumarbeit von #80/#81.

**Geändert:** Für Nicht-Partner setzt die Schleife diese Partner-Felder jetzt auf ihre Standardwerte, statt sie aus der Tracking-Tabelle zu lesen. Das echte Beobachtungs-Feld „archiviert?" bleibt unangetastet.

**Wie's funktioniert:** Aktive Partner kommen ohnehin schon aus der zentralen Partner-Sicht; nur der Zweig für reine Beobachtungs-Kanäle griff noch auf die gespiegelten Partner-Spalten zu. Für solche Kanäle sind diese Felder jetzt fest auf den Standard gesetzt (kein Link-Zwang, keine Live-Ping-Rolle) — exakt das, was dort heute ohnehin drinstand. Damit liest kein Programmteil mehr Partner-Status aus der Tracking-Tabelle; die doppelten Spalten lassen sich in einem späteren Schritt gefahrlos entfernen.

**Betroffen:** Nichts sichtbar — interne Datenhygiene, Abschluss der Leser-Umstellung.

## #81 — Partner-Status-Konsolidierung: zwei weitere Leser auf die zentrale Wahrheit

**Problem:** Mehrere Programmteile lasen Partner-Eigenschaften (z. B. „verifiziert seit", Raid-/Live-Ping-Einstellungen) direkt aus der breiten Tracking-Tabelle statt aus der zentralen Partner-Sicht. Dort liegen diese Eigenschaften nur als gespiegelte Kopie — pflegt man denselben Wert an zwei Orten, driften die beiden Stände früher oder später auseinander. Das ist die Fortsetzung der Aufräumarbeit aus #80.

**Geändert:** Zwei dieser Stellen beziehen den Partner-Status jetzt aus der zentralen Wahrheit bzw. fragen ihn gar nicht mehr aus der Tracking-Tabelle ab.

**Wie's funktioniert:** Der Post-Stream-Report lud zwei Partner-Konfig-Werte mit, die er nirgends verwendet hat — die werden schlicht nicht mehr abgefragt. Der Admin-Verifizierungs-Dialog entscheidet „schon verifiziert → keine erneute Benachrichtigung" jetzt anhand des echten Partner-Datensatzes statt anhand der gespiegelten Kopie in der Tracking-Tabelle. Für bestehende Partner ist das Ergebnis heute identisch (beide Kopien sind gleich); der Unterschied greift erst, wenn die doppelten Spalten später ganz aus der Tracking-Tabelle entfernt werden — dann gibt es nur noch eine Quelle, die nicht mehr veralten kann.

**Betroffen:** Für Zuschauer und Streamer nichts sichtbar — reine interne Datenhygiene als Zwischenschritt der Partner-DB-Konsolidierung.

## #80 — Partner-Status-Check: kein veralteter Nachbau mehr

**Problem:** Der Bot beantwortet an mehreren Stellen die Frage „ist das ein operativ aktiver Partner?" — und zwei dieser Stellen waren sich uneinig. Die eine las den zentral gepflegten Partner-Status; die andere, eigentlich die strengere (sie steuert u. a. den Bot-Ban-/Blacklist-Schutz und seit der letzten Änderung auch das Engagement-AI-Gate), rechnete den Status von Hand aus dem rohen Partner-Datensatz nach. Dieser Nachbau war veraltet: Er prüfte nur, ob die Partnerschaft formal aktiv, nicht per Opt-out abgeschaltet und nicht admin-archiviert war — den Zustand „technisch pausiert" bzw. „Bot wurde in diesem Kanal gebannt" hat er übersehen. Folge: Kanäle, in denen der Bot pausiert oder gebannt war, zählten trotzdem als operative Partner. Genau das war der Auslöser dafür, dass die Engagement-AI in solchen Kanälen anschlug.

**Geändert:** Die strenge „operativ aktiver Partner"-Prüfung rechnet nichts mehr selbst nach, sondern liest dieselbe zentrale Wahrheit wie alle übrigen Partner-Checks.

**Wie's funktioniert:** Es gibt eine zentrale, laufend gepflegte Sicht auf den Partner-Status, die alle Ausschlussgründe zu einem einzigen „aktiv: ja/nein" zusammenfasst — formal aktiv, kein Opt-out, nicht archiviert, nicht pausiert, nicht gebannt. Die strenge Prüfung schaut jetzt genau dort nach, statt die Bedingungen einzeln und unvollständig selbst zusammenzusetzen. Dadurch ist sie immer auf demselben Stand wie diese Sicht: Wird ein Partner pausiert oder der Bot dort gebannt, fällt der Kanal sofort aus allen Pfaden, die diese Prüfung nutzen. Vorher konnte der handgestrickte Nachbau der zentralen Sicht „hinterherhinken", weil er bei jeder Erweiterung der Ausschlussgründe mit angepasst werden musste — und das zuletzt nicht geschehen war.

**Betroffen:** Bot-Ban-/Blacklist-Schutz und die (noch nicht scharfgeschaltete) Engagement-AI — jeweils nur für Kanäle, in denen der Bot pausiert oder gebannt ist.

## #79 — Engagement-AI: redet nur noch in echten Partner-Kanälen

**Problem:** Die KI prüfte vor dem Antworten nur zwei Dinge: ob sie für den Kanal eingeschaltet ist und ob dort gerade Deadlock live läuft. Den Partner-Status hat sie dabei komplett übersprungen. Der Bot ist aber auch in beobachteten Nicht-Partner-Kanälen präsent (rein fürs Statistik-Tracking), und genau dort war die Engagement-AI die einzige Chat-Funktion, die trotzdem mitgeredet hat — Moderation, Promos und die frechen Auto-Antworten schweigen in solchen Kanälen längst, weil sie alle denselben Partner-Check davorhängen. Zusätzlich blieb der Engagement-Schalter eines Kanals auf „an", selbst wenn die Partnerschaft später beendet wurde.

**Geändert:** Ein Partner-Gate vor jeder Engagement-Antwort, plus ein automatischer Aufräum-Schritt beim Beenden einer Partnerschaft.

**Wie's funktioniert:** Vor jeder möglichen Antwort prüft die KI jetzt zusätzlich denselben Partner-Status, den Moderation, Promos und das Kanal-Beitreten ohnehin schon verwenden — also über eine zentrale, gemeinsame Stelle, nicht über eine eigene Sonderlogik. Durch kommen nur operativ aktive Partner: reines Beobachten, ein Admin-Opt-out oder eine pausierte bzw. beendete Partnerschaft zählen bewusst nicht. Trifft das nicht zu, bleibt sie still — selbst wenn ihr Schalter (noch) auf „an" steht. Als zweite, unabhängige Sicherung wird beim Beenden einer Partnerschaft der Engagement-Schalter des Kanals automatisch ausgeschaltet, genau wie es dort schon mit der Raid-Berechtigung passiert; so bleibt kein „verwaister An-Zustand" in der Datenbank zurück. Damit greifen zwei Schichten: Das Gate fängt jede einzelne Nachricht in Echtzeit ab, der Aufräum-Schritt hält den gespeicherten Zustand sauber — fällt eine Schicht aus, schützt die andere weiter.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #78 — Engagement-AI: lockerer Stammgast statt Deadlock-Roboter

**Problem:** Nach der Themen-Eingrenzung war die KI zu steif — sie redete fast nur noch in ausformulierten Deadlock-Takes und ging auf lockeren Chat-Banter gar nicht mehr ein. Das wirkte wieder nach Bot, nur andersrum.

**Geändert:** Die KI ist wieder ein lockerer Stammgast, der mit dem Chat vibet — und sie schreibt kürzer und trockener, an einem echten Vorbild ausgerichtet.

**Wie's funktioniert:** Deadlock bleibt ihre Stärke und sie ist nur in Deadlock-Streams aktiv, aber sie zwingt das Thema nicht mehr in jede Nachricht: Banter, Reaktionen und Mitlachen sind wieder erlaubt, solange es zur Runde passt. Nachrichten, die klar an den Streamer gerichtet sind, lässt sie weiter aus, und bei fremden Themen spielt sie sich nicht als Experte auf. Für den Schreibstil dient jetzt der echte Chat-Ton eines Stamm-Streamers als Gold-Vorlage (kurz, trocken, viel Banter, oft nur ein paar Wörter) — diese Beispiele stehen im Stil-Vorbild immer ganz vorne, sodass die KI in diesem Register schreibt statt in langen Absätzen.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #77 — Engagement-AI: schweigt, wenn kein Deadlock läuft

**Problem:** Die KI hat auch in Streams geantwortet, die gerade gar kein Deadlock zeigten — bei „Just Chatting", anderen Spielen oder wenn der Kanal offline war. Sie hing nur am Chat-Inhalt, nicht daran, was im Stream tatsächlich lief.

**Geändert:** Ein Stream-Gate vor allem anderen: Die KI wird pro Kanal nur aktiv, wenn der gerade live ist UND als Kategorie Deadlock läuft.

**Wie's funktioniert:** Der Bot pflegt ohnehin für jeden Streamer einen Live-Status samt aktueller Spiel-Kategorie. Bevor die KI überhaupt über eine Antwort nachdenkt, prüft sie diesen Status (kurz zwischengespeichert, damit das nicht jede Nachricht die Datenbank trifft): Ist der Kanal offline oder läuft etwas anderes als Deadlock, bleibt sie komplett still — egal wie deadlock-lastig eine einzelne Chat-Nachricht klingt. So redet sie nur dort, wo wirklich gerade Deadlock gestreamt wird.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #76 — Engagement-AI: nur noch Deadlock, kürzer, mit Streamer-Gespür

**Problem:** Im Mehr-Kanal-Test fiel auf, dass die KI sich überall einmischte — bei Begrüßungen, Resubs, Smalltalk, sogar bei Nachrichten, die klar an den Streamer gerichtet waren. Das wirkte aufdringlich und nach Bot. Dazu zwei technische Sachen: In belebten Chats (mehrere Leute gleichzeitig) brach jede Antwort mit einem Schnittstellen-Fehler ab, und die KI hatte keinerlei Hintergrundwissen über den einzelnen Streamer.

**Geändert:** Klare Eingrenzung aufs Thema, knappere Antworten, ein Fix für den Mehr-Personen-Fehler und ein wachsendes Profil pro Streamer.

**Wie's funktioniert:** Die KI antwortet jetzt nur noch, wenn es im Chat tatsächlich um Deadlock geht (Helden, Matches, Plays, Meta, Patches) — bei reinem Smalltalk, Subs, Begrüßungen oder Off-Topic bleibt sie still. Ist eine Nachricht erkennbar an den Streamer oder eine bestimmte Person gerichtet, hält sie sich raus, weil das nicht ihre Nachricht ist. Antworten sind kürzer und haben mehr Kante statt abwägender Absätze, und auf reine Emotes reagiert sie gar nicht. Der Mehr-Personen-Fehler lag daran, dass der Sprechername in einem separaten Feld mitgeschickt wurde, das die KI-Schnittstelle bei wechselnden Namen ablehnte — jetzt steht der Name direkt im Nachrichtentext, was den Verlauf bei vielen Chattern sogar klarer macht. Zusätzlich destilliert ein Hintergrund-Durchlauf alle paar Stunden pro Kanal ein kurzes Profil aus dem Chat (welche Helden der Streamer spielt, sein Hintergrund, der Community-Vibe, Running-Gags) — reines Kontextwissen, das der KI hilft, sich natürlich einzufügen, ohne es je vorzulesen.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #75 — Engagement-AI: Stats-Gespür + die Soul wächst mit

**Problem:** Nach dem großen Umbau fehlten dem KI-Stammgast noch zwei Sachen. Erstens hatte er kein Gefühl dafür, welcher Held gerade wirklich stark oder beliebt ist — er konnte über die Meta-Stimmung reden, aber nicht über die nackte Stärke einzelner Helden. Zweitens war seine Persönlichkeit statisch: Er konnte sich nichts aus laufenden Gesprächen merken.

**Geändert:** Zwei neue Bausteine — echte Spielstatistiken als Stärke-Anhaltspunkt pro Held, und ein „Gedächtnis", das sich aus den Gesprächen selbst füllt.

**Wie's funktioniert:** Wird ein Held erwähnt, zieht die KI dessen aggregierte Win- und Pick-Rate aus der Statistik-Schnittstelle — aber bewusst nur als grobes Gefühl („Winrate über 50%, wird oft gespielt") statt als Zahlen-Tabelle zum Vorlesen. So weiß sie, ob ein Held gerade meta ist. Parallel läuft alle paar Stunden ein Reflexions-Durchgang: Die KI schaut sich die letzten Chats an, in denen sie mitgemischt hat, und merkt sich — wenn etwas hängen blieb (ein gutes Gespräch, ein Running Gag, ein cooler Move, ein lustiges Wort) — eine kurze Notiz. Diese Notizen hängen unter ihrer Grund-Persönlichkeit und werden später nur beiläufig aufgegriffen, wenn es gerade passt — nicht ausgepackt. So fühlt sich der Charakter mit der Zeit lebendiger an, statt jeden Tag bei null zu starten.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #74 — Engagement-AI: großer Qualitäts-Umbau gegen „klingt wie ein Bot"

**Problem:** Der KI-Stammgast, der sich in den Twitch-Chat einklinken soll, hatte zwei Kernprobleme. Erstens halluzinierte er Deadlock-Fakten — im Test erfand er ein Item samt frei erfundener Mechanik. Zweitens klang er unverkennbar nach KI: Floskeln wie „gute Frage, das kann ich gerade nicht belegen", reflexhaftes Zustimmen zu jeder Meinung, steife ganze Sätze statt Chat-Sprache.

**Geändert:** Der Antwort-Aufbau wurde von Grund auf umgebaut. Die KI bekommt jetzt echte Fakten vorgesetzt und bildet sich darauf eine Meinung, statt aus dem Gedächtnis zu raten — und sie hat einen festen Charakter mit eigenem Geschmack.

**Wie's funktioniert:** Vor jeder Antwort werden mehrere Faktenquellen zusammengezogen und der KI als Beleg mitgegeben: (1) Helden-/Item-Beschreibungen aus dem Deadlock-Wiki, (2) ein Stimmungsbild, das laufend aus den echten Chat-Nachrichten *aller* mitgelesenen Streams destilliert wird („wie fühlt sich die Meta gerade an"), und (3) die echten Änderungen des aktuellen Patches direkt aus den offiziellen Notes. Die KI darf nur über das reden, was in diesen Belegen steht; fehlt ein Beleg, trifft sie keine faktische Aussage, sondern weicht locker aus oder schweigt — statt einen Disclaimer abzulassen. Dazu kam ein fester Charakter („Soul"), den das Modell sich selbst geschrieben hat, plus eigene, aus allen 38 Helden samt Fähigkeiten gebildete Lieblings- und Hass-Helden — die liefern aber nur die Meinung, nicht den Ton: im Chat bleibt die KI knapp und trocken. Eiserne Regeln unterbinden das KI-Gehabe: nie zugeben, eine KI zu sein, nie über die eigene Funktionsweise reden, nicht jeder Meinung hinterherlaufen.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI; am restlichen Bot ändert sich nichts.

## #73 — Highlight-Clips landen jetzt im dedizierten Thread

**Problem:** Der Highlight-Clipper hat fertige Gameplay-Clips in den allgemeinen Bot-Log-Kanal gepostet, wo sie zwischen anderen Bot-Meldungen untergingen.

**Geändert:** Ziel-Kanal auf den dedizierten Highlight-Thread umgestellt.

**Wie's funktioniert:** Der Clipper schickt fertige Clips per interner HTTP-API an den Deadlock-Bots-Prozess, der den Discord-Post übernimmt. Der API-Payload enthält eine `channel_id` — die zeigt jetzt auf den Thread. Discord-Threads verhalten sich aus Bot-Sicht identisch zu normalen Kanälen (gleiches `send()`-Interface), daher war keine weitere Code-Änderung nötig.

## #72 — Interne Blacklist-API komplett: Check, List und Remove

**Problem:** Nach dem ersten Blacklist-Endpunkt (`/add`) fehlten noch die restlichen Operationen — prüfen ob ein Kanal gebannt ist, alle gesperrten Kanäle auflisten und einen Bann wieder aufheben.

**Geändert:** Drei neue Endpunkte auf demselben internen API-Port (8776, nur localhost):

- `GET /raid/blacklist/check?login=kanalname` — antwortet mit `blacklisted: true/false` plus Grund und Zeitstempel falls vorhanden
- `GET /raid/blacklist` — gibt alle aktuell gesperrten Kanäle sortiert nach Eintragsdatum zurück
- `POST /raid/blacklist/remove` — entfernt einen Kanal aus der Blacklist, `removed: true` wenn er tatsächlich drin war

**Wie's funktioniert:** Alle drei Operationen laufen über direktes SQL auf `twitch_raid_blacklist`, in `asyncio.to_thread` gekapselt damit der Event-Loop nicht blockiert. Auth und Loopback-Schutz identisch zu `/add`. Die Endpunkte sind rein additiv — kein bestehender Code-Pfad wurde geändert.

---

## #71 — Interner API-Endpunkt: Twitch-Kanäle manuell in die Raid-Blacklist eintragen

**Problem:** Es gab keinen direkten Weg, einen Twitch-Kanal per API in die Raid-Blacklist einzutragen — der einzige Pfad war ein manuelles SQL-Insert in die Datenbank, was jedes Mal erforderte, den DSN-Secret in eine Shell zu laden.

**Geändert:** Neuer interner HTTP-Endpunkt `POST /internal/twitch/v1/raid/blacklist/add`. Akzeptiert `login` und optional `reason` im JSON-Body. Trägt den Kanal direkt per `ON CONFLICT`-Insert in `twitch_raid_blacklist` ein — idempotent, d.h. ein wiederholter Aufruf überschreibt nur Grund und Zeitstempel, legt keinen Duplikat-Eintrag an.

**Wie's funktioniert:** Der Endpunkt hängt in der bestehenden internen API auf Port 8776, die ausschließlich auf `127.0.0.1` lauscht und durch zwei Middleware-Schichten abgesichert ist: eine Loopback-Prüfung (Verbindungen nur von localhost) und eine Token-Auth via `X-Internal-Token`-Header. Port 8776 ist zusätzlich nicht in der UFW-Allowlist — von extern ist der Port komplett unerreichbar. Die DB-Operation läuft in einem Thread-Pool (`asyncio.to_thread`), damit der async Event-Loop nicht blockiert.

---

## #70 — Viewbot-Spam-Filter: trennscharf gegen Verschleierung, sicherer für echte Zuschauer

**Problem:** Die Viewbot-/SMM-Werbung im Chat (Marke „Ai viewers streamboo. com" und Verwandte) rutschte zuletzt durch, obwohl der Filter die Marke kannte. Der Trick der Spammer: Sie streuen Leerzeichen und Sonderzeichen in die Domain (`streamboo. com`, `s t r e a m b o o . c o m`), wodurch die wörtliche Mustererkennung ins Leere lief — der Treffer reichte nur für „verdächtig", nicht für eine Aktion, und derselbe Spam kam tagelang immer wieder. Gleichzeitig steckte ein latentes Fehlban-Risiko im Filter: generische Wendungen wie „best viewers" zählten so stark, dass ein normales Kompliment an den Streamer („you have the best viewers today") rechnerisch die Ban-Schwelle erreichen konnte.

**Geändert:**
- **Verschleierungs-robuste Erkennung:** Bekannte Spam-Domains werden jetzt erkannt, egal wie viele Leerzeichen oder Trenner dazwischenstehen.
- **Hart/Weich-Trennung:** Signale werden unterteilt in „hart" (echte Spam-Domain/Markenname — kommt in normalem Chat praktisch nie vor) und „weich" (Alltagswörter wie „viewers", „best viewers"). Nur harte Signale lösen automatische Maßnahmen aus.
- **Selbstlernend in beide Richtungen:** Eine KI prüft Grenzfälle und pflegt zwei Listen — eine Spam-Liste für neue/abgewandelte Schreibweisen und eine Schutz-Liste für Fehlalarme (harmlose Wendungen, die fälschlich angeschlagen haben).
- **Generische Floskeln entschärft:** Reine Komplimente wie „best viewers" können allein keinen Bann mehr auslösen.
- **Fremdschrift-Tarnung erkannt:** Buchstaben aus anderen Alphabeten, die wie lateinische aussehen (z. B. kyrillisches „о/е/а" in „strеаmbоо"), werden vor der Prüfung auf normale Buchstaben zurückgeführt.
- **Kontext als Verstärker:** Frisch erstelltes Konto und allererste Nachricht im Kanal erhöhen den Verdacht — aber nur, wenn ohnehin schon ein echtes Spam-Signal vorliegt.

**Wie's funktioniert:** Jede Chat-Nachricht bekommt einen Spam-Score. Ab 3 Punkten wird automatisch gelöscht und gebannt; darunter passiert je nach Signalstärke nichts oder die Nachricht wird (nur bei einem harten Treffer) entfernt, ohne den Schreiber zu bannen. Punkte gibt es u. a. für eine bekannte Spam-Domain (+2) und für das Muster „viewers <Wort>" (+1) — Alltagswörter allein erreichen die Ban-Schwelle also nie. Vor der Bewertung wird der Text „geglättet": eingestreute Leerzeichen und Sonderzeichen werden ignoriert, sodass `streamboo. com` und `s t r e a m b o o` denselben Treffer ergeben wie die saubere Domain. Damit das nicht überschießt, ist die Domain-Erkennung an eine echte Endung gekoppelt (`.com`, `.ru`, …) — harmlose Wortpaare wie „laptop smm" oder „stream boo" lösen daher nichts aus.

Auch Tarnung über fremde Alphabete läuft ins Leere: Zeichen wie das kyrillische „о", das optisch identisch zum lateinischen „o" ist, werden beim Glätten auf den lateinischen Buchstaben zurückgeführt — `strеаmbоо` wird also wie `streamboo` behandelt.

Zwei zusätzliche Kontext-Punkte verschärfen den Verdacht gezielt: ein erst kürzlich erstelltes Twitch-Konto (+1) und die allererste Nachricht eines Accounts in genau diesem Kanal (+1). Beide zählen aber **nur**, wenn bereits ein hartes Spam-Signal vorliegt — ein bekannter Spam-Bot, der mit frischem Konto seine erste Nachricht absetzt, kippt damit über die Ban-Schwelle, während ein neuer echter Zuschauer mit einer harmlosen Begrüßung garantiert unberührt bleibt.

Die KI-Prüfung läuft im Hintergrund: Schlägt der Filter an, ohne sicher zu sein, bewertet die KI nach, ob es echte Viewbot-Werbung ist. Bestätigt sie es, lernt das System die neue Schreibweise und erkennt sie beim nächsten Mal sofort. Sagt die KI „kein Spam", wandert das auslösende Alltagswort auf die Schutz-Liste und senkt den Score solcher Nachrichten künftig wieder — **außer** es kommt zusätzlich ein hartes Spam-Signal hinzu. Diese Vorrang-Regel verhindert, dass jemand ein harmloses Wort an eine echte Spam-Domain anhängt, um den Filter auszuhebeln.

**Für Zuschauer wichtig:** Automatische Banns treffen ausschließlich klar erkennbare Viewbot-Werbung mit echter Spam-Domain. Ein normales Gespräch über „viewers" oder ein Kompliment an den Streamer löst keine Maßnahme aus. Sollte trotzdem jemand fälschlich getimeoutet oder gebannt werden: Das ist sehr unwahrscheinlich, aber kein Bann ist endgültig — meldet euch beim Streamer oder einem Mod (Rückgängigmachen per `!unban`), wir schauen es uns an. Genau für solche Fälle gibt es die lernende Schutz-Liste.

---

## #69 — Chat-Actions im Admin-Dashboard vollständig repariert

**Problem 1 (Senden-Button tut nichts):** Die Streamer-Suchbox hat einen internen Debounce (220 ms). Wenn der Anzeigename vom Login-Slug abweicht (z. B. `"Earl Salty"` vs. `"earlsalty"`), hat der Debounce 220 ms nach dem Klick die Auswahl gecleart — `canSubmit` wurde false, der Button disabled. Fix: Vergleich jetzt gegen Login **und** Anzeigenamen.

**Problem 2 (Fehlermeldung „Twitch Chat Bot ist aktuell nicht verfügbar"):** Bot und Dashboard laufen als getrennte Prozesse. Alle Bot-Operationen (Streamer hinzufügen, entfernen, …) gehen über eine interne REST-API. Chat-Actions hatten diese Brücke nie bekommen — das Dashboard versuchte den Chat-Bot direkt anzusprechen und schlug deshalb immer fehl.

**Geändert:** Die interne API-Kette wurde um eine `partner_chat_action`-Callback-Route erweitert. Der Bot-Prozess führt die eigentliche Chat-Aktion aus (er hat Zugriff auf den Chat-Bot); der Dashboard-Prozess leitet die Anfrage über HTTP weiter. Wenn kein lokaler Chat-Bot vorhanden ist, greift das Dashboard jetzt auf diesen Callback zurück statt mit einer Fehlermeldung abzubrechen.

---

## #69 — Chat-Actions im Admin-Dashboard funktionstüchtig

**Problem:** Auf der Seite `/twitch/admin/community/chat` passierte beim Drücken von "Nachricht senden" scheinbar nichts — der Button blieb inaktiv oder die Aktion wurde nie abgeschickt.

**Ursache:** Die Streamer-Suchbox hat einen internen Debounce (220 ms). Wenn nach einem Klick auf einen Streamer-Vorschlag der Anzeigename (`displayName`) vom Login-Slug abweicht (z. B. `"Earl Salty"` vs. `"earlsalty"`), hat der Debounce die Auswahl 220 ms später wieder gecleart. Danach war kein Streamer mehr ausgewählt, `canSubmit` war false und der Button disabled.

**Geändert:** Die Vergleich-Logik prüft jetzt, ob der Eingabewert mit dem Login **oder** dem Anzeigenamen des ausgewählten Streamers übereinstimmt. Stimmt beides nicht, wird die Auswahl gecleart — stimmt eines, bleibt sie erhalten.

---

## #68 — Zielgerichtete Discord-Promos mit MiniMax-Preset-Auswahl

**Problem:** Alle Promo-Nachrichten waren bisher kanal-weite Announcements — gleicher Text für alle, egal ob jemand seit Monaten dabei ist oder gerade zum ersten Mal reinschaut.

**Geändert:**
- **Preset-System** (neu): 10 definierte Templates — 5 globale Announcements (competitive, community, chill etc.) und 5 user-spezifische Nachrichten mit @mention (welcome, lurker, mates, ranked, new player).
- **MiniMax-Auswahl**: MiniMax bekommt die letzten paar Chat-Messages des Ziel-Users + die Preset-Tags und entscheidet welches Template am besten passt. Kein Freitext — nur Auswahl aus vordefinierten Texten. Timeout 5s, Fallback auf Zufalls-Preset.
- **Stammgast-Ausschluss**: Wer ≥ 10 Chat-Messages in den letzten 30 Tagen hat (Stammgast), wird beim User-Targeting übersprungen. Zielgruppe: neue Chatter und stille Lurker.
- **1x pro User pro Tag**: Jeder User bekommt höchstens einmal täglich einen personalisierten Pitch.
- **Abwechslung Global/User**: Das System wechselt zwischen kanal-weiter Announcement und direktem @mention-Pitch — kein Double-Ping.
- **Vorrang + Fallback**: Targeted Promo hat Vorrang (eigener 15-Min-Cooldown). Feuert er nicht (Cooldown läuft noch), übernimmt das bestehende Activity/Spike-System.

**Wie's funktioniert:** Jeder Promo-Loop-Tick prüft pro Channel ob der targeted 15-Min-Cooldown abgelaufen ist. Wenn ja: aktive Chatter aus dem 8-Min-Aktivitätsfenster filtern, Kandidaten-Check (nicht Stammgast, nicht heute gepitcht, user_id via DB), MiniMax-Aufruf für Preset-Wahl, Message rendern + senden. @mention-Pitches gehen als normale Chat-Message, globale als farbige Announcement.

## #67 — Timeout-Erkennung über EventSub + Stream-Start-Pitch

**Problem:** Ein 10-Minuten-Timeout des Bots wurde über den bisherigen Drop-Code-Ansatz nicht erkannt, weil der Bot in dieser Zeit nichts schreibt. Außerdem kam der Werbefrei-Pitch beim nächsten Promo-Slot statt am nächsten Stream-Start. Pitch-Text war zu generisch.

**Geändert:**
- **Echtzeit-Erkennung via EventSub**: Die bestehende `channel.ban`-Subscription (feuert bei Timeout und permanentem Ban) prüft jetzt ob der Bot selbst das Ziel ist. Bei `is_permanent: false` → Timeout erkannt → TimeoutGuard wird aktualisiert.
- **Pitch zum Stream-Start**: Der Werbefrei-Pitch wird nicht mehr beim nächsten freien Promo-Slot gesendet, sondern 90 Sekunden nach dem nächsten Stream-Start – so ist der Streamer gerade frisch live und liest tatsächlich mit.
- **Klarerer Pitch-Text**: "Beim letzten Stream wurde der Bot in diesem Chat getimed outed 🙈 Falls die automatischen Promo-Nachrichten stören – es gibt ein Werbefrei-Abo…" Klar formuliert, kein Rätselraten.

**Wie's funktioniert:** Der `channel.ban`-Callback im Monitoring-Bot vergleicht die `user_id` des Gebannten mit der Bot-ID (aus `_twitch_chat_bot.bot_id`). Passt sie, trägt er den Timeout im `TimeoutGuard` (Singleton, gleicher Prozess) ein. Wenn der Streamer das nächste Mal live geht, überprüft der Go-Live-Handler ob ein Pitch aussteht, und plant ihn mit 90s Delay als asyncio-Task.

## #66 — Timeout-Schutz, Werbefrei-Pitch und schärfere Promo-Logik

**Problem:** Der Bot hat in Channels, die ihn regelmäßig timeout'en, trotzdem weitergemacht — ohne Konsequenz. Außerdem hat die Deadlock-Beta-Zugangserkennung zu viele Fehlzündungen produziert (z.B. "brauchst mitspieler?" löste fälschlicherweise einen Invite-Hinweis aus). Die Promo-Nachrichten wiederholten sich zu schnell und kamen zu oft.

**Geändert:**
- **Timeout-Tracking** (neu): Wenn der Bot in einem Channel getimed outed wird (Twitch meldet `sender_banned` beim nächsten Send-Versuch), wird das gezählt. Bei 2x an einem Tag oder 5x in einer Woche werden **alle Bot-Funktionen in diesem Channel für 7 Tage deaktiviert**.
- **Werbefrei-Pitch**: Nach einem erkannten Timeout schickt der Bot (sobald er wieder darf) einmalig einen Hinweis auf das Werbefrei-Abo, das Promo-Nachrichten abschaltet.
- **Beta-Zugang-Fix**: "Mitspiel\*"-Signale lösen nur noch einen Invite-Hinweis aus, wenn gleichzeitig ein starkes Zugangs-Signal ("beta", "zugang", "key" etc.) oder expliziter Deadlock-Bezug vorliegt. Damit fällt "moin, brauchst mitspieler?" nicht mehr in die Kategorie.
- **Mehr Nachrichten-Vielfalt**: Der Promo-Pool wuchs von 14 auf 22 Texte (aufgeteilt in 5 Kategorien). Zusätzlich wird die zuletzt gesendete Nachricht beim nächsten Mal ausgeschlossen.
- **Höhere Cooldowns**: Minimaler Promo-Cooldown von 30 auf 45 Min, maximaler von 120 auf 180 Min, globaler Cooldown von 60 auf 90 Min, Attempt-Cooldown von 5 auf 10 Min.

**Wie's funktioniert:** Der `TimeoutGuard` hält eine In-Memory-Liste der Timeout-Zeitpunkte pro Channel (rollendes 7-Tage-Fenster). Überschreitet die Tageszahl ≥ 2 oder die Wochenzahl ≥ 5, wird der Channel für 604.800 Sekunden stumm geschaltet — kein Discord-Post, kein Raid, kein Bot-Ban, keine Promos, nichts. Der Werbefrei-Pitch erscheint maximal einmal pro 24 Stunden pro Channel.

## #65 — Globale Chatter-Bannliste + Discord-Invite-Erkennung

- Bestimmte Nutzer können jetzt global gesperrt werden und werden in jedem Partner-Kanal sofort gebannt, sobald sie dort schreiben
- Discord-Invite-Links (discord.gg/...) werden automatisch im Log als verdächtig markiert
- Normales Reden über Discord oder "mitspielen" wird davon nicht berührt

## #64 — Highlight-Clips zeigen jetzt echte Combos aus dem Replay

- Clips werden ab jetzt nur noch für echte Combo-Kills erstellt — wenn der Spieler kurz vor dem Kill mindestens 2 Abilities eingesetzt hat (z. B. Hook → Sticky Bomb → Uppercut)
- Solo-Kills ohne Combo-Bewegung werden automatisch herausgefiltert
- Combo-Label erscheint im Discord-Clip-Embed (z. B. "Hook → Bomb → Uppercut")
- Für jedes Match wird das Valve-Replay (227 MB) automatisch geladen und mit dem Open-Source-Tool "boon" analysiert — kein manueller Eingriff nötig

## #63 — Highlight-Erkennung deutlich smarter

- Close Situations werden jetzt erkannt: wenn ein Kill und ein eigener Tod nah beieinander liegen (z. B. Kill dann 3 Sekunden später gestorben), wird das als Highlight gewertet
- Teamfights werden jetzt auch geclippt wenn der Spieler nur einen Kill gemacht hat (nicht mehr min. 2 nötig)
- Einzelne, isolierte Kills werden weiterhin nicht geclippt

## #62 — Highlight-Clips zeigen jetzt echte Highlights

- Einzelne Kills werden nicht mehr als Clips verschickt — das war uninteressantes 0815-Gameplay
- Nur noch Double Kills, Triple Kills und Team Fights werden geclippt
- Neue Bezeichnungen: Double Kill, Triple Kill, Quadra Kill statt generischem "Multi Kill"

## #61 — Highlight-Clips landen jetzt wirklich im Discord-Channel

- Clips wurden erstellt aber konnten nicht gesendet werden — Fehler beim Discord-Zugriff behoben
- Clips werden jetzt über den Deadlock-Bot in den Highlight-Channel gepostet

## #60 — Highlight-Clipper: Clip-Download funktioniert jetzt zuverlässig

- Clips konnten bisher wegen eines ffmpeg-Crashes nicht erstellt werden — der Fehler ist behoben
- yt-dlp nutzt jetzt das stabile System-ffmpeg statt einer statischen Binary die abstürzte
- VODs mit eingeschränkten Qualitätsstufen (z. B. Sub-Only-Streams) werden trotzdem verarbeitet

## #59 — Highlight-Clipper funktioniert jetzt für alle Partner-Streamer

- Der automatische Clip-Ersteller lief bisher gar nicht — Twitch-Zugangsdaten wurden falsch gesucht und nie gefunden
- Clips werden jetzt automatisch für alle 18 aktiven Partner-Streamer erstellt, die ihren Steam-Account bereits im Discord verknüpft haben
- Steam-Accounts werden direkt aus der Discord-Verknüpfung übernommen — kein separates Einrichten nötig
- Fertige Clips landen in einem Discord-Channel zur Durchsicht

## #58 — Streamer-Seite lädt jetzt blitzschnell ohne weißen Flash

- Die Streamer-Seite wird beim Build jetzt vollständig als fertiges HTML vorgerendert — Besucher sehen die Seite sofort, ohne kurzen weißen Ladeblitz
- Kein sichtbarer Unterschied für normale User, aber Suchmaschinen und KI-Crawler sehen exakt dasselbe wie echte Besucher — keine versteckten Tricks mehr

## #57 — Streamer-Landingpage komplett SEO-fähig gemacht

- Die Streamer-Seite hatte vorher nur einen leeren Titel ("Twitch-Bot") und keinen lesbaren Inhalt für Google — deshalb hat sie Google bisher gar nicht gecrawlt
- Jetzt mit klarem Titel ("Deadlock Auto-Raid Bot für Twitch — Streamer-Netzwerk"), Hero-Text, Feature-Liste, So-funktioniert-es-Schritten und FAQ-Block, die alle Suchmaschinen direkt lesen können
- Strukturierte Daten (Schema.org) für die Web-App, Breadcrumbs und FAQ ergänzt — damit Google AI Overviews, Bing Copilot und Perplexity die Seite zitieren können
- Vollständige Social-Media-Vorschau (Open Graph + Twitter Card) für Discord, Twitter und WhatsApp ergänzt — Links zeigen jetzt Vorschaubild und Beschreibung
- Statischer Inhalt bleibt für Crawler ohne JavaScript (Bing, Brave, DuckDuckGo) dauerhaft sichtbar, normale Besucher sehen die interaktive Version wie gewohnt

## #56 — Twitch Admin Dashboard komplett neu strukturiert

- Neues Admin-Cockpit auf `/twitch/admin` mit Live-Status auf einen Blick: aktive EventSub-Verbindung, ausstehende OAuth-Reauths, Bot-Uptime und Datenbank-Status
- Sidebar in fünf klare Bereiche gegliedert: Cockpit, Operations, Community, Content & Comms, Money & Compliance — keine endlose flache Linkliste mehr
- Globale Streamer-Suche oben in der Top-Bar — Login eintippen, Enter, direkt in der Detail-Ansicht
- Neue Bereiche im Dashboard: OAuth-Scope-Diff pro Streamer, Bot-Control (Reload, Promo-Mode), Engagement-AI-Steuerung, Chat-Aktionen senden, Roadmap- und Legal-Editor mit Vorschau, Audit-Log über alle Admin-Aktionen
- Das alte Legacy-Dashboard leitet jetzt automatisch auf die neue Oberfläche um — Bookmarks bleiben funktionieren

## #55 — Lade-Bildschirm für Rechtstexte vereinfacht

- Die Bot-Prüfseite zeigt jetzt nur noch einen Spinner mit „Einen Moment bitte …" und einem dezenten Hinweis
- Kein sichtbares Captcha, keine Erklärungstexte mehr — für normale Besucher wirkt es wie ein kurzer Ladevorgang

## #54 — Impressum & Datenschutz jetzt Bot-geschützt, Dashboard erweitert

- Impressum, Datenschutz und AGB sind jetzt durch eine unsichtbare KI-Bot-Sperre geschützt — normale Besucher werden automatisch weitergeleitet, ohne ein Captcha zu sehen
- Admin-Dashboard: Inhalte der Rechtstexte (Impressum, AGB, Datenschutz) und der Roadmap können direkt im Dashboard bearbeitet werden
- Admin-Dashboard: Neue Analytics-Übersicht und überarbeitete Navigation mit erweitertem Sidebar-Menü
- Zugriffs-Log des Dashboards wird jetzt in eine eigene Datei geschrieben (rotierend, max. 5 MB)

## #53 — Spam-Filter präziser und resistenter gegen Unicode-Tricks

- Spam-Schreibweisen mit verschlüsselten Buchstaben (z. B. `sᴛʀᴇᴀᴍbᴏᴏ`, `𝗩𝗶𝗲𝘄𝗲𝗿𝘀`) werden jetzt korrekt erkannt und gebannt
- Die KI-Überprüfung startet nur noch bei echten Spam-Signalen (Phrase, Fragment oder Spam-Domain) — harmlose Viewer-Erwähnungen werden nicht mehr unnötig geprüft
- Von der KI bestätigte Muster fließen sofort als vollwertige Filter-Bedingung in die Spam-Bewertung ein — gleicher Score-Mechanismus wie für manuell gepflegte Einträge

## #52 — Auto-Ban lernt jetzt selbst neue Spam-Muster dazu

- Nachrichten die verdächtig wirken aber noch nicht gebannt werden, prüft MiniMax M2.7 automatisch im Hintergrund
- Wird Spam bestätigt, merkt sich der Bot das Kernmuster (Domain, Phrase, Keyword) dauerhaft in der Datenbank
- Zukünftige Nachrichten mit demselben Muster werden direkt erkannt und gebannt — kein manuelles Nachtragen mehr nötig
- Auch Score-0-Nachrichten mit URLs oder Viewer-Keywords werden geprüft (wie der Miracle-Ghost-Fall)

## #51 — AI-Engagement pro Kanal im Admin-Dashboard steuerbar

- Im Streamer-Detail des Admin-Dashboards gibt es jetzt einen Toggle für den AI-Engagement-Chatter
- Der Schalter zeigt den aktuellen Status (Aktiv/Inaktiv) und wer ihn zuletzt geändert hat
- Änderungen greifen sofort — kein Reload nötig

## #50 — Streamer-Dashboard und Clip-Workflow zentral dokumentiert

- Tarife (Free, Raid Boost, Werbefrei, Analyse Dashboard, Bundles), Free-vs-Paid-Cutoffs, Testphase und Kündigung sind jetzt klar erklärt
- Auch der Clip-Workflow (Approval, Upload, Retention) und das Affiliate-Portal sind dokumentiert — Streamer- und Viewer-Fragen dazu werden vom FAQ-Bot im Discord direkt beantwortet

## #49 — KI-Analyse besser auffindbar im "Was tun?"-Bereich

- Die KI-Analyse ist jetzt ein eigener Reiter neben "Pro Session" und "Empfehlungen" — vorher lag sie ganz unten am Seitenende und war kaum zu finden

## #48 — Analyse-Dashboard aufgeräumt: 14 Tabs werden zu 7

- Statt 14 Tabs gibt es jetzt 7 klar getrennte Bereiche — kein langes Suchen mehr in einer überfüllten Leiste
- Coaching, KI-Analyse und Stream-Reports sind im neuen Bereich "Was tun?" gebündelt — alle Empfehlungen an einem Ort
- Audience, Viewer und Chat liegen jetzt zusammen unter "Publikum"
- Wachstum, Vergleich und die Kategorie-Leaderboards sind unter "Wachstum" zusammengefasst
- Alte gespeicherte Links zu einzelnen Tabs funktionieren weiterhin

## #47 — Analytics-Tour komplett überarbeitet: 5 Schritte durch echte Dashboard-Features

- Tour zeigt jetzt die KPI-Kacheln direkt (Viewer, Follower, Retention) statt nur Tab-Buttons
- Neuer Schritt erklärt das KI-Insights-Panel und was der Bot analysiert
- Wachstum, Chat und Coaching als eigene Schritte mit besseren Erklärungen
- Tour startet jetzt zuverlässig — alter "gesehen"-Status wird beim Seitenwechsel automatisch zurückgesetzt

## #46 — Onboarding-Tour: Übergabe zur Abo-Seite zuverlässiger + Button-Label korrigiert

- Letzter Button im Dashboard-Onboarding heißt jetzt "Zur Abo-Seite" statt "Fertig"
- Beim Wechsel zur Abo-Seite wird die alte "Tour gesehen"-Markierung automatisch gelöscht — Tour startet immer korrekt
- Kein manuelles Löschen von Browserdaten mehr nötig

## #45 — Tour-Robustheit: fehlende Anker überspringen ohne permanente Deaktivierung

- Wenn eine Tour beim Start keine Elemente im DOM findet, wird sie nicht mehr dauerhaft als "gesehen" markiert
- Bisheriges Verhalten: Tour hat sich selbst deaktiviert, obwohl der Nutzer sie nie gesehen hat
- Tour wird beim nächsten Seitenaufruf korrekt nochmal angezeigt, sobald die Seite vollständig geladen ist

## #44 — Pricing-Tour Bugfix: richtige Anker im FeaturePicker

- Pricing-Tour wurde sofort übersprungen weil die Anker auf dem falschen Komponent lagen (PlanCardRedesign statt FeaturePicker)
- Tour-Kacheln zeigen jetzt die drei echten Features: Werbefrei, Raid Boost und Analyse
- Preview-Version: Tour startet erst wenn Pläne vom Server geladen sind (kein Frühstart mit leerem DOM)

## #43 — Automatische Onboarding-Tour durch Dashboard, Pläne und Analytics

- Neue Nutzer werden nach der Dashboard-Tour automatisch zur Abo-Seite weitergeleitet
- Auf der Abo-Seite erklärt eine Spotlight-Tour die drei Plan-Stufen (Free, Basic, Extended) mit je einer kurzen Erklärung
- Nach der Pricing-Tour geht es direkt weiter zum Analyse-Dashboard mit einer eigenen Tour
- Die Analytics-Tour zeigt die wichtigsten Tabs: Übersicht, Streams, Wachstum und Chat
- Alle drei Touren können einzeln übersprungen werden und zeigen sich nur beim ersten Besuch

## #42 — Pricing-Seite überarbeitet: Vergleich korrigiert & Auswahl klarer

- Feature-Vergleichstabelle zeigt jetzt echte Plan-Spalten: Free / Werbefrei / Raid Boost / Analyse / Alles drin — statt ungenauer Tier-Labels
- „Bot-Werbung deaktivieren" steht jetzt korrekt beim Werbefrei-Plan, nicht erst beim Bundle
- Trial-Plan zeigt keine Chat-Werbung-Deaktivierung mehr (war schon im Backend korrekt, ist jetzt auch im Vergleich sichtbar)
- Feature-Kacheln auf der Pricing-Seite haben jetzt einen klaren Hinweis „+ Auswählen" und einen erklärenden Titel darüber

## #41 — Bundle-Preise angepasst

- Werbefrei + Raid Boost: 5,99 → 6,99 €/Mo. (geringerer Rabatt, da Raid Boost von anderen Streamern abhängt)
- Werbefrei + Analyse: 11,49 → 10,49 €/Mo. (2 € Rabatt statt bisher 99 ct)
- Alles drin und Analyse + Raid Boost bleiben unverändert

## #40 — AGB: Name und Links korrigiert

- Name „EarlySalty / Deadlock Partner Network" → „Deutsche Deadlock Community" in AGB und Seitentitel
- Zurück-Button und Footer-Link auf der AGB-Seite zeigen jetzt korrekt auf `/twitch/pricing`
- Kündigung in §5 verlinkt jetzt auf das Dashboard statt auf die veraltete Abo-Seite

## #39 — Jahresplan: Sofortabbuchung, 14 Monate Zugang und Rechtssicherheit

- Jahrestarif wird jetzt direkt beim Kauf abgebucht (kein Trial-Zeitraum mehr)
- Käufer eines Jahresabos erhalten automatisch 2 Bonusmonate on top — insgesamt 14 Monate Zugang
- Widerrufsbelehrung nach § 356 Abs. 5 BGB direkt im Checkout sichtbar und bestätigungspflichtig
- AGB aktualisiert: neuer Abschnitt zu sofortiger Leistungserbringung, Jahresbonus klar beschrieben, „6-Monats"-Option entfernt
- „30 Tage kostenlos testen"-Button auf der Pricing-Seite ist jetzt vollständig funktional (einmalig pro Account)
- Feature-Picker auf der Pricing-Seite ersetzt die alten Plan-Karten

## #38 — Neue Abo-Pläne, Jahrestarif und überarbeitete Pricing-Seite

- Zwei neue Bundle-Pläne: „Werbefrei + Analyse" (11,49 €/Mo.) und „Alles drin" (13,99 €/Mo.)
- Jahrestarif (12 Monate) mit 20 % Rabatt auf allen bezahlten Plänen wählbar
- Pricing-Seite vollständig überarbeitet: übersichtliches Toggle Monatlich/Jährlich, keine Informations-Überflutung
- Alte `/twitch/abbo`-Seite leitet dauerhaft auf `/twitch/pricing` weiter
- Stripe-Produkte und -Preise für die neuen Bundles automatisch angelegt

## #37 — Security-Scanner-Alerts bereinigt

- 271 offene Code-Scanning-Alerts (Semgrep + CodeQL) vollständig bearbeitet
- Echte Bugs behoben: undefinierte Variable in Tests, ungenutzte Variable, Lambda-Zuweisung
- Alle False-Positive-Alerts mit korrekten Suppression-Kommentaren versehen (`# nosemgrep`, `# lgtm[...]`)
- 4 Discord-Snowflake-ID-Alerts als False Positive über GitHub API dismissed

## #36 — Werbefrei-Plan für 3,99 € + besseres Onboarding und FAQ

- Neuer Plan „Werbefrei" für 3,99 €/Monat: Die Bot-Discord-Einladung in deinem Chat ist dauerhaft aus — auch wenn ein Admin gerade einen globalen Aktions-Text aktiv hat
- Combo „Werbefrei + Raid Boost" für 5,99 €/Monat (spart 2 € gegenüber Einzelkauf)
- Onboarding-Tour im Dashboard erweitert: erklärt jetzt auch was der Bot grundsätzlich macht, deinen aktuellen Plan, Werbung-Einstellungen und wo du Hilfe findest
- Neue Sidebar-Sektion „Hilfe" mit Direktlink zur FAQ und Knopf zum Neu-Starten der Tour
- FAQ um drei neue Sektionen ergänzt: „Was macht der Bot eigentlich?", „Pläne & Preise" und „Chat-Werbung"

## #35 — Clips ohne Alterswarnung und Twitch-Embeds funktionsfähig

- Clips von xradoo_ und miracleghost9 ersetzt — beide hatten eine Twitch-Alterswarnung, die das Einbetten verhinderte
- Ersetzt durch Clips von friduzockt und einsbezi aus der Community
- Caddy-Konfiguration repariert: Twitch-Embeds auf der /streamer/-Seite wurden durch eine zu restriktive Content-Security-Policy geblockt — jetzt erlaubt
- Demo-Dashboard kann wieder in die Streamer-Website eingebettet werden (gleiches CSP-Problem behoben)

## #34 — Branding der öffentlichen Website vereinheitlicht

- Titel und Link-Vorschau zeigen jetzt „Deutsche Deadlock Community" statt „EarlySalty"
- Kaputte Clips (gelöschte Medal-Links, tote EarlySalty-Clips) durch echte Community-Clips ersetzt
- Favicon auf allen Seiten einheitlich — EarlySalty-„E" im Bot-Favicon entfernt
- Logos über alle Projekte auf denselben Stand gebracht
- „DDC"-Abkürzung im gesamten öffentlichen Website-Text durch ausgeschriebenen Namen ersetzt

## #33 — Dashboard zeigt Twitch-Auth nach Erstanmeldung korrekt als aktiv

- AutoRaid-Status wird nach der ersten Twitch-Autorisierung sofort als aktiv angezeigt
- Bisher blieb das Dashboard auf „inaktiv", obwohl die Auth korrekt gespeichert war und Raids funktionierten
- Betrifft nur neue Streamer beim allerersten Auth-Vorgang, nicht Re-Auth

## #32 — Bot-Absturz beim Start behoben

- Twitch-Bot und Dashboard starten wieder fehlerfrei
- Fehler trat auf, weil eine neue Storage-Funktion intern vergessen wurde zu verknüpfen

## #31 — GitHub Actions Minutenverbrauch deutlich reduziert

- Fünf tägliche Security-Workflows auf reine Event-Trigger umgestellt (kein Schedule mehr)
- Security-Scans und Secret-Scanning laufen jetzt nur noch bei Push und Pull Request
- Semgrep bricht den Build nicht mehr bei jedem Fund ab — Ergebnisse werden als Artifact gespeichert

## #30 — Sicherheitslücke in Test-Abhängigkeit geschlossen

- pytest in der CI-Test-Pipeline auf Version 9.0.3 angehoben — schließt eine Privilege-Escalation-Lücke über das /tmp-Verzeichnis

## #29 — Security-Scan: 390 Alerts bereinigt

- Sensible Werte (User-IDs, Streamer-Logins, Dateipfade) werden jetzt überall vor dem Logging bereinigt — verhindert Log-Injection
- Discord-Nutzer-IDs im Code sind als öffentliche IDs markiert, nicht als Secrets
- Rund 350 False-Positive-Alerts aus dem Semgrep-Scanner (SQL-Queries, Logger-Credential, HTML-Format, Dynamic-Imports) wurden mit präzisen Suppression-Kommentaren ausgestattet, damit echte neue Probleme künftig auffallen
- 63 unbenutzte Imports, 41 unbenutzte Variablen und weitere kleine Stil-Verstöße automatisch bereinigt

## #28 — Hintergrund-Konsolidierungen (Audit-Cleanup Phase 2)

- Streamer-Plan- und Billing-Lookups laufen jetzt über eine gemeinsame Quelle — ein Test, der wegen Schema-Drift rot war, ist wieder grün
- Schnellere Streamer-Suche im Dashboard durch zwei neue Datenbank-Indexe für case-insensitive Logins
- Datenbank-Pool gehärtet: Default-Verbindungen 4 → 10, Connect-Timeouts gesetzt, automatischer Retry bei seltenen Postgres-Deadlocks
- Internal-API-Server -319 Code-Zeilen schlanker (doppelte Helper-Logik in policy.py konsolidiert)
- Doku auf Stand gebracht: korrekte Routen-Übersicht in INDEX/API.md, Stream-Report-Sektion ergänzt, veraltete „geplant"-Marker entfernt

## #27 — Stabilität verbessert (Audit-Cleanup Phase 1)

- Social-Media-Uploads laufen nicht mehr in endlose Wartezeiten, wenn TikTok-, Instagram- oder Login-Provider hängen — alle externen Calls haben jetzt feste Timeouts
- Bot-Reload entfernt nicht mehr versehentlich noch laufende Hintergrund-Module aus dem Speicher — weniger sporadische Crashes nach Cog-Reloads
- Doppelte HTTP- und KI-Client-Initialisierungen zusammengezogen, künftige Wartung einfacher
- Test- und Linter-Konfiguration vereinheitlicht, fehlende Dependency `cryptography` korrekt deklariert

## #26 — KI Chat-Analyse (MiniMax Deep) funktioniert jetzt

- "Analyse starten"-Button im Chat-Analytics-Dashboard war kaputt — der Backend-Endpoint crashte sofort
- Ursache: fehlendes `import json` im Backend-Modul
- Zusätzlich: TypeScript-Buildfehler behoben (fehlende `streamer`-Prop und Typcast im Donut-Chart)
- Dashboard neu gebaut und Bot neu gestartet

## #25 — Security-Deep-Scan verschlankt, kein Sicherheitsverlust

- Python-Security-, JavaScript-Security- und Semgrep-Scans aus dem Deep-Scan entfernt — diese laufen bereits täglich in der Security-Fortress
- Deep-Scan fokussiert sich jetzt auf Trivy-Filesystem-Scan und OSSF-Scorecard — beides hat keinen Doppelläufer
- Weniger doppelte CI-Minuten, gleiche Abdeckung

## #24 — CI-Laufzeiten optimiert, kein Sicherheitsverlust

- Security-Scans (Container, IaC, Supply-Chain) laufen jetzt wöchentlich statt täglich — Schutz bleibt vollständig durch Push-/PR-Trigger
- Security-Incident-Automation läuft jetzt täglich statt alle 6 Stunden — 75 % weniger Runs
- Dependency-Review hat keinen sinnlosen Tages-Schedule mehr

## #23 — CI-Artifacts werden nach 30 Tagen automatisch gelöscht

- Alle automatisch erzeugten CI-Berichte (Security-Scans, Dependency-Reports, Logs) werden ab jetzt nach 30 Tagen automatisch von GitHub entfernt
- Verhindert, dass sich der GitHub-Actions-Speicher dauerhaft volläuft

# Changelog

## #22 — Werbung sensibler für neue Zuschauer + Viewer-Trigger auch bei Normalzahlen

- Discord-Einladung wird jetzt schon bei 3 Chat-Nachrichten im Fenster ausgelöst statt 5
- Viewer-basierter Trigger greift ab sofort auch wenn die Zuschauerzahl einfach auf normalem Niveau liegt — kein "Spike" mehr nötig
- Cooldown für den Viewer-Trigger auf 60 Minuten gesenkt (war 90)
- Neue Bedingung: Promo wird nur gesendet, wenn mindestens 2 Chatter im aktuellen Fenster die letzte Werbung noch nicht gesehen haben — verhindert, dass dieselbe Zuschauerschaft mehrfach dieselbe Meldung bekommt; nach 2 Stunden gilt ein Zuschauer wieder als "neu"
- Im Admin-Dashboard gibt es jetzt Felder für Start- und Endzeit der globalen Promo-Überschreibung — die zeitbefristete Steuerung funktioniert damit korrekt

## #20 — Stream-Reports: Rating-System, neues Report-Layout + Auto-Retry

- Jeder Report hat jetzt Bewertungs-Buttons (Gut / Neutral / Schlecht) mit optionalem Kommentar — direkt unter dem jeweiligen Report sichtbar
- Reports zeigen jetzt alle Analyse-Abschnitte aus dem neuen Minimax-Schema: Snapshot, Kritische Momente, Audience, Chat-Diagnose, Wachstum, Vergleich und Maßnahmen
- Keine chinesischen Zeichen mehr in Reports — Minimax bekommt jetzt eine explizite Sprachanweisung
- Fehlgeschlagene Reports werden jetzt automatisch alle 30 Minuten bis zu 3x erneut versucht
- Minimax-Anfragen brechen nach 3 Minuten automatisch ab statt ewig zu hängen

## #19 — Stream-Reports für alle Partner freigegeben

- Stream-Reports sind jetzt für alle aktiven Partner sichtbar — kein kostenpflichtiger KI-Plan mehr nötig
- Fehlerbehebung: Dashboard-Service wurde nach Code-Änderungen nicht neu gestartet, Reports haben deshalb nicht geladen

## #18 — Stream-Reports: Neues Analyse-Schema + Minimax komplett aufgedreht

- Report-Prompt komplett neu geschrieben: 5 konkrete Analyse-Aufgaben (Kritische Momente, Audience-Qualität, Chat-Diagnose, Wachstums-Signale, Ehrlicher Vergleich)
- Minimax darf jetzt deutlich mehr schreiben: Token-Limit von 6.000 auf 16.000 erhöht
- Report-Ausgabe folgt jetzt einem klaren deutschen Schema (snapshot, momente, audience, chat_diagnose, wachstum, vergleich, massnahmen)
- Fehler-Fallback passt sich dem neuen Schema an — Dashboard bricht nicht mehr bei Parse-Fehlern

## #17 — Stream-Reports: Backfill beim Start + weitere SQL-Bugfixes

- Beim Bot-Start werden automatisch die letzten 3 Sessions pro Streamer mit einem Minimax-Report nachgefüllt, falls noch keiner existiert
- Wöchentliche Titel-Insights: zweiter SQL-Bug behoben (Session-Lookup ging an falscher Spalte, Sessions wurden nie geladen)
- Bisherige Stream-Reports ohne Fehler werden nicht doppelt generiert

## #16 — Stream-Reports mit Minimax funktionieren jetzt für alle Streamer

- Nach jedem Stream-Ende erstellt Minimax automatisch einen detaillierten Report mit Viewer-Kurve, Chat-Analyse und Vergleich zu früheren Sessions
- Reports werden für alle Streamer generiert — kein kostenpflichtiger Plan mehr nötig, um die Funktion zu nutzen
- Admins können alle Reports im Dashboard einsehen, unabhängig vom Streamer-Plan
- Wöchentliche Titel-Insights waren wegen eines Datenbankfehlers kaputt — dieser ist jetzt behoben
- Tabellen für KI-Reports werden beim ersten Start automatisch angelegt, falls die Migration noch nicht gelaufen ist

## #15 — Voice-Reaction: Bot führt jetzt echte Gespräche nach Outreach & Raids

- Nach jedem Outreach und jedem Outreach-Boost-Raid kann der Bot eigenständig im Streamer-Chat antworten — wie ein echter Community-Mensch, nicht wie ein Sales-Bot
- Der Bot hört kurz in den Stream rein und reagiert sinnvoll auf das, was der Streamer gerade sagt oder im eigenen Chat schreibt
- Bei klar interessierten Streamern landet automatisch eine Discord-Notification beim Team, damit ein Mensch persönlich übernimmt
- Standardmäßig deaktiviert — wird im Staging mit Trockenlauf aktiviert, bevor live geantwortet wird
- Komplettes Audit-Log pro Konversation, damit das Verhalten manuell durchgesehen und nachjustiert werden kann

## #14 — Beta: Auto-Highlight-Clips per Discord DM (EarlySalty)

- Nach jedem Deadlock-Match werden automatisch Highlights erkannt (Triple Kill, Multi Kill, Team Fights)
- Clips werden direkt aus dem Twitch-VOD ausgeschnitten und per Discord DM gesendet
- Erkennt Multi-Kills (≥3 Kills in 10 Sek) und Team-Fights (≥4 Kills in 15 Sek)
- Prüft alle 10 Minuten auf neue Matches der letzten 24 Stunden

## #13 — Bot taucht nicht mehr in Analyse-Daten auf

- Der Bot selbst (`deutschedeadlockcommunity`) wird jetzt überall aus Chat-Statistiken und Analyse-Dashboards herausgefiltert
- Viewer-Rankings, Publikumsauswertungen und Chat-Tiefenanalysen zeigen keine Bot-Einträge mehr

## #12 — Dashboard-Login funktioniert wieder + sauberes Partner-Status-Gating

- Dashboard-Login war komplett kaputt: SQL-Query referenzierte nicht-existierende Spalten (`is_partner`, `archived_at`, `created_at`) und brach mit 503 ab. Login funktioniert jetzt wieder zuverlässig.
- Wer sich erfolgreich einloggt und nicht permanent gesperrt ist, wird automatisch wieder als aktiver Partner geführt — kein manuelles Reset mehr nötig nach Re-Auth.
- Departnered/archivierte Streamer können sich jetzt einloggen und kommen ins Dashboard, sehen aber nur Verwaltung, Pläne und Affiliate-Bereich. Analyse, Social Media und Title-Generator bleiben gesperrt bis ein gültiger Twitch-OAuth durchläuft (außer Bot-Bann oder permanenter Block).
- Verwaltung-Seite zeigt einen klaren Hinweis-Banner mit „Jetzt neu autorisieren"-Button, wenn der Partner-Status nicht aktiv ist.

## #11 — Plan-Preise um 50% gesenkt

- Raid Boost: 7,99 € → **3,99 €** pro Monat
- Analyse Dashboard: 16,99 € → **8,49 €** pro Monat
- Bundle (Analyse + Raid Boost): 22,99 € → **11,49 €** pro Monat
- 6-Monats- und 12-Monats-Tarife folgen automatisch (mit den bestehenden 10% / 20% Mehrjahresrabatten)
- Stripe-Preise synchronisiert; bestehende Abos rechnet Stripe weiter zum bisherigen Betrag ab, neue Buchungen laufen automatisch auf die halbierten Preise

## #10 — Social-Media-Dashboard ist jetzt eine eigene Seite

- Im Analyse-Dashboard gibt es keinen „Social Media"-Tab mehr; das Tooling sitzt unter der eigenen URL `https://deutsche-deadlock-community.de/social-media-admin`
- Die neue Seite hat einen schlanken eigenen Header (Admin-Badge + Rück-Link auf `/analyse`) und zeigt direkt die Clip-Pipeline ohne Tab-Navigation drumherum
- Partner sehen die Seite weiterhin nicht; ohne Admin-Recht kommt eine klare „Admin-Zugriff erforderlich"-Meldung
- Caddy ist um die neue Route erweitert, der Login-Redirect kehrt nach erfolgreicher Twitch-Auth direkt auf das Social-Media-Dashboard zurück

## #9 — Discord-Freigabe für Clips + Auto-Approve pro Plattform

- Fertig angereicherte Clips landen jetzt zuerst in einer Freigabe-Schleife, statt sofort in die Upload-Pipeline zu rutschen
- Ein Admin bekommt pro Clip eine Discord-DM mit Vorschau, plattformspezifischen Hashtags und den Aktionen „Posten", „Bearbeiten" oder „Skip"
- Beim Freigeben lassen sich YouTube Shorts, TikTok und Instagram Reels einzeln auswählen
- Zusätzlich gibt es im Social-Media-Dashboard neue Auto-Approve-Schalter pro Plattform, damit bestimmte Ziele nach einer Freigabe immer automatisch mit in die Queue gelegt werden
- Cross-Posting startet erst nach Freigabe oder Auto-Approve und nicht mehr schon vor dem Approval-Schritt

## #8 — Social-Media-Phase 3: Performance-Tracking, LLM-Reports und Analytics-Tab

- Veröffentlichten Clips werden jetzt pro Plattform in 24h-, 7d- und 30d-Buckets nachgezogen, inklusive Views, Likes, Comments, Shares, Watch-Time, CTR und Engagement-Rate
- Jede Woche kann automatisch ein deutscher LLM-Report für einzelne Streamer entstehen; zusätzlich gibt es einen monatlichen Cross-Streamer-Report sowie einen wöchentlichen Admin-Report per Discord-DM
- Im Admin-Dashboard gibt es jetzt einen eigenen Analytics-Bereich mit Charts pro Clip und einer Report-Liste für gespeicherte Streamer-, Cross- und Admin-Reports
- Migration weiter separat: vor dem ersten Einsatz einmal `python bot/migrations/social_media_phase3_analytics.py` ausführen, damit die Analytics-Spalten und die neue Tabelle `social_media_reports` angelegt werden

## #7 — Social-Media-Dashboard 2.0: Phase 0–2 (Layout-Editor, Auto-Aufbereitung)

- Bestehender Tab „Streams" wurde in „Social Media" umbenannt und ist vorerst nur für Admins sichtbar
- Clips bekommen jetzt automatisch ein vertikales 9:16-Layout mit Game- und Cam-Box, das pro Streamer als Default speicherbar ist und pro Clip übersteuert werden kann
- Eigene MP4s lassen sich direkt im Dashboard hochladen und werden 14 Tage aufbewahrt, bevor sie automatisch aufgeräumt werden
- Neue Auto-Aufbereitung: Clips werden lokal transkribiert, Deadlock-Begriffe (Helden, Items, Abilities, Slang) werden korrigiert und ein lokales LLM (Ollama auf dem Server, kein Datenabfluss) schlägt Title, Description und Hashtags je YouTube/TikTok/Instagram vor
- Externe LLMs (z. B. MiniMax oder Claude Haiku) bleiben standardmäßig aus und werden nur genutzt, wenn ein Admin den Schalter „External-LLM-Consent" ausdrücklich aktiviert
- Migration nicht automatisch — vor dem ersten Lauf einmal `python bot/migrations/social_media_phase2_enrichment.py` ausführen, damit die neuen Tabellen `deadlock_vocab` und `social_media_clip_enrichment` angelegt sind

## #6 — Changelog im Dashboard zeigt jetzt die letzten Updates

- Die Sektion „Was gibt's Neues" auf dem Streamer-Dashboard ist jetzt befüllt
- Alle bisherigen Verbesserungen (#1–#5) sind dort als Einträge sichtbar
- Künftige Updates erscheinen automatisch dort, sobald sie veröffentlicht werden

## #5 — Aktive Tab-Buttons im Analyse-Dashboard ohne kaputten 1px-Halo

- Aktiver Tab (z. B. „Übersicht") hatte einen harten cyan 1px-Strich am Rand, der mit dem Card-Highlight kollidierte und broken aussah
- Border ersetzt durch weichen Inset-Highlight + sanfteren Außen-Glow

## #4 — Glow-Tuning und feines Hintergrund-Grid

- Mini-KPI-Karten (Ø Viewer, Follower, Chat-Aktivität, Stream-Stunden) leuchten jetzt dauerhaft in ihrer Trendfarbe und nicht erst beim Hover
- Health-Score-Karte hat ein deutlich dezenteres Glow, damit es nicht mehr in den Bereich daneben überstrahlt
- Subtiles Gitternetz-Pattern im Dashboard-Hintergrund — wirkt weniger leblos, aber bleibt im Hintergrund

## #3 — Build-Toolchain auf aktuelle Node-LTS aktualisiert

- Build-System läuft jetzt auf Node 22 LTS statt der alten Node-18-Version
- Frontend-Build ist etwas schneller und ohne Versionswarnungen
- Keine Auswirkungen auf die Bot-Funktionalität, rein interne Aufräumung

## #2 — Streamer-Dashboard mit deutlich mehr Vibe

- Karten haben jetzt einen weichen farbigen Glow am Rand und heben sich beim Hover sichtbar an
- Header bekommt eine subtile rotierende Aura im Hintergrund
- Health-Score-Ring leuchtet farblich passend (grün/gelb/rot) mit Drop-Shadow
- Wochen-KPIs bekommen pro Karte eine farbige Trend-Aura (grün bei +, rot bei -)
- Sparkline-Linien glühen leicht in ihrer Trendfarbe
- Last-Stream-Mini-Stats (Ø Viewer, Peak, Follower, Chat) bekommen Hover-Spotlight und Text-Glow
- Activity-Items haben jetzt einen vertikalen Akzent-Streifen, farblich nach Typ (Raid grün, Ban rot, Warnung gelb)
- Letzte Streams Liste hat einen blau-violetten Akzent-Streifen pro Eintrag
- Live-Indikator pulsiert mit zusätzlichem roten Außenglow

## #1 — Streamer-Dashboard schneller, schöner und mit funktionierender Navigation

- Dashboard lädt deutlich schneller (Doppelter API-Request entfernt, Backend in mehrere parallele Aggregationen aufgeteilt)
- Sidebar-Links zu Overview, Streams und Chat funktionieren wieder und springen direkt auf den richtigen Tab
- Beim Laden erscheint sofort eine animierte Vorschau (Skeleton) statt eines leeren Spinners
- Neuer Live-Indikator im Header zeigt, ob du gerade live bist, mit aktueller Viewer-Zahl und Stream-Titel
- Wochen-KPIs (Ø Viewer, Follower, Chat, Stream-Stunden) haben jetzt eine Mini-Trendlinie der letzten 7 Tage
- Neue Sektion „Letzte Streams" listet die letzten 5 Streams mit Datum, Dauer, Ø Viewer, Peak und Follower-Zuwachs
- Aktivitäts-Feed lässt sich nach Raids, Bans und Warnungen filtern und mit „Mehr laden" ausklappen
- Sanftere Hintergrund-Animation, dezentere Optik und mehr Mikro-Animationen in Sidebar, Cards und Listen
