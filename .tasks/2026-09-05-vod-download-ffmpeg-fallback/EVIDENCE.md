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

