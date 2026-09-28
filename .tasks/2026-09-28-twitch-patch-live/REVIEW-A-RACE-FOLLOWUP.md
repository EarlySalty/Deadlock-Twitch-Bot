status: aktiv (2026-09-29)

# Unabhängige Nachprüfung A: lokaler Guard-Race

Lies `AUFTRAG.md`, `WORKER-A-RACE-FIX.md`, die A-Befunde in `REVIEW.md` und den gepushten, sauberen A-Worktree `/home/nathanael/.worktrees/twitch-patch-transport-20260928` bei Commit `cc22c3d3` gegen `8a01fb17`. Der Fixer war ein anderes Modell. Prüfe ausschließlich lesend, keine Änderungen, Unter-Threads, Unter-Agenten, Commits, echten Twitch-Sends oder Main-Aktion. Intent-Thread `4ddc68d5-0c42-41ce-b02c-c1be909c20fd`.

Prüfe den tatsächlichen Decorator-Pfad `ChannelPolicyChatApi<TimeoutTrackingChatApi<Moderated...>>` und jeden Eintritt in `send_source_only_message` sowie `send_source_only_message_guarded`: Weder der neue Callback noch Defaultimplementierungen dürfen lokale Stummschaltung, Kanalprüfung oder bisherige Fehlerbereinigung umgehen. Die überprüfte Broadcaster-ID muss die Sperre unmittelbar nach dem optionalen App-Token-Refresh und direkt vor `.send().await` erneut prüfen. Reproduziere wenn möglich den parallelen zweiten Timeout während eines angehaltenen Refresh mit null Chat-POSTs und einen zulässigen Kanal mit genau einem POST. Prüfe frühe Unknown-Identity- und Mute-Fälle sowie A2 (ungewisser POST ohne Retry) und A3 (bereinigte Details) auf Regression. Trenne den konkreten lokalen Guard-Race vom unvermeidbaren externen Kanalstatus-TOCTOU.

Der Fixer meldete 114 bestandene Transporttests, zwei gezielte `tb-chat`-Tests, `cargo check` erfolgreich und Clippy mit Warnungen. Eine volle `tb-chat`-Suite ohne Test-DB ergab 867 bestanden/39 fehlgeschlagen; eine zuvor mit isoliertem Postgres vermessene Feature- und Basis-Suite hatte jeweils dieselben 36 Fehler. Die Zahlen sind nicht vergleichbar, bevor die DB-Umgebung stimmt. Wenn du eine Vollsuite bewertest, gleiche Test-DB und Flags auf beiden Ständen ab, gib passed/failed/ignored wörtlich an. Bestehende Tests nicht überspringen oder abschwächen. Format- und Clippy-Grenzen konkret benennen, nicht als sauberes Grün verkaufen.

Urteil `fertig J/N, Fix nötig J/N` für den neuen Race-Fix und getrennt für das gesamte A-Paket. Befunde mit Pfad:Zeile und reproduzierbarem Szenario. Keine D-Freigabe ohne verifizierten Guard-Pfad.
