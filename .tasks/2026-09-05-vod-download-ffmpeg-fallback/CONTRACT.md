# Contract: VOD-Download ffmpeg-Fallback bei doppeltem EXT-X-MAP

## Ziel

Twitch-VODs, deren HLS-Playlist nach einer Discontinuity ein zweites
`#EXT-X-MAP` traegt, lassen sich mit dem internen yt-dlp-Downloader nicht laden:
yt-dlp bricht mit `Initialization fragment found after media fragments` ab, die
Zeile bleibt auf `download_failed` und wird bei jedem Lauf erneut vergeblich
versucht. Der Download soll in genau diesem Fall automatisch ein zweites Mal mit
dem ffmpeg-Downloader laufen und so das VOD vollstaendig laden.

## REQ

- REQ-1 Scheitert der normale yt-dlp-Download und enthaelt die Fehlerausgabe den Teilstring `Initialization fragment found after media fragments`, laeuft derselbe Download genau einmal erneut mit `--downloader ffmpeg --hls-use-mpegts`.
- REQ-2 Der zweite Versuch nutzt dieselbe Ausgabedatei und dieselbe Zeitgrenze wie der erste.
- REQ-3 Der ffmpeg-Pfad fuer den zweiten Versuch stammt aus der bestehenden Config `ffmpeg` (VodArchiveConfig.ffmpeg).
- REQ-4 Der Fallback wird einmal pro VOD als `tracing::info!` protokolliert.
- REQ-5 Andere Fehler und der Erfolgsfall bleiben unveraendert; kein zweiter Versuch bei abweichender Fehlermeldung.

## INV

- INV-1 Kein neuer Config-Wert, keine ENV-Datei, kein Secret.
- INV-2 Keine Code-Kommentare in neu geschriebenem Code.
- INV-3 Bestehende Tests bleiben gruen; die rote Baseline aus vorbestehenden Fehlern bleibt unveraendert.

## Nicht-Ziele

- Kein genereller Wechsel auf den ffmpeg-Downloader.
- Keine Aenderung an Live-Pruefung, VOD-Liste, Schnitt oder Upload.
- Keine neue Retry-Logik fuer andere yt-dlp-Fehler.

## Erlaubter Bereich

rust/crates/tb-vod-archive/
.tasks/2026-09-05-vod-download-ffmpeg-fallback/
