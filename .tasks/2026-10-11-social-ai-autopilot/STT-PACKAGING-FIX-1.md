# STT packaging repair, round 1

Worktree: `/home/nathanael/.worktrees/tb-social-ai-stt-deploy`.
Branch: `feat/social-ai-stt-deploy-20261011`.
Preserved clean HEAD: `1056488f6afc2c541edffba307f6b6c41b6edf19`.
Completed implementer/reviewer are not to be resumed.

## Three parent-confirmed defects

1. `ops/systemd/deploy-twitch-release:370-375` unconditionally invokes the model installer. An unrelated/default-path deployment now fails when the HOME model cache is absent. The AI pipeline is optional; default compatibility must not depend on copying models.
2. `tb-stt-server/src/main.rs:484-520` rejects symlink ancestors but checks write permissions only on the final directory and compares file owner to directory owner, not root. A user-owned ancestor can rename/replace the verified path before Whisper opens it at `:213` or VAD opens it during `:399-408`.
3. `install-stt-models.sh:52-69` streams the user-writable source into a root-owned temporary file without a byte bound. Hash rejection happens after unbounded copying, including a growing source.

## Required minimal repair

Keep default HOME-cache behavior unchanged when `model_directory` is unset. Do not make optional AI model installation a requirement of unrelated deployments. Use an explicit, bounded one-time installation step through the verified normal wrapper, or an equivalent fail-closed existing configuration path. A narrowly additive deployment opt-in is acceptable as a technical default choice: normal invocations remain unchanged, installation uses only the frozen verified release's helper, and integration records the exact invocation. Do not add another product feature flag, broad privilege path or ad hoc untrusted config parser.

For explicitly pinned service models, verify root ownership and non-writable/non-symlink regular-file/directory ancestry before loading. The complete ancestor chain must prevent an unprivileged user from replacing verified paths. Preserve the current pinned hash checks and no-download behavior. Scope guarantees to the unprivileged service threat model; do not claim protection from a malicious root operator. No user-home bind or weakened hardening.

Bound root copy bytes while reading, not merely with a pre-copy stat. Require exact expected size plus hash and cleanup owned temporary files on all failures/interruption. Preserve user caches and unknown existing target files; do not delete them. Parent verified the cached files against the pinned public hashes on 2026-10-11:

| File | Exact bytes | SHA256 |
| --- | ---: | --- |
| ggml-large-v3-turbo-q5_0.bin | 574041195 | 394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2 |
| ggml-silero-v6.2.0.bin | 885098 | 2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987 |

These are model metadata, not secrets. No production copy/installation has occurred. Keep only this configured supported Whisper model and required VAD file; never copy models per worktree/release.

## Ownership, checks and handover

Own only the previous ten STT packaging/config/test paths and the minimum directly necessary additive helper changes. Do not touch AI provider/jobs/UI/source/core worktrees or concurrently owned installer extensions. Parent will add editor YAML/skill archive packaging later, after this handover.

Verify default deployment with absent HOME models, explicit installation, repeat no-copy, wrong/hash/size/growing-source bounds, insecure ancestors/non-root ownership/symlinks, owned cleanup, default config compatibility and existing STT configuration/client contracts. Use isolated fixtures only. Do not run the ignored systemd/recovery fixture, start a real service or unmask anything. The earlier accidental ignored fixture must be named in the report with its stopped/inactive proof, not silently counted green.

Run matching formatting, ShellCheck/Bash syntax, deployment tests, release build, targeted Clippy/Rust tests and revision-stamp proof through shared cargo-slot policy. Global slots were measured held at 07:15 UTC; wait normally, never bypass or stop foreign jobs. The earlier broad tb-config failure at `discord.streamer_role_targets` has no baseline proof; either verify it independently at the identical base or fix a demonstrated regression in owned code. No false blanket green. The existing dirty-base binary stamp is not final deployment proof; the final verified main release is rebuilt before installation.

No root/service/model installation, secrets, paid calls, production DB probes, push/main/deploy or publication by the leaf. English report and commit, natural German operator/product errors, no code comments or em dashes. Commit only verified own files with required Claude Code attribution. Parent owns acceptance, gate and normal deployment. Report exact SHA/paths, default-vs-explicit invocation, enforcement limits, check commands/results and actual remaining blockers.
