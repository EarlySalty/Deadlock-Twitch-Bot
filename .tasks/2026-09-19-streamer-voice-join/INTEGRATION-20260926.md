# Integration: Mitspielen im Partnerchat

Stand: 26. September 2026. Dieser Branch startet direkt von `origin/main` und
übernimmt nur den Twitch-Eigenanteil aus `feat/streamer-voice-join` sowie die
eindeutigen Invite- und Mitspieler-Textvarianten aus
`feat/werbetexte-neu-raid-dank`. Alte Main-Merges und der Raid-Dank-/Promo-/SQL-
Umbau sind nicht Teil dieser Integration.

## Geltungsbereich

Der ursprüngliche Auftrag verlangt eine vorhandene **Partner-/Discord-
Verknüpfung**. Der Twitch-Chat-Responder läuft deshalb weiter nur in aktiven
Partnerkanälen mit einem frischen Deadlock-Live-Nachweis und bestehender
`channel:bot`-Chat-Subscription. Es werden weder Nichtpartner-Kanäle zusätzlich
abonniert noch deren Senderechte erweitert. Die Discord-Voice-Berechtigung aus
PR #968 gilt dagegen für verknüpfte Streamer-Rolleninhaber bei jedem Spiel;
dieser andere Vertrag erweitert den Twitch-Chat-Auftrag nicht.

Der Broadcaster wird ausschließlich über `broadcaster_user_id` gegen
`twitch_partners`, `twitch_streamers_partner_state`,
`twitch_streamer_identities` und `twitch_live_state` geprüft. Eine Discord-ID,
die mehreren Twitch-IDs zugeordnet ist, eine inaktive Partnerschaft, ein
veralteter Live-Wert oder eine andere Kategorie gibt keine Broker-Aktion frei.
Der bestehende LFG-Judge entscheidet über die konkrete Mitspiel-Anfrage; die
Antwort steht vor Spielzugangs-FAQ und Werbe-Pitch.

## Cross-Repo-Vertrag und Freigabegrenze

Der Twitch-Client sendet einen authentifizierten, nach Chat-Nachricht
idempotenten Request an
`POST /internal/master/v1/discord/streamer-voice-invite` mit Community-Guild-ID
und verknüpfter Discord-User-ID. Er sendet nur bei `available=true`, gültigem
`https://discord.gg/`-Link und der Angabe `slot_added=true` einen Platz-Hinweis.
Der Producer liegt derzeit nur auf dem **ungeöffneten** Deadlock-Bots-Branch
`feat/streamer-voice-join` (`40bd611b`). Dort prüfen Broker und Adapter
Authentifizierung, Guild-/Kanal-Allowlist, aktuelle Präsenz, öffentliche
Join-Rechte, Kanalzustand und Kapazität. Der Producer erstellt momentan einen
zehnminütigen Invite mit `max_uses: 0`, also beliebig vielen Nutzungen. Da der
Link öffentlich im Twitch-Chat steht, braucht dieser Punkt eine unabhängige
Entscheidung gegen den Auftrag „genau einen freien Platz“; ein Kanallimit allein
begrenzt die spätere Wiederverwendung nach Abgängen nicht. Dieser Twitch-PR darf **nicht** vor
der separaten unabhängigen Abnahme und Integration des Discord-Producers
gemergt oder deployed werden. Discord-Dateien wurden hier nicht verändert.

Der Streamer-eigene, rollenbeschränkte Voice-Kanal bleibt bewusst außen vor:
der ursprüngliche Contract erlaubt nur öffentliche Community-Spielkanäle und
keine Rechteausweitung. Eine Einladung ersetzt weder Discord-Anmeldung noch
den tatsächlichen Beitritt.

## Prüfstand

- `cargo test -p tb-chat -p tb-transport-discord --lib voice --jobs 2`: 8 Voice-
  Chat- und 2 Broker-Client-Tests bestanden.
- `cargo test -p tb-bot --bin tb-bot voice_identity_braucht --jobs 2`: isolierter
  PostgreSQL-Test bestanden; prüft Twitch-ID, eindeutige Discord-Verknüpfung,
  aktive Partnerschaft und frischen Deadlock-Live-Nachweis.
- `cargo test -p tb-chat --lib invite_question::tests --jobs 2`: 35 Tests bestanden.
- `cargo test -p tb-chat --lib lfg_pitch::tests --jobs 2`: 25 Tests bestanden.

Das Selbstreview-Gate, der produktive Compile-Check und ein unabhängiges
Rust-/Security-Review prüfen den fertigen Branch. Bis diese Nachweise und der
Discord-Producer vorliegen, bleibt der PR Draft/BLOCK.
