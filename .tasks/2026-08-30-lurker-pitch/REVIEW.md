status: aktiv
Datum: 2026-10-01

# Review-Runde 1

Unabhängiger Intent-Review am rebasierten Stand `c8de263b85e99aeeea707cb80e13e91194bc19cb` meldete vier HIGH- und einen MEDIUM-Befund. Die Folgekorrekturen sind uncommittet und benötigen erneute unabhängige Prüfung sowie Tests.

1. `promos.rs`: Pitch-Claim wurde auch bei unklarem Send-Ausgang freigegeben. Folgekorrektur: nur eine eindeutige Twitch-4xx-Ablehnung gibt den Claim frei; Transportfehler und 5xx bleiben beansprucht, damit ein Retry keine mögliche Doppelnachricht erzeugt. Prozessabbruch zwischen Claim und Send kann weiterhin eine Ansprache auslassen; at-most-once hat Vorrang, da Twitch keinen transaktionalen Send/DB-Claim anbietet.
2. `promos.rs`: fehlende oder inaktive Lurker-Tax-Belohnung beendete auch den unabhängigen Pitch-Pfad. Folgekorrektur: Reward- und Paid-Plan-Gates begrenzen die Tax-Erinnerung, nicht den aktivierten Pitch im eigenen Kanal.
3. `promos.rs`: Wechsel vom Login-Fallback zur Twitch-ID konnte denselben Chatter erneut auswählbar machen. Folgekorrektur: bereits gepitchte Login-Aliase werden neben Identitätsschlüsseln berücksichtigt; Regression ergänzt.
4. `promos.rs`: Partner-State-Guard blockierte den Bot-eigenen Kanal. Folgekorrektur: eigener Broadcaster wird vor dem Partner-State-Guard zugelassen.
5. `lurker_tax_settings.rs`: POST ohne Pitch-Feld erhielt den gespeicherten Wert, gab aber `null` zurück. Folgekorrektur: bestehende `query!`-Schreibpfade bleiben erhalten; die bereits vorhandene `SELECT_SQL` liest den gespeicherten Wert für die Antwort zurück. Die Scope-Regression ergänzt das neue Feld im Test-Struct und Testschema.

## Nachprüfung der Folgekorrekturen

Frischer read-only Review bestätigte die Fixes für unklare Sends, unabhängige Pitch-Gates, eigenen Kanal und POST-Antwort. Er meldete zusätzlich:

1. Die neue Pitch-Regression verwendete ein `apply_ddl`-Fixture ohne Pitch-Migration. Korrigiert: die Regression wendet die additive Pitch-Migration im isolierten Test-Postgres an.
2. Login-Fallback- und ID-Claim konnten bei parallelem Identitätswechsel verschiedene Primärschlüssel beanspruchen. Korrigiert: `claim_lurker_pitch` serialisiert denselben Kanal/Login mit einem transaktionalen Advisory-Lock und prüft Key sowie Login vor dem Insert; Parallelregression ergänzt.
3. Runtime-Queries in den Dashboard-Schreibpfaden widersprachen `CONTRACT.md`. Korrigiert: die bestehenden `query!`-Schreibpfade sind wiederhergestellt; nur der vorhandene dynamische `SELECT_SQL`-Lesepfad wird erneut verwendet.

Der erste gemeinsame Cargo-Lauf kompilierte `tb-chat`, scheiterte danach in der Dashboard-Testkompilierung: `streamer_scope.rs` initialisierte `LurkerTaxUpdate` ohne neues Feld. Testmacro und Testschema sind angepasst. Erneuter vollständiger Testlauf und frischer Review stehen aus.

Gate-Runde 1: ausstehend. Erneute unabhängige Intent-Prüfung steht nach Tests und Integration in die aktive Gruppe Twitch PR #1035 / Deadlock-Bots PR #472 aus. Kein Merge, Push oder Produktionsschritt erfolgt.
