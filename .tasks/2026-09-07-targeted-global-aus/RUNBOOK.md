# Runbook: Fertigstellen nach Gate-Quota-Reset

Stand 2026-09-07 ca. 15:15: Commit 481e701a liegt auf `fix/targeted-global-aus`
(Worktree `~/.worktrees/targeted-global-aus`), Tests laut REVIEW.md gruen,
Release-Binaries schon gebaut unter
`~/.worktrees/targeted-global-aus/rust/target/release/{tb-bot,tb-dashboard,tb-stream-audit}`.
Der Merge-Kritiker war am 07.09. nicht lauffaehig (Claude-Wochenlimit,
Reset 08.09. 22:00 Europe/Berlin), deshalb steht Merge/Deploy aus.

## Ablauf (nach dem Reset oder auf ausdrueckliches Nutzer-Go ohne Kritiker)

1. Gate: `python3 ~/Documents/.claude/gpt-workers/gate_hook.py --review \
   --repo /home/nathanael/repos/Deadlock-Twitch-Bot --base main \
   --head fix/targeted-global-aus`
   (vorher `git -C ~/repos/_ttb-main-deploy pull --ff-only origin main`)
2. Bei ALLOW, von `~/repos/_ttb-main-deploy` (dort liegt main):
   `git merge --ff-only fix/targeted-global-aus && git push origin HEAD:main`
3. Branch und Worktree loeschen:
   `git worktree remove ~/.worktrees/targeted-global-aus` (erst nach Step 2),
   `git branch -d fix/targeted-global-aus`
4. Release nach Memory twitch-release-deploy-weg:
   - Freeze-Clone: `sudo git clone --no-hardlinks ~/repos/_ttb-main-deploy \
     /opt/deadlock/twitch/builds/<voller-sha>`, darin `<sha>` auschecken,
     Remote wieder auf GitHub setzen.
   - Die drei Binaries aus dem Worktree-Release-Build hineinkopieren (`cp -L`)
     nach `rust/target/release/`, dazu die drei Frontend-Dists
     (`bot/analytics/dashboard_v2/dist`, `bot/admin_dashboard/dist`,
     `website/dist`) vom laufenden Release
     `/opt/deadlock/twitch/current` uebernehmen (Frontends unveraendert).
   - `sudo chown -R root:root` + `sudo chmod -R go-w` auf dem Build-Verzeichnis.
   - `sudo /usr/local/sbin/install-twitch-release \
     /opt/deadlock/twitch/builds/<sha> <sha>`
   - `sudo systemctl restart deadlock-twitch-bot-rust deadlock-twitch-dashboard-rust`
   - Beweis: `/proc/<MainPID>/exe` zeigt auf `releases/<sha>`.
5. Live-Pruefung: nach dem naechsten Promo-Slot in `twitch_promo_pitch_log`
   keine neuen Zeilen mit `pfad = 'targeted_global'`; neue Sends nur noch
   `periodic`/`anlass`/`partner`/`gezielt`. Bot-Log pruefen, dass der
   Promo-Loop laeuft (Periodik-Sends erscheinen wie bisher).
