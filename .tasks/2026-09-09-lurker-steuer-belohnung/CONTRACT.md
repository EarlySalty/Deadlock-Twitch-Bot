# Contract: Lurker-Steuer mit Kanalpunkte-Belohnung

Klasse: mittel. Repo: Deadlock-Twitch-Bot (Rust). Modell: Opus 4.8.

## Ziel

Die Lurker-Steuer bekommt eine Standard-Belohnung als Vorlage. Der Streamer legt in Twitch eine Kanalpunkte-Belohnung mit dem Standardnamen an, der Bot spricht anwesende Stamm-Lurker charmant an und bittet sie, genau diese Belohnung einzulösen. Wer eingelöst hat, bekommt eine kurze Dankes-Antwort und wird in dieser Session nicht mehr erinnert.

## Anforderungen

- REQ-1 Standardname der Belohnung ist "Lurker Steuer". Erkennung im Bot ist unempfindlich gegen Groß- und Kleinschreibung und akzeptiert auch "Lurker Steuern" (Präfixvergleich auf normalisiertem Titel). Kosten legt der Streamer frei fest, der Bot behauptet keine Kosten im Chat.
- REQ-2 Erinnerungstext im Chat nach diesem Muster, echte Umlaute, keine Gedankenstriche: "Hey @xy, schön dass du da bist! Vergiss nicht, deine Lurker Steuer zu zahlen: Belohnung 'Lurker Steuer' einlösen." Bei zwei Namen beide Erwähnungen in einem Satz. Bestehende Kandidatenlogik, Limits und Cooldown bleiben (promos.rs, 2 Erwähnungen, 60 Minuten Promo-Slot, ein Viewer je Session).
- REQ-3 Löst ein Zuschauer die Belohnung ein (EventSub `channel.channel_points_custom_reward_redemption.add`, Titel passt zu REQ-1), antwortet der Bot einmal im Chat, z. B. "@xy hat die Lurker Steuer bezahlt. Vorbildlich, danke!" Höchstens eine Dankes-Antwort je Zuschauer und Session.
- REQ-4 Zuschauer, die in der laufenden Session eingelöst haben, werden in dieser Session nicht mehr erinnert (Quelle `twitch_channel_points_events` je Session).
- REQ-5 Die Erinnerung wird nur gesendet, wenn die Belohnung im Kanal existiert und aktiv ist (Helix GET custom_rewards mit dem bestehenden Streamer-Token, Scope `channel:read:redemptions` ist im Basisprofil). Fehlt sie, kein Chat-Text.
- REQ-6 Dashboard-Karte "Lurker-Steuer": zeigt die Vorlage (Name "Lurker Steuer", Vorschlag 10 Punkte, Beschreibungstext) mit Kurzanleitung, wo man sie in Twitch anlegt, und einen Status "Belohnung gefunden" oder "Belohnung fehlt".
- REQ-7 Der Warnhinweis "Chatter-Leseberechtigung fehlt" wird korrigiert: die Laufzeit akzeptiert den Scope `moderator:read:chatters` auch aus dem zentralen Bot-Token (promos.rs `has_chatters_scope`), die Dashboard-Prüfung schaut nur auf den Streamer-Eintrag und kein Streamer-OAuth-Profil fragt diesen Scope überhaupt ab. Die Karte prüft künftig dieselbe Bedingung wie die Laufzeit und sagt bei echtem Fehlen, was der Betreiber tun muss, statt dem Streamer ein wirkungsloses Neu-Verbinden zu empfehlen.
- REQ-8 Nutzersichtbare Texte in Nutzersprache mit echten Umlauten, kein internes Vokabular, keine Gedankenstriche.

## Invarianten

- INV-1 Keine neuen Secrets, keine ENV-Variablen, keine neuen OAuth-Wege; bestehende Token und Scope-Profile wiederverwenden.
- INV-2 Kein LLM-Aufruf für diese Texte.
- INV-3 Kandidatenlogik, Plan-Gate (bezahlter Plan) und `!lurkersteuer_off` unverändert.
- INV-4 Kein Wiederholungs-Spam: Dank und Erinnerung je Zuschauer höchstens einmal pro Session.
- INV-5 Keine Code-Kommentare.

## Nicht-Ziele

- Der Bot legt die Belohnung nicht selbst über Helix an.
- Kein Punktestand, keine Kostenangabe im Chat.
- Keine Änderung an Anlass-Pitch, Partner-Pitch oder Promo-Engine jenseits des Lurker-Steuer-Pfads.

## Erlaubter Bereich

- rust/crates/tb-chat/src/promos.rs
- rust/crates/tb-chat/src/lib.rs
- rust/crates/tb-monitoring/src/dispatch.rs
- rust/crates/tb-monitoring/src/telemetry.rs
- rust/crates/tb-monitoring/src/lib.rs
- rust/bin/tb-bot/src/chat_wiring.rs
- rust/bin/tb-bot/src/chatters_wiring.rs
- rust/bin/tb-bot/src/main.rs
- rust/crates/tb-transport-twitch/src/
- rust/crates/tb-dashboard-api/src/handlers/lurker_tax_settings.rs
- rust/crates/tb-dashboard-api/src/lib.rs
- bot/dashboard_v2/
- rust/.sqlx/
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt
- rust/migrations/
- docs/LURKER_TAX.md
- .tasks/2026-09-09-lurker-steuer-belohnung/
