# M1 media-only runtime interface

Role: sole Rust interface implementer within the CURRENT M1 gate-fix round. Parent78e43a4b-d7c4-4910-adb3-26be78165791; main orchestrator eddfce29-0b2a-4f47-931f-8b5387672f94. Own ONLY /home/nathanael/.worktrees/tb-social-ai-m1-media-runtime-20261012, feat/social-ai-m1-media-runtime-20261012, clean creation810b5e76e63df33e5fe12d9bbe0ac7fea52cdfff above main0085. No extra agents/workflows/T3 threads, peers or user questions. Continue directly to checked implementation.

## Genuine owner/interface decision

Static production must never depend on AI/provider validation. P2 owns this addition; P1 B is already coding against the exact signature. Implement in ai_editor/runtime_config.rs:

pub struct MediaRuntime {
    pub package_root: PathBuf,
    pub source_root: PathBuf,
    pub work_root: PathBuf,
    pub output_root: PathBuf,
    pub resources: ResourceLimits,
}
impl MediaRuntime {
    pub fn load_packaged() -> Result<Self, AiEditorError>;
    pub fn load(package_root: &Path) -> Result<Self, AiEditorError>;
}

ResourceLimits is the EXISTING ai_editor model type, not a replacement. Read the SAME ops/social-ai/editor.yaml runtime section. Validate only its paths/roots and resource limits with today's intrinsic rules; ignore provider/model/skills/STT/consent validation. Invalid or absent AI-only sections must not disable an otherwise valid media runtime. Malformed YAML or invalid media paths/resources still fails closed. Provider-relative resource checks remain only in the full editor loader, not media-only loading.

Single loader: EditorRuntime::from_configuration builds on MediaRuntime internally and exposes pub fn media(&self) -> &MediaRuntime. No duplicated defaults or fallback values. Reuse the existing packaged-root/path resolution and shared media validation. Preserve existing EditorRuntime public configuration/limits/package_root consumers and full AI validation, snapshots and resolved values. Make MediaRuntime reachable for P1 consistently with existing EditorRuntime exports; the user specified runtime_config.rs, so a public runtime_config module plus root re-export is acceptable minimal exposure.

P1 owns its runner/startup changes: it loads MediaRuntime independently, passes editor=None on full EditorRuntime failure, holds AI as ai_editor_unavailable, and logs one error/keeps bot running if MediaRuntime fails. DO NOT edit P1 files or implement that caller here. P1 has read-only access to P2 files.

## Disjoint ownership and scope

Exclusive writes: ai_editor/runtime_config.rs; ai_editor/mod.rs solely module visibility/re-export; tb-social-media/Cargo.toml only promoting the EXISTING serde_yaml0.9 dev dependency to a normal dependency if needed; Cargo.lock only its necessary existing-dependency bookkeeping; a new dedicated media-runtime test file or tests inside runtime_config.rs. No new library/version, migration, YAML/default/model/rate/cap, provider internals, production queue, startup, frontend or unrelated refactor.

Concurrent expiry fixer a7f918507a42a0562 owns pipeline.rs, pipeline_tests.rs, service.rs, store.rs and a necessary produced_video helper. Those files are READ-ONLY here. Parent will combine both checked patches before the ONE next M1 gate. Do not take the expiry repair or modify its tests. Direct constructor search found no external EditorRuntime struct literals: existing test helper returns the normal factory, so this interface can remain confined to your files.

## Acceptance and checks

Contract guards: valid media section loads despite invalid/missing provider/model, unavailable skills and invalid STT; full EditorRuntime still rejects invalid AI configuration; invalid media roots/resource bounds reject; a valid editor's media() returns the same canonical roots/resource values as its normalized runtime. Use existing packaged YAML/fixtures and synthetic/local inputs, no secret/production data or external call. Test only concrete contract coverage, preserve existing checks.

Use code-suche/Graphify before targeted reads. Existing resource type and root rules are in model.rs/runtime_config.rs; serde_yaml0.9 is already a dev dependency, so do not invent parsing or a different library. Run matching owned formatter, affected Social library build/lint and relevant media-runtime tests once. No full API/import-pool/STT/whole-workspace suite. Inherited owned-lint101 remains explicit and does not authorize unrelated repairs. Every compiler command: cargo-slot --release --jobs 2 --locked, ionos remote DEFAULT, NEVER CARGO_SLOT_REMOTE=0, one release build host-wide. Normal shared locks, shared sccache/central per-workspace artifacts, frozen source during Cargo; no profile/target/build-dir/cache/model/artifact copies or foreign process stop.

SPEED DIRECTIVE: no hash manifests, scratch hosts, pinned-copy re-proofs, extra source review or parent acceptance round. No UI change/browser required; parent reuses the existing390/1440 set. Keep this in the current gate round. If measured compiler/contract scope makes the ADDITION exceed about one hour, preserve the concrete source/checkpoint and report that blocker immediately so parent can follow the user's M1-first follow-up decision. Do not spend that hour on unrelated code or verification ceremony.

## Safety and return

Follow global CLAUDE.md, ABLAUF.md, COMMON.md, RUST-BUILDS.md. Rust production/Postgres only, no code comments/production unwrap/needless unsafe. English internal text and commits, German product strings, no em dashes. Never inspect/output secrets, token/session files, unredacted DB data, process arguments/environments or excluded reviewer log; pid/comm only. No production data/consent/migration/grant writes, service/install changes, publication, deploy or main action. Ordinary fixtures need no production DSN; rotated credentials are only via the prescribed Infisical boundary for an authorized action with an actual auth failure.

Commit own checked files, normal hooks, no .tasks in source commits, no force update, final Co-Authored-By: Claude Code <noreply@anthropic.com>. If a higher-priority metadata restriction applies, return unpublished source for parent correction rather than publishing known-wrong metadata. Reread THIS brief at interface/checkpoint boundaries for a possible concrete expiry-handover amendment, without polling. Return clean actual HEAD, exact public signatures/type, media-vs-AI validation behavior, affected build/tests/exclusions, dependency bookkeeping and measured blocker if any. Parent combines expiry/interface patches, immediately gates once, merges after ALLOW and sends main SHA. Deployment is batched after M1 plus P1 B, P3 if ready.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-social-ai-m1-media-runtime-20261012
