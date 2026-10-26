# Social AI revision trim, 2026-10-26

Measured on live release 270865120 (fix/social-ai-revision-trim, deployed 2026-10-26 19:55 UTC). Same 7 luckymakkydead clips and the same two requests as the baseline: `start_later` ("Fang bitte 3 Sekunden später an.") and `shorter`. Every request runs from the root version, so both requests are measured against the same parent.

## Runs

- `runs/revision-trim-270865120/`: started from the baseline roots (`loop.py --seed-root baseline --from-root --requests start_later shorter`).
- `runs/revision-trim-270865120-r2/`: `shorter` on fresh roots for dunked, ez and midair. Their baseline roots had already used up `max_revisions`, and the API answered 409 "Die erlaubten Versuche sind aufgebraucht."
- `runs/revision-trim-270865120-combined/`: the scored set. It is r1 with those three 409 entries replaced by r2. clipdesstreams `shorter` keeps its 409 entry and counts as not taking effect.

## Before and after for start_later and shorter (14 requests)

| metric | baseline (69d47eb02) | after (270865120) |
|---|---|---|
| requests with a new version | 0 of 14 | 8 of 14 |
| change_effect | 0.0 | 1.14 |
| answer_match | 0.0 | 1.86 |
| cut (manual, mean over 7 clips) | 0.29 | 1.0 |

Cut is scored on the version the streamer ends up with after asking, shown on the contact sheets in `evidence/revision-trim-*`. The notes are in `runs/revision-trim-270865120-combined/manual-scores.json`.

## Jobs and durations

| clip | request | baseline job | after job | parent output | after output | after kept / hook (ms) | answer |
|---|---|---|---|---|---|---|---|
| dunked | start_later | 63 planning_failed | 125 review | 26.8 s | 23.8 s | 3000-22300 / 11000-15500 | Geändert: Start 3 Sekunden später, jetzt 23,8 s lang. |
| dunked | shorter | 66 planning_failed | 136 review (root 135) | 23.5 s | 18.6 s | 6500-22100 / 12000-15000 | Geändert: Start 2,5 Sekunden später, Ende 0,4 Sekunden früher und Vorschau am Anfang geändert, jetzt 18,6 s lang. |
| ez | start_later | 69 planning_failed | 126 review | 15.7 s | 12.7 s | 3000-11833 / 3700-7600 | Geändert: Start 3 Sekunden später, jetzt 12,7 s lang. |
| ez | shorter | 72 planning_failed | 138 review (root 137) | 15.8 s | 10.6 s | 2000-10600 / 7000-9000 | Geändert: Start 2 Sekunden später, Ende 1,2 Sekunden früher und Vorschau am Anfang geändert, jetzt 10,6 s lang. |
| midair | start_later | 81 planning_failed | 128 review | 18.9 s | 15.9 s | 3000-14416 / 9380-13820 | Geändert: Start 3 Sekunden später, jetzt 15,9 s lang. |
| midair | shorter | 84 planning_failed | 140 review (root 139) | 19.2 s | 12.0 s | 2000-14000 / none | Geändert: Start 2 Sekunden später, Ende 0,4 Sekunden früher und Vorschau am Anfang entfernt, jetzt 12 s lang. |
| talon | start_later | 99 planning_failed | 133 review | 8.1 s | 5.2 s | 2880-8128 / none | Geändert: Start 2,9 Sekunden später, jetzt 5,2 s lang. |
| talon | shorter | 102 planning_failed | 134 review | 8.1 s | 4.3 s | 1000-5250 / none | Geändert: Start 1 Sekunde später, Ende 2,9 Sekunden früher und Plattformtexte geändert, jetzt 4,3 s lang. |
| clipdesstreams | start_later | 75 planning_failed | 127 provider_unavailable | 62.5 s | unchanged | | Der KI-Dienst ist derzeit nicht verfügbar. Die bisherige Fassung bleibt unverändert. |
| clipdesstreams | shorter | 78 planning_failed | not run (409, attempts used up) | 62.5 s | unchanged | | |
| holybag | start_later | 87 planning_failed | 129 provider_unavailable | 65.0 s | unchanged | | as clipdesstreams |
| holybag | shorter | 90 planning_failed | 130 provider_unavailable | 65.0 s | unchanged | | as clipdesstreams |
| doorman | start_later | 93 planning_failed | 131 provider_unavailable | 44.3 s | unchanged | | as clipdesstreams |
| doorman | shorter | 96 planning_failed | 132 provider_unavailable | 44.3 s | unchanged | | as clipdesstreams |

ffprobe output durations match the plans (23.9, 12.8, 15.9, 5.3, 4.3, 18.7, 10.7, 12.0 s).

Start frames were checked with ffmpeg PSNR. The first kept frame of each revision was compared with the parent output at the same time plus the requested shift:

| clip | match | unshifted |
|---|---|---|
| dunked v92 | 30.9 dB | 18.5 dB |
| ez v93 | 34.9 dB | 17.1 dB |
| midair v94 | 40.5 dB | 17.4 dB |
| talon v95 | 21.9 dB | 13.4 dB |
| talon v96 | 43.6 dB | 15.6 dB |
| dunked v98 | 38.0 dB | 20.3 dB |
| ez v100 | 22.9 dB | 17.6 dB |
| midair v102 | 26.8 dB | 17.8 dB |

The cut really starts later.

## Open: long clips fail before the AI call

All 5 failed jobs share one cause:

- Every clip over about 40 s fails within 2 to 11 s, before any request reaches tb-claude-relay. The relay logged exactly the 5 successful plans.
- The planning request (system plus text) is larger than `social_ai.provider.max_text_bytes = 16000` in `/var/lib/deadlock-twitch/config/bot.toml`. `validate_request` returns `ErrorKind::InvalidRequest`, and the `From<EditorError>` fallback maps that to `AiEditorError::Provider`, which reaches the streamer as "Der KI-Dienst ist derzeit nicht verfügbar."
- The request grows with clip length through audio_levels (4 per second), segments and the analysis. It also carries the revision instruction:
  - e65c1b6f8 (subtitle fix) grew it to about 2.5 KB, and jobs 113, 114, 116 and 122 already failed this way before this change;
  - this change adds 467 bytes, which pushes doorman (39 s) over the limit too.
- Estimated request size per clip, consistent with the observed split:
  - passes: midair 15.8 KB, talon 16.3 KB, ez 16.6 KB, dunked 17.2 KB;
  - fails: doorman 17.6 KB, holybag 17.8 KB, clipdesstreams 17.8 KB.
  - The estimate overshoots by a constant, so the real threshold lies between dunked and doorman.

Fix options, which need a decision:

- Raise `max_text_bytes`, a provider input limit up to 32000.
- Shrink the planning request, for example by coarser audio levels for long clips.

In both cases InvalidRequest needs its own honest German answer instead of "KI-Dienst nicht verfügbar". The answer_match of 2 for these jobs only means "unchanged" was stated; the reason given is wrong.
