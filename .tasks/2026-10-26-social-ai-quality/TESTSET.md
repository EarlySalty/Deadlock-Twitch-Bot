# Social AI quality test set

Source of truth: `testset.json`. Only clips from luckymakkydead (twitch_user_id 1376724506) and the owner channel earlysalty (1186925760) are used, so no other streamer's footage reaches an AI provider.

| key | owner | clip_db_id | length | why it is in the set |
|---|---|---|---|---|
| dunked | luckymakkydead | 140450 | 29.9 s | kill on Vindicta, the clip behind jobs 54, 58, 59, 60 (shorten, remove subtitle) |
| ez | luckymakkydead | 125390 | 11.8 s | short kill, jobs 45, 47, 49, 52 (overlay title remove and replace) |
| clipdesstreams | luckymakkydead | 130466 | 60 s | long clip, job 11 render failure, jobs 53, 57 |
| midair | luckymakkydead | 140447 | 14.4 s | single kill moment in the air |
| holybag | luckymakkydead | 140448 | 59.9 s | long clip with a lot of filler |
| doorman | luckymakkydead | 140452 | 39.3 s | one-shot kill moment in the middle |
| talon | luckymakkydead | 140451 | 8.1 s | very short clip |
| owner_billy | earlysalty | 136881 | 60 s | owner channel, stacked layout with facecam (clip 124199 was dropped: its Twitch source is gone, create returns 404) |

## Case 0: the reported before/after pair

The first end-to-end case is clip 140450 (dunked), reported by luckymakkydead on Discord on 2026-10-26 between 18:13 and 18:20.

- Root job 54, version with the hook title "Dunked on her ahh" at the top and the burned-in caption "Yeah" at the bottom: `evidence/case0-job54-before.jpg`
- Job 60, "remove the subtitle in that clip": the title is gone and the caption "Yeah" is still there: `evidence/case0-job60-after-remove-subtitle.jpg`
- Job 61, "no remove every text in this video no text" on top of job 60: no new version; the card shows input tokens 2, output 1,544 and FFmpeg time "Not yet known": `evidence/case0-job61-card.jpg`. llm_usage for job 61 has tokens_in 2, cache_creation 7,805 and cache_read 3,485.

The loop replays this case on a fresh root job for every clip: the four subtitle and text requests below run against the root version, so the hook title and the captions are both present when the request arrives.

## Change requests

Every clip gets the same nine requests. The first five are chained on the latest successful version; the four marked `root` run against the root version.

| key | text sent | expected effect |
|---|---|---|
| start_later | Fang bitte 3 Sekunden später an. | first kept interval starts 2 to 4.5 s later |
| new_title | Bitte einen anderen Titel. | overlay title or platform titles change |
| caption_end | Nimm die Untertitel am Ende raus. | speech captions in the last part are gone, video otherwise intact |
| shorter | Mach das Video kürzer. | output at least 10 % shorter |
| remove_subtitle_en | remove subtitle | speech captions switched off, hook title untouched, answer in English |
| de_remove_subtitle (root) | entferne den Untertitel | speech captions switched off, hook title untouched |
| de_no_text (root) | kein Text mehr im Video | all burned-in text gone: captions off and overlay title mode none |
| remove_subtitle_clip_en (root) | remove the subtitle in that clip | speech captions switched off, hook title untouched, answer in English |
| no_text_en (root) | no remove every text in this video no text | all burned-in text gone, answer in English |
