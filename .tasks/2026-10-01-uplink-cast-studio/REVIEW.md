status: aktiv | Datum: 2026-10-01

# Review Runde 1

Gate: `gpt-6.1-sol` BLOCK.

1. BLOCKING, `bot/dashboard_v2/src/pages/UplinkCastStudio.tsx:420`. Fehler von `rotateSource`, `removeSource`, `SceneList.createScene`, `SceneList.removeScene`, `selectPreview` und `takeProgram` werden nicht angezeigt. Ein abgewiesenes TAKE lässt den vorigen Program-Stream ohne Erklärung aktiv. Änderung an TypeScript ist durch die Rust-only-Arbeitsregel ausgeschlossen.
2. BLOCKING, `bot/dashboard_v2/src/pages/UplinkCastStudio.tsx:152`. Der Decoderfehler schließt WebCodecs, doch die Vorschau startet Decoder oder Socket danach nicht neu. Änderung an TypeScript ist durch die Rust-only-Arbeitsregel ausgeschlossen.
3. NIT, `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:650`. Das Proxy-Schema wird immer auf `ws` gesetzt. Die erlaubten Relay-Schemata von `uplink_config::runtime` prüfen. Rust-Fix im eigenen Scope.
4. NIT, `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:486`. Der `/v1/me/cast*`-Relayvertrag ist nicht durch die vorgesehene Relay-Revision belegt. Die aktuelle Prüfung fand diese Endpunkte nicht in `rs-relay`-Main; die Laufzeitrevision ließ sich nicht auflösen. `rs-relay` bleibt außerhalb dieses Branchauftrags und wird nicht verändert.

## Runde 1 Status

- Gefixt vor diesem Review: inkompatible Axum-Pfadcaptures zu `{source_id}` und `{scene_id}` geändert; den gemeinsamen optionalen WebSocket-Extractor verwendet; Browser-Binaryframes als `Bytes` weitergereicht.
- Verifikation: `cargo test --manifest-path /home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-feat-uplink-cast-studio-20260912-1269ad60/rust/Cargo.toml -p tb-dashboard-api --lib --locked --offline --jobs 1 spa_shells_sind_ohne_session_gegated_pricing_bleibt_offen`, 1 passed, 0 failed, 0 ignored, 1313 gefiltert.
- Offen: TS-Änderungen sind durch Rust-only-Regel gesperrt; Live-Relayvertrag ist unbestätigt. Rust-Scheme-Validierung kann im eigenen Scope korrigiert werden.
