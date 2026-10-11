# Provider accounting and consent-permission repair, round 3

Worktree: `/home/nathanael/.worktrees/tb-social-ai-provider`.
Branch: `feat/social-ai-provider-20261011`.
Parent verified clean HEAD: `c69bd31ebd6edac33812d02b97772b10e63b9123`.
Completed round-2 workers must not be resumed.

## Parent-confirmed accounting defect

`ledger.rs:171-183` accepts a historical generic completion without `social_accounting` for any started row with matching ID/project/service. It can finalize a Social/Anthropic row without restoring its higher known estimate and cache accounting. The new started-attempt block then disappears; native replay cannot update the finalized row. Preserve compatible historical non-Social/Fireworks replay, but reject incomplete generic replay for native Social attempts and leave them unresolved/fail-closed until authoritative reconciliation. Also do not treat an already-finalized Social row as successful generic replay without accounting. Native replay remains idempotent only for matching accounting.

Use a concrete historical Social payload, including an estimate above the reservation, failed-finalization recovery and next reservation. Prove incomplete replay cannot clear the hold, higher debit is not forgiven and correct native replay permits safe continuation. Keep unknown usage reserved/unknown and legacy non-Social behavior unchanged. No second budget, local files, pricing/model/cap change or invoice claim.

## Effective role-matrix finding and bounded triage

The earlier audit-write claim is still refuted: the migration explicitly grants consent SELECT only to `twitchaudit`. Parent now read the entire actual `ops/systemd/twitch-runtime-roles.sql`. Its broad grants give all-table writes to bot/dashboard/legacy, and no later block narrows the new consent table. That is a real least-privilege omission for the legacy role and bot. Dashboard write authority is necessary for the authenticated consent UI, not by itself an unauthorized cross-user request.

Establish the narrowly intended matrix after all broad grants: dashboard may read/insert/update consent through the existing authenticated numeric-channel guards; the production bot reads consent for its external-transmission gate but cannot grant itself consent; legacy has no consent writes. Keep audit non-writing and preserve explicitly intended read access where applicable. Remove unneeded DELETE/DDL-style rights. Do not invent per-user DB accounts/RLS architecture or claim shared service-role grants prove channel isolation. The later API authorization proof remains mandatory. Manual consent endpoints in the integration will use the dashboard writer, not bot-side consent mutation.

Reuse the repo's convergent feature-permission pattern so migration plus post-migration runtime-role application cannot widen rights again. Own the minimum additive consent block/helper in the runtime-role script and the still-unapplied provider migration, plus directly relevant isolated tests. No broad role rewrite or unrelated ledger privilege changes. Execute the real post-migration role script in disposable Postgres and verify effective privileges, repeat application, missing optional table behavior and required legitimate reads/writes.

## Checks and boundaries

Prior round reports package fmt, release build, strict all-targets Clippy and 57 tests green. Some legacy DB tests silently self-skip without their test DSN; distinguish that from actually executed disposable-PG proof. Retain the new native recovery/schema/consent/budget tests. Run matching release/jobs3 checks through cargo-slot and shared sccache/artifacts, no bypass or foreign process interruption. Commit only verified owned files, English message ending `Co-Authored-By: Claude Code <noreply@anthropic.com>`.

No further delegation, T3/session coordination, production DB/data, secrets, paid call, push/main/gate/deploy or publication. Process inspection pid/comm only; never arguments/environment or the excluded core reviewer log. English work/report, German product text, no added code comments or em dashes. Parent owns final intent acceptance, integration, migrations/grants, gate/deploy and first paid proof. Handover full SHA, exact paths, actual accounting/permission semantics and checks/limitations.
