# Merge-Gate

Gate-Aufruf nach dem Implementierungscommit:

```sh
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-tiktok-vollbild --base origin/main --head feat/tiktok-vollbild-render
```

Die lokale main-Referenz im geteilten, unveränderten Checkout liegt hinter origin/main. Deshalb wird der tatsächlich frisch geholte Hauptbranch als Vergleichsbasis benutzt.

Status: erste Gate-Runde noch ausstehend. Bei BLOCK erfolgt ein frischer nativer Fixer, danach derselbe Reviewer bis ALLOW. Keine separaten Review-Threads.
