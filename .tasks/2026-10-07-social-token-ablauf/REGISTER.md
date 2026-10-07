# Register

- Intent-Thread: d3a1741e-82bc-4a48-865b-2845c663dca7
- Worker: aktuelle T3-Session, keine weiteren Threads
- Worktree: /home/nathanael/.worktrees/tb-social-token-ablauf
- Branch: fix/social-token-ablauf, Basis origin/main 67786ba2
- Status: Implementierung fertig, 351 Social-/DB-Tests und 2 Bot-DM-Tests bestanden. API-Suite läuft als Nachvergleich zur Baseline. Merge-Gate und Betrieb stehen aus.
- Test-DB: tb_social_token_ablauf im vorhandenen Wegwerf-Cluster tb-test-postgres, Port 33045. Eigener Container konnte wegen cgroup-Speichermangel nicht starten.
