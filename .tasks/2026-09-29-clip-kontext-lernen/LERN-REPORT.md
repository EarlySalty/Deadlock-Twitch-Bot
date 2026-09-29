status: aktiv 2026-09-29

# Lernbericht: Kontext um Twitch-Clips

## Vorgehen

Quelle sind echte Deadlock-Clips von earlysalty und Partnerkanälen aus `twitch_clips_social_media`, gelesen aus der Produktionsdatenbank. Gemessen wird das VOD-Fenster vom überlieferten `vod_offset_s` minus 90 Sekunden bis plus 90 Sekunden. `yt-dlp --download-sections` lädt nur diesen Ausschnitt; FFmpeg ermittelt pro Sekunde Lautheit und Spitzenpegel sowie Bildmerkmale. Tesseract liest ausgewählte HUD-Ausschnitte, der Chat wird zeitlich nach Kanal zugeordnet. Die VOD-Dateien und Einzelbilder liegen nur während der Auswertung in einem temporären Verzeichnis und werden anschließend entfernt. Alle neuen Messzeilen und die gelernte Vorlage liegen ausschließlich in der separaten Testdatenbank `tb_clip_context_test`. Die Produktionsdatenbank wurde nicht migriert oder beschrieben.

Historische Clips erhalten ihren Zeitpunkt aus den vorhandenen Twitch-Feldern `video_id` und `vod_offset`. Für zukünftige Chat-Aufrufe erfasst der Bot den Zeitpunkt beim `!clip`-Aufruf und fragt nach dem Anlegen des Clips die VOD-Felder über Helix ab. Fehlende VOD-Felder bleiben ausdrücklich unbekannt; für ältere Einträge gibt es einen einmaligen Helix-Backfill. Die Migration legt `twitch_clip_command_events`, `twitch_clip_context_runs`, `twitch_clip_context_seconds` und `twitch_clip_cut_templates` an. `tb_social_media::clip_context_harvest::recommend_for_clip` liest die gespeicherte Vorlage und liefert Start, Höhepunkt und Ende als VOD-Sekunden; die Upload-Pipeline bleibt unverändert.

Die Vorlage vergleicht die Häufigkeit jedes beobachtbaren Signals im Bereich 35 Sekunden vor bis 10 Sekunden nach dem Clip-Zeitpunkt mit Sekunden, die mindestens 60 Sekunden entfernt liegen. Positive Unterschiede bestimmen die Gewichte. Die Schnittfunktion sucht einen Signalhöhepunkt in der Umgebung des Clip-Zeitpunkts und setzt den Einstieg vor erkennbarer Aktivität sowie das Ende nach einer Reaktionsfrist von mindestens 12 Sekunden ab dem Clip-Aufruf, sofern der VOD-Ausschnitt so weit reicht. Die Schnittpunkte sind Empfehlungen ohne manuell annotierte Qualitätsreferenz und nicht automatisch freigegebene Uploads.

## Messumfang und Signale

35 echte Clips vom 15. bis 28. September 2026 kamen als Kandidaten infrage. Für 31 Clips aus 19 verschiedenen VODs und sechs Kanälen wurden jeweils genau 180 Sekunden gespeichert: insgesamt 5.580 Zeilen. Die Kanäle sind earlysalty (11), marcymcwhy (9), arslanfps (5), itzgenox (4), skene13 (1) und johnnyblazedx (1). Bei 22 der 31 ausgewerteten Clips steht innerhalb von zehn Sekunden um die Twitch-Erstellzeit ein `!clip`- oder `!createclip`-Chatbefehl desselben Kanals. Die übrigen neun sind echte Streamer-Clips, deren Befehlsursprung nicht belegt ist. Drei Paare gemessener Fenster überschneiden sich innerhalb desselben VODs.

Vier Kandidaten konnten nicht ausgewertet werden: Die drei referenzierten VODs `2877380209`, `2873022452` und `2874970394` wurden beim Abruf als nicht vorhanden gemeldet. Fehlversuche erzeugten keine Messzeilen. Nach dem Durchlauf verblieb kein temporäres VOD-Verzeichnis.

| Messung | Beobachtung |
|---|---:|
| Sekunden mit Lautheit und Spitzenpegel | jeweils 5.579 von 5.580 |
| Per OCR ausgewählte Einzelbilder | 1.176 |
| OCR-Zahlen für Souls / erkannte Sprünge ab 120 | 629 / 83 |
| Kill-Feed-Rohtexte / ausdrücklich bestätigte Ereigniswörter | 361 / 0 |
| Als Objective / farbarmes Bild / Szenenwechsel markierte Sekunden | 1 / 219 / 44 |
| Chatnachrichten in den Fenstern | 169 |
| Zeitgestempelte Sprachsegmente | 0, lokaler Dienst nicht erreichbar |

Für jedes Fenster wurde die mittlere Lautheit und die übliche Chat-Aktivität als eigener Bezugspunkt berechnet. Nahe am Clip-Zeitpunkt (−35 bis +10 Sekunden) lagen 32,7 % der Sekunden mindestens 6 LUFS über dem Fenster-Median; weit entfernt (Abstand mindestens 60 Sekunden) waren es 17,6 %. Ein Anstieg um mindestens zwei Chatnachrichten pro Sekunde trat nahebei in 2,3 % und weit entfernt in 0,4 % auf. OCR-Soul-Sprünge traten unter den ausgewählten Bildern nahebei in 4,7 % und weit entfernt in 8,7 % auf; dieses Signal ist hier gerade **kein** positiver Prädiktor.

| Abstand zum Clip-Zeitpunkt | Sekunden mit erhöhtem Pegel | Chatnachrichten |
|---|---:|---:|
| −90 bis −61 s | 18,2 % | 18 |
| −60 bis −31 s | 19,5 % | 14 |
| −30 bis −1 s | 37,2 % | 33 |
| 0 bis +29 s | 26,9 % | 86 |
| +30 bis +59 s | 21,1 % | 13 |
| +60 bis +89 s | 17,0 % | 5 |

Das häufigere laute Spielgeschehen in den letzten 30 Sekunden vor dem Clip und die konzentrierte Chat-Reaktion in den ersten 30 Sekunden danach sind die beobachtbare Signalfolge. Dies ist eine Häufigkeitsmessung, keine kausale oder redaktionell bestätigte Szenenerkennung. Die gespeicherten Gewichte betragen rund 84,9 % Ton, 11,1 % Chat, 2,5 % farbarmes Bild, 1,2 % Objective und 0,4 % Szenenwechsel. Souls, Kill-Feed, Sprache und Lachen haben wegen fehlender positiver beziehungsweise belastbarer Beobachtungen Gewicht null.


Die Vorlage `chat_clip_v1` wurde mit 31 Fenstern gespeichert. Sie empfiehlt 59 Sekunden Vorlauf und 26 Sekunden Nachlauf relativ zum erkannten Höhepunkt. Der Vorlauf wurde innerhalb eines auf 60 Sekunden begrenzten Suchbereichs ermittelt und liegt fast an dessen Grenze: Er ist **keine** redaktionell bestätigte typische Aufbaudauer. Die endgültigen Endpunkte liegen bei diesen 31 Clips 12 bis 32 Sekunden nach dem Chat-Clip-Zeitpunkt, im Mittel 16,6 Sekunden. Ein erster Lernlauf endete bei 25 von 31 Clips weniger als 12 Sekunden nach dem Aufruf. Die korrigierte Schnittfunktion hält nun mindestens zwölf Sekunden und, wenn im folgenden 30-Sekunden-Fenster Chat- oder Sprachreaktionen vorliegen, zusätzlich deren Ausklang fest. Nach erneutem Lernen unterschreiten null von 31 Vorschlägen diese Grenze.

## Echte Beispiele

Alle Angaben sind Sekunden im jeweiligen VOD. Die Abstände in der Zeitlinie beziehen sich auf den Clip-Aufruf und fassen jeweils zehn Messzeilen zusammen; OCR-Soul-Sprünge sind unbestätigte Bildlesungen.

| Clip und Kanal | Beobachtete Zeitlinie | Vorschlag: Start / Höhepunkt / Ende |
|---|---|---|
| [earlysalty: HardDifferentPoxTBTacoLeft](https://www.twitch.tv/earlysalty/clip/HardDifferentPoxTBTacoLeft-0EglD0sRNLf5yLsr), Moment 4000 | −10 bis −1 s: −21,7 LUFS, 3 Chatnachrichten, OCR-Soul-Sprung 861. +10 bis +19 s: −24,4 LUFS, 2 Nachrichten. | 3951 / 4005 / 4032 |
| [itzgenox: InventiveZealousCoffeePrimeMe](https://www.twitch.tv/itzgenox/clip/InventiveZealousCoffeePrimeMe-2s7J8cObfAa-f8bq), Moment 8854 | −20 bis −11 s: −21,6 LUFS, 4 Nachrichten. 0 bis +9 s: 6 Nachrichten; +20 bis +29 s: weitere 3. | 8805 / 8841 / 8880 |
| [marcymcwhy: SpunkySillyVultureFunRun](https://www.twitch.tv/marcymcwhy/clip/SpunkySillyVultureFunRun-J-XIkSFlT_t_bMMR), Moment 12722 | −10 bis −1 s: Pegelspitze −12,5 dBFS. 0 bis +9 s: 3 Nachrichten; +20 bis +29 s: weitere 6. | 12649 / 12710 / 12754 |

Die Empfehlung für marcymcwhy liegt absichtlich 32 Sekunden nach dem Aufruf, weil Chatnachrichten erst spät folgen. Ein unabhängiger Prozess konnte dieselben Schnittpunkte aus der gespeicherten Vorlage per `--recommend` erneut lesen.

## Grenzen

- Der lokale Sprachdienst unter `127.0.0.1:8791` beantwortet `/health` nicht: Sein Prozess `tb-stt-server` steht im Zustand `T` (angehalten), die Listener-Warteschlange ist mit 129 von 128 Plätzen überfüllt. Sprache mit Zeitstempeln, Lachen und Ausrufe wurden deshalb nicht gemessen. Die entsprechenden Datenbankfelder bleiben `NULL`, nicht `false`; die Vorlage darf diese Signale nicht als Negativbelege werten. Der gemeinsam genutzte Dienst wurde nicht eigenmächtig neu gestartet. Es wurden keine Streamdaten an externe Sprachanbieter übertragen.
- Kill-Feed-OCR bei 480p liefert häufig unlesbare Zeichenfolgen. Nur explizit erkennbare Ereigniswörter fließen in die Schnittgewichtung ein. Rohtext ist kein Nachweis für einen Kill. Soul-Zahlen und farbarme Bilder sind ebenfalls fehleranfällige Näherungen und keine bestätigten Spielereignisse.
- Mehrere Clips können aus demselben VOD stammen. Die Anzahl der Clips ist daher nicht die Anzahl unabhängiger Streams. Der genaue VOD- und Überschneidungsumfang steht in den Messzahlen.
- Die gemessene Vorlage existiert nur in der Testdatenbank. Nach Review, Merge und Migration muss die Hauptsession den Lernlauf ausdrücklich für die Produktionsdatenbank ausführen und die Sprachmessung nach Wiederherstellung des lokalen Dienstes nachholen. Paket A schaltet den Uploader nicht durch diesen Branch um.

## Prüfung

- `tb-social-media` mit `--include-ignored`: 254 bestanden, 0 fehlgeschlagen, 0 ignoriert. Der frische Migrations- und Schema-Snapshot-Test von `tb-db` bestand mit 1 von 1 Tests.
- Die gezielten Chat-Verdrahtungstests von `tb-bot` ergaben 23 bestanden und 1 fehlgeschlagen. Der unveränderte Ausgangsstand zeigt für genau diesen Test ebenfalls 0 bestanden und 1 fehlgeschlagen (`chat_wiring.rs:2985`, Bestätigung von `!silentban`); die Clip-Änderung liegt außerhalb seines Pfades. Dieses bestehende Testergebnis wird nicht als grün ausgegeben.
- `cargo clippy` für beide berührten Crates beendete sich erfolgreich; es meldete acht vorhandene Warnungen in anderen Bot- und Abhängigkeitsstellen, keine in den neuen Modulen. Die neuen Dateien bestehen `rustfmt --check`; der bestehende Formatierungsabstand in den beiden geänderten Bestandsdateien blieb unverändert (19 beziehungsweise 1 Fundstellen).
- Die Migration wurde auf einer frischen Testdatenbank ausgeführt; eine zurückgerollte Rollenprobe bestätigte Schreibrechte für `twitchbot` und ausschließlich Leserechte für `twitchdash`. Die Vorlage und ein Vorschlag wurden nach einem Prozessneustart erneut aus der Datenbank geladen.
