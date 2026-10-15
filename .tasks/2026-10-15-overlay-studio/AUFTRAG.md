# Stream overlay studio

Goal: refactor `/twitch/overlay` into a compact action bar and responsive stage/inspector workspace. Preserve navigation, theme tokens, generated URLs, storage schema, presets and editor behavior.

Acceptance: one heading; save/copy in the action bar; always-visible grouped 11 module switches; adjacent style/game-mode controls; editor/live/split views; compact selection footer and expandable shortcuts; inline OBS URL and five-step instructions; one column below 1200px without horizontal overflow.

Implementation retains all existing source-editing, selection, drag, resize, snapping, URL and storage handlers. The fixed card remains 440 × 660, matching the actual existing renderer rather than changing overlay output to 640px.

Evidence: `/tmp/overlay-studio-evidence/verification.json`, `desktop-final.png`, `mobile-final.png`. Moli verification completed 23 checks including all 11 switches, presets, mode preservation, centering, actual CDP pointer drag/resize, snapping toggle, storage, URL copy, instructions, theater/Escape and overflow at 1440/390px. No page errors. Moli requires bubbling change events for select controls and explicit disclosure setup in automation; the header instructions trigger itself was exercised.

Existing regression checks before rebase: 38 passed, 0 failed, 0 ignored (`brandPalette`, `dashboardShell`, `i18n`). After rebase onto current main: 37 passed, 1 inherited failure, 0 ignored. The clean-main `dashboardShell` baseline is 12 passed/1 failed and identifies `Verwaltung.tsx` with `max-w-[1440px]`, unchanged by this package. TypeScript and targeted ESLint pass. Production frontend build passes. Preview fixture now supplies the raw profile fields consumed by `fetchInternalHome`, retaining its existing normalized fields.
