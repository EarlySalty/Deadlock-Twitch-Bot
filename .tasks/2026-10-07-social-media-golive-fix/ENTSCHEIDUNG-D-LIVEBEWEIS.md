[Orchestrator] Entscheidung zum Live-Nachweis Auftrag D:
1. Kein sudo-readlink. Nutze den lesenden Weg ohne Root: `systemctl show <unit> -p MainPID -p ExecMainStartTimestamp -p ExecStart` für deadlock-twitch-bot-rust und deadlock-twitch-dashboard-rust, dazu das Ziel des Release-Symlinks, auf den ExecStart zeigt (normales readlink ohne sudo), und den Vergleich Startzeit nach Deploy-Zeit sowie den Commit-SHA im Release-Pfad bzw. Binary (`strings` oder Versionsausgabe). Das zusammen gilt als Prozessnachweis.
2. Den direkten /proc/<PID>/exe-Abgleich mache ich als Orchestrator nach deinem Deploy selbst und trage ihn nach; du wartest darauf nicht.
3. Deploy, Neustart, Funktionsbeweis und Aufräumen wie im Auftrag fortsetzen.
