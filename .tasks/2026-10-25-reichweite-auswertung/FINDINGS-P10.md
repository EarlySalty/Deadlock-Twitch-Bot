P10 is complete. The coordinator's bot correction is applied to item 4, and `queries-P10.sql` is updated. `FINDINGS-P10.md` could not be written because the harness rejected the Write call, so the full findings follow in this reply.

Saved by the orchestrator from the P10 agent's final report (the agent's Write call was rejected). Queries: `queries-P10.sql`.

**Summary (20 lines or fewer)**
- Scope: all Deadlock category streams, 2026-10-09 to 2026-10-25 (week 3 is partial). Items 1, 2, 3 and 5 use no chat data.
- Bot correction: 221 chatter IDs seen in 30+ channels (21 in 100+, 200 in 30 to 99) are excluded from every chat count. Only item 4 changes.
- German is a small niche: 538 of 36,353 channels (1.5%), 1.4% of streams, 0.5% of viewer-hours. About 79% of German channels average under 5 viewers.
- Only 36 of 8,203 eligible small channels grew strongly (later median at least 10 and at least 3x base). None were German.
- In the matched base band (median 3 to 9), growers and non-growers look similar on live hours, streams per week, session length, tags, affiliate status and followers.
- Growers start evenings more often by median (0.27 vs 0.14), but the means are close (0.39 vs 0.34).
- After Benjamini-Hochberg correction across 1,346 title and tag terms, only "short" survives, at about 5% fewer viewers. No German-word effect is robust.
- Lively chat: the top quartile has 0.66 human chatters per average viewer, the bottom quartile 0.08.
- The broadcaster writes in chat in 56% of lively small channels and 23% of dead ones. This is correlation only.
- Lively channels have more affiliates (68% vs 38%), longer titles (36 vs 29 characters) and more tags (5 vs 4).
- Followers per average viewer: median 59.5 (n=12,990). German median 48.7 (n=189), English 68.1. Among 0 to 4 viewer channels, German 50.2 vs English 58.3 vs Russian 32.0.
- Bot exclusion lowers the overall chat ratio median from 0.378 to 0.332. The quartile gap stays about eightfold.
- The lurker late-sampling correction does not apply to P10, since no lurker or session-chatter table is used.
- The database reported "recovery mode" once during the bot query. The retry succeeded, and a status check confirmed the database was back. Python peak memory stayed under 600 MB.

## FINDINGS P10: small channels in the Deadlock category (all languages)

**Window and data**
- Category snapshots, 2026-10-09 to 2026-10-25 (16 days). Weeks: W1 10-09 to 10-15, W2 10-16 to 10-22, W3 10-23 to 10-25 (partial, 3 days).
- Tables: category_snapshot_samples, category_snapshot_versions, category_channels, category_chat_metrics. Time weight is sample_seconds (60 s for over 99% of rows).
- Small channel: average viewers below 10 and at least 5 live hours in the window (10,882 channels).
- Chatter bots: 221 chatter IDs seen in 30 or more channels in the window are excluded from all chat counts (21 in 100+ channels, 200 in 30 to 99). Shared-chat copies are excluded.
- No lurker or session-chatter table is used, so the late-sampling correction does not apply to P10.

**1. Size and language (36,353 channels, 121,438 streams, 9.07 M viewer-hours)**

Size classes by average viewers: 0-4: 28,273 (77.8%); 5-19: 6,034 (16.6%); 20-99: 1,533 (4.2%); 100+: 513 (1.4%).

| Language | Channels | % channels | % streams | % viewer-hours | Median channel avg |
|---|---|---|---|---|---|
| en | 22,304 | 61.4 | 59.5 | 70.2 | 2.10 |
| ru | 5,789 | 15.9 | 18.7 | 17.3 | 1.45 |
| pt | 2,191 | 6.0 | 5.8 | 2.4 | 1.78 |
| es | 2,049 | 5.6 | 5.0 | 1.7 | 2.02 |
| fr | 1,195 | 3.3 | 2.8 | 4.2 | 2.07 |
| de | 538 | 1.5 | 1.4 | 0.5 | 2.34 |
| ja | 441 | 1.2 | 1.6 | 1.4 | 2.21 |
| pl | 376 | 1.0 | 0.9 | 0.8 | 2.35 |
| th | 298 | 0.8 | 0.9 | 0.2 | 1.81 |
| ar | 243 | 0.7 | 0.7 | 0.1 | 2.29 |

Size shares (0-4 / 5-19 / 20-99 / 100+): en 76.0 / 18.2 / 4.5 / 1.4; ru 83.7 / 10.6 / 4.0 / 1.7; de 78.6 / 16.2 / 3.7 / 1.5. German channels by size: 423, 87, 20, 8 (1,710 German streams).

**2. Growth outliers (eligible: first observed week median below 10, 8,203 channels)**

- Strong grower: later median at least 10 and at least 3x base. 36 channels (30 first grew in W1, 6 in W2), none German.
- Broad grower: later median at least 5 and at least 2x base. 239 channels, 5 German.
- Base-band control: base median 3 to 9. Without it, base level, title length, affiliate status and followers confound the comparison.
- In the band with the broad rule: 146 growers vs 2,779 non-growers (German: 2 vs 53). Strong rule in the band: 24 vs 2,901 (partners 2 vs 26).

| Metric (median, mean in brackets) | Growers | Non-growers |
|---|---|---|
| Live hours per week | 5.80 (9.04) | 5.98 (9.26) |
| Streams per week | 2.0 (3.03) | 2.0 (3.10) |
| Session length, h | 2.63 (3.03) | 2.72 (3.02) |
| Evening start share (18:00-23:59 Berlin) | 0.27 (0.39) | 0.14 (0.34) |
| Title length, characters | 38.1 (42.5) | 41.0 (44.5) |
| Tags per stream | 6.0 (5.99) | 6.0 (5.81) |
| Share of titles with "?" | mean 0.04 | mean 0.08 |
| Channels with a digit in any title | 43.2% | 33.5% |
| Affiliate | 87.7% | 85.9% |
| Partner | 1.4% | 1.0% |
| No broadcaster type | 11.0% | 13.1% |
| Followers total | 296.5 | 290 |
| Account age, years | 8.00 | 8.01 |

Growth shape (broad rule, band 3 to 9): median viewers in the growth week 9.0 (IQR 7 to 13); peak/median 1.67; 10.3% peaked at 3x median or more; 83.6% had a p25 of at least 5. First growth week: W2 for 55 channels, W3 for 91.

Reading: growth in this band is not explained by schedule, session length, tags or title wording. The evening-start gap rests on medians; the means are close.

**3. Title words and tags (within channel)**

- Method: for each word or tag used by at least 5 channels, compare each channel's average viewers in streams with and without the term (streams of 30 minutes or more: 104,061 streams, 33,444 channels). Effect is the median of per-channel log2 ratios, shown as a multiplicative factor. Sign test, with Benjamini-Hochberg across the 1,346 terms that have at least 15 evaluable channels (1,560 terms with at least 5).
- Only "short" survives correction: x0.95, 39% of 395 channels positive, q=0.037. A roughly 5% dip, practically negligible.
- Uncorrected, not surviving: charity x1.31 (15 channels, 87% positive, p=0.007, q=0.86); birthday x1.09 (125 channels, 66% positive, p=0.0006, q=0.21); tag leagueoflegends x1.25 (15 channels, not significant after correction).
- Common terms, near neutral: stream x0.98 (1,454 channels); ranked x0.99 (1,471); deadlock x1.00 (4,581); tag english x0.99 (199); tag deadlock x0.99 (748).
- German words: "ist" x0.83 (24 German channels, 21% positive, p=0.007, q=0.86); "das" x1.05 (16 channels, 69% positive, p=0.036, q=0.86). Tag deutsch is used by only 8 channels. No German transfer effect survives.

**4. Lively vs dead chat among small channels (10,880 channels, quartiles of 2,720)**

- Chat ratio: distinct human chatters per hour (non-broadcaster, bots and shared copies excluded) divided by summed average viewers in the same hours.
- Broadcaster share: broadcaster messages / human messages. Human messages per viewer-hour include the broadcaster's own messages.
- Ratio across all small channels: median 0.332 (p25 0.172, p75 0.515). Bot-inclusive version: 0.378.

| Metric | Dead (bottom quartile) | Lively (top quartile) |
|---|---|---|
| Average viewers (median) | 1.23 | 2.34 |
| Chat ratio (median) | 0.083 | 0.660 |
| Broadcaster wrote at least once | 22.7% | 55.8% |
| Broadcaster share of human chat, median | 0.0% | 0.25% |
| Broadcaster share, mean | 6.3% | 2.0% |
| Channels with broadcaster share 5% or more | 17.0% | 9.1% |
| Human messages per viewer-hour (median) | 0.26 | 8.45 |
| Session length, h (median) | 2.47 | 2.41 |
| Streams (median) | 5 | 5 |
| Any "?" in titles | 12.1% | 17.4% |
| Mean share of titles with "?" | 4.6% | 5.9% |
| Title length (median) | 29.2 | 36.2 |
| Tags per stream (median) | 4.0 | 5.0 |
| Affiliate | 37.5% | 68.1% |
| Partner | 0.3% | 0.3% |
| German channels | 29 | 32 |
| Bot share of chatters (mean) | 14.7% | 9.4% |

Within average-viewer bands (quartiles within each band, dead vs lively):
- 0 to 3 viewers (1,914 channels per group): ratio 0.056 vs 0.637; broadcaster wrote 18.4% vs 43.9%; messages per viewer-hour 0.15 vs 7.56; title length 28.0 vs 33.3; tags 4.0 vs 4.5; affiliate 30.4% vs 53.4%.
- 3 to 10 viewers (806 channels per group): ratio 0.199 vs 0.694; broadcaster wrote 52.7% vs 73.1%; messages per viewer-hour 1.30 vs 10.08; title length 39.5 vs 41.1; tags 6.0 vs 6.0; affiliate 83.9% vs 90.7%; any "?" 20.5% vs 21.8%.

Change from the bot-inclusive run: ratio medians 0.095 / 0.768 became 0.083 / 0.660; broadcaster presence 21.4% / 57.3% became 22.7% / 55.8%; messages per viewer-hour 0.29 / 8.73 became 0.26 / 8.45; affiliate 35.1% / 72.7% became 37.5% / 68.1%; title length 28.8 / 38.2 became 29.2 / 36.2; tags 4.0 / 5.5 became 4.0 / 5.0; broadcaster share mean 5.3% / 2.0% became 6.3% / 2.0% (denominator now human messages only). German channels moved from 31 / 31 to 29 / 32. The picture is unchanged.

Reading: lively small channels have the broadcaster present in chat, more messages per viewer, longer titles, more tags and more affiliates. The broadcaster's presence is the largest difference. Reverse causation is plausible (busy chat draws the broadcaster in), so this cannot be read as a cause.

**5. Followers per average viewer (channels with at least 5 live hours and a followers value, n=12,990)**

- Median 59.5 (IQR 21.3 to 151.8).
- By size: 0-4: 50.7 (n 9,290); 5-19: 73.5 (2,592); 20-99: 97.0 (811); 100+: 190.6 (297).
- By language (all sizes): en 68.1 (8,116); es 68.1 (611); ar 69.5 (88); fr 64.6 (365); zh 55.4 (77); pt 54.5 (681); de 48.7 (189, IQR 22.4 to 126.9); pl 44.2 (110); ru 38.4 (2,155); th 35.7 (126); ja 35.7 (226); uk 21.5 (71).
- Size 0-4 by language: en 58.3 (5,636); es 56.5 (448); fr 55.3 (251); pt 44.9 (510); de 50.2 (144); pl 31.7 (76); ru 32.0 (1,698); ja 26.4 (147); th 24.5 (88); uk 17.7 (59).
- German 5-19: 38.7 (n 32). German 20-99: 63.8 (n 11).

Reading: at the same size, German small channels sit a little below English (50.2 vs 58.3) and well above Russian (32.0). The ratio rises with size. Followers are measured after growth, so this is a benchmark, not a cause.

**6. What German streamers can take from this**
- German Deadlock is a small niche (538 channels, 0.5% of viewer-hours). Growth examples are rare, and none of the strong growers were German. Evidence: strong.
- Growth in the small band is not explained by schedule, session length, tags or title wording. Evidence: moderate, German samples are small.
- Lively chat goes with the streamer writing in chat. Evidence: correlational only.
- No title word or tag shows a robust lift. Keep titles clear; the data offers no specific wording to copy.
- Small German channels hold about as many followers per average viewer as English ones (50 vs 58). This is a follower stock built up over each channel's life, not a conversion rate; the category data has no follow events, so conversion by language is not measured.

**Caveats**
- 16-day window; week 3 has 3 days; channels already live before 10-09 have no earlier baseline.
- Category snapshots only: raids, hosts, browse and other discovery paths are not visible.
- The chat ratio uses small denominators and is per hour. The bot cut-off (30 channels, 221 IDs) is the coordinator's rule. Live ingestion changed chatter-hours by about 0.4% between extracts.
- German sample sizes are small: 29 to 32 channels per chat quartile, 2 German growers in the matched band.
- Multiple testing: 1,346 terms, Benjamini-Hochberg correction applied.
- Correlational analysis only; no causal claims.

**Files**
- Queries: /home/nathanael/.worktrees/twitch-reichweite/.tasks/2026-10-25-reichweite-auswertung/queries-P10.sql (bot-size check and bot-excluded room-hour query; the room-hour query takes `-v wfrom=... -v wto=...` and was run in three weekly chunks)
- Scratch extracts and scripts: /tmp/reichweite-scratch/P10/ (not in the task folder)
