# Befund: Aktualität von !rank und Discord-Rangrollen

Stand: 7. Oktober 2026. Nur Diagnose, keine Umsetzung, kein Deploy und keine Rollenänderung.

## Ergebnis

Es gibt mehrere getrennte Ursachen für verzögerte Anzeigen. Ein HTTP-Aufruf durch `!rank` bedeutet derzeit nicht, dass der Rang neu ermittelt wird. Außerdem liefert die native Rangquelle den Rang beim Eintritt ins letzte erfasste Ranked-Match, nicht den Rang nach dessen Abschluss. Alle Caches zu entfernen würde diesen Unterschied nicht lösen.

## 1. Discord-verknüpfte Twitch-Konten erhalten bevorzugt den gespeicherten Rang

`rust/crates/tb-chat/src/rank_lookup.rs:244` fragt den lokalen Steam-Bot ab. Ist die Verbindung bestätigt, besteht die Steam-Freundschaft und ist ein Rangname vorhanden, wird dieser Rang sofort ausgegeben (`:245-254`).

Der Steam-Endpunkt liest den Rang aus `core.steam_links` (`Deadlock-Steam-Bot/rust/crates/steam-web/src/routes/rank.rs:95-104`, `:135-166`). Die Nachabfrage läuft nur, wenn der Rangname fehlt, nicht wenn ein vorhandener Rang alt ist. Bei einem kürzlich bestätigten fehlenden Rang wird die Nachabfrage außerdem zehn Minuten unterdrückt (`:198-204`).

Der Steam-Endpunkt liefert `rank_updated_at`. Der Twitch-Typ `RankInfo` übernimmt dieses Feld nicht (`tb-chat/src/stats.rs:45-59`). Der Twitch-Bot prüft damit das Alter des gespeicherten Ranges nicht.

### Live-Beweis

Ein vorhandener, bestätigter und befreundeter Link mit gespeichertem Rang wurde über `http://127.0.0.1:8783/rank` abgefragt. Die Antwort war `Initiate 4`, `rank_updated_at=1790728157`, also rund 175 Stunden alt. Rang und Zeitstempel waren identisch mit der gespeicherten Zeile. Da ein Rang vorhanden war, wurde dabei keine Nachabfrage ausgelöst.

Zum Messzeitpunkt hatten 328 Links die Voraussetzungen bestätigt plus Steam-Freundschaft. 164 hatten einen Rangnamen. Bei 90 davon lag der gespeicherte Rangzeitstempel mehr als einen Tag zurück, bei 52 mehr als sieben Tage. Diese Zahlen belegen alte gespeicherte Zeitstempel, nicht 90 oder 52 falsche Ränge. Unveränderte Matches werden im Hintergrund bewusst wiederverwendet, ohne diesen Zeitstempel neu zu schreiben.

## 2. Direkt mit Twitch verknüpfte Steam-Konten haben einen anderen Abrufweg

Direkte Twitch-Steam-Links sowie `!rank steam:<ID>` nutzen den öffentlichen Endpunkt `/players/<account_id>/rank` (`tb-chat/src/rank_lookup.rs:202-209`, `:324`).

Dessen Antworten werden im Twitch-Prozess 120 Sekunden gehalten. Rangnamen und Bilder aus `/assets/ranks` werden eine Stunde gehalten, Fehler 15 Sekunden (`:105-127`). Gleichzeitige Anfragen teilen sich einen Abruf. Es bestehen außerdem ein Limit von 16 externen Abrufen pro Minute und die Behandlung von HTTP 429 (`:139-173`).

Die Rangantwort benennt die Quelle bereits als letztes erfasstes Ranked-Match (`:352-360`). Der Abruf aktualisiert keine Discord-Rolle.

## 3. Discord-Rollen werden erst nach einer vollständigen Hintergrundrunde abgeglichen

Der Steam-Bot ist für die Rangrollen zuständig. `Deadlock-Steam-Bot/rust/crates/steam-flows/src/rank.rs:997-1001` begrenzt jeden Batch. Im Live-Journal wurden jeweils acht Accounts pro Batch bei insgesamt 328 geeigneten Links abgefragt.

Die Datenbank kann bereits während der Runde neue Ränge erhalten. Der globale Rollenabgleich findet aber erst am Rundenende statt (`:1182-1210`). Dadurch können gespeicherter Rang und Discord-Rolle zwischenzeitlich auseinanderliegen. Mehrere Steam-Konten je Person werden gemeinsam berücksichtigt, daher darf eine spätere Änderung nicht einfach beliebige einzelne Links zur Rolle machen.

Im Sechs-Stunden-Fenster während der Diagnose: 78 abgeschlossene Batches, insgesamt 618 Account-Abfragen, 292 Wiederverwendungen unveränderter Matches, vier Metadaten-Downloads sowie jeweils vier hinzugefügte und entfernte Rollen. Das sind kumulierte Vorgänge, keine Zahl verschiedener Personen. Die Dienste waren aktiv; der Hintergrundabgleich steht nicht grundsätzlich still.

Kein konkreter Discord-Nutzer mit falscher Rolle wurde im Auftrag genannt. Eine bestimmte Rollenabweichung wurde daher nicht gegen den tatsächlich im Spiel angezeigten Rang bestätigt.

## 4. Die native Datenquelle liegt prinzipbedingt hinter dem Ergebnis des letzten Matches

`Deadlock-Steam-Bot/rust/crates/steam-flows/src/native_rank.rs:84-89` wählt das letzte Ranked-Match. Die Rangstufe wird aus `player_rank_data.initial_display_rank` gelesen (`:170-174`). Das ist der Rang beim Eintritt in dieses Match. Ein Aufstieg oder Abstieg im letzten Match ist aus diesem Feld nicht als neuer Abschlussrang abzulesen.

Bei gleicher Match-ID und vorhandenem Badge wird das gespeicherte Ergebnis erneut verwendet (`:101-105`). Ein frischer Download derselben Metadaten liefert trotzdem denselben Eintrittsrang.

Die Behauptung, Valve pflege die Profilkarten seit dem 30. Juli 2026 nicht weiter, steht als Hintergrund im bestehenden Code. Ein aktueller Live-Vergleich dieser Profilkarten mit dem im Spiel angezeigten Rang war nicht Teil des erbrachten Beweises. Daraus wird hier keine Aussage abgeleitet, dass eine bessere aktuelle Quelle grundsätzlich unmöglich wäre.

## Grundlage für die anschließende Planung

Der spätere Umbau sollte `!rank` als ausdrücklich frischen Abruf behandeln, statt einen vorhandenen Datenbankrang ungeprüft auszugeben. Der bestehende zentrale Steam-Abruf und die bestehende Rollenlogik sind wiederverwendbar. Für bestätigte Discord-Verbindungen sollte geprüft werden, wie ein frisches Ergebnis den Rollenabgleich der betroffenen Person auslösen kann, ohne eine ganze Hintergrundrunde abzuwarten.

Vor einem Versprechen über den exakt aktuellen Spielrang muss eine Quelle nachgewiesen werden, die den Rang nach dem letzten Match liefert. Falls nur der Eintrittsrang des letzten erfassten Matches verfügbar ist, muss die Antwort das offen benennen. Abrufschutz und das Zusammenfassen gleichzeitig laufender identischer Anfragen sind von einem Antwortcache zu trennen. Ein fehlgeschlagener frischer Abruf darf nicht als unveränderter oder fehlender Rang ausgegeben werden und keine Rolle entfernen.

Das ist eine Planungsgrundlage, noch kein freigegebener Implementierungsplan.

## Verifizierte Produktionsstände und Grenzen

- Twitch-Release: `e0b0dbaf662d7680c4ceaa210bf15f1443693cd8`. Die untersuchten Dateien `rank_lookup.rs` und `stats.rs` entsprechen exakt diesem Release-Commit.
- Steam-Release und sauberer Checkout: `4a5c4ff3a38e9a4bf7fe2dbf8de882b00700233d`.
- Discord-Release: `e1f11614e437d5e4e5610f5a9c5d997913f292ad`.
- Keine Secrets ausgegeben, keine Produktionsdaten oder Rollen verändert, keine Twitch-Chatnachricht gesendet.
- Der öffentliche OpenAPI-Abruf antwortete mit HTTP 403. Eine aktuelle Prüfung des externen Rangendpunkts und seiner eigenen Aktualitätsgarantie ist damit offen.
- Graphify zuerst verwendet. Die globalen Queries liefen in Timeouts; lokale Graph-Erklärungen lieferten die verifizierten Fundstellen.

BESTAND[BS-1]: ja | Fundort: rust/crates/tb-chat/src/rank_lookup.rs:188 | Anknüpfung: bestehenden zentralen Steam-Rangabruf und Rollenabgleich wiederverwenden
