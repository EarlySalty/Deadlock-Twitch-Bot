# Social AI revision request size, 2026-10-26

Fix: a94d19c8b on main, deployed 2026-10-26 22:25 local. Live check: `deploy-twitch-release --pruefen` passed, and `/proc/<pid>/exe` of deadlock-twitch-bot-rust points to `releases/a94d19c8b.../tb-bot`.

## Cause

- Since the subtitle fix e65c1b6f8, every revision of a clip longer than about 40 s built a planning request larger than `social_ai.provider.max_text_bytes = 16000`.
- The provider rejected it with InvalidRequest, and the streamer saw "Der KI-Dienst ist derzeit nicht verfügbar."
- Failed jobs: 113, 114, 116, 127 (root 74); 122, 129, 130 (root 86); 131, 132 (root 92).

## Change

- The revision context sends a compact `previous` plan. It drops review_reasons, which duplicate analysis.uncertainty. It also drops the observation texts of the highlights (only time spans remain), revision_proposal, empty variants and the duplicate clip_title.
- Audio levels are max-pooled to at most 125 values: 500 ms steps for 60 s clips, 1 s steps for 120 s clips.
- The planning request sends only the 6 limits the model uses.
- A pre-check runs before every AI call. An oversized request now ends as `request_too_large` with the message "Dieser Clip ist für die KI-Bearbeitung zu umfangreich. Es wurde keine neue Fassung erstellt.", and the job is not retryable.
- The limit stays at 16000.

## Request size (system plus text bytes, limit 16000)

Measured with the real Rust request builder on the live checkpoints of the failed jobs.

| request | before | after |
|---|---|---|
| revision job 127 (clipdesstreams) | 16559 | 15172 |
| revision job 129 (holybag) | 16480 | 14987 |
| revision job 131 (doorman) | 16354 | 14998 |
| first plan, root 74 | 10651 | 10036 |
| first plan, root 86 | 10543 | 9927 |
| first plan, root 92 | 10302 | 9850 |

Parts of job 127 after the change: system 4335, task 1676, analysis 2064, segments 515, audio_levels 904, limits 138, revision 4729.

The parts that changed:

| part of job 127 | before | after |
|---|---|---|
| revision.previous | 2420 | 1521 |
| revision.instruction | 2902 | 3054 |
| audio_levels | 1371 | 904 |
| limits | 286 | 138 |

The contract test for a synthetic 60 s revision measures 15558 bytes and requires a 256-byte margin.

## Live run `runs/revision-size-a94d19c8b/`

- Roots 74, 86 and 92 had already used 10, 8 and 7 of their 10 attempts, so the run used fresh roots on the same clips.
- Owner was luckymakkydead (1376724506). Nothing was published.
- All four requests ran from the root (`loop.py --from-root --requests start_later shorter de_remove_subtitle de_no_text`).

| clip | request | before | after job | version | ffprobe | change | answer |
|---|---|---|---|---|---|---|---|
| holybag (59.9 s, root 141, v104 54.0 s) | start_later | 129 provider_unavailable | 144 review | v107 | 51.0 s | start +3 s | Geändert: Start 3 Sekunden später, jetzt 51 s lang. |
| holybag | shorter | 130 provider_unavailable | 147 review | v110 | 35.6 s | middle cut 36-52 s | Geändert: Ende 1,5 Sekunden früher, Mittelteil neu geschnitten, Vorschau am Anfang geändert und Plattformtexte geändert, jetzt 35,5 s lang. |
| holybag | de_remove_subtitle | | 150 review | v112 | 54.0 s | subtitles off | Geändert: Untertitel ausgeblendet. |
| holybag | de_no_text | | 152 review | v115 | 54.0 s | subtitles and title off | Geändert: Untertitel ausgeblendet und Titel „Riesiger Seelenbeutel geschnappt“ entfernt. Der Kanalname bleibt als Quellenangabe eingeblendet. |
| clipdesstreams (60 s, root 142, v105 31.6 s) | start_later | 127 provider_unavailable | 145 review | v108 | 28.6 s | start +3 s | Geändert: Start 3 Sekunden später, jetzt 28,5 s lang. |
| clipdesstreams | shorter | not run (409) | 148 review | v111 | 22.0 s | start +5 s, end -3 s | Geändert: Start 5 Sekunden später, Ende 3 Sekunden früher, Vorschau am Anfang geändert und Plattformtexte geändert, jetzt 22 s lang. |
| clipdesstreams | de_remove_subtitle | | 151 review | v114 | 31.6 s | subtitles off | Geändert: Untertitel ausgeblendet. |
| clipdesstreams | de_no_text | | 154 review | v117 | 31.6 s | subtitles and title off | as holybag |
| doorman (39.3 s, root 143, v106 29.0 s) | start_later | 131 provider_unavailable | 146 review | v109 | 26.0 s | start +3 s | Geändert: Start 3 Sekunden später, jetzt 26 s lang. |
| doorman | shorter | 132 provider_unavailable | 149 review | v113 | 23.6 s | start +1.5 s, end -2 s | Geändert: Start 1,5 Sekunden später, Ende 2 Sekunden früher und Vorschau am Anfang geändert, jetzt 23,5 s lang. |
| doorman | de_remove_subtitle | | 153 review | v116 | 29.0 s | subtitles off | Geändert: Untertitel ausgeblendet. |
| doorman | de_no_text | | 155 review | v118 | 29.0 s | subtitles and title off | as holybag |

The two requests from the earlier runs went from 0 of 6 to 6 of 6 for these clips. The two subtitle requests produced a new, changed version in 6 of 6 cases.

- Since the deploy, jobs 141 to 155 all reached review, with no failure_code.
- The bot journal has no plan rejection and no request_too_large since the deploy.

### Frame check

- `evidence/revision-size-a94d19c8b/captions-root-vs-remove-subtitle.jpg` shows root and subtitle-free versions side by side at the frame where they differ most:
  - holybag at 49.5 s: "Yeah, I saw it, I" in the root, gone in v112;
  - doorman at 12.5 s: "Holy what what what what" in the root, gone in v116;
  - clipdesstreams at 23.5 s: "What?!" in the root, gone in v114.
  - The image is otherwise identical.
- `holybag-root-subtitle-notext.jpg` and `doorman-root-subtitle-notext.jpg` stack root, de_remove_subtitle and de_no_text. The overlay title is in the first two rows and gone in the third. The channel name stays as the source credit.
- Frame-wise PSNR between root and de_remove_subtitle is infinite outside the caption frames (holybag 74 of 108, doorman 23 of 58, clipdesstreams 16 of 63 sampled at 2 fps). So only the captions changed.

## Remaining risk

- The live revision request still has about 830 bytes of margin.
- A very long feedback (the UI allows up to 2000 bytes) or full streamer preferences can still exceed the limit.
- That case now ends honestly as `request_too_large` instead of "KI-Dienst nicht verfügbar". No streamer has preferences set today.
