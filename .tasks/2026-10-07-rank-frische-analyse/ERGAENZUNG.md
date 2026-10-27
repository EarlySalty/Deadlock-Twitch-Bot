# Ergänzung: Direktabruf, Prüfzeitpunkt und Twitch-Emotes

Stand: 7. Oktober 2026. Ergänzt `BEFUND.md`. Keine Implementierung, kein Deploy, keine Rollenänderung und keine Nachricht in einem Twitch-Chat.

## Wunsch des Nutzers

- Den Prüfzeitpunkt kompakt neben dem Rang anzeigen.
- Erfolgreiche Prüfungen auch bei unverändertem Matchstand zeitlich festhalten.
- Den bestehenden öffentlichen Abrufweg mit seinem Cache vorerst behalten.
- Zunächst ausprobieren, ob ein einzelner Rang direkt abrufbar ist, ohne unsere Match-History-Kette auszuführen.
- Eigene Rangbilder als Twitch-Emotes prüfen.
- Die neue Abrufgrundlage zunächst nicht umsetzen.

## Direktabruf live erfolgreich

Für den bestätigten Steam-Link von EarlySalty wurde einmal der öffentliche Rangendpunkt aufgerufen:

`GET https://api.deadlock-api.com/v1/players/1066648163/rank`

Antwort:

```json
{
  "badge": 76,
  "rank": 7,
  "subrank": 6,
  "last_match": {
    "match_id": 108353960,
    "start_time": 1790631092,
    "player_rank_initial_display_rank": 76,
    "player_rank_initial_flat_progress": 47160,
    "player_rank_final_flat_progress": 47000,
    "player_rank_desired_progress_change": -300,
    "player_rank_initial_calibration_games": 0,
    "player_rank_initial_demotion_protection_games": 0,
    "player_rank_consumed_demotion_protection": true,
    "player_rank_initial_win_streak": 0
  }
}
```

Das entspricht **Emissary VI**. Das letzte von der API erfasste Match begann am 28. September 2026 um 23:31:32 Uhr deutscher Zeit. Die Match-ID und Badge stimmen mit dem gespeicherten nativen Ergebnis überein. Dessen gespeicherter Zeitstempel ist 30. September 2026 um 02:35:17 Uhr.

Unser Bot musste für dieses Experiment keine Match-History laden. Das bedeutet nicht, dass der Dienst den aktuellen Profilrang unmittelbar bei Valve abfragt. Der Endpunkt verwendet die von der Deadlock API erfassten Matchdaten.

Ein frischer direkter Steam-GC-Profilkartenabruf wurde in dieser Diagnose nicht ausgeführt. Die öffentliche Anfrage auf `/v1/players/1066648163/card` scheiterte mit HTTP 403; das belegt keinen fehlenden Rang. Der bereits laufende lokale Steam-Core meldete Steam und GC als verbunden.

## Wichtige Präzisierung des ersten Befunds

Die native Implementierung des Steam-Bots liest weiterhin nur `initial_display_rank`. Der aktuelle öffentliche Rangendpunkt ist aber weiter: Er verwendet für normale Rangstufen den Fortschritt **nach** dem Match und berechnet daraus den Abschlussrang.

Das ist sowohl in der aktuellen OpenAPI-Beschreibung als auch im veröffentlichten Rust-Code belegt:

- `api/src/routes/v1/players/rank.rs:154-161`: `LastRankedMatch::badge()` nutzt `player_rank_final_flat_progress`, soweit vorhanden.
- `:229-244`: Das neueste Ranked-Match wird aus `player_match_stats` ausgewählt.
- `:258-263`: Die API hält dieses Ergebnis selbst zehn Minuten im Cache.
- `:313-314`: Der Einzelendpunkt gibt das aus diesem letzten Match abgeleitete Ergebnis zurück.
- Innerhalb Eternus bleiben die Unterränge von Valves täglichen Verteilungen abhängig. Fehlt der Abschlussfortschritt, fällt die API auf den Eintrittsrang zurück.

Damit wäre die pauschale Aussage falsch, jeder mögliche Abruf liege immer ein Match zurück. Das trifft auf unseren aktuellen nativen Rangpfad zu, nicht grundsätzlich auf den aktuellen öffentlichen Rangendpunkt. Die frühere Aussage über die fehlende Abbildung von Fortschritt auf die Rangstufe darf nicht ungeprüft aus dem alten Codekommentar übernommen werden.

Der Test mit EarlySalty belegt einen erfolgreichen Einzelabruf und die Übereinstimmung beider Wege für diesen Account. Er belegt keinen Rangwechsel über eine Stufengrenze und keinen Abgleich mit einer aktuellen Anzeige im Spiel.

## Prüfzeitpunkt und Rangherkunft getrennt speichern

Der Nutzer möchte sehen, dass der Bot weiter prüft, auch wenn kein neues Match existiert. Dafür sind zwei unterschiedliche Zeitpunkte nötig:

1. **Zuletzt erfolgreich geprüft:** Die letzte erfolgreiche Prüfung, ob ein neuer Match- oder Rangstand vorliegt. Dieser Zeitpunkt darf bei unverändertem Ergebnis steigen.
2. **Rangherkunft:** Das Match oder die Quellenmessung, aus der der gespeicherte Rang stammt. Dieser Zeitpunkt darf nicht durch einen bloßen HTTP-Abruf künstlich verjüngt werden.

Der bestehende Steam-Endpunkt bezeichnet `rank_updated_at` ausdrücklich als Quellenzeitpunkt. Dieses Feld einfach bei jeder Prüfung auf jetzt zu setzen würde diese Bedeutung brechen. Stattdessen sollte die spätere Umsetzung einen eigenen erfolgreichen Prüfzeitpunkt übernehmen und anzeigen.

Mögliche kompakte Anzeige:

`Emissary VI | Zuletzt geprüft: vor 2 Minuten`

Ein Cachetreffer allein ist keine neue Matchprüfung. Ein fehlgeschlagener Prüfversuch darf ebenfalls nicht als erfolgreiche Prüfung zählen. Bei der öffentlichen API bedeutet ein erfolgreicher HTTP-Abruf lediglich, dass ihr Stand abgerufen wurde; ihre eigene zehnminütige Wiederverwendung und die Verzögerung der Match-Erfassung verschwinden dadurch nicht.

## Rangprüfung beim Aufruf von !rank

Für die spätere Planung ist ein einzelner öffentlicher Rangabruf eine bereits vorhandene Möglichkeit. Die verifizierte Steam-ID und der vorhandene öffentliche Client reichen; dafür braucht es keinen zweiten Connector.

Wenn stattdessen tatsächlich das Vorliegen eines neuen Matches bei Valve geprüft werden soll, bleibt der bestehende native Weg relevant: aktuelle Match-History prüfen, die neueste Ranked-Match-ID vergleichen, bei gleicher ID den Rang behalten und nur den erfolgreichen Prüfzeitpunkt aktualisieren; bei neuer ID den Rang neu bestimmen. Der aktuelle nativen Datenquelle entnommene Eintrittsrang und der öffentlich berechnete Abschlussrang müssen dabei bewusst unterschieden werden.

Dies ist eine untersuchte Möglichkeit, noch keine umgesetzte oder abschließend gewählte Abrufstrategie.

## Twitch-Emotes

Ein Bot kann Emotes in einem fremden Twitch-Chat senden. Der Streamer muss sie nicht als eigene Emotes besitzen. Entscheidend ist, ob der **sendende Bot-Account** diese Twitch-Emotes dort verwenden darf.

Twitch-Nachrichten transportieren Emotes als deren exakt geschriebenen Textnamen. Twitch erkennt und rendert den Namen als Bild. Der Bot kann keine beliebige Rang-PNG als Chat-Anhang oder Discord-Custom-Emoji senden.

Für Rangbilder bedeutet das:

- Die Bilder müssten als echte Twitch-Emotes in einem geeigneten Kanal freigeschaltet sein.
- Der Bot benötigt die entsprechende Nutzungsmöglichkeit, beispielsweise eigene freigeschaltete Kanal-Emotes oder zugängliche Abo-Emotes.
- Nur für einen bestimmten Kanal verfügbare Emotes sind keine überall nutzbare Lösung.
- Fremde Streamer brauchen die Rangbilder nicht selbst hochzuladen.
- Drittanbieter-Emotes wie 7TV sind kein universeller Ersatz für Twitch-Emotes; deren Anzeige hängt vom Kanal und den Clients der Zuschauer ab.

Es wurde weder geprüft, welche Rang-Emotes der konkrete Bot-Account bereits freigeschaltet hat, noch eine Nachricht als sichtbarer Emote-Test veröffentlicht.

## Quellen

- [Erfolgreich abgefragter Rangendpunkt](https://api.deadlock-api.com/v1/players/1066648163/rank)
- [Aktuelle OpenAPI-Beschreibung](https://api.deadlock-api.com/openapi.json)
- [Veröffentlichter Rust-Code des Rangendpunkts](https://github.com/deadlock-api/deadlock-api/blob/master/api/src/routes/v1/players/rank.rs)
- [Twitch: Nachrichten und Emotes senden](https://dev.twitch.tv/docs/chat/send-receive-messages/)
- [Twitch: verfügbare Emotes eines Nutzers](https://dev.twitch.tv/docs/api/reference/#get-user-emotes)

BESTAND[BS-1]: ja | Fundort: rust/crates/tb-chat/src/rank_lookup.rs:324 | Anknüpfung: bestehenden öffentlichen Einzelabruf und bestehenden nativen Matchvergleich verwenden
