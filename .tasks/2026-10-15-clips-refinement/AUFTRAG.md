# Clips page refinement

Goal: implement the user's five specified visual refinements to /clips while preserving the anthracite/brass palette, serif headings, all props, handlers, form behavior and data bindings.

Scope: existing clips frontend components and their scoped styles. No backend changes, redesign of other routes or new dependencies. Main orchestrator owns integration. Worker owns clips UI changes in this worktree.

Acceptance:
1. Secondary instructions and empty states have quiet surfaces instead of hard gold frames; submission remains the gold-framed focal point with a subtle radial glow.
2. Instructions become a full-width horizontal three-step timeline below the hero, stacking cleanly on mobile. Existing phase data determines the active indication; no hard-coded October phase.
3. Usage count becomes frameless helper text or a quiet label badge. Input and submit button both measure 44px, with a gold keyboard focus ring.
4. Top-three empty state has a centered trophy badge and constrained text (the user's explicitly permitted single-container option).
5. Hall of Fame loses its duplicate selected-month subheading; month selector aligns with the section title, with a compact archive/film empty state.

Proof: inspect incumbent components and design tokens first; run existing relevant formatter, lint, build and tests; use Moli only for visual and functional checks at 390px and 1440px. Inspect saved screenshots. Verify empty and populated states, usage count, phase display and month selection without real submissions. Preserve existing logic.

Working branch: ui/clips-refinement-20261015, initial HEAD dbc872be1. Do not commit, push, merge, deploy or publish without explicit permission from the main session. Report checked working-tree changes, commands/results, screenshots and real blockers. No secrets, code comments, unrelated refactoring or foreign-checkout writes. Product UI text stays natural German; internal work and reports are English.
