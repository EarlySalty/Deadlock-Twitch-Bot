# Social AI quality baseline, 2026-10-26

Measured on live release 69d47eb02. 7 luckymakkydead clips, 47 change requests, run sequentially through the admin API (X-Internal-Token), with the same AI provider and models as production. The test set is in TESTSET.md and the scoring is in RUBRIC.md.

## Mid-run deploy

The subtitle fix b04328b26 went live at 2026-10-26 18:48:21 UTC, while pass 2 was still running. The data is split at that time:

- `runs/baseline/`: every job that started before the restart. dunked and ez have all 9 requests. The other 5 clips have the first 5 requests.
- `runs/baseline/EXCLUDED.json`:
  - Job 112 (clipdesstreams de_remove_subtitle) was killed by the restart with failure_code interrupted.
  - Jobs 115 and 118 were not started by this loop.
  - Job 122 (holybag) failed after the fix with provider_unavailable.
- `runs/subtitles-fix-b04328b26/`: the post-fix revisions on clipdesstreams (113, 114, 116, all provider_unavailable right after the restart) and midair (117, 119, 120, 121). This is a first look at the fix, not a full rerun.
- The owner clip owner_billy (136881) was never run.

## Per-clip baseline scores

| clip | moment | cut | framing | text_de | change_effect | answer_match | render | subtitle_target | tokens | total | requests that took effect |
|---|---|---|---|---|---|---|---|---|---|---|---|
| clipdesstreams | 1 | 0 | 1 | 1 | 0.4 | 1.0 | 2 | 2.0 | 0.0 | 8.4 | start_later no, new_title yes, caption_end no, shorter no, remove_subtitle_en no |
| doorman | 2 | 0 | 1 | 2 | 0.4 | 1.0 | 2 | 2.0 | 0.0 | 10.4 | start_later no, new_title yes, caption_end no, shorter no, remove_subtitle_en no |
| dunked | 2 | 1 | 1 | 1 | 0.2 | 1.0 | 2 | 0.7 | 0.0 | 8.9 | start_later no, new_title yes, caption_end no, shorter no, remove_subtitle_en no, de_remove_subtitle no, de_no_text no, remove_subtitle_clip_en no, no_text_en no |
| ez | 2 | 0 | 1 | 2 | 0.2 | 1.0 | 2 | 0.7 | 0.0 | 8.9 | same as dunked |
| holybag | 1 | 0 | 1 | 2 | 0.4 | 1.0 | 2 | 2.0 | 0.0 | 9.4 | start_later no, new_title yes, caption_end no, shorter no, remove_subtitle_en no |
| midair | 2 | 0 | 1 | 1 | 0.4 | 1.0 | 2 | 0.0 | 0.0 | 7.4 | start_later no, new_title yes, caption_end no, shorter no, remove_subtitle_en no |
| talon | 2 | 1 | 1 | 2 | 0.4 | 1.0 | 2 | 2.0 | 0.0 | 11.4 | start_later no, new_title yes, caption_end no, shorter no, remove_subtitle_en no |
| **mean** | 1.71 | 0.29 | 1.0 | 1.57 | 0.34 | 1.0 | 2.0 | 1.34 | 0.0 | **9.25 of 18** | |

The manual items (moment, cut, framing, text_de) come from `runs/baseline/manual-scores.json`, which has one note per clip. They were scored from the 4-frame contact sheets and the plan data.

subtitle_target is 2.0 on clipdesstreams, doorman, holybag and talon only because remove_subtitle_en ended as revision_unchanged there. Nothing changed, so the title was not touched either.

## Post-fix first look (b04328b26, midair only)

| clip | change_effect | answer_match | render | subtitle_target | tokens | requests that took effect |
|---|---|---|---|---|---|---|
| midair | 2.0 | 1.8 | 2 | 2.0 | 1.6 | de_remove_subtitle yes, de_no_text yes, remove_subtitle_clip_en yes, no_text_en yes |
| clipdesstreams | 0.0 | 0.0 | 2 | 2.0 | 1.5 | all 3 failed with provider_unavailable directly after the restart |

Evidence is in `evidence/postfix-midair-root-v85-v89.jpg`, with rows for the root v66, then v85 "entferne den Untertitel", then v89 "no remove every text in this video no text":

- v85: the captions are gone and the title stays.
- v89: the captions and the title are both gone, and the channel handle @luckymakkydead stays.
- The answers state the change correctly ("Geändert: Untertitel ausgeblendet."), but they are still German for English requests.

## Failure classes, ranked by how many requests they break

1. **Trimming is impossible: 14 of 14 start_later/shorter requests failed.** Jobs 63, 66, 69, 72, 75, 78, 81, 84, 87, 90, 93, 96, 99 and 102 all ended as planning_failed with no AI answer.
   - The cause is the journal line "analysis highlight cut out of the plan". The analysis highlight on every luckymakkydead clip starts at 0 and covers most of the clip.
   - Outputs are always longer than the source, because a hook teaser is added and nothing is cut: ez 11.8 s becomes 15.8 s, doorman 39.3 s becomes 44.4 s, holybag 60.0 s becomes 65.0 s.
2. **Captions cannot be removed: 0 of 15 subtitle/no-text requests took effect.**
   - remove_subtitle_en was revision_unchanged on ez, clipdesstreams, holybag, doorman and talon (jobs 73, 79, 91, 97, 103). It removed the hook title instead on dunked and midair (jobs 67, 85).
   - Case 0 reproduced on dunked: job 104 shrank the captions to 60 % and jobs 105, 106 and 107 removed only the title (`evidence/case0-baseline-root-desub-detext-notext.jpg`).
   - On ez, jobs 108 to 111 all removed the title. Job 111 also shrank the captions.
3. **"Captions at the end" is never possible: 7 of 7 caption_end requests were revision_unchanged** (jobs 65, 71, 77, 83, 89, 95, 101). The plan has no time range for the captions.
4. **Tokens: no baseline job shows a plausible input count.** The card shows input 4 to 6, but each root job has about 26 to 28k cache_creation tokens and up to 9.7k cache_read tokens (root dunked: shown 6, cache_creation 28,316, cache_read 9,716).
5. **Answer language and silence on failure.**
   - Every answer is German, including those to the 4 English requests.
   - planning_failed jobs return no chat answer at all.
   - Root answers offer "kann gekürzt werden" although cuts are blocked.
6. **Clip craft (manual).**
   - The blur-pad layout leaves the gameplay as a band about a third of the screen height.
   - Captions collide with the stream's own chat overlay (midair).
   - Titles are correct but often generic.
   - clipdesstreams new_title changed only the overlay, not the platform titles (job 76).

new_title worked on 7 of 7 clips, and every render succeeded. Root renders take 153 to 492 s and revisions take 20 to 120 s.

## Evidence

- `evidence/case0-*`: the user's before/after screenshots of root job 54, revision job 60 and the job 61 card, plus the baseline reproduction sheet.
- `evidence/baseline-root-<clip>-v<id>.jpg`: a 4-frame contact sheet for each root render.
- `evidence/postfix-midair-root-v85-v89.jpg`: the first post-fix look.
- `evidence/ui-manager-review-list.jpg`: the Social media manager loaded as Admin for luckymakkydead, with the review list. Moli cannot decode video (networkState 1), so playback was checked with ffprobe/ffmpeg on the served file instead.
- Full MP4s and sheets are not committed. They are in `~/.cache/social-ai-quality/<run>/<clip>/`.

## Rerun

```
cd .tasks/2026-10-26-social-ai-quality
python3 -I loop.py --run <name> [--only dunked ez] [--timeout 1800]
python3 -I score.py --run <name>
SAI_PROXY_PORT=18791 MOLI_CDP=http://127.0.0.1:9558 ./ui.sh <steps.json> <outdir>
```

- loop.py resumes `runs/<name>/<clip>.json` and only runs the requests that are missing.
- Copy `runs/baseline/manual-scores.json` and adjust it after looking at the new sheets.
- Run the loop only on luckymakkydead or owner clips, and one job at a time.
- The test jobs show up in luckymakkydead's review list (29 entries after this run).
