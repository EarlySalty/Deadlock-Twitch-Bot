# AP remaining work

## Current state

Worktree: /home/nathanael/.worktrees/tb-social-ap-finish
Branch: feat/social-ap-finish-20261014
Integrated upstream: 50125b0bb67fc2d32b440594a5cd5e95add21c4d, merge 83f2d65fb7a758bfc0365eb84898a6c3cb5c0d8c. Its four changed language files have a matching successful build and 19 passing language tests. No new Rust source came from this merge.
Published checkpoints: implementation 2189ab9f1, formatting fedc7515a, authenticated route regression fc9bec76a.

The five lifecycle corrections passed PostgreSQL/real FFmpeg checks in bd4tda2q9: 36 passed, 0 failed, 0 ignored. Authenticated route checks b950fv4ce passed 5 tests. Normal workspace-policy Clippy bifkm99hl exited 0 with warnings outside AP-owned changes. The independent reviewer accepted all five corrections; a read-only continuation checks compatibility with the newly integrated AI changes.

## Pending integrated checks

Lifecycle checkpoint a14986a24 was committed and pushed. Compatibility review then confirmed an additional partial-variant skip race from upstream: approved/scheduled variants were not discarded and a preselected promotion omitted expiry at its owner-locked recheck. Final owned queue.rs now includes those outputs, rejects AI outputs using upstream's existing rejected-output cleanup, expires ready static reviews, and sets needs_review=true. Source acceptance confirmed this boundary. The matching six-case real-executor regression remains pending with the final package checks. No protected source was edited.

Final AP PostgreSQL/FFmpeg run bscx9ik98 passed all 37 tests, including the six-case real-executor regression: 0 failed, 0 ignored. Its subsequent API compilation was terminated by signal 9 before tests, and Clippy was not reached. Kernel/earlyoom inspection did not establish the cause. Do not duplicate targeted retry bzpjrwa21: it runs only the authenticated API namespace and package Clippy. Do not rerun the passing AP suite unless source changes.

The six-case fixture correction matched the incumbent publisher's legacy upload columns without weakening production code or assertions. Container dl-central-test-postgres-48623 and its throwaway label/binding were confirmed without reading environment values. The dedicated standalone release clone /home/nathanael/repos/twitch-release-ap-finish-20261014 has all three locked frontend dependencies installed; no AP release build or deploy has started. Freeze it to the verified origin/main state after integration.

Slots 2 and 4 are held by foreign old Brain-test PostgreSQL processes (3291556 and 1271923); slot 3 is held by a live Brain test. Their temporary working directories and ages were checked. Do not stop foreign resources or bypass the build locks.

Logs: /tmp/claude-1000/-home-nathanael-repos-Deadlock-Twitch-Bot/615c87f0-2910-4057-9c03-152fd513780e/tasks/<task-id>.output.

Dashboard production build/typecheck after integration passed. The correct tsx invocation with tsconfig.app.json passed all 11 AP adapter contracts. The initial unconfigured Node invocation failed to resolve @/api and is not passing evidence. Final owned Rust formatting and diff checks passed.

## Browser acceptance

Six Moli captures were viewed, covering plan/review/import at 390 and 1440. The final desktop import image renders the actual ImportPoolPanel with existing fixture API and studio CSS. Document width equals viewport width 1440, zero page errors. Whole-page desktop Moli capture and cross-browser pointer compatibility are not claimed. EVIDENCE.md records the viewed files and earlier failed attempts.

Owned preview and Moli processes are stopped. Other browsers, compilers, locks and PostgreSQL instances are not ours to terminate.

## Finalization

1. Collect integrated checks and the independent compatibility verdict; correct concrete AP failures. Commit verified lifecycle corrections and task records, staging only owned paths; push promptly with the required attribution.
2. Run a fresh local gate on the final committed branch. The old ALLOW on 8a23ca5ea does not cover these corrections. On BLOCK use a fresh fixer, at most five unsuccessful rounds.
3. Fetch upstream again after ALLOW. Integrate any new main changes and rerun affected checks before merging and pushing main from owned isolation, never the foreign canonical checkout.
4. Build the verified origin/main state in a standalone clone. The installed deploy wrapper requires nine binaries plus three frontend dist directories and runs deadlock-twitch-migrate.service before application restarts. Follow the verified wrapper, not stale no-migrator/eight-binary notes. No paid AI, real publication or real streamer-setting changes.
5. Verify live PIDs/executable revisions, priority-error journal, binary and public-bundle anchors, and the new unauthenticated route returning 401 rather than 404. Preserve evidence, remove only merged owned resources after an ancestry check, and settle this T3 thread at completion.

Last read-only live check: current d915c030aa08fa65dd730bad4f4edd8feded36e7; bot PID 2642229, dashboard PID 2642391, coaching watcher PID 2643551. All active with zero restarts and no deleted executable. Migration metadata was current. No AP main merge or deployment has occurred.

Final report: at most eight lines with main SHA, fresh gate verdict, release SHA, live proof/UI location and open items. Then python3 ~/Documents/tools/t3-thread.py settle --selbst.
