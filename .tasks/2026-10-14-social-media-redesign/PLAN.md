# Social Media Manager redesign

Goal: guided structure (Heute, Freigabe, Planen, Veröffentlicht, Einstellungen) for /twitch/social-media. No function lost, backend behaviour unchanged, UI and structure only.

Acceptance: every function in the inventory below is reachable in the new structure; build, lint, existing tests green; Moli check of every view at 1440 px and 390 px; listed bugs reproducibly fixed against live data.

## Inventory (today)

Page shell (SocialMediaManager.tsx): admin streamer select, partner access grant, VOD archive link, trial banner and modal, plan provider.

Global header of every view: "Auswertung" (analytics dialog), "Clip hinzufügen" (MP4 upload), 5-box StudioStatusStrip (Auto-Pilot, Nächster Termin, Warteschlange, Prüfungen offen, Plattformen).

Pipeline (pool):
- PipelineOverview (Jetzt / Braucht dich / Als Nächstes), VorratsHinweis
- filters Alle/Neu/Freigabe/Geplant/Gepostet/Fehler/Archiv, search (title, login), platform filter, list/grid, "Clips holen", pagination 24, "MP4 hinzufügen"
- ClipCard: status chip, title, source/login/views, platform checkboxes, "Clip freigeben", "Ablehnen", "Post stoppen", "Archivieren", "Plattformstatus", "TikTok erneut einplanen"/"TikTok-Veröffentlichung vorbereiten", terms per platform, TikTok inbox/pending/waiting hints, waiting-connection hint plus "Verbindungen öffnen", YouTube visibility, TikTok publish state, own layout, upload errors, cancel result, "Kadenz auf null" hint, contest result
- clip menu: "KI-Bearbeitung & Prüfung", "Layout anpassen", "Titel, Beschreibung & Hashtags", "Vorschau prüfen", "Vorschau rendern", "Für Clip-Contest einreichen", "Original ansehen", retention date
- dialogs: ClipEditorDialog (layout override, EnrichmentPanel), PlatformStatusDialog, TikTokPostDialog

Auto-Pilot & Zeitplan (plan):
- ScheduleQueuePanel: next free slot, hold reason, switches "Auto-Pilot einschalten", "Neue Produktionen pausieren", "KI-Bearbeitung verwenden"; target platforms; timezone; lead time; weekly slots (max 28); save; production queue with "Als Nächstes", up/down, "Entfernen", review actions (process/skip)
- PublishReviewPanel: publish_mode review/full_auto ("Vollautomatik einrichten" dialog with pending handling), pause/resume, produced video rows (per-platform connection, approval, publication state, retry, select all, "Ausgewählte Ziele freigeben", "In den Zeitplan übernehmen", "KI-Fassung überarbeiten", TikTok options), notification and retention preferences, notifications list
- "Bisherige Pipeline" details: approval mode manual / veto_window / full_auto, per-platform auto_post, posts per week, max per day, post times, timezone, categories, posting rhythm aside, VorratsHinweis

Clips & Import (library): pool preferences (date filter all/year/quarter/range/before/after, sort, timezone, browser timezone), save/discard, "Import starten", "Import fortsetzen", ImportProgress (status, filter, capture window, 5 counters, active window, retry, error, incomplete windows), pool list (thumb, select checkbox, duration, title, date, views, status chip, Twitch link, notes), bulk select/unselect per page, pagination.

KI-Bearbeitung & Prüfung (ai): external AI consent switch with confirmation, global lock state, options (multiple videos, experimental layout, publish language), clip picker, job list, job details, versions, revision controls, metadata, retry.

Vorlagen & Konten (konten): PlatformConnectionsCard (states unknown/unavailable/not connected/expired/shared/connected, expiry hint, TikTok stats note, connect/reconnect/disconnect, OAuth return message), VodArchiveCard (enabled, privacy), LanguageCard, 3 layout presets plus saved layout, LayoutEditor.

VOD-Archiv (archiv): VodArchiveTab entries with status, retry, Drive fallback, hide, YouTube check.

## Findings

Two automation systems exist and stay separate in the backend:
1. Ongoing pipeline for new Twitch clips (legacy PostingPlan, approval.rs): approval mode manual / veto_window / full_auto, per-platform cadence, legacy upload queue. This is "Bisherige Pipeline".
2. Auto-Pilot from the clip pool (autopilot_settings, production jobs, produced videos): "Auto-Pilot einschalten" starts production at weekly slots; "Vollautomatik einrichten" is its publish_mode (publish produced videos without per-video approval).
"Vollautomatik" therefore exists twice with different meaning. Not merged in the backend; UI names them distinctly.

Clip pool selection: the Auto-Pilot planner orders candidates by `selected_at IS NULL` first (planner.rs select_on). Selection means "Auto-Pilot nimmt diesen Clip zuerst". Real function, kept, labelled "Vorziehen".

Clip numbers:
- 174 "im Pool" = posting_plan::verfuegbare_clips: legacy clip table, enabled categories, not discarded, not posted everywhere
- 184 "erfasste Clips" = social_clip_pool total for the import (Deadlock clips captured by the historical import)
- 188 "Einträge im Clipbestand" = AI editor clip list total (all legacy clips, any status)
- 348 Posts = 174 x 2 active platforms; 221 days = 348 x 7 / posts per week; warning below 7 days
Unified terms: "Clips verfügbar" (174), "Im Clip-Vorrat erfasst" (184), "Alle Clips" (188). "348 Posts" dropped; banner says "174 Clips verfügbar, reicht für etwa 221 Tage".

Bugs, causes:
- past "Nächster Termin": StudioStatusStrip does not filter terms < now (stale next_slot, past jobs or events). Fix: one shared upcoming-terms function filtered by now, used by status bar and "Als Nächstes".
- 01.01.1970: ImportProgress prints job.started_at, which is epoch for filter kind all. Show "Alle verfügbaren Clips".
- Vorrat icon: always AlertCircle. Use CheckCircle green when not warnung, AlertTriangle orange when warnung.
- "Freigabe" shows "Geplant": queueStage returns review for TikTok waiting states even when the clip is approved and other platforms are scheduled; card shows "Geplant". Fix: approved clips with TikTok waiting go to "Braucht dich" only as a TikTok action, stage planned.
- identical titles: list shows Twitch clip title (often stream title). Generated titles exist in enrichment (title_youtube etc.) but are not in the list payload.
- Language: the global header already has DE/EN; LanguageCard is a duplicate.
- Timezone: three stores today (legacy plan timezone, Auto-Pilot settings timezone, import pool preferences timezone). No global dashboard timezone exists.

## Target structure

Views (`?view=`): heute (default), freigabe, planen, veroeffentlicht, einstellungen. Old values redirect: pool to freigabe; plan, schedule, autopilot, library, import to planen; ai, review to freigabe; konten, settings and OAuth returns to einstellungen; archiv, vod to veroeffentlicht.

Status bar (all views): Auto-Pilot an/aus, Konten verbunden x/3, Vorrat reicht X Tage.

Heute: "Braucht dich" (open clip approvals, produced videos in review, review jobs, expiring connections), "Läuft gerade" (running production, running AI jobs, imports), warnings and errors, next upcoming terms. One primary: the first open action.

Freigabe: clip list with filters, search, platform filter (old Pipeline); second segment "Fertige Videos" (PublishReview video rows). Upload as secondary. Clip menu gets "Mit KI bearbeiten" (opens the AI editor for that clip in a dialog; the result lands as a produced video in Freigabe).

Planen: section Zeitplan (Auto-Pilot switch, pause, slots, lead time, target platforms, production queue, publish mode), section "Neue Twitch-Clips" (old Bisherige Pipeline: approval mode, cadence, times, categories), section Clip-Vorrat (import with a popover for time range and order, one result line with expandable details, pool list with "Vorziehen").

Veröffentlicht: posted clips, errors, VOD archive as a plain list with a detail menu ("Bei YouTube prüfen", "Zuletzt geprüft", "Aus der Liste ausblenden", retry, Drive). "Auswertung" lives here.

Einstellungen: Konten (one PlatformCheck row per platform, local SVG logos), VOD-Archiv switch and privacy, Layout-Vorlagen, Produktion (external AI consent as status plus change link, multiple videos, experimental layout, publish language, "KI-Bearbeitung verwenden"; greyed out with hint when the global lock is active), Benachrichtigungen.

New components: StudioStatusBar, PlatformCheck, StatusChip, HintChip (tooltip), ImportPopover, VodArchiveList.

Removed: LanguageCard (header switch covers it), 5-box strip, separate KI tab, "348 Posts" figure. Nothing else removed.

## Steps

1. Structure: new views, URL aliases, nav, status bar.
2. Pages: Heute, Freigabe, Planen, Veröffentlicht, Einstellungen, moving existing components.
3. Global settings: language removal, timezone per decision.
4. Visuals: chips, one primary per view, no nested cards or glass, 9:16 thumbs, type scale, contrast, max width.
5. Bugs.
Each step: tsc, lint, vitest, Moli check, commit, push.

## Open questions

See chat; answers recorded here when given.
