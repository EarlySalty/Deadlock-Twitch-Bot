# Scam ban + Anthropic Haiku switch (Twitch bot)

Branch `fix/scam-ban-haiku`, worktree `/home/nathanael/.worktrees/ttb-scam-ban-haiku`.

## State

- Gate round 1 BLOCK fixed in ada00b0fa; gate round 2 ALLOW (Opus).
- Merged to main: ada00b0fa9da67d3e639af38fa70117a7d4ab614.
- Deploy submitted via deploy-queue (thread da46f63f-6b02-4847-af81-2c6d90b72c82).

## Function proof after the deploy-queue live message

1. `/usr/local/bin/deploy-twitch-release --pruefen` exit 0 on SHA ada00b0fa (bot + dashboard units).
2. `journalctl -u deadlock-twitch-bot-rust.service --since <deploy time>`: no errors from tb_llm, no 412 from Fireworks for spam_judge / conversation scam judge.
3. Postgres `llm_usage` (DEADLOCK_CENTRAL_DSN via export_claude_secret.py): new rows with provider `anthropic`, model `claude-haiku-5-5`, success; Fireworks-only use cases (ricky_crew_review, outreach_shadow, promo_pitch) still provider fireworks.
4. Journal shows `Conversation-Scam-Judge-Entscheidung` when a judged chat happens (may need waiting for real traffic).
5. Then delete local branch, remote branch `fix/scam-ban-haiku` and this worktree.
6. Tell the user: Fireworks account suspended for billing (HTTP 412), still hits Fireworks-only use cases and other bots.
