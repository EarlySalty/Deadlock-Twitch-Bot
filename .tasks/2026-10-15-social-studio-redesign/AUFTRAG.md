# Social studio structural redesign (dashboard_v2)

Goal: calm, card-based layout for the social media manager (tabs Heute, Freigabe, Planen, Veröffentlicht, Einstellungen) per user spec of 2026-10-15.
Branch: feat/social-studio-redesign-20261015, worktree /home/nathanael/.worktrees/tb-social-studio-redesign-20261015
Invariants: DashboardSidebar, DashboardShell, shared-theme/industrial-gold.css, API contracts in src/api/socialMedia.ts and autopilot/*/api.ts unchanged.
Acceptance: npm test green, build green, Moli screenshots 1440 and 390 for all five views: no broken platform icons, consistent cards, no per-panel refresh buttons, single header sync action.
Root cause icon 404: PlatformCheck.tsx uses absolute /brand/platforms/*.svg while vite base is /twitch/dashboard-v2/.
