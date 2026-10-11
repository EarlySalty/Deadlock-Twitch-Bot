# Complete local sources against accepted STT and M0

## Goal and ownership

Finish the preserved local STT lifecycle and read-only Brain adapter for M1, not a new transcription client. Read COMMON.md/AUFTRAG.md in `/home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/`, M1-RUNTIME-INTEGRATION.md and STT-NO-SPEECH-FIX-2.md here. Graphify before source searching.

Owned worktree `/home/nathanael/.worktrees/tb-social-ai-sources`, branch `feat/social-ai-sources-20261011`, committed Brain HEAD `7232bc80e7603d9a6f5c455cd6a499ce553de502`. Own preserved dirty Cargo.toml, untracked ai_editor/stt.rs and tests/ai_editor_sources.rs; Cargo.lock needs reconciliation. No active previous worker. Preserve all this owned work and the Brain commit. Own only these source/test/dependency files and minimal source module wiring, no engagement client/server/installer/config/provider/store/media production edits. Parent owns integration/backup.

The orchestrator requires rebase onto actual live M0 `33656f2c2d2154a8ce888ed1a39180935209ea6e`. Safely complete and verify the owned dependency/source state enough to commit it before rebase if dirty files block rebase. Do not drop dirty work, use shared stash/pop, commit known broken code or force-update branches. Resolve actual conflicts minimally. Adopt P1's Social lint fixes, not copies.

## Accepted prerequisite and repairs

Accepted STT/config/packaging source handover `30d39c41d2a4fcb6a3c938fc2e7eca70bbc53911` exists in `/home/nathanael/.worktrees/tb-social-ai-stt-deploy`, branch `feat/social-ai-stt-deploy-20261011`, pushed and remotely verified. Source ALLOW and eleven permitted server tests plus strict scoped checks passed. Fresh binary `/home/nathanael/.worktrees/tb-social-ai-stt-deploy/rust/target/release/tb-stt-server` reported exact 30d39c41 source stamp. Inspect safe version metadata before using it; no rebuilding/copying models/caches. Parent may rebase the STT branch onto M0 without changing the accepted server behavior. Do not modify that worktree.

The server explicitly runs standalone VAD before full_with_state and rejects swallowed native VAD errors through existing safe logging hooks. Fresh silence returns empty text/segments with original duration; synthetic English stays English, including original-time segments after leading silence. Do not use the old ~/.local/bin server as proof, relax silence assertions or change confidence/translation policy.

Complete Cargo.lock, bind synthetic lifecycle/no-speech proof explicitly to the verified fresh owned server instead of a silent installed-binary fallback, and preserve one-shot spawn/stop/kill/reap, config fingerprint and occupied-port refusal. Genuine no-speech is successful empty output. Preserve full timed segments; detected language remains honestly None if the existing client cannot expose it. Do not invent translation or language inference from text.

Production transient STT: packaged release binary; required root-owned/read-only model files outside /home; legacy_cache_home None. Preserve ProtectHome=yes and masked STT/recovery units, no permanent service, root installation or unit changes. Audit complete executable/model-directory ancestry, not just final-path ownership. The accepted server independently verifies its pinned models and ancestry, but that does not prove adapter executable/path guards. Repair only a demonstrated adapter gap, using safe standard APIs, no unsafe/FFI/dependency invention. Test-only legacy cache support must not become a production fallback.

Existing OpenAiTranscriber::from_local_config remains authoritative. It has fixed 25 MiB upload and 300-second extraction constraints; the adapter currently requires the upload value exactly 25 MiB. Do not promise tighter/during-write audio bounds unsupported by this client or edit engagement production to expand scope. Document precise constraints for runtime configuration. Preserve local loopback/no-proxy/no-redirect boundaries and original-language/translate=false server configuration.

Brain stays optional relevant read-only /v1/retrieve evidence, bounded body/evidence/deadline, release/identity checks, injected credential only. No answer/write calls, arbitrary terms, production credential/request or data duplication. Review the preserved Brain commit too, since previous review stopped before accepted source completion.

## Proof and handover

Run matching formatter, locked release build/check, strict scoped all-targets Clippy and existing source tests via cargo-slot release/jobs3/shared sccache/artifacts. Wait normally on flocks, no bypass/foreign interruption/cargo clean or model/build copies. Do not run ignored systemd/recovery fixtures. Real synthetic silence/lifecycle/cancellation evidence must use the fresh verified binary. Keep any home-model proof explicitly nonproduction, never hardened live proof. Distinguish executed tests and missing/explicit optional prerequisites.

Inherited main tb-raid lint is not a source-review blocker: establish parity, run package --no-deps lint and review the owned diff. No tb-raid edit absent actual parent gate BLOCK. Commit only verified owned files ending `Co-Authored-By: Claude Code <noreply@anthropic.com>`. Report clean SHA, source API/constraints, locked dependency proof, exact server stamp, synthetic lifecycle/no-speech/Brain checks and real remaining limitations.

No agents/workflows/T3/SendMessage/session coordination, push/main/gate/deploy, secrets, production DB/data, external/paid calls, root/service changes or publication. Process inspection pid/comm only; no arguments/environments or excluded reviewer log. English work/report, German product text, no added code comments/em dashes. Main orchestrator `eddfce29-0b2a-4f47-931f-8b5387672f94`; direct coordinator P2 session `78e43a4b-d7c4-4910-adb3-26be78165791`.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-social-ai-sources
