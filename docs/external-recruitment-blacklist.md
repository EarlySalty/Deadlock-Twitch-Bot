# External Recruitment Blacklist

This document describes how external recruitment raid targets are handled in the Twitch raid flow.

## Scope

This logic applies only to external, non-partner targets.

It does not apply to:

- active/internal partners
- targets that already transitioned into the partner flow before raid arrival confirmation
- normal internal partner raid messaging

## Confirmed External Raid Counting

External recruitment raid counts are written only after raid arrival is confirmed.

The count is stored in `twitch_confirmed_external_recruitment_raids`.

Important consequences:

- API success alone does not increment the external recruitment count
- recruitment chat messages use the confirmed external count
- if a target becomes a partner before arrival confirmation, the external counting path is skipped

## 4-Raid Threshold

The 4th confirmed external recruitment raid does not immediately blacklist the target.

Instead:

1. The target reaches the threshold at 4 confirmed external raids.
2. A delayed blacklist entry is scheduled in `twitch_external_recruitment_blacklist_pending`.
3. The grace period is 48 hours from the moment the threshold is reached.
4. After the grace period, the target is written into `twitch_raid_blacklist` if the target is still external.

Important consequences:

- a 5th raid is still possible during the 48-hour grace period
- the blacklist is intentionally delayed so targets still have time to react or onboard
- if the target becomes a partner during the grace period, the pending blacklist entry is removed and no external blacklist is applied

## 1-Hour Bot-Ban Recheck

Whenever the bot successfully sends an external recruitment/outreach message, a follow-up bot-ban check is scheduled for 1 hour later.

The pending follow-up is stored in `twitch_external_bot_ban_check_pending`.

At due time, the bot performs a best-effort rejoin probe:

1. Skip if the target is already blacklisted.
2. Skip if the target is now a partner.
3. Ask the chat bot to `part` and then `join` the target again.
4. If the existing chat/join logic detects a bot-ban condition, the target is written into the same `twitch_raid_blacklist`.

Important consequences:

- this is persisted in Postgres and survives process restarts
- this uses the same real blacklist as other bot-ban paths
- failed/transient checks are rescheduled instead of being silently dropped
- the check is best-effort; it confirms chat access through the existing join path and its existing ban detection

## Source of Truth

The effective raid exclusion still comes from `twitch_raid_blacklist`.

The new pending tables only stage future actions:

- `twitch_confirmed_external_recruitment_raids` tracks confirmed external arrivals
- `twitch_external_recruitment_blacklist_pending` stages the delayed 48-hour blacklist
- `twitch_external_bot_ban_check_pending` stages the delayed 1-hour bot-ban recheck
