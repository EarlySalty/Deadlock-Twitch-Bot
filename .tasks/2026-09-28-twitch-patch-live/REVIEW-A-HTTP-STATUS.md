status: aktiv (2026-09-29)

# Unabhängige Nachprüfung A: HTTP-204-Status

Lies `WORKER-A-HTTP-STATUS.md`, den B5-Befund in `REVIEW.md` und den sauber gepushten A-Worktree `/home/nathanael/.worktrees/twitch-patch-transport-20260928`, Commit `b94eab52` gegen `cc22c3d3`. Der Fixer war ein anderes Modell. Nur lesend prüfen, keine Edits, Unter-Threads, Unter-Agenten, echten Twitch-Sends oder Main-Aktion. Intent-Thread `4ddc68d5-0c42-41ce-b02c-c1be909c20fd`.

Prüfe im tatsächlichen Source-only-Pfad, dass ein HTTP 204 mit genau einem POST als bekannte numerische Statusinformation und ungewissem Ergebnis an den `tb-internal-api`-Empfänger gelangen kann. Der neue `SendOutcome::HttpError { status: 204, body: "" }` darf weder als erfolgreiche Sendung noch als definitiv nicht gesendet gelten und keinen Retry oder User-Token-Fallback auslösen. Das übliche 200/is_sent, 4xx/5xx, Verbindungsabbruch und der vorhandene finale lokale Mute-Guard müssen unverändert bleiben. Der B-Empfänger bei Commit `45edaeef` speichert den Status, meldet derzeit für 2xx aber noch `http_error` statt `unexpected_success_status`. Dieses B-Problem separat als gezielte B-Fixpflicht benennen, nicht A wegen eines fremden Dateipfads blockieren.

Fixer meldet 8 gezielte Tests bestanden, `cargo check` und Clippy bestanden, Formatcheck wegen vorhandener Abweichungen rot. Testbedingungen und Diff prüfen; Urteil `A-Statusfix fertig J/N, Fix nötig J/N`, Befunde mit Pfad:Zeile und Gegenfall. Eine gesamte A/B-Freigabe kommt erst nach dem B-seitigen Reason-Fix und gemeinsamem unabhängigen Review.
