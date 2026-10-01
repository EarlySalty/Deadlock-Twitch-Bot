# Register

Paket: Social-Media-IDs und gemeinsame Kopfzeile.
Intent: 37ae96e4-49e9-46b7-a77f-e148784352c1.
Branch: fix/social-media-ids-kopfzeile.
Worktree: /home/nathanael/.worktrees/tb-sm-ids-kopfzeile.
Basis: origin/main 14bc1f479e32394fe2977f8c8ef85e6c5bff66e1.
Status: Source-Prüfung und Gate-Vorbereitung; Main-Push und sämtliche Produktivmutationen auf expliziten Root-Handoff gesetzt.

Deploy-Koordination: Der nächste Twitch-Current-/Release-/Restart-Slot gehört Challenges 4b7edf8f-bac5-4feb-a763-b4c6550345a2 nach unabhängiger Abnahme und regulärem ALLOW. Keine eigene Deploymutation bis zur abgestimmten Übergabe. Root und Challenges sind informiert. Vorhandenes bot.toml und BrainShadow-SHA301b802b668b2c12d4ca9895677b46db1d9626a620dd2c77dc961946f9d4b7e0 bleiben erhalten.

Verbindliche Reihenfolge vom 02.10.: Challenges exakt 63e45b937e0bbff27d7034208b87678254fdcf8f vollständig abnehmen, regulär mergen/pushen und Cutover live prüfen. Erst nach ausdrücklichem Root-Handoff Social auf neuem Main integrieren, regulär reviewen und pushen. Keine Ref-Overrides, keine Social-Mitintegration in Challenges und keine Prod-DDL für Migration 20261001090000 vor/unter Challenges63.
