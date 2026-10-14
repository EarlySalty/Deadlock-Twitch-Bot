# AP finish contract

Goal: finish connected target selection, review-job decisions, selected-clip priority, shared-slot retiming and automatic review accounting from PKG-AP.md. Preserve static rendering and all unrelated packages. Acceptance: authenticated schedule/resolve routes persist owned changes; one review job does not block; full auto avoids regular review backpressure; selected clips precede ordinary eligible clips in the configured sort; schedule edits assign future distinct slots without touching running work; manual AI reviews do not count; real static fixture records no social_editor usage.

BESTAND[BS-1]: teilweise | Fundort: rust/crates/tb-social-media/src/autopilot_schedule/settings.rs:35 | Anknüpfung: existing settings, owner locks, shared slots, queue routes and dashboard fixtures

Source: /home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/briefs-20261014/PKG-AP.md and COMMON.md. No paid AI, real publication or live streamer-setting changes permitted. Initial branch contains inherited uncommitted backend changes. UI/API wiring and concrete PostgreSQL regressions complete this package.
