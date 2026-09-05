# Evidence

## Bug 1: Resume-Abfrage scheitert mit 411

### Live-Log (deadlock-twitch-bot-rust, 2026-09-05)

Zehn VODs von earlysalty scheitern in ~1,2 s sofort, ohne Byte-Transfer:

    2026-09-05T09:42:36.891Z ERROR tb_vod_archive::worker: VOD fehlgeschlagen
      kanal=earlysalty vod=v2854336768
      fehler=upload: api error: YouTube resume query failed (411): <!DOCTYPE html>
        <title>Error 411 (Length Required)!!1</title>
    ... (v2853130679, v2854652600, v2855186418, v2858314199, v2858173542,
         v2858069769, v2858051384, v2859516020, v2859508854)
    2026-09-05T09:42:37.964Z INFO VOD-Archiv-Lauf beendet kanaele=1 geladen=0 hochgeladen=0 uebersprungen=0

### Codestelle

`rust/crates/tb-social-media/src/uploaders/youtube.rs:511-519` (`resumable_offset`):
PUT auf die Session-URL mit `Content-Range: bytes */<size>` und `.body(Vec::new())`,
ohne expliziten `Content-Length`.

### Diagnose (roher Request, reqwest 0.12.28)

Temporaerer Test mit rohem TcpListener, der die vom Client gesendeten Bytes
mitliest. Ausgabe des aktuellen Codes:

    === ROHER RESUME-REQUEST ===
    PUT /status HTTP/1.1
    content-range: bytes */500
    host: 127.0.0.1:40615
    === ENDE ===

Ergebnis: reqwest sendet bei `.body(Vec::new())` KEINEN `content-length`-Header.
Ohne Content-Length weist das Google-Upload-Frontend die PUT-Statusabfrage mit
411 (Length Required) ab. Das ist die Ursache, nicht ein verlorener Body beim
Retry (der `call`-Wrapper baut den RequestBuilder je Versuch frisch) und nicht
allein die 24-h-Verfallsfrist der Sessions.

### Regressionstests (rot vor dem Fix)

Befehl:

    cd rust && cargo test -p tb-social-media --lib
      resume_abfrage_sendet_content_length_null resume_offset_411_ist_verfallen

Rot vor dem Fix:

- `resume_abfrage_sendet_content_length_null`:
  FAILED, assert `ResumeStand::Fertig` erwartet, tatsaechlich `Verfallen`
  (Mock verlangt `content-length: 0`; ohne den Header liefert wiremock 404,
  der Code liest das als verfallen).
- `resume_offset_411_ist_verfallen`:
  FAILED, assert `ResumeStand::Verfallen` erwartet, tatsaechlich
  `Err(UploadError::Api("YouTube resume query failed (411): ..."))`.

Rot-Lauf siehe `rot-baseline.txt`.

## Bug 2: Entdeckung liefert nichts (geladen=0)

### Live-Log

    2026-09-05T09:42:35.859Z ERROR tb_vod_archive::worker: VODs nicht abrufbar,
      Kanal wird uebersprungen kanal=earlysalty fehler=io: No such file or directory (os error 2)
    2026-09-05T09:42:36.772Z INFO Lade VOD kanal=earlysalty vod=v2832851252 ...
    2026-09-05T09:42:36.774Z ERROR VOD fehlgeschlagen kanal=earlysalty vod=v2832851252
      fehler=io: No such file or directory (os error 2)
    2026-09-05T09:37:30.840Z INFO tb_bot: yt-dlp-Pfad aufgeloest pfad=yt-dlp

### Ursache

`liste_vods` (`twitch.rs:184`) startet yt-dlp ueber `cfg.yt_dlp`. Der Bot loest
den Pfad in `resolve_yt_dlp_path` (`rust/bin/tb-bot/src/main.rs:122`) auf:
YT_DLP_PATH, dann `<cwd>/.venv/bin/yt-dlp`, dann `<HOME>/.local/bin/yt-dlp`,
sonst der blanke Name `yt-dlp`. Fuer den Service-User `twitchbot` gilt:
HOME=/var/lib/deadlock-twitch-bot, WorkingDirectory=/opt/deadlock/twitch/current.
Keiner der festen Pfade existiert, der Resolver faellt auf `yt-dlp` zurueck, und
der systemd-PATH von `twitchbot` enthaelt kein yt-dlp. Der Spawn stirbt mit
ENOENT. Deshalb entdeckt der Worker keine neuen VODs (v2862489221, v2863272984,
v2864193655) und laedt auch die alten nicht.

Belege:

- `sudo -u twitchbot which yt-dlp` -> leer (nicht im PATH).
- yt-dlp existiert nur unter `/home/nathanael/.local/bin/yt-dlp` (User nathanael).
- `/opt/yt-dlp`, `/usr/local/bin/yt-dlp`, `/var/lib/deadlock-twitch-bot/.local/bin/yt-dlp`
  existieren nicht.

Widerlegt: kein MIN_FREE_GB-Gate (kein "Zu wenig Plattenplatz" im Log, 203 GB
frei), keine Helix-Zeitfenster-Frage (Discovery laeuft ueber yt-dlp, nicht
Helix), und keine Blockade durch die 10 Fehl-VODs (offene_vods `store.rs:115`
schliesst nur `uploaded`/`archived` aus, upload_failed wird weiter angefasst;
die 411-Fehler brechen den Lauf nicht ab).

### Fix

Kein tb-vod-archive-Codefehler. Fix ist operativ: yt-dlp fuer den Service-User
erreichbar machen. Der Resolver findet es ohne Codeaenderung, sobald es unter
`/var/lib/deadlock-twitch-bot/.local/bin/yt-dlp` (ausfuehrbar) liegt, oder per
YT_DLP_PATH in der Bot-Runtime-Config, oder als Wrapper in /usr/local/bin analog
`/usr/local/libexec/deadlock-streamlink`. Danach Bot-Neustart, damit der Resolver
neu laeuft. Dieser Schritt liegt ausserhalb des Code-Scopes und der Deploy-
Abstimmung dieses Auftrags und ist im Bericht als offener Punkt gefuehrt.
