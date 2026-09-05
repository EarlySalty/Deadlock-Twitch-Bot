# Contract: VOD-Archiv Resume-411 und leere Entdeckung

## Ziel

Der VOD-Archiv-Worker soll die offenen VODs von earlysalty wieder hochladen und
neue VODs entdecken. Zwei Fehlerbilder sind zu beheben: die Resume-Abfrage bei
YouTube scheitert mit HTTP 411, und der Lauf meldet dauerhaft geladen=0.

## REQ

- REQ-1 Die Resume-Statusabfrage (`resumable_offset`) sendet einen expliziten
  `Content-Length: 0`-Header, damit das Google-Upload-Frontend die Abfrage nicht
  mit 411 (Length Required) abweist.
- REQ-2 Eine verfallene oder unbekannte Upload-Session (HTTP 404, 410 oder 411)
  fuehrt zu `ResumeStand::Verfallen`, sodass der Worker eine neue Upload-Session
  anlegt statt dauerhaft `upload_failed` zu setzen.
- REQ-3 Die echte Ursache der leeren Entdeckung (geladen=0) ist belegt und im
  Bericht mit konkretem Fix genannt.

## INV

- INV-1 Ein HTTP-5xx bleibt ein Fehler und wirft den Upload nicht auf Null
  zurueck (kein Verfallen bei Serverfehler).
- INV-2 Der Worker fasst VODs mit Status `upload_failed` beim naechsten Lauf
  weiter an (offene_vods schliesst nur `uploaded` und `archived` aus).
- INV-3 Kein neuer OAuth-Weg, keine neue Token-Ablage, keine ENV-Datei.

## Nicht-Ziele

- Kein Umbau des resumable Upload-Chunkings.
- Keine Aenderung der YouTube-Ablehnungslogik (15-Minuten-Limit, rejectionReason).
- Kein Refactoring ausserhalb der betroffenen Funktionen.

## Erlaubter Bereich

- rust/crates/tb-vod-archive/
- rust/crates/tb-social-media/src/uploaders/youtube.rs
- .tasks/2026-09-05-vod-resume-411/
