# Implementation Brief: Twitch Social Media Automation, AI Video Editing & Dashboard Improvements

## 1. General Instructions

**Project:** https://deutsche-deadlock-community.de/twitch/social-media

**Admin Dashboard:** https://admin.deutsche-deadlock-community.de/twitch/admin

### Language Requirements

- Think, reason, plan, document technical decisions, and communicate with me exclusively in **English**.
- All end-user-facing dashboard text must remain in **German**.
- This includes buttons, labels, headings, tooltips, notifications, errors, and other interface text.
- Code, variable names, internal documentation, and agent instructions should use English.
- Social media titles and descriptions are an exception: their language must follow the user's configured publishing language.

### Development Instructions

- Inspect the existing repositories, architecture, services, database models, API integrations, and dashboard components before changing anything.
- Reuse working functionality wherever possible.
- Do not unnecessarily rebuild existing systems.
- Do not delegate any tasks to Codex.
- You may use suitable Claude agents and distribute independent tasks where this is beneficial.
- Manage Opus 5.5 usage efficiently. Avoid unnecessary agent calls and redundant investigations.
- Independent implementation tasks may run in parallel, but coordinate shared files, database migrations, and interfaces to avoid conflicts.
- Do not overengineer the solution.
- Prefer practical, working implementations over elaborate abstractions.
- Implement the agreed functionality, not just conceptual mockups or documentation.
- Investigate technical limitations before making assumptions about APIs or model capabilities.

**Most importantly: We are extending an existing working social media automation system. We are not building an entirely new product.**

---

# 2. Primary Objective

Improve our existing Twitch Social Media Manager by integrating an optional AI-powered video editing pipeline while preserving the original static video processing workflow.

The system must support:

1. Existing static Twitch clip processing without AI editing.
2. Optional AI-powered Deadlock gameplay analysis and video editing.
3. Historical Twitch clip import with flexible date filtering.
4. A single shared posting schedule across all connected platforms.
5. Manual review and fully automatic publishing modes.
6. User feedback and AI-assisted video revisions.
7. Personal and global learning from feedback.
8. Transparent AI processing decisions and costs.
9. Improved dashboard usability and visual presentation.
10. Integration with our existing Twitch Admin Dashboard for administrative controls.

All discussed features belong to the overall implementation scope unless explicitly marked as future work.

The Auto-Pilot is a priority, but other independent components may be developed in parallel.

Our first essential technical milestone is a working manual end-to-end AI processing test with a real Twitch clip.

---

# 3. Preserve the Existing Static Pipeline

This is a critical architectural requirement.

Originally, the Social Media Manager was designed to process and publish Twitch clips using the existing static video pipeline.

**The new AI editor must be optional, not mandatory.**

## Static Mode — Default

When AI video editing is disabled:

- Use the existing Twitch clip processing implementation.
- Preserve existing static layouts and templates.
- Preserve existing FFmpeg-based video processing.
- Preserve existing publishing integrations.
- Do not invoke expensive AI gameplay analysis unnecessarily.
- Do not change the video creatively through AI.

## AI Editing Mode — Optional

When AI editing is enabled:

- Analyze the Twitch clip.
- Understand the gameplay as accurately as possible.
- Identify relevant moments.
- Generate intelligent editing instructions.
- Process and render the video.
- Generate appropriate subtitles.
- Prepare the finished video for review or automatic publishing.

Both modes must reuse the same underlying scheduling, queue, approval, and publishing infrastructure.

### AI Toggle

Provide one central AI editing toggle within the Auto-Pilot settings.

- Default: disabled.
- Store the setting using the existing per-user persistence system.
- When changed, it applies to all processing jobs that have not started.
- Already running jobs finish using their original processing mode.
- Previously completed videos are not rendered again.
- Do not add unnecessary per-clip configuration switches throughout the automatic pipeline.

The separate manual AI testing workflow must still allow specific Twitch clips to be processed explicitly.

---

# 4. Twitch Clip Import and Historical Filtering

## Source

Import clips from the user's own Twitch channel, regardless of which viewer originally created the clip.

For the initial implementation, support **Deadlock clips only**.

Clips belonging to unrelated game categories must be excluded from automatic selection.

Do not build general-purpose gameplay understanding for other games at this stage.

## Historical Clip Support

Users must be able to select clips from any accessible historical period.

Support:

- All available historical clips.
- Individual years, such as 2024 or 2025.
- Individual quarters, such as Q1 2025.
- Custom start and end dates.
- Before or after a selected date.
- Oldest clips first.
- Newest clips first.

The user decides the chronological selection order.

Do not automatically prioritize clips using views, engagement, or AI quality predictions instead of the user's chosen sorting rules.

## Strict Date Filters

The configured date range must strictly determine which new clips enter the pipeline.

For example, when a user selects 2024–2025, new clips from 2026 must not automatically enter the queue.

If the user changes the date range:

- Apply it to future clip selection.
- Preserve clips already in the queue.
- Preserve currently running jobs.
- Preserve completed videos.
- Do not discard already scheduled work unnecessarily.

## Twitch API Requirements

Investigate the available Twitch Helix endpoints and their actual historical retrieval limitations.

Handle pagination, date windows, rate limits, and interrupted imports correctly.

Never assume that a single API request can retrieve the complete historical clip archive.

Deduplicate clips and track whether each clip has already been selected, processed, or published.

**Importing 800 historical clips must not trigger 800 AI analysis jobs.**

The historical clips form a selectable pool. Actual processing happens when the queue and posting schedule require it.

---

# 5. Single Shared Publishing Schedule

Replace separate per-platform publishing schedules with **one shared schedule**.

Users configure explicit weekdays and times.

Example:

- Monday at 18:00.
- Wednesday at 19:00.
- Friday at 17:30.

Each posting slot uses one prepared video and publishes that video to the selected platforms.

The same rendered video is reused across platforms.

Platform-specific titles, descriptions, hashtags, publishing statuses, and authorization requirements may still differ.

## Timezones

- Initially detect the browser's timezone.
- Allow users to override it manually.
- Store the selected timezone per user.
- Respect daylight saving time transitions.
- Use the configured timezone consistently for scheduling.

## Production Lead Time

The pipeline should normally prepare a video before its publication date.

- Default lead time: 24 hours.
- Configurable per user.
- Examples: 12, 24, 48, or 72 hours.

**The lead time is a best-effort target, not a strict minimum.**

If the Auto-Pilot is activated only 22 hours before a scheduled post, start production rather than skipping the slot unnecessarily.

Even if only a few hours remain, the pipeline may attempt production.

## Flexible Publication Timing

The exact scheduled minute is not critical.

If a video is ready one or two hours late, publishing shortly afterward is acceptable as long as it is still a reasonable local posting time and all required approvals exist.

Avoid automatically publishing at unreasonable nighttime hours.

If a reasonable publication opportunity has passed, use the next regular slot.

Do not create complicated catch-up scheduling or publish multiple videos in a sudden burst.

Missed posting slots do not accumulate.

---

# 6. Sequential Production Queue

Use a simple, reliable sequential processing model.

**Process one video job at a time.**

The system should not run multiple expensive video production jobs concurrently by default.

- Process queued jobs in their selected order.
- Allow manual queue reordering.
- Allow marking a clip as next.
- Allow removing unstarted jobs.
- Do not interrupt already running jobs unnecessarily.
- Do not introduce a complex priority scheduling engine.

The user controls clip selection and order.

## Unsuitable Clips

When the AI considers a clip unsuitable:

- Flag it for user review.
- Explain why it may be unsuitable.
- Allow the user to decide whether it should still be processed or skipped.
- Continue with the next queued clip without blocking the entire pipeline.

The AI must not permanently reject potentially usable clips without allowing a user decision.

## Uncertain Gameplay Understanding

The AI should still attempt a reasonable video edit when gameplay understanding is incomplete.

After processing:

- Clearly identify significant uncertainty.
- Present the result for review.
- Allow the user to explain what happened and request corrections.
- Use feedback to improve future editing behavior.

In fully automatic mode, videos with significant unresolved content or quality concerns must be held for review rather than automatically published.

---

# 7. Review Mode and Full Automation

The Social Media Manager has two publishing modes.

## Review Mode — Default

The user must explicitly approve a finished video before publication.

**No approval means no publication.**

There must be no automatic approval due to inactivity.

The user is responsible for reviewing the content.

## Fully Automatic Mode

When explicitly enabled:

- The pipeline selects the next eligible clip.
- Processes it using the currently selected editing mode.
- Performs the required technical and content quality checks.
- Prepares publication metadata.
- Publishes according to the configured schedule.

No regular manual approval is required.

However, serious processing errors or substantial content uncertainty should still prevent unsafe or obviously broken uploads.

## Switching Between Modes

When changing from Review to Full Automation, show a confirmation dialog asking how already pending videos should be handled.

Allow the user to:

- Keep existing pending videos under manual review.
- Explicitly transfer pending videos into automatic publishing.

When changing back to Review Mode, not-yet-published videos without explicit user approval must require review.

Already published videos remain unaffected.

---

# 8. Review Queue Backpressure

In Review Mode, automatically pause new scheduled productions when **two videos are already waiting for approval**.

- Zero or one pending review: production can continue.
- Two pending reviews: do not start another automatic production.
- When the count falls below two, resume according to the normal queue and schedule.
- Full Automation is not subject to this regular review limit.

This prevents unnecessary processing costs and storage consumption when users are inactive.

Do not accumulate missed publication slots during a review backlog.

---

# 9. Multi-Platform Publishing and Approval

Reuse the existing social media publishing integrations.

Do not implement new platform integrations as part of the current scope.

The initial request mentioned TikTok, YouTube, Instagram, and potentially Twitch Stories. However, expanding platform API integrations is **outside the current implementation scope**.

Twitch Stories and additional platform integrations may remain future ideas.

## Generalized Platform Status

The existing TikTok-specific approval/status button is too narrowly designed.

Generalize the relevant platform authorization and connection status functionality so the UI can clearly represent all existing integrations.

Distinguish between:

- Platform connection or authorization status.
- Video publishing approval.
- Upload and publishing status.

Do not confuse API authorization with a user's approval of a video.

## Approval Controls

A prepared video supports:

- **Approve all selected platforms.**
- **Approve individual platforms.**

Each platform retains its own approval and publication status, even though there is only one shared video and one shared posting slot.

## Partial Approval

At publication time, publish only to explicitly approved platforms in Review Mode.

If YouTube is approved but TikTok is not:

- Publish to YouTube.
- Discard TikTok's publication for that posting event.
- Do not automatically reschedule unapproved platforms.

If no platform has been approved by the scheduled time, the video may remain in the review queue until its retention deadline.

If the user later approves it, schedule it for the next regular available posting slot.

This is different from partial approval: once a posting event has been executed for its approved platforms, the unapproved destinations for that event are discarded.

## Platform Upload Errors

If an approved upload fails on one platform:

- Do not undo successful uploads on other platforms.
- Retry only the failed platform.
- Track the result independently.
- Use idempotent publishing operations to avoid duplicate uploads.
- Never bypass a missing user approval through a retry.

---

# 10. AI Video Editing Agent

The AI agent should transform existing Twitch clips into polished, engaging social media videos.

However, remember that these are **already short Twitch clips**, not full-length streams.

The editing process must therefore be conservative and context-aware.

## Main Output

Default:

**One Twitch clip → one finished social media video.**

The AI should preserve the important content and produce the strongest reasonable version.

It may:

- Remove unnecessary pauses.
- Improve the beginning and ending.
- Preserve important reactions.
- Identify the actual highlight.
- Adjust the duration.
- Improve pacing.
- Apply the selected layout.
- Generate subtitles.

Do not force unnecessary cuts, transitions, or artificial excitement.

A 55-second teamfight may need to remain 55 seconds.

A short isolated kill may only need 15 seconds.

Let gameplay context guide these decisions.

## Optional Multiple Outputs

Prepare support for:

**One Twitch clip → multiple edited videos.**

This capability must be configurable per user through the existing persistence system and disabled by default.

The primary output remains one coherent main video. Additional variants are optional and should only be created when the source genuinely contains enough useful material.

Display all generated variants in the backend.

Do not unnecessarily render multiple versions during normal operation.

---

# 11. AI Models and Video Understanding

Start by testing gameplay understanding with Claude.

We do not yet know which model will deliver the best practical results.

## Model Strategy

1. Build the first working integration around Claude.
2. Test actual Deadlock Twitch clips.
3. Evaluate gameplay understanding, useful scene selection, quality, latency, and costs.
4. Later test Gemini on comparable clips.
5. Keep model/provider selection configurable globally.

Do not build a dedicated model comparison dashboard at this stage.

### Global YAML Configuration

Use YAML for global system settings, such as:

- AI provider.
- AI model.
- Provider-specific technical settings.
- Global technical limits.
- Relevant service configuration.

**Do not use YAML for per-user preferences or per-user feature states.**

These belong in the existing backend persistence system.

## Multimodal Analysis

Investigate which model capabilities are actually available.

If the selected model cannot ingest video directly through the existing API, use a practical supported workflow such as frame extraction, audio transcription, timestamps, and structured context.

Do not claim unsupported capabilities.

## FFmpeg and HyperFrames

Use FFmpeg for the actual video processing and rendering.

Investigate HyperFrames and determine whether it provides useful capabilities for analysis, frame processing, or editing.

We initially considered HyperFrames, but it should not become a mandatory dependency without a verified technical benefit.

Choose the simplest working approach for the first milestone.

---

# 12. Modular Agent Skills

Do not build one enormous prompt containing every responsibility.

Create specialized, versioned skills that are loaded with the appropriate context at the relevant pipeline stage.

Potential skills include:

### Deadlock Understanding

Understand:

- Heroes.
- Abilities.
- Items.
- Gameplay mechanics.
- Important fights.
- Kills and team interactions.
- Relevant game context.

### Highlight Analysis

Identify which moments are worth preserving and why.

### Editing Strategy

Determine:

- Useful cut points.
- Clip duration.
- Scene continuity.
- Relevant reactions.
- Appropriate pacing.

### Video Layout

Apply existing static layouts and template configuration.

### Subtitle Generation

Create timed subtitles in the actual spoken language.

### Social Media Copywriting

Generate platform-appropriate titles, descriptions, and hashtags using existing templates.

### Quality Validation

Detect obvious technical and content problems without requiring a separate QA agent.

### Feedback and Learning

Interpret user corrections, improve future editing behavior, and maintain versioned preferences.

## Context Loading

Only load relevant skills and supporting knowledge when a pipeline stage needs them.

Do not repeatedly provide the entire knowledge base to every model call.

Use structured outputs where appropriate so editing instructions can be validated before FFmpeg executes them.

---

# 13. Deadlock Brain Integration

The Deadlock Brain project is still under development.

Connect the editing agent to it as an external gameplay knowledge source.

**The connection must be read-only.**

The editing agent may retrieve:

- Hero information.
- Ability explanations.
- Item information.
- Gameplay mechanics.
- Other useful game knowledge.

However:

- Do not allow the editing agent to modify Deadlock Brain.
- Do not automatically transfer user feedback into Brain knowledge.
- Do not duplicate the complete knowledge base inside the Social Media Manager.
- Use a modular integration because the Brain is still evolving.
- The video processing pipeline must remain usable when Brain information is temporarily unavailable or incomplete.

The Deadlock Brain and Social Media Editing Agent are separate systems with separate learning responsibilities.

---

# 14. Video Layouts and Experimental Mode

Preserve our existing layout configuration tools.

Users already have controls for fitting widescreen Twitch gameplay into vertical 9:16 social media videos.

Do not rebuild those controls unnecessarily.

## Default: Stable Static Layouts

The standard pipeline uses the existing:

- Aspect-ratio configuration.
- Video positioning.
- Scaling.
- Static layouts.
- Templates.
- Facecam placement.
- Existing rendering settings.

Preserve important gameplay information and avoid cutting off the HUD.

## Experimental AI Layout Mode

Add support for an experimental mode stored as a per-user setting.

The AI may eventually:

- Detect facecam placement.
- Recognize unused space in the video frame.
- Adjust gameplay positioning.
- Explore better compositions.
- Place verified Deadlock item graphics in suitable areas near the facecam.
- Adapt overlays to the actual gameplay.

This is experimental and must not compromise the stable default processing mode.

Do not force item overlays when information is uncertain or there is insufficient space.

Prioritize getting the static layout pipeline and core AI editor reliably working first.

---

# 15. Audio and Subtitles

## Original Audio

Preserve the original Twitch clip audio.

Do not add artificial music, synthetic voices, memes, or unnecessary sound effects in the initial implementation.

Copyright and music-rights analysis are outside this pipeline's current scope.

The user is responsible for ensuring they have the necessary rights to publish their content.

## Subtitles

Extend the existing template functionality instead of building an unrelated subtitle system.

The default style should be classic and readable.

- Use accurate timestamps.
- Avoid obscuring gameplay.
- Avoid unnecessary animation.
- Respect the existing visual settings.

### Language Handling

Generate subtitles in the language actually being spoken.

For example:

- German speech → German subtitles.
- English calls → English subtitles.
- Mixed German and English speech → Mixed-language subtitles matching the original statements.

Do not automatically translate everything into German or English.

Preserve Deadlock terms and gaming terminology where appropriate.

If there is no speech, do not invent spoken subtitles.

## Future Creative Editing

A separate Meme / Creative Editing Skill may eventually introduce:

- Memes.
- Sound effects.
- Creative zooms.
- Additional animations.
- Contextual humorous edits.

This requires an appropriate meme knowledge base and media library.

**Treat this as future work, not a requirement for the initial implementation.**

---

# 16. AI-Generated Publishing Metadata

Generate titles, descriptions, and hashtags using the user's existing social media templates.

The AI should understand the actual clip rather than relying on generic text.

For example, use relevant information about a hero, a clutch play, a surprising fight, or a funny reaction when confidently identified.

## Publishing Language

Provide a per-user setting:

- German.
- English.
- Automatic based on the clip.

The generated language preference does not affect subtitle language.

## Platform Metadata

The video itself is shared across platforms.

However, platform-specific titles, descriptions, hashtags, and other metadata may be different.

Keep the existing publishing integrations and platform-specific constraints.

The user can manually edit generated text or request improvements through the AI feedback interface.

---

# 17. Manual AI Production Mode

This is an essential feature for our initial testing.

Add the ability to select a specific Twitch clip in the dashboard and manually run it through the complete AI pipeline.

The user should be able to:

1. Select a Twitch clip.
2. Start AI processing.
3. See the production status.
4. Review the finished result.
5. Compare the source with the output.
6. Request improvements.
7. Approve the result.
8. Optionally transfer the finished video to the normal publishing schedule.

The manual process must not automatically publish the video.

Do not require the Auto-Pilot to be enabled just to perform a manual AI test.

## Reuse Finished Videos

When the user decides that a manually produced video should be published:

- Reuse the existing rendered file.
- Do not rerun analysis or editing.
- Allow the user to approve the selected platforms.
- Assign the video to a suitable available posting slot.

Do not automatically add every successful test video to the publishing queue.

---
# 18. Review Interface and Before/After Comparison

The AI editing review interface must include a direct comparison of:

**Original Twitch Clip vs. AI-Edited Video**

Display both video players next to each other where the available screen size permits.

Support synchronized playback if practical.

Also display:

- Original duration.
- Processed duration.
- Recognized highlights.
- Cut points and removed sections.
- Concise explanations of editing decisions.
- Relevant gameplay uncertainty.
- Processing duration.
- AI model used.
- Estimated and/or measured AI costs.
- Rendering status.
- Video version history.

Do not expose unnecessary internal reasoning traces. Instead, show useful, structured explanations of the decisions and observable processing actions.

Keep technical details understandable and the interface visually clean.

---

# 19. AI Feedback and Video Revision

Users must be able to give natural-language instructions to improve an edited video.

Examples:

- "Make the intro shorter."
- "Keep more of the teamfight."
- "Make the subtitles smaller."
- "The facecam is covering the important action."
- "You misunderstood this hero ability."
- "Keep the original reaction."

## Revision Workflow

The AI interprets the requested changes and decides whether it can update the video directly.

### Small Changes

Apply straightforward, low-cost changes automatically.

### Significant Changes

For expensive changes, major reanalysis, or substantial editing alterations:

- Show a concise proposed change.
- Explain the expected impact.
- Provide a cost estimate where practical.
- Request confirmation before proceeding.

## Version Handling

Preserve earlier versions temporarily while the review is active.

Allow users to compare the previous and updated versions.

Do not permanently overwrite the only usable result before a new render succeeds.

All temporary versions remain subject to the agreed retention policy.

---

# 20. Personal AI Learning

The editing system should improve through user feedback.

Store personal preferences using the existing per-user persistence architecture.

Potential learned preferences:

- Typical video length.
- Preferred editing intensity.
- Subtitle size and placement.
- Facecam positioning.
- Preferred layouts.
- Repeatedly requested corrections.
- Editing styles the user consistently accepts or rejects.

Do not treat every individual instruction as a permanent universal preference.

Consider subsequent corrections.

For example, if a user asks for smaller subtitles and then restores the original size, the system must learn from the complete feedback sequence.

Positive examples include accepted versions.

Negative examples include explicitly rejected edits or corrections.

---

# 21. Automatic Global Skill Learning

The system must also support learning from recurring, anonymized feedback patterns across users.

Unlike personal learning, global learning should only happen when the available evidence is sufficiently reliable.

The AI may automatically improve:

- Editing prompts.
- Skill instructions.
- Highlight selection strategies.
- Subtitle strategies.
- Editing parameters.
- Safe FFmpeg rendering parameters.
- Other non-code configuration relevant to video quality.

## Safety Boundaries

**The AI must never automatically modify executable application code.**

It may identify improvements that would require code or architecture changes, but those must be proposed to the admin for manual review.

Do not automatically apply such proposals.

User feedback must never automatically modify Deadlock Brain.

Keep personal user data separate from global knowledge.

Global improvements should use anonymous or appropriately aggregated evidence, not identifiable user feedback.

## Versioning and Rollback

Every automatic global skill change must have:

- A skill identifier.
- Previous and new versions.
- Timestamp.
- Summary of the change.
- Reason for the change.
- Available supporting evidence.
- A readable textual diff.
- Rollback support.

Apply suitable validation before activating changed skill instructions or rendering parameters.

Prevent a faulty global update from permanently breaking the editing pipeline.

---

# 22. Global Skill Change History in Twitch Admin

Integrate the global AI learning administration into:

https://admin.deutsche-deadlock-community.de/twitch/admin

Add an admin-accessible interface for inspecting what the AI has changed.

The interface should resemble a GitHub-style diff.

Show:

- Skill name.
- Current version.
- Previous version.
- Removed instructions.
- Added instructions.
- Explanation of the improvement.
- Modification date.
- Available quality evidence.
- Rollback action.

Global skill improvements do not require prior admin approval.

However, the admin must be able to inspect and revert them.

## Critical Change Proposals

The AI may also suggest:

- Application code improvements.
- Architecture changes.
- New editing tools.
- More efficient processing implementations.
- Technical pipeline changes.

These proposals must remain suggestions until manually reviewed and implemented.

**No autonomous code modification or deployment.**

---

# 23. AI Resource Tracking and User Budgets

We do not yet know the real production costs of this system.

Therefore, do not invent permanent pricing or plan limits.

## Initial Cost Measurement

Measure actual usage during our internal tests.

Track where practical:

- AI provider.
- AI model.
- Input and output token usage.
- Provider costs.
- Processing time.
- Analysis costs.
- Revision costs.
- Rendering runtime.
- Relevant compute/resource consumption.
- Per-job and per-user totals.

FFmpeg compute time must not be mislabeled as a directly billed API charge.

Separate measured costs from estimated internal resource costs.

## Future Plan-Based Budgets

Prepare the architecture for limits based on the user's existing subscription plan.

Possible limits include:

- Monthly video productions.
- AI editing jobs.
- AI revision counts.
- Monetary or provider-cost budgets.
- Processing resources.

These limits will be calibrated using real cost data.

## Admin Controls

Integrate administration into the existing Twitch Admin Dashboard.

Support:

- Global processing limits.
- Individual user overrides.
- Per-user consumption reporting.
- Future plan-based limit configuration.

Do not create an entirely separate billing or subscription system.

---

# 24. Notifications

Users may configure notifications through supported channels such as:

- Dashboard.
- Discord.
- E-mail.

The available options should follow actual existing integrations.

Notifications may cover:

- Videos ready for review.
- Upcoming publication deadlines.
- Failed processing jobs.
- Publication errors.
- Empty clip pools.
- Expiring reviews.

Allow users to configure suitable reminders.

## Approval Location

For now, external notifications should link to the dashboard.

The actual video review, editing feedback, and platform approvals remain inside the dashboard.

Direct approvals through Discord buttons or email links are future functionality and are not part of the initial scope.

---

# 25. File Retention and Storage

Avoid unnecessary long-term storage of processed videos.

## Pending Review

A rendered video waiting for approval is retained for seven days after completion.

After seven days without approval:

- Delete the rendered temporary files.
- Preserve relevant metadata and status history.
- Do not automatically regenerate the expired video.
- Notify the user before expiry where supported and configured.

## Approved and Scheduled

Once a video is approved and queued for publication:

- Keep the required rendered file until publishing.
- Do not delete it merely because seven days have passed.
- Retain it as necessary for bounded upload retries.

## Successfully Published

After all required publishing operations and applicable retries have completed:

- Delete temporary rendered videos.
- Remove unnecessary working files.
- Preserve publication records and processing metadata.

Do not build a permanent video storage archive merely to support reuse.

## No Recycling

Do not automatically repost previously published clips.

Do not rerun old processed videos through the AI pipeline because the clip pool is empty.

If no eligible clips remain, pause production and notify the user.

The user is responsible for providing new content or adjusting the historical selection.

---

# 26. Intelligent Error Handling

Use resilient, step-based processing rather than restarting everything whenever a single stage fails.

For example:

If FFmpeg rendering fails after gameplay analysis succeeds, do not repeat the expensive analysis unnecessarily.

Implement:

- Clear processing stages.
- Reusable intermediate results during active jobs.
- Bounded retries.
- Appropriate handling of temporary provider failures.
- Timeout handling.
- Error classification.
- Recoverable job status.
- Clear failure messages.

Avoid infinite retry loops.

If a clip remains unprocessable after the allowed attempts:

- Move it to a review/error state.
- Preserve the relevant explanation.
- Continue with the next eligible clip.

## No Static Fallback

If the user enabled AI editing and that process fails, do **not** silently publish the clip using the static pipeline.

The clip should be returned for review while the queue proceeds to the next item.

The static processing mode is a deliberate user setting, not a hidden failure fallback.

---

# 27. Quality Validation

Do not create a separate independent QA agent for the first implementation.

The existing processing pipeline should validate its own output.

Include reasonable checks for:

- File integrity.
- Video resolution.
- Aspect ratio.
- Audio presence.
- Subtitle positioning.
- Unexpected rendering failures.
- Obvious framing problems.
- Major content continuity issues where detectable.

This does not need to guarantee perfect editorial quality.

We accept some residual quality risk in exchange for lower cost and a simpler architecture.

For significant quality concerns, return the video for user review and continue processing other eligible clips.

---

# 28. Pausing the Auto-Pilot

Implement a simple graceful pause.

When paused:

- Do not start new automatic productions.
- Do not start new scheduled publications.
- Let already running processing jobs complete.
- Preserve queue state.
- Continue from the preserved queue when resumed.
- Do not automatically catch up all missed slots.

Manual AI test jobs may still be started independently.

---

# 29. Dashboard Redesign

The current Social Media Manager needs substantial visual and usability improvements.

However, **do not replace the existing visual identity**.

Our current design language should remain recognizable.

Preserve:

- Dark charcoal backgrounds.
- Subtle gold/yellow accents.
- Existing grid/background details.
- Existing sidebar/navigation style.
- Existing component aesthetics where practical.
- The overall visual identity of the Deutsche Deadlock Community.

## What to Improve

- Visual hierarchy.
- Excessive empty space.
- Inconsistent spacing.
- Oversized or awkward controls.
- Unnecessarily long settings forms.
- Clip library readability.
- Publishing statuses.
- Review workflow.
- Processing progress.
- Video previews.
- Platform connection visibility.

The objective is a cleaner and more professional version of our existing dashboard, **not a completely new futuristic neon gaming interface**.

## Navigation

Do not overengineer the navigation.

Avoid both extremes:

- Putting every function into one enormous page.
- Splitting every small feature into a separate tab.

Claude should inspect the current component structure and make a reasonable decision about which features belong together.

A compact overview plus logically grouped work areas is appropriate.

For example, consider grouping:

- Pipeline and overview.
- Auto-Pilot and scheduling.
- Clip library and historical import.
- AI editing, previews, and review.
- Templates and account settings.

These are suggestions, not a mandatory five-tab layout.

Reuse existing pages and components wherever sensible.

## Generalization of Existing UI

Replace the narrowly TikTok-specific approval/status check with a clearer platform-independent representation of authorization and connectivity.

Use German UI terminology.

Make platform-specific publishing status easy to understand without creating unnecessary extra dashboards.

---

# 30. Implementation Strategy

The full agreed scope should be implemented, but work may be split into coordinated independent packages.

**Auto-Pilot functionality is a priority.**

At the same time, the first manual end-to-end AI pipeline is our primary technical validation milestone.

Suggested work packages:

### Work Package A — Auto-Pilot and Scheduling

- One shared publication schedule.
- Sequential job queue.
- Configurable production lead time.
- Review and Full Automation.
- Review backlog limit.
- Late publishing handling.
- Queue preservation.
- Pause/resume.
- Existing platform publishing integration.

### Work Package B — Twitch Clip Selection

- Historical import.
- Deadlock filtering.
- Date ranges and quarters.
- Sorting.
- Deduplication.
- Manual clip selection.
- Queue management.

### Work Package C — AI Video Pipeline

- First Claude gameplay analysis.
- Deadlock Brain read-only integration.
- Modular editing skills.
- FFmpeg rendering.
- Subtitle generation.
- Static layout integration.
- Manual end-to-end test.
- Optional multi-output architecture.
- Experimental layout capabilities.

### Work Package D — Feedback and Learning

- Natural-language revisions.
- Personal preferences.
- Version comparisons.
- Global anonymous learning.
- Global skill versioning.
- Admin diff history.
- Rollback.
- Critical code-change proposals.

### Work Package E — Dashboard and Administration

- Visual cleanup.
- Clip preview and before/after comparison.
- Approval workflow.
- Publishing status.
- Notifications.
- AI job progress.
- Processing cost reporting.
- Admin user-budget controls.

These work packages are not separate applications.

They must integrate into the existing architecture.

Coordinate interfaces and migrations before implementing overlapping changes.

---

# 31. Explicitly Out of Scope for Now

Do not expand the implementation into unrelated features.

The following are intentionally postponed:

- New social media platform API integrations.
- Twitch Stories publishing implementation.
- A new standalone subscription/billing system.
- A full manual timeline video editor.
- A separate independent QA agent.
- Automatically generated meme edits and meme libraries.
- AI-generated background music or sound effects.
- Automatic copyright/music-rights detection.
- Direct Discord or E-mail approvals.
- Automatic reposting or video recycling.
- A permanent archive of generated videos.
- Autonomous source-code modifications.
- Autonomous updates to Deadlock Brain.
- A dedicated Claude-versus-Gemini comparison dashboard.
- Complex distributed job scheduling without a demonstrated need.

Optional reference-video-based editing style learning may be added later. The initial system must learn from editing skills and user feedback without requiring a reference video library.

---

# 32. Acceptance Criteria

The implementation should be considered successful when we can demonstrate the following.

## Manual AI Workflow

1. Select a real Deadlock Twitch clip.
2. Start AI processing manually.
3. Analyze it with the configured model.
4. Produce a usable video using FFmpeg.
5. Generate original-language subtitles.
6. Show the original and output side by side.
7. Display editing decisions and processing costs.
8. Submit natural-language revision feedback.
9. Produce and compare an improved version.
10. Optionally approve and queue the result without rendering it again.

## Static Workflow

1. Disable AI editing.
2. Process a Twitch clip using the existing static workflow.
3. Apply existing layouts and templates.
4. Publish through the existing integration.
5. Verify that no unnecessary gameplay-analysis model calls were made.

## Auto-Pilot Workflow

1. Configure fixed weekdays and posting times.
2. Select the historical clip range and sorting order.
3. Configure the processing lead time.
4. Start the Auto-Pilot.
5. Verify that only required upcoming clips are processed.
6. Confirm the review backlog limit works.
7. Approve all or selected platforms.
8. Verify publication uses the single shared slot.
9. Verify failed platform uploads are retried independently.
10. Verify queue state survives pauses and setting changes.

## Learning Workflow

1. Request an AI editing change.
2. Approve or reject the resulting version.
3. Verify personal preferences are updated appropriately.
4. Verify recurring global improvements can update editing skills.
5. Verify changes are versioned.
6. Inspect a readable skill diff in the Twitch Admin Dashboard.
7. Roll back a global skill change.
8. Verify that no application code or Deadlock Brain content is automatically modified.

## Storage and Reliability

1. Verify seven-day expiration of unapproved review files.
2. Verify approved scheduled videos remain available until publication.
3. Verify temporary media cleanup after publishing.
4. Verify completed uploads are not duplicated.
5. Verify step-level retries do not unnecessarily repeat expensive analysis.
6. Verify exhausted or filtered clip pools stop production cleanly.

---

# 33. How to Proceed

Start by inspecting the relevant repositories and current production architecture.

Identify what already exists, what can be reused, and what needs modification.

Produce a concise implementation plan with:

- Relevant files and services.
- Existing features that will be reused.
- Required backend and database changes.
- AI model integration approach.
- Clearly separated implementation tasks.
- Dependencies between those tasks.
- Any genuine API or infrastructure blockers.

Then proceed with implementation rather than stopping at a planning document.

Use parallel Claude agents only when their responsibilities are genuinely independent and can be integrated safely.

Do not delegate any work to Codex.

Do not rewrite unrelated functionality.

Do not invent capabilities of external services.

Validate significant changes using appropriate tests and build checks.

Keep the application usable during incremental implementation wherever practical.

**All communication with me must be in English. All visible dashboard interface text must be in German.**

The final result should feel like our existing Deutsche Deadlock Community dashboard has evolved into a reliable, intelligent social media production system — not like we replaced it with an entirely different application.
