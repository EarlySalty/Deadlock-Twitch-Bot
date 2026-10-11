# Consent reload recovery repair, round 4

Worktree: `/home/nathanael/.worktrees/tb-social-ai-review-ui`.
Branch: `feat/social-ai-review-ui-20261011`.
Parent verified clean HEAD: `756407c8fd29f0b085574be570c96af02482f196` at 07:43 UTC on 2026-10-11.
Completed round-3 workers must not be resumed.

## Confirmed defect

Parent read the actual `AiEditorPanel.tsx`. After an ambiguous PUT error, `reloadConsent()` at lines 214-223 resets the failed mutation and restores switch focus only after a successful scoped GET. If that first GET fails, the query-error branch at line 233 replaces the recovery control with `EditorError`, whose retry invokes `consent.refetch()` directly. A later successful GET never resets `savingConsent`, so `consentError` remains true and both consent changes and starting stay disabled. The disappearing query retry also has no explicit focus restoration.

## Minimal repair and acceptance

Route every consent reload/retry through the same guarded reconciliation handler. After an unsuccessful GET, keep starting and consent mutation blocked without claiming the remote state is known. After a successful retry for the current scope/revision, clear the obsolete mutation error, render the authoritative returned consent state and restore keyboard focus to the consent switch. Preserve initial GET retry, channel/remount isolation, revocation, confirmation discovery/focus and pending/success/error announcements. No player, API, clip-selection or product redesign.

Check the complete sequence using synthetic requests: ambiguous PUT error, failed reload GET, successful retry GET. Demonstrate that the switch becomes usable and start follows the returned individual/global consent values, not the previous cached values. Include initial GET-error recovery and both returned allowed/denied states where directly necessary. Browser proof is Moli-only; first read the existing agent-browser guide. Use the isolated preview and actually inspect relevant 390px keyboard/focus evidence in `evidence/ai-ui-fix-4/`. Synthetic proof is not authenticated live proof or real screen-reader speech. Do not change or hide the existing red Moli playback assertion.

Own only the AI component and its directly relevant tests/preview checks under `bot/dashboard_v2`. Run matching build, targeted lint and relevant existing tests. No unrelated fixes or broad formatting. Commit only verified owned source files with an English message ending `Co-Authored-By: Claude Code <noreply@anthropic.com>`. Task/evidence files stay out of main.

No further delegation, T3 threads, session coordination, secrets, production data, paid calls, push/main/gate/deploy, or publication. Process inspection is pid/comm only. Never access or reproduce the excluded original core reviewer log. English work and report, German product text, no added code comments or em dashes. Handover full SHA, exact paths and actual checks, evidence and limitations. Parent owns combined intent acceptance, merge gate and live proof.
