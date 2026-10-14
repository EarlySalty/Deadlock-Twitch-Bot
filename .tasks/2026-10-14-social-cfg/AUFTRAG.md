# CFG continuation

Scope: BRIEF-CFG.md and COMMON.md in the coordinator task, resumed in the existing CFG worktree. Phase 1 consolidates Social AI settings into the hash-checked main bot TOML, removes separate TOMLs, preserves provider and runtime validation, then deploys and verifies startup and admin routes. Phase 2 follows only after phase 1 is live: typed operating catalog, admin read/write API, existing Betriebseinstellungen page, German copy, Moli screenshots at 390 and 1440.

No paid AI calls, publishing, account-setting changes, Brain enablement, secrets in outputs, or edits to foreign checkouts. Own branch: feat/social-cfg-bot-toml-20261014. Fresh origin/main must be integrated before gate. Migrations checked before deploy.

BESTAND[BS-1]: teilweise | Fundort: rust/crates/tb-config/src/editor.rs:580 | Anknüpfung: existing shared file lock, hash checks, metadata-preserving writes and backup; existing operating admin routes and page; Deadlock-Bots typed catalog reference.

Inherited changes reviewed. Existing media-loader tests migrated to table input; malformed, oversized and invalid-encoding checks now exercise the main config loader. Added active main snapshot proof for both packaged loaders. First queued Cargo check was stopped at its 120-second background limit before acquiring a slot; restarted with the normal background limit.
