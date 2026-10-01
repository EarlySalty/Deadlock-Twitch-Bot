# Partner effort engine

User scope: Rust/Postgres engine, two session-only challenge read endpoints, one migration, configuration, SQLx metadata, schema snapshot, tests and CI. No chat or Discord writes. No merge, deploy or service restart. Implement directly, no local delegation.

Reuse graphify graph at 8c800bb and global graph: community co-play/session history; canonical category snapshots for Deadlock duration; shared-chat Helix transport; Steam party membership and cached match history; invite qualification/referral source tables; clip contest outbox. Existing draft is untouched. Ledger name partner_effort_events remains compatible with PR 995/996.

Allowed paths: rust/Cargo.toml, rust/Cargo.lock, rust/crates/tb-effort/**, rust/crates/tb-config/**, rust/crates/tb-transport-twitch/**, rust/crates/tb-dashboard-api/**, rust/bin/tb-bot/**, rust/migrations/20260927110000_partner_effort_engine.sql, rust/.sqlx/**, rust/crates/tb-db/tests/fresh_schema_snapshot.txt, docs/partner-effort-engine.md, docs/partner-effort-config.toml, .github/workflows/**, .tasks/2026-09-27-partner-effort-engine/**.
