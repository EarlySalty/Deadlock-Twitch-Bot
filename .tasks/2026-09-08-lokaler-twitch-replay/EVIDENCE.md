status: aktiv
datum: 2026-09-08

# Fundstellen

- rust/crates/tb-llm/src/hub.rs:289: complete_detailed wählt den Anbieter und prüft die produktive Freigabe.
- rust/crates/tb-llm/src/hub.rs:553: openai_compatible_body trennt Systemnachricht und Nutzerdaten.
- rust/crates/tb-llm/src/hub.rs:583: send_openai_compatible ist der bestehende zentrale HTTP-Transport.
- rust/crates/tb-llm/src/selection.rs:17: LlmEndpoint trägt Anbieter, Modell, Basis-URL und optionalen Schlüssel.
- rust/crates/tb-llm/Cargo.toml:1: eigene kleine Crate mit vorhandenen reqwest-, tokio- und serde_json-Abhängigkeiten.
- b8ae776f:rust/bin/tb-smalltalk-bench/src/main.rs:29: historischer Runner bindet Engagement, SQL-Bench und Judge ein.
- /tmp/twitch-onboarding-codebefund-2026-09-08.md: Messfehler durch Zukunftsaudio und fehlende saubere Stil-/Testtrennung.
