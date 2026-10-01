status: aktiv
Datum: 2026-10-01

# Review-Runde 1

Unabhängiger Intent-Review am rebasierten Stand `c8de263b85e99aeeea707cb80e13e91194bc19cb` meldete vier HIGH- und einen MEDIUM-Befund. Die Folgekorrekturen sind in WIP-Commits gesichert; unabhängige Nachprüfung der jüngsten Alias-Korrektur ohne Befund. Tests bleiben ausstehend.

1. `promos.rs`: Pitch-Claim wurde auch bei unklarem Send-Ausgang freigegeben. Folgekorrektur: nur eine eindeutige Twitch-4xx-Ablehnung gibt den Claim frei; Transportfehler und 5xx bleiben beansprucht, damit ein Retry keine mögliche Doppelnachricht erzeugt. Prozessabbruch zwischen Claim und Send kann weiterhin eine Ansprache auslassen; at-most-once hat Vorrang, da Twitch keinen transaktionalen Send/DB-Claim anbietet.
2. `promos.rs`: fehlende oder inaktive Lurker-Tax-Belohnung beendete auch den unabhängigen Pitch-Pfad. Folgekorrektur: Reward- und Paid-Plan-Gates begrenzen die Tax-Erinnerung, nicht den aktivierten Pitch im eigenen Kanal.
3. `promos.rs`: Wechsel vom Login-Fallback zur Twitch-ID konnte denselben Chatter erneut auswählbar machen. Folgekorrektur: ein bereits gepitchter Fallback-Login wird nur beim Übergang zu einer stabilen ID als Alias berücksichtigt; unterschiedliche bekannte IDs bleiben getrennte Identitäten.
4. `promos.rs`: Partner-State-Guard blockierte den Bot-eigenen Kanal. Folgekorrektur: eigener Broadcaster wird vor dem Partner-State-Guard zugelassen.
5. `lurker_tax_settings.rs`: POST ohne Pitch-Feld erhielt den gespeicherten Wert, gab aber `null` zurück. Folgekorrektur: bestehende `query!`-Schreibpfade bleiben erhalten; die bereits vorhandene `SELECT_SQL` liest den gespeicherten Wert für die Antwort zurück. Die Scope-Regression ergänzt das neue Feld im Test-Struct und Testschema.

## Nachprüfung der Folgekorrekturen

Frischer read-only Review bestätigte die Fixes für unklare Sends, unabhängige Pitch-Gates, eigenen Kanal und POST-Antwort. Er meldete zusätzlich:

1. Die neue Pitch-Regression verwendete ein `apply_ddl`-Fixture ohne Pitch-Migration. Korrigiert: die Regression wendet die additive Pitch-Migration im isolierten Test-Postgres an.
2. Login-Fallback- und ID-Claim konnten bei parallelem Identitätswechsel verschiedene Primärschlüssel beanspruchen. Korrigiert: `claim_lurker_pitch` serialisiert denselben Kanal/Login mit einem transaktionalen Advisory-Lock und prüft Key sowie Login vor dem Insert; Parallelregression ergänzt.
3. Runtime-Queries in den Dashboard-Schreibpfaden widersprachen `CONTRACT.md`. Korrigiert: die bestehenden `query!`-Schreibpfade sind wiederhergestellt; nur der vorhandene dynamische `SELECT_SQL`-Lesepfad wird erneut verwendet.

## Nachprüfung nach Folgefixes

Der frische read-only Review bestätigte die übrigen Korrekturen und meldete einen MEDIUM-Befund: ein Login-Alias konnte bei Wiedervergabe des Twitch-Logins eine andere bekannte ID fälschlich als bereits gepitcht einstufen. Korrigiert: Alias-Abgleich gilt nur noch von `login:` auf `id:`; unterschiedliche bekannte IDs bleiben getrennt. Regression für wiedervergebenen Login ergänzt. Die unabhängige Nachprüfung bestätigte die Korrektur ohne Befund.

Der erste gemeinsame Cargo-Lauf kompilierte `tb-chat`, scheiterte danach in der Dashboard-Testkompilierung: `streamer_scope.rs` initialisierte `LurkerTaxUpdate` ohne neues Feld. Testmacro und Testschema sind angepasst. Ein erneuter Lauf startete nicht, weil `/tmp/deadlock-cargo-release.lock` von einem anderen Category-Test gehalten wurde. Der unabhängige read-only Review bestätigte anschließend die Login-Alias-Korrektur ohne Befund; Cargo-Tests bleiben offen.

Gate-Runde 1: ausstehend. Cargo-Tests stehen aus; PR #1035 und PR #472 gehören zur aktiven Gruppe, daher kein Einzel-Gate oder -Merge. Kein Push oder Produktionsschritt erfolgt.
