# Zuständigkeit

Twitch-Community: Thread `3a7b1c91-7837-4fa0-a927-f275f30d9368` trägt ausschließlich Twitch #1035 und koordiniert die Communitygruppe. Discord-Community: Thread `c0b1d111-e402-4ed3-bb0b-68958a1ba699` trägt Bots #472 in seinem eigenen Worktree. C9 wird vor der Communitygruppe integriert. Die gemeinsame Auslieferung beginnt erst nach erneuter Abnahme beider aktualisierter Stände.

Der Hauptthread bestätigte die selektive Übernahme der Herkunftsarbeit. Übertragen wurden der Community-Diff von `14bc1f47` nach `d1a66662` und genau die drei freigegebenen uncommitteten Quell- und Testdateien. Der Konflikt in der FAQ wurde durch Erhalt der heutigen Hilfe und Ergänzung des neuen Befehls gelöst. Im Herkunftsworktree wurden keine Dateien verändert.

Prüfungen verwenden den gepinnten Rustup-Shim aus `/home/nathanael/.cargo/bin/` und maximal zwei Compilerjobs. `host-checks.lock` und die Hostsperre werden über den gesamten Rust-Prüflauf gehalten. Produktivzugriffe, Release, Merge und Bereinigung sind noch offen.
