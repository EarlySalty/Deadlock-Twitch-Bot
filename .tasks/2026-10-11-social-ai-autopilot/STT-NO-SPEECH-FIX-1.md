# Genuine no-speech repair using the existing Rust VAD

Worktree: `/home/nathanael/.worktrees/tb-social-ai-stt-deploy`.
Branch: `feat/social-ai-stt-deploy-20261011`.
Parent verified clean HEAD: `c0d51087dae8941aba678203978cc4ae254c585a`.
Packaging round 1 completed source review ALLOW. Its workers must not be resumed. Preserve the accepted optional-deploy/root-path/bounded-copy repair and its ownership boundaries.

## Confirmed cause and exact available APIs

The source adapter's real two-second exact-zero 16-kHz mono fixture returned nonempty text from the existing installed binary; that binary's revision is unverified, so this is not fresh-release regression proof.

Parent independently read the installed dependency sources after the read-only scout: `WhisperState::full` at `whisper-rs-0.16.0/src/whisper_state/mod.rs:292-305` calls `whisper_full_with_state` directly. VAD preprocessing and the successful empty-VAD return are in `whisper_full` at `whisper-rs-sys-0.15.0/whisper.cpp/src/whisper.cpp:7743-7763`, before it calls `whisper_full_with_state`. The existing server's `params.enable_vad(true)` therefore does not run that VAD path through the Rust state API.

Safe existing standalone APIs are in `whisper-rs-0.16.0/src/whisper_vad.rs`: `WhisperVadContextParams::set_n_threads`, `WhisperVadContext::new`, `segments_from_samples(WhisperVadParams, &[f32])`, `WhisperVadSegments::num_segments`. Parent read these actual signatures. No manual FFI, unsafe, new model or library is needed.

## Minimal behavior contract

Explicitly evaluate the already configured Silero VAD before decoding. Use configured CPU threads and the existing supported model file, preserve pinned-path checks and default HOME-cache compatibility. A valid zero-speech result returns a successful empty transcript with original duration and unknown language, without generating model text. VAD errors are errors, never silent successful no-speech.

For speech-positive input, preserve original audio and timestamps, automatic original-language detection and `translate=false`. Do not concatenate/collapse silent gaps or rebase the source timeline. If speech is detected but decoding/filtering produces no reliable transcript, do not label that uncertainty as genuine no-speech. Keep the existing configured confidence policy unless a concrete independently demonstrated voice regression makes a narrowly documented adjustment necessary. The scout's OR-vs-upstream-AND observation is not by itself an accepted mandate to change those thresholds/policy. No transcript blacklists, invented speech or unconditional mute/empty workaround.

Own only `rust/bin/tb-stt-server/src/main.rs` and directly necessary concrete server regression fixtures/tests. No deployment/config/provider/jobs/source/UI changes. Source adapters will adopt the final accepted server/config handover afterward; do not edit their dirty worktree or claim that unfinished fixture now passed.

## Proof and handover

Reproduce the exact-zero silence case against a freshly built release of this worktree, not the old installed binary. Demonstrate zero VAD segments, empty text/segments and preserved duration. Use a locally generated known original-language speech fixture through available local tooling or an already-owned synthetic fixture, not personal/community data or external STT/TTS. Prove speech-positive detection, recognizable original-language text, bounded original-time segments and no translation; preserve input audio. Test VAD failure does not become no-speech and mixed silence/speech does not collapse timing. No model copies/downloads or production model install.

Run matching scoped formatter/release build/Clippy/relevant existing server tests via shared cargo-slot release/jobs3/sccache/central artifact policy. Do not rerun either systemd/recovery fixture or start real services. Preserve masked units and ProtectHome; no root calls or deploy. After a verified commit, rebuild/stamp-check the clean release as needed; no false final-SHA claim from an earlier dirty binary.

No agents/workflows/T3/session coordination, secrets, process arguments/environments, production data, paid calls, push/main/gate/deploy or publication. Process inspection pid/comm only and no excluded reviewer log. English work/report/commit, German product errors, no added code comments or em dashes. Commit own verified files with `Co-Authored-By: Claude Code <noreply@anthropic.com>`. Handover SHA, exact paths/APIs, actual reproduction/speech/VAD/error/timing checks and remaining hardened-runtime prerequisites. Parent owns acceptance and integration.
