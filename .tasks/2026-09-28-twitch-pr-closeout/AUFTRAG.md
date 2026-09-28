status: aktiv
Datum: 2026-09-28

# Twitch-PRs und Arbeitszweige abschließen

Ziel: Offene PRs und Arbeitszweige nach tatsächlichem Zustand prüfen, mergefähige Änderungen durch das lokale Gate auf `main` bringen, anschließend deployen und live prüfen. Unfertige Drafts und schmutzige Arbeitsbäume nicht blind übernehmen oder löschen.

Erstinventar: 17 offene Twitch-PRs, 84 Worktrees, 30 Worktrees mit Änderungen an getrackten Dateien. Der Haupt-Checkout enthält fremde ungetrackte Dateien und bleibt unangetastet.

Zwölf Dependabot-PRs #1000 bis #1011 wurden im isolierten Zweig `chore/twitch-pr-closeout-20260928` zusammengeführt und dort gebaut. Der geschützte Push auf `main` wird vom unveränderten Scheduler-Review-Gate für Head `d3fb1c665196` gegen Basis `992e26596104` in der Review-Queue gehalten. Kein Merge und kein Deploy behaupten, bevor das Gate selbst `release` meldet.

Produkt-PRs #995 bis #999 erfordern gemeinsame Prüfung: #995 braucht #997 und eine Shell-Route; #996 braucht das Ledger von #997; #997 verwendet Daten aus #998 und #999. #999 hängt zusätzlich an Deadlock-Bots #466; #995 und #998 brauchen Caddy #3 beziehungsweise #7. Draft-Status und offene Befunde sind in `REVIEW.md` dokumentiert.
