[Orchestrator] Package P3: Social Media Manager dashboard redesign and platform status

Read first: COMMON.md, AUFTRAG.md, BRIEF.md sections 9 (Generalized Platform Status), 18, 29 in `/home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/`. Load the `rolle-sicht-pruefer` skill for browser checks.

Worktree: `/home/nathanael/.worktrees/tb-social-dash`, branch `feat/social-dashboard-redesign-20261011` from origin/main `3862b764a`. Clean.

Goal: a cleaner, more professional version of the existing Social Media Manager at https://deutsche-deadlock-community.de/twitch/social-media that keeps the current visual identity (dark charcoal, subtle gold accents, grid background, sidebar style). Inspect the existing structure (inventory F01 to F11) and group work areas sensibly: a compact overview plus a few logical areas, for example pipeline/overview, Auto-Pilot and schedule, clip library and import, AI editing and review, templates and accounts. Fix hierarchy, empty space, spacing, oversized controls, long forms, clip library readability, publishing status, processing progress, previews and platform connection visibility.

Generalize the TikTok-specific approval/status check into a platform-independent view that separates platform connection/authorization, per-video publishing approval, and upload/publishing status for all existing integrations (YouTube, TikTok, Instagram). Small backend changes needed for that status view are yours, inside the existing handlers.

Boundaries: P1 and P2 build new components under `components/socialmedia/autopilot/` and `components/socialmedia/ai/`. You own the existing pages, shell, navigation, `studio.css`, existing components, and the final mounting of P1/P2 components into your structure as they land on main. Do not re-implement their features. All UI text German with real umlauts. Do not delete working functionality.

Milestones: M1 navigation grouping, visual cleanup of existing views and generalized platform status, merged and live. M2 after P1/P2 milestones land on main: integrate their components into the grouped areas with consistent styling, including the before/after review layout from BRIEF 18.

Proof: Moli screenshots at 1440, 768 and 390 px before and after, actually looked at; real data on the logged-in path; no regressions in existing Social Media flows (layout editor, preview, approval, accounts, VOD archive).
