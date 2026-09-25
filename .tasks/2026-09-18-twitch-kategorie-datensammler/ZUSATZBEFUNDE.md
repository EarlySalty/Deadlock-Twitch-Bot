# Zusatzbefunde aus unabhängiger Prüfung

## Kanal-Schreibschutz: konkreter Teil des Nutzerauftrags
Die Aussage „Hauptbot schreibt nur in freigegebene Kanäle“ ist noch NICHT allgemein belegbar.
- rust/crates/tb-chat/src/channel_policy.rs: PolicyContext::Standard prüft PartnerRoster. PolicyContext::Raid erlaubt SendMessage/SendWhisper ausschließlich anhand der Aktionsart, ohne Zielprüfung.
- rust/bin/tb-bot/src/main.rs:908 gibt diesen ungebundenen Raid-Kontext an RaidGreetingMonitor.
- rust/bin/tb-bot/src/raid_greeting.rs:479 send_source_hint schreibt an from_broadcaster_id, send_reminder:639 whisper an Alias-Hauptaccount oder Quelle.
- rust/bin/tb-bot/src/chat_wiring.rs:2743 DbPartnerRoster prüft is_partner_active und blockiert bei DB-Fehlern. Andere Standardports h.api() sind bereits so eingezäunt.
- rust/crates/tb-chat/tests/channel_policy.rs testet aktuell ausdrücklich das Senden an non_partner im Raid-Kontext. Nicht als erfüllter Sicherheitsnachweis verkaufen!

Ergänzung zu Paket A: sichere den Raid-Pfad gezielt gegen unfreigegebene Ziele, nach Prüfung der bestehenden Raid-Registrierungs-/Aliaslogik. Kein pauschaler ungebundener Kontext darf die Partnersperre umgehen. Bevorzugt derselbe zentrale Roster-/Berechtigungscheck vor dem tatsächlichen Schreiben, auch bei Widerruf zwischen Raid und Timer. Unbekannter oder fehlerhafter Berechtigungsstand muss blockieren. Legitime, explizit verknüpfte Alias-Empfänger nur mit nachvollziehbarer Berechtigung erlauben, kein Wildcard-Bypass. Tests für fremdes Ziel und widerrufenen Partner ohne echten Twitch-Send. Keine Änderung fremder Produktfunktionen außerhalb dieses Sicherheitschecks.
WICHTIG: Der separat autorisierte Engagement-/Smalltalk-Account ist ein bewusst anderer Account mit bereits beauftragtem One-Shot in kleinen Nichtpartner-Kanälen. Nicht versehentlich dessen Funktion abschalten oder mit dem Hauptbot/Collector gleichsetzen. In REPORT.md diese Identitäten ausdrücklich unterscheiden.

## Tatsächliche Produktion, read-only bestätigt
- Hauptbot und Dashboard sind SYSTEMD-SYSTEMDIENSTE, keine laufenden User-Units: deadlock-twitch-bot-rust.service als twitchbot, deadlock-twitch-dashboard-rust.service als twitchdash.
- WorkingDirectory /opt/deadlock/twitch/current, aktuell Symlink auf /opt/deadlock/twitch/releases/44cc2aa90e1d1b59269533b5d50b643e56bfbba8. Kanon-Checkout ist NICHT der Livebaum.
- Neue Collector-Unit soll weiterhin getrennt und separat betrieben werden. Keine bestehenden Unit-Rechte/Secret-Leserechte eigenmächtig lockern. Bootstrap/runbook muss zur tatsächlichen Rollen-Trennung passen. Bestehende vendor/uplink-infisical-transport-Implementierung für credential-basierten Zugriff ohne ENV prüfen.
- postgres-Zugriff für ausschließlich lesende Prüfung funktioniert: sudo -n -u postgres psql -X -d twitch_analytics.
- Es existieren noch keine category_*-Tabellen, nur u.a. twitch_stats_category und twitch_live_state.
- df: / und PG auf 1,7 TB Partition, 680 GB belegt, aktuell 907 GB frei. Die Retention bleibt Pflicht, ein niedriger zweistelliger Verbrauch ist NICHT gemessen.
- Legacy run_tb_bot_service.sh lädt ENV-Konfiguration und viele fremde Geheimnisse; NICHT einfach für den neuen Collector kopieren (Nutzer fordert keine ENV-Konfiguration).

## Extern geprüft (offizielle Twitch-Dokumentation)
https://dev.twitch.tv/docs/api/reference/#get-channel-followers : User-Token ohne Moderatorrolle/Scope liefert total, nicht die einzelnen Follower.
https://dev.twitch.tv/docs/chat/ : Join-/Auth-Limits pro User, nicht pauschal je TCP-Verbindung. Anonyme Sonderregeln sind nicht als garantierter unbegrenzter Zugang dokumentiert; konservativer gemeinsamer Limiter, kein Ban-Risiko-versprechen.
https://dev.twitch.tv/docs/api/guide/ : Ratelimit-Limit/Remaining/Reset verwenden, 800 ist kein im Collector hart garantiertes Budget.
https://dev.twitch.tv/docs/chat/irc/ : Shared Chat source-id/source-room-id, RECONNECT, CLEARMSG/CLEARCHAT beachten.
