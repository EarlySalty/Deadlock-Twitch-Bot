# Evidence: VOD-Download ffmpeg-Fallback

## Bestandsaufnahme

- Download-Aufruf: `rust/crates/tb-vod-archive/src/twitch.rs:248` (`lade_vod`) ruft `runner.run(&cfg.yt_dlp, &args, cfg.download_timeout)`; bei `!output.success` sofort `VodArchiveError::Werkzeug { schritt: "Download" }`, kein zweiter Versuch.
- Argumentbau: `rust/crates/tb-vod-archive/src/twitch.rs:216` (`download_args`) setzt den internen HLS-Downloader (kein `--downloader`), Ausgabemuster `-o <ziel>/<id>.%(ext)s`.
- yt-dlp-Pfad: `rust/crates/tb-vod-archive/src/config.rs:39` (`yt_dlp`), ffmpeg-Pfad `rust/crates/tb-vod-archive/src/config.rs:40` (`ffmpeg`), Zeitgrenze `rust/crates/tb-vod-archive/src/config.rs:43` (`download_timeout`).
- Worker-Konsequenz: eine gescheiterte Zeile landet auf `download_failed` und wird bei jedem Lauf erneut versucht (Befund aus dem Auftrag, Live-VOD v2862490204 von earlysalty: 1570 Segmente, `EXT-X-MAP` an Playlist-Zeile 8 und 3136).
- Handverifiziert: derselbe Aufruf mit `--downloader ffmpeg --hls-use-mpegts` laedt das VOD vollstaendig (13 GB, 15716 s).

## Regressionstest rot (vor dem Fix)

- Test: `twitch::tests::zweiter_ext_x_map_loest_ffmpeg_fallback_aus` in `rust/crates/tb-vod-archive/src/twitch.rs`.
- Aufbau: Fake-yt-dlp-Skript als `cfg.yt_dlp`; erster Aufruf schreibt `ERROR: Initialization fragment found after media fragments` auf stderr und Exit 1, zweiter Aufruf prueft `--downloader ffmpeg` in den Argumenten und schreibt die Ausgabedatei.
- Befehl: `SQLX_OFFLINE=true cargo test -p tb-vod-archive --lib zweiter_ext_x_map_loest_ffmpeg_fallback_aus`
- Ergebnis: EXIT=101, `test result: FAILED. 0 passed; 1 failed`.
- Fehlermeldung: `called Result::unwrap() on an Err value: Werkzeug { schritt: "Download", meldung: "ERROR: Initialization fragment found after media fragments" }` (twitch.rs:676).

## Fix und gruener Lauf

- Fix: `lade_vod` startet bei Teilstring `Initialization fragment found after media fragments` einen zweiten `runner.run` mit `download_ffmpeg_fallback_args` (`--downloader ffmpeg --hls-use-mpegts --ffmpeg-location <cfg.ffmpeg>`), gleiche Ausgabedatei und `cfg.download_timeout`, einmal `tracing::info!`.
- Test gehaertet: das Fake-Skript prueft jetzt zusaetzlich `--hls-use-mpegts` (REQ-1) und `--ffmpeg-location ffmpeg` (REQ-3); neuer Test `anderer_fehler_loest_keinen_zweiten_versuch_aus` deckt REQ-5 (kein zweiter Versuch, genau ein `run`-Aufruf).
- Gegencheck durch Sabotage: `--hls-use-mpegts` aus dem Fix entfernt -> `zweiter_ext_x_map_loest_ffmpeg_fallback_aus` rot mit `Werkzeug { schritt: "Download", meldung: "kein hls-use-mpegts" }`; Fix danach wiederhergestellt.
- Gruen: `cargo test -p tb-vod-archive` (SQLX_OFFLINE=true, TB_TEST_DATABASE_URL auf Wegwerf-Postgres) -> `test result: ok. 39 passed; 0 failed`.
- `rustfmt --check` auf twitch.rs sauber, `cargo clippy -p tb-vod-archive --tests` ohne Warnung in der Crate.
- Self-Review `gate_hook.py --review`: ALLOW, kein Merge-Blocker; NITs 1/2 sind Live-Verifikationspunkte (ffmpeg-Downloader kennt kein `--limit-rate`; Container/Endung des Fallback-Downloads), NIT 4 (doppelte Zeitgrenze) ist per Contract REQ-2 so gewollt, NIT 5 (noexec-/tmp) auf diesem Host nicht gegeben.


## Nachtrag: Remux nach Fallback (REQ-6..8)

- Befund des Koordinators: 13-GB-VOD mit den Fallback-Args geladen, Datei heisst `.mp4`, `ffprobe` meldet `format_name=mpegts`.
- Fix: `remux_mpegts_nach_mp4` laeuft in `lade_vod` nur nach erfolgreichem Fallback (`ffmpeg_fallback == true`). `remux_args` = `-hide_banner -loglevel error -y -i <datei> -c copy -movflags +faststart <temp>`; bei Erfolg `tokio::fs::rename(temp, pfad)` (atomar, gleiche Platte), bei Misserfolg temp geloescht und `VodArchiveError::Werkzeug { schritt: "Remux" }`. Zeitgrenze `cfg.download_timeout`.
- Test gehaertet: `zweiter_ext_x_map_loest_ffmpeg_fallback_aus` nutzt jetzt zusaetzlich ein Fake-`cfg.ffmpeg`, das `-c copy` und `-movflags +faststart` prueft und den Zielpfad schreibt; der Test weist nach, dass der Remux-Aufruf stattfindet (Inhalt der Zieldatei ist `remuxed`, vorher `mpegts`) und die Zieldatei existiert.
- Sabotage-Gegencheck: `+faststart` im Fix zu `faststartKAPUTT` verfaelscht -> Test rot mit `Werkzeug { schritt: "Remux", meldung: "kein faststart" }`; Fix danach wiederhergestellt.
- Gruen: `cargo test -p tb-vod-archive` (SQLX_OFFLINE=true, Wegwerf-Postgres) -> `test result: ok. 39 passed; 0 failed`.
- `rustfmt --check` sauber, `cargo clippy -p tb-vod-archive --tests` ohne Warnung in der Crate.

## Review-BLOCK behoben: Remux-Entscheidung am Container statt am Prozess-Flag

- Review-Befund (BLOCKING): der Remux hing an `ffmpeg_fallback` (nur in diesem Aufruf). Scheitert der Remux einmal (Platte, Zeitgrenze, Neustart), bleibt die mpegts-Datei unter `v<id>.mp4` liegen; beim naechsten Lauf sieht yt-dlp die fertige Datei ("already been downloaded", Exit 0), `ffmpeg_fallback == false`, kein Remux, und der kaputte Container wird hochgeladen. Der Zustand heilt nie und betrifft auch die vom Koordinator bereits von Hand geladene 13-GB-Datei.
- Ursachen-Fix: `lade_vod` entscheidet den Remux jetzt am tatsaechlichen Container. Nach jedem erfolgreichen Download prueft `ist_mpegts` (ffprobe `format=format_name`), ob `mpegts` gemeldet wird; nur dann laeuft der Remux. Damit heilt der Zustand von selbst (eine liegengebliebene mpegts-Datei wird beim naechsten Lauf erkannt und geremuxt), ein sauberer mp4-Download wird nie geremuxt (REQ-8-Absicht erfuellt). Das Prozess-Flag ist raus.
- Weitere Review-NITs mitbehoben: Temp-Datei wird jetzt auf allen Fehlerpfaden geloescht (auch Zeitgrenze/Spawn-Fehler, NIT 2); der Remux schreibt fest nach `<stamm>.mp4` und entfernt eine abweichende Quelldatei, damit kein mp4-Inhalt unter `.ts` liegen bleibt (NIT 4).
- Tests: `zweiter_ext_x_map_loest_ffmpeg_fallback_aus` bekommt ein Fake-ffprobe (meldet `mpegts`). Neu `remux_fehler_bleibt_download_fehler` (REQ-7: ffmpeg exit 1 -> Fehler Schritt "Remux", Temp geloescht) und `sauberer_mp4_download_wird_nicht_remuxt` (REQ-8: ffprobe meldet `mov,mp4,...` -> ffmpeg wird nie aufgerufen, Originaldatei bleibt).
- Sabotage-Gegencheck: `ist_mpegts` fest auf `true` gezwungen -> `sauberer_mp4_download_wird_nicht_remuxt` rot (ffmpeg-Marke entsteht); danach wiederhergestellt.
- Gruen: `cargo test -p tb-vod-archive` (SQLX_OFFLINE=true, Wegwerf-Postgres) -> `test result: ok. 41 passed; 0 failed`. `rustfmt --check` sauber, `cargo clippy -p tb-vod-archive --tests` ohne Warnung in der Crate.
- Hinweis zu den offenen NITs 1 (ffmpeg-Downloader ignoriert `--limit-rate`) und 3 (kein Platz-Vorabcheck vor dem Remux): NIT 3 ist durch die Selbstheilung entschaerft (ein am Platz gescheiterter Remux wird beim naechsten Lauf erneut versucht); NIT 1 bleibt als Live-Verifikationspunkt.

## Zwischenfall: Worktree und Branch extern geloescht

- Waehrend der Arbeit wurden Worktree `~/.worktrees/tb-vod-ffmpeg-fallback` und Branch `fix/vod-download-ffmpeg-fallback` von aussen entfernt (vermutlich ein Cleanup-Hook/Peer). Die Commit-Objekte b30ca725, d41d3960, 4b9961f3 blieben erhalten; Branch und Worktree aus 4b9961f3 rekonstruiert, danach der Container-Fix erneut angewandt.
