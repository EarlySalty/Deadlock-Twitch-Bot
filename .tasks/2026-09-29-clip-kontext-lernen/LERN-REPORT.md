status: aktiv 2026-09-29

# Lernbericht: Kontext um Twitch-Clips

## Vorgehen

Quelle sind echte Deadlock-Clips von earlysalty und Partnerkanälen aus `twitch_clips_social_media`, gelesen aus der Produktionsdatenbank. [Twitch definiert `Get Clips.vod_offset` als Anfang des Clips](https://dev.twitch.tv/docs/api/reference#get-clips), nicht als Zeitpunkt des Chat-Aufrufs. Für historische Clips wird der Aufrufzeitpunkt deshalb aus Clip-Anfang plus gerundeter Clip-Dauer angenähert. Um diesen Zeitpunkt wurden jeweils 90 Sekunden davor und danach aus dem VOD gemessen. `yt-dlp --download-sections` lud nur diesen Ausschnitt; FFmpeg ermittelte Lautheit, Spitzenpegel und Bildmerkmale pro Sekunde. Tesseract las ausgewählte HUD-Bereiche, Chatnachrichten wurden nach Kanal und Zeit zugeordnet. Nach der Auswertung verblieben keine temporären VOD-Dateien.

Für zukünftige Chat-Aufrufe erfasst der Bot `requested_at`, den Clip-Anfang und den daraus beim Anlegen berechneten VOD-Zeitpunkt. Der Lernlauf bevorzugt diesen gespeicherten VOD-Zeitpunkt bei passender VOD-ID und verwendet `requested_at` zur Chat-Ausrichtung. Fehlende VOD-Felder bleiben unbekannt; für ältere Einträge gibt es einen einmaligen, seitenweise fortschreitenden Helix-Backfill. Die Migration legt `twitch_clip_command_events`, `twitch_clip_context_runs`, `twitch_clip_context_seconds` und `twitch_clip_cut_templates` an. Alle Messzeilen und die gelernte Vorlage liegen ausschließlich in der separaten Testdatenbank `tb_clip_context_test` im Container `tb-clip-context-20260929` auf Port 32971. Die Produktionsdatenbank wurde weder migriert noch beschrieben.

Die Vorlage vergleicht die Häufigkeit beobachtbarer Signale 35 Sekunden vor bis 10 Sekunden nach dem angenäherten Clip-Aufruf mit Sekunden, die mindestens 60 Sekunden davon entfernt liegen. Positive Unterschiede bestimmen die Gewichte. Die Schnittfunktion sucht einen Signalhöhepunkt, setzt den Einstieg vor erkennbarer Aktivität und lässt nach dem Aufruf mindestens zwölf Sekunden sowie eine Frist für spätere Reaktionen stehen. `tb_social_media::clip_context_harvest::recommend_for_clip` liest Vorlage und Zeitleiste aus der Datenbank und liefert Start, Höhepunkt und Ende als VOD-Sekunden. Dies sind Empfehlungen ohne manuell bewertete Qualitätsreferenz, keine automatisch freigegebenen Uploads.

## Messumfang und Signale

35 echte Clips vom 15. bis 28. September 2026 kamen als Kandidaten infrage. Für 31 Clips aus 19 verschiedenen VODs und sechs Kanälen wurden jeweils genau 180 Sekunden gespeichert: insgesamt 5.580 Zeilen. Die Kanäle sind earlysalty (11), marcymcwhy (9), arslanfps (5), itzgenox (4), johnnyblazedx (1) und skene13 (1). Bei 22 der 31 ausgewerteten Clips steht innerhalb von zehn Sekunden um die Twitch-Erstellzeit ein `!clip`- oder `!createclip`-Chatbefehl desselben Kanals. Die übrigen neun sind echte Streamer-Clips, deren Befehlsursprung nicht belegt ist. Drei Paare gemessener Fenster überschneiden sich innerhalb desselben VODs.

Vier Kandidaten konnten nicht ausgewertet werden: Die drei referenzierten VODs `2877380209`, `2873022452` und `2874970394` wurden beim Abruf als nicht vorhanden gemeldet. Fehlversuche erzeugten keine Messzeilen. Die Ernte hinterließ keine temporären VOD-Dateien.

| Messung | Beobachtung |
|---|---:|
| Sekunden mit Lautheit und Spitzenpegel | jeweils 5.579 von 5.580 |
| Für OCR ausgewählte Einzelbilder | 1.199 |
| OCR-Zahlen für Souls / erkannte Sprünge ab 120 | 570 / 119 |
| Kill-Feed-Rohtexte / ausdrücklich bestätigte Ereigniswörter | 382 / 0 |
| Als Objective / farbarmes Bild / Szenenwechsel markierte Sekunden | 2 / 177 / 41 |
| Chatnachrichten in den Fenstern | 169 |
| Zeitgestempelte Sprachsegmente | 0, lokaler Dienst nicht erreichbar |

Für jedes Fenster wurde die mittlere Lautheit und die übliche Chat-Aktivität als eigener Bezugspunkt berechnet. Nahe am Clip-Zeitpunkt (−35 bis +10 Sekunden) lagen 25,5 % der Sekunden mindestens 6 LUFS über dem Fenster-Median; weit entfernt (Abstand mindestens 60 Sekunden) waren es 21,2 %. Mindestens zwei Chatnachrichten pro Sekunde über dem üblichen Wert traten nahebei in 2,3 % und weit entfernt in 0,4 % auf. OCR-Soul-Sprünge traten unter den ausgewählten Bildern nahebei in 7,1 % und weit entfernt in 13,9 % auf; dieses Signal ist hier kein positiver Prädiktor.

| Abstand zum Clip-Zeitpunkt | Sekunden mit erhöhtem Pegel | Chatnachrichten |
|---|---:|---:|
| −90 bis −61 s | 23,7 % | 18 |
| −60 bis −31 s | 36,9 % | 14 |
| −30 bis −1 s | 25,3 % | 33 |
| 0 bis +29 s | 20,5 % | 86 |
| +30 bis +59 s | 18,5 % | 13 |
| +60 bis +89 s | 19,1 % | 5 |

Das häufigere laute Spielgeschehen 31 bis 60 Sekunden vor dem angenäherten Clip-Aufruf und die konzentrierte Chat-Aktivität in den ersten 30 Sekunden danach sind die beobachtbare Signalfolge. Der `!clip`-Befehl selbst kann in den Chat-Zahlen enthalten sein; diese Messung ist weder ein unabhängiger Chat-Prädiktor noch eine kausale oder redaktionell bestätigte Szenenerkennung. Die gespeicherten Gewichte betragen rund 56,2 % Ton, 24,6 % Chat, 17,6 % farbarmes Bild und 1,5 % Szenenwechsel. Souls, Objective, Kill-Feed, Sprache und Lachen haben wegen fehlender positiver beziehungsweise belastbarer Beobachtungen Gewicht null.

Die Vorlage `chat_clip_v1` wurde mit 31 Fenstern gespeichert. Sie empfiehlt 53 Sekunden Vorlauf und 40 Sekunden Nachlauf relativ zum erkannten Höhepunkt. Der Vorlauf wurde innerhalb eines auf 60 Sekunden begrenzten Suchbereichs ermittelt und ist keine redaktionell bestätigte typische Aufbaudauer. Die endgültigen Endpunkte liegen bei allen 31 Clips mindestens zwölf Sekunden nach dem angenäherten Aufruf: 12 bis 32 Sekunden, im Mittel 17,4 Sekunden. Die Funktion verlängert bei Chat- oder Sprachreaktionen im folgenden 30-Sekunden-Fenster den Schnitt bis nach deren Ausklang.

## Echte Beispiele

Alle Angaben sind Sekunden im jeweiligen VOD. Die Abstände in der Zeitlinie beziehen sich auf den angenäherten Clip-Aufruf und fassen jeweils zehn Messzeilen zusammen; OCR-Soul-Sprünge sind unbestätigte Bildlesungen.

| Clip und Kanal | Beobachtete Zeitlinie | Vorschlag: Start / Höhepunkt / Ende |
|---|---|---|
| [earlysalty: HardDifferentPoxTBTacoLeft](https://www.twitch.tv/earlysalty/clip/HardDifferentPoxTBTacoLeft-0EglD0sRNLf5yLsr), Moment 4030 | −20 bis −11 s: −22,9 LUFS. 0 bis +9 s: −21,4 LUFS, 4 Chatnachrichten und ein OCR-Soul-Sprung. +10 bis +19 s: 2 weitere Nachrichten. | 3969 / 4022 / 4049 |
| [itzgenox: InventiveZealousCoffeePrimeMe](https://www.twitch.tv/itzgenox/clip/InventiveZealousCoffeePrimeMe-2s7J8cObfAa-f8bq), Moment 8884 | −10 bis −1 s: 4 Chatnachrichten. 0 bis +9 s: 6 Nachrichten; +20 bis +29 s: weitere 3. | 8805 / 8857 / 8910 |
| [marcymcwhy: SpunkySillyVultureFunRun](https://www.twitch.tv/marcymcwhy/clip/SpunkySillyVultureFunRun-J-XIkSFlT_t_bMMR), Moment 12752 | −20 bis −11 s: 4 Chatnachrichten. 0 bis +9 s: 4 Nachrichten; +20 bis +29 s: weitere 6. | 12706 / 12755 / 12784 |

Die Empfehlung für marcymcwhy liegt 32 Sekunden nach dem angenäherten Aufruf, weil Chatnachrichten erst spät folgen. Ein unabhängiger Prozess las dieselben drei Schnittpunkte per `--recommend` aus der gespeicherten Vorlage und den Sekundensignalen erneut.

## Grenzen

- Der lokale Sprachdienst unter `127.0.0.1:8791` beantwortet `/health` nicht: Sein Prozess `tb-stt-server` steht im Zustand `T` (angehalten). Sprache mit Zeitstempeln, Lachen und Ausrufe wurden deshalb nicht gemessen. Die entsprechenden Datenbankfelder bleiben `NULL`, nicht `false`; die Vorlage wertet diese Signale nicht als Negativbelege. Der gemeinsam genutzte Dienst wurde nicht eigenmächtig neu gestartet. Es wurden keine Streamdaten an externe Sprachanbieter übertragen.
- Kill-Feed-OCR bei 480p liefert häufig unlesbare Zeichenfolgen. Nur ausdrücklich erkennbare Ereigniswörter fließen in die Schnittgewichtung ein. Rohtext ist kein Nachweis für einen Kill. Soul-Zahlen und farbarme Bilder sind ebenfalls fehleranfällige Näherungen und keine bestätigten Spielereignisse.
- Mehrere Clips stammen aus demselben VOD, und drei Fensterpaare überschneiden sich. 31 Clips sind daher nicht 31 unabhängige Streams. Die für alte Clips verwendete Twitch-Erstellzeit ist eine Annäherung an den Aufruf, und Clip-Anfang plus Dauer ist eine auf Sekunden gerundete Schätzung des VOD-Zeitpunkts.
- Die gemessene Vorlage existiert nur in der Testdatenbank. Nach Review, Merge und Migration muss die Hauptsession den Lernlauf ausdrücklich für die Produktionsdatenbank ausführen und die Sprachmessung nach Wiederherstellung des lokalen Dienstes nachholen. Paket A schaltet den Uploader nicht durch diesen Branch um.

## Prüfung

- `tb-social-media` mit `--all-targets --include-ignored`: 255 bestanden, 0 fehlgeschlagen, 0 ignoriert. Der frische Migrations- und Schema-Snapshot-Test von `tb-db` bestand mit 1 von 1 Tests.
- Die gezielten Chat-Verdrahtungstests von `tb-bot` ergaben 23 bestanden und 1 fehlgeschlagen. Derselbe `!silentban`-Test scheiterte auf dem unveränderten Ausgangsstand mit 0 bestanden und 1 fehlgeschlagen (`left: 1`, `right: 2`); dieser bestehende Fehler wird nicht als grün ausgegeben.
- `cargo check` und `cargo clippy` für beide berührten Crates bestanden. Clippy meldete acht vorhandene Warnungen in anderen Bot- und Abhängigkeitsstellen, keine in den neuen Modulen. Die neuen Rust-Dateien bestehen `rustfmt --check`; die Formatabweichungen in den geänderten Bestandsdateien entsprechen mit 19 beziehungsweise 1 Fundstellen der unveränderten Ausgangsbasis.
- Die Migration wurde auf einer frischen Testdatenbank ausgeführt; eine zurückgerollte Rollenprobe bestätigte Schreibrechte für `twitchbot` und ausschließlich Leserechte für `twitchdash`. In einer separaten leeren Testdatenbank wurde eine veraltete Vorlage beim Lernen ohne Korpus entfernt. Die gelernte Vorlage und drei Vorschläge wurden nach einem Prozessneustart erneut aus der Datenbank geladen.
