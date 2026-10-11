# Review UI consent accessibility repair, round 3

Preserved clean HEAD: `35847ff6c435316825ede962b1833231f6bc9af3`.
Worktree: `/home/nathanael/.worktrees/tb-social-ai-review-ui`.
Branch: `feat/social-ai-review-ui-20261011`.
Completed previous fixer/reviewer must not be resumed.

## Parent-verified remaining defects

1. `AiEditorPanel.tsx:218-220` opens consent confirmation while leaving focus on the unchanged off-state switch. There is no disclosure relationship, focus move or opening announcement. A screen-reader user is not told that confirmation is now required.
2. The pending grant/revoke disables controls, but there is no announced saving status. Existing completion/error announcements do not cover the pending state.
3. The save-error text at `:222` implies the previous remote state is authoritative. An ambiguous network failure after the PUT was accepted cannot establish that. Use bounded truthful German feedback and a read-only reload action; do not claim network cancellation or unchanged server state.

Goal: make opening, pending, completion and ambiguous failure understandable with keyboard and assistive technology, without rewriting the panel, API or established consent policy.

## Minimal ownership and proof

Only the AI component directory and `tests/socialAiEditor.test.tsx`. Preserve the finite failure mapping, action-target guards, bounded clip lookup, disabled cancel while a grant is pending, focus restoration, same-channel/cross-channel protections, existing media players and API calls.

Use valid ARIA for the chosen role. An off switch accurately represents saved consent, so it need not falsely turn on merely to open confirmation. Focus into the real confirmation content and a suitable disclosure relationship/announcement can solve discovery without changing saved state. Do not put unsupported role states on a switch. Saving feedback should use a persistent live region that announces actual pending grant/revoke, completion or uncertainty.

Check keyboard opening/focus, pending disabled controls and announced progress, restoration on cancel/save/revoke/error, ambiguous error/reload, and target guards. Run matching frontend build, targeted lint and the existing relevant Social AI tests. No blanket full suite.

Use only permitted dedicated Moli preview for visual/DOM proof. Read `/home/nathanael/Documents/claude-config/wissen/agent-browser.md` first. Record actual focus/semantic target state during consent opening and pending state at 390px. Save and inspect an actual consent/progress image. Preserve the red playback advancement check and report it separately; no other browser or player changes to make that result appear green.

Previous full-page consent image was parent-inspected and genuinely shows its checkbox/actions at x=33, y=925, 324x256. This fixes the prior screenshot-target gap, but is still synthetic preview proof only.

No shared API/shell/backend/dependency changes, paid calls, secrets, production probes, publication, push/merge/deploy or extra agents. English report and commit, natural German product strings, no new code comments or em dashes. Commit own verified files with required Claude Code attribution. Parent owns final mount, integration/gate/deploy and live proof. Actual authenticated playback still requires the main orchestrator to obtain the user's login and explicit OK.
