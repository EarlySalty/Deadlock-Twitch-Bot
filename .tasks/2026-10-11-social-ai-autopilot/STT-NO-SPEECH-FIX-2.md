# Preserve VAD backend errors, round 2

Worktree: `/home/nathanael/.worktrees/tb-social-ai-stt-deploy`.
Branch: `feat/social-ai-stt-deploy-20261011`.
Parent-verified clean HEAD: `d1caa8b14e186f455aebc719b268334acff58b5d`.
Round 1 and packaging workers are completed and must not be resumed.

## Confirmed defect and preserved work

Round 1 proves fresh-release exact-zero silence, recognizable local synthetic English speech, original mixed timing and fail-closed unreliable speech. Preserve that repair and the accepted optional model-install, bounded-copy and full root-controlled-path protections. No production installation or source-adapter acceptance has occurred.

Independent reviewer `a180926556b50fdb9`, actual Opus 5.5 verified, returned BLOCK for hidden native VAD compute failure. Parent verified the pinned dependency source: `whisper_vad_detect_speech` resizes `vctx->probs` at whisper.cpp:5101; on failed graph compute it emits `WHISPER_LOG_ERROR` and breaks at 5149-5151, then returns true at 5165. Resizing can leave zero or previous-call probabilities. A full-length probability vector or successful Rust Result alone therefore cannot distinguish completed inference from this failure. `segments_from_samples` can produce zero segments and our server returns successful empty text.

## Minimal repair

Make native VAD errors fail closed before either no-speech success or speech decoding. Do not infer success merely from probability length, finite zeros or stale buffers. Do not parse stderr from another process, blacklist transcript text, invent speech, disable VAD or change the accepted confidence policy.

An existing safe route is available for investigation: whisper-rs 0.16 exposes `install_logging_hooks()` in lib.rs:75 and the existing `tracing_backend` feature. The parent read whisper_logging_hook.rs:39-53: native GGMLLogLevel::Error reaches the tracing error macro through the installed hook. A narrowly scoped native-error observer may detect this otherwise swallowed failure, including errors on native worker threads, independently of the configured console verbosity. Verify the actual GGML/Whisper hook targets and feature implementation before relying on this route. Do not claim a returned Err-only fixture exercises the swallowed path. Keep unrelated native errors conservative rather than manufacturing success.

Own only `rust/bin/tb-stt-server/src/main.rs`, its Cargo.toml and directly necessary locked feature changes or synthetic regression fixtures/tests. No new library, manual FFI, application unsafe, registry edits or vendoring. If existing safe APIs cannot reliably enforce the contract, return the precise dependency blocker rather than widening into an unapproved architecture change.

## Acceptance and checks

Prove the emitted native backend error cannot produce HTTP success/empty text, including reused same-size probability state. Demonstrate error observation is not disabled by console filtering and covers the pinned error-emission path. Separate source-verified callback mapping and injected diagnostic proof from an actual resource-induced backend failure; report honestly which ran.

Rerun the relevant fresh-release silence/synthetic original-language speech/mixed original-timing checks, with unchanged confidence policy and translation disabled. Preserve original duration/audio, successful empty no-speech only after valid complete VAD, and error for speech without reliable text. Run scoped fmt, strict Clippy, release build and existing server tests through cargo-slot release/jobs3 and shared artifacts. Exclude both systemd/recovery fixtures. Commit verified owned files with the required Claude Code trailer, rebuild and verify the clean SHA stamp. Handover exact APIs, proof, limitations and SHA.

No extra agents/workflows/T3/session coordination, secrets, process arguments/environments, production data, paid/external STT/TTS calls, model downloads/copies/installation, root/service changes, push/main/gate/deploy or publication. Process inspection pid/comm only. Never access the excluded reviewer log. English work/report/commit, German product errors, no added code comments or em dashes. Parent P2 owns integration and final acceptance; main orchestrator is `eddfce29-0b2a-4f47-931f-8b5387672f94`.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-social-ai-stt-deploy
