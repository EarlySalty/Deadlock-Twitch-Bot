# Rule ban for Discord contact drops (Twitch bot)

Branch `fix/contact-drop-rule-ban`, worktree `/home/nathanael/.worktrees/ttb-scam-ban-haiku`.
Gate: round 1 BLOCK, round 2 BLOCK, round 3 ALLOW (Opus).

## Rule

First-time chatter in the channel, account younger than 90 days, AutoBan mode, and a
hard contact drop (a Discord handle that starts with a letter, has at least 4
characters, contains a digit or `_`, and is not an invite, URL or domain) gets a
channel ban without the LLM judge. No global ban. Verdict category `contact_drop_rule`,
kept out of the learning corpus. Everything else still goes to the judge.

Backtest over 180 days of `twitch_chat_messages`: 19 early-message hits of the soft
pattern; every hit from a new account or one already deleted by Twitch, except one
streamer's `discord.gg` invite, which the hard rule excludes.

## Function proof after the deploy-queue live message

1. `/usr/local/bin/deploy-twitch-release --pruefen` exit 0 on the merged main SHA.
2. `journalctl -u deadlock-twitch-bot-rust.service`: no new errors.
3. When traffic allows: a `Conversation-Scam-Judge-Entscheidung` line with
   `threshold_source=contact_drop_rule`, and a row in `twitch_scam_guard_verdicts`
   with `category='contact_drop_rule'` and `action_taken='banned'`.
4. Then the proof of `.tasks/2026-10-29-scam-ban-haiku/TODO.md` (Anthropic Haiku rows in `llm_usage`).
5. Delete local and remote branches `fix/scam-ban-haiku`, `fix/contact-drop-rule-ban` and this worktree.
