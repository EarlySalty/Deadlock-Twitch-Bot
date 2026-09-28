status: aktiv (2026-09-28)

# Twitch-Patch-Pakete

Basis `origin/main` am 28.09.: `992e265961048ee72d03a483673c92ef3c49e715`. Vor eigenem Beginn aktuellen Remote-SHA prüfen. Der alte Entwurf `/home/nathanael/.worktrees/twitch-patch-hype-20260920/` gehört keinem Paket und bleibt unverändert. Ein Worker je Worktree, keine Unter-Threads, keine Eigen-Merges nach main, keine neuen Code-Kommentare.

| Paket | Datei-Zuständigkeit | Vorbedingung | Ergebnis | Status |
|---|---|---|---|---|
| A | `rust/crates/tb-chat/src/{api,channel_policy,moderation,timeout_tracking}.rs`, `rust/crates/tb-transport-twitch/src/chat.rs` | keine | Source-only-Sendepfad mit bestehenden Channel- und Spam-/Timeout-Gates; vorhandene Chat-Tests grün | startklar |
| B | `rust/crates/tb-internal-api/src/{lib.rs,handlers/mod.rs,handlers/patch_announcement.rs,handlers/patch_announcement_tests.rs}`, `rust/migrations/20260920143000_patch_announcements.sql` oder neuer freier Versionspfad | A committed, als Basis übernehmen | API/Receiver mit Website-Link-Vertrag, aktiven Partnern, at-most-once-Receipts, Cursor-DDL und DB-Rechten | wartet auf A |
| C | `rust/bin/tb-bot/src/patch_feed.rs` sowie allein für `mod patch_feed;` `rust/bin/tb-bot/src/main.rs`; kein Startup-Wiring | keine | index.json/meta.json sicher lesen, neue IDs mit persistentem Cursor erkennen, Bootstrap für historische IDs, fehlertolerant und ohne doppelte Ausgabe. Integrator verbindet Callback später | startklar |
| D | `rust/bin/tb-bot/src/main.rs`, `rust/bin/tb-bot/src/chat_wiring.rs`, Integrations-/Deploy-Aufzeichnungen unter `.tasks/2026-09-28-twitch-patch-live/` | A, B, C | Alle Branches auf aktuelles origin/main integrieren; API/Callback verdrahten, gemeinsame Compilation/Tests/Review/Gate und Release | wartet |

`rust/bin/tb-bot/src/main.rs` gehört C ausschließlich für die Moduldeklaration, D danach für das Wiring. Keine gleichzeitig schreibenden Worker an derselben Datei. A darf den bisherigen `send_source_only_message`-Entwurf nur bei nachgewiesener Twitch-API-Semantik übernehmen; Shared-Chat-Fanout und falsches Channel-Mapping sind ausgeschlossen. B behält den At-most-once-Grundsatz bei ungewissem HTTP-Ergebnis. C akzeptiert ausschließlich die feste Website-Domain und den exakten Patchpfad, folgt keinen externen Weiterleitungen und verwendet weder Discord noch Python-Pfade. Der Integrator ist allein für `Cargo.lock`, Konfliktlösung, Migration und Live-Schaltung zuständig.

Abgleich zum Parallelprojekt: Rust-Patchnotes-Port `17337791-d5fe-49c8-ad55-2e31a2e016d2` schreibt nicht im Twitch-Repo. D muss vor Merge prüfen, dass neue Artikel auch im Rust-Port weiter atomar vor `index.json` veröffentlicht werden. Wenn sich der Vertrag ändert, beide Integratoren vor Live-Schaltung zusammenbringen; keine unbemerkte Fallback-Ankündigung.
