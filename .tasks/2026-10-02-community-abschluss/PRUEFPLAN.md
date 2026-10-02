# Gezielte Prüfung der Producer-Fixrunde

Die erste Suite auf `c759cfcb` hatte 20 echte erfolgreiche Tests. Die folgende Fixrunde ist bis zur tatsächlichen Slotübergabe ungetestet. Keine neue Cargo-Sperre wird ohne Übergabe erworben. Die bestätigte Reihenfolge lautet Discord, Docs, Bots-C9, danach ein neu bestätigter Twitch-Slot.

Werkzeuge: Rustup-Shim `/home/nathanael/.cargo/bin/cargo`, gepinnte stable-Toolchain aus `rust/rust-toolchain.toml`, `--locked --jobs 2`. Vor dem Lauf Hostlast sowie sämtliche fremden Cargo-/Rustc-Prozesse prüfen. `locks/host-checks.lock` und `/tmp/deadlock-cargo-release.lock` bleiben über den gesamten Prüflauf gehalten. Testdatenbank ausschließlich im eigenen begrenzten Wegwerfcontainer; `TB_TEST_REQUIRE_DB=1` verhindert still übersprungene DB-Prüfungen. Container regulär entfernen und beide Sperren nach Abschluss freigeben.

Geplante Befehle aus `rust/`:

```sh
/home/nathanael/.cargo/bin/cargo test --locked --jobs 2 -p tb-analytics --lib community_points -- --test-threads=1
/home/nathanael/.cargo/bin/cargo test --locked --jobs 2 -p tb-chat --lib clip_contest -- --test-threads=1
/home/nathanael/.cargo/bin/cargo test --locked --jobs 2 -p tb-internal-api --lib scout_community -- --test-threads=1
/home/nathanael/.cargo/bin/cargo test --locked --jobs 2 -p tb-internal-api --lib clip_contest -- --test-threads=1
/home/nathanael/.cargo/bin/cargo test --locked --jobs 2 -p tb-dashboard-api --lib social_media_clip_contest -- --test-threads=1
/home/nathanael/.cargo/bin/cargo check --locked --jobs 2 -p tb-bot -p tb-dashboard
```

Die Rollenprüfung erfolgt in `twitch_all_live_test` im selben eigenen Container nach vollständigem SQLx-Migrationslauf. Die eigene flüchtige Instanz benötigt auch eine leere Datenbank `twitch_analytics`, weil die reale Rollenmatrix datenbankspezifische Rolleneinstellungen genau für diesen Namen setzt. Ein `psql --file=ops/systemd/test_community_runtime_roles.sql` prüft die komplette Matrix zweimal. Schema `public`: die beiden Punktetabellen, `twitch_clip_contest_forwards`, `twitch_scout_community_suggestions`, Vorschlagssequenz `twitch_scout_community_suggestions_id_seq`. Als `twitchdash` sind SELECT-Zugriffe erfolgreich und echte INSERT-/UPDATE-/DELETE-Versuche auf allen vier Tabellen sowie nextval verweigert; als `twitchbot` gelingt die Einreichung. Bestehende legitime Clipverwaltungsrechte bleiben erhalten. Der Test rollt seine Änderungen zurück.

Die UI-Preview läuft ausschließlich lokal unter `http://127.0.0.1:4186/twitch/dashboard-v2/`; das Social-Studio desselben gebauten Bundles ist unter `/social-media-admin`. Aktuell sind weder in dieser Session noch in der Hauptsession Browser verfügbar. Sichtnachweis und Screenshots bleiben offen. Der erfolgreiche TypeScript-/Frontendbuild ersetzt diese Abnahme nicht. Zu prüfen sind Aktionsmenü für einen Twitch-Clip, manuelle Uploads, Einreichung, dauerhaftes Duplikat, fehlende Twitch-Session, falscher Clip-Eigentümer, Ablehnung, Rate-Limit, laufender Versuch, Helix-/Broker-/Speicherfehler sowie alle sichtbaren Rückmeldungen.
