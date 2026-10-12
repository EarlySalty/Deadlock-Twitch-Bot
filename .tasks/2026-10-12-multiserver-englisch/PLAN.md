# Plan: Twitch-Bot für englische Kanäle und fremde Discord-Server

Stand 2026-10-12. Ergebnis der Planungsunterhaltung mit dem Nutzer (Intent-Session `5f7811c4-86ac-4398-83e4-f929f2139a47`). Basis: `origin/main` 0085b717f.

## 1. Ziel

Der Twitch-Bot soll über die Deutsche Deadlock Community hinaus nutzbar werden:

- Englische Streamer und fremde Discord-Server (zunächst ein Testserver, Owner wird vom Nutzer angefragt) können den Bot nutzen.
- Alles, was Nutzer sehen, ist auf Englisch verfügbar: Dashboard, Twitch-Chat, Discord-Nachrichten, Website.
- Werbung bleibt im eigenen Netzwerk: Jeder Server bewirbt nur sich selbst.
- Raids laufen nach Sprache: Deutsche zu Deutschen, Englische zu Englischen.
- Die bestehende deutsche Community und ihr Betrieb bleiben unverändert.

Der Wert für fremde Server soll aus den Produkten für Streamer kommen. Fremde Server nutzen den Bot zunächst **kostenlos**.

## 2. Getroffene Entscheidungen

| Nr. | Entscheidung | Quelle |
|---|---|---|
| E1 | Alle heutigen Streamer und Partner gehören fest zum deutschen Community-Server. Für sie ändert sich nichts. | Nutzer |
| E2 | Neue internationale Seite unter `dead-lock.de`. Die heutige Weiterleitung von `dead-lock.de` auf `deutsche-deadlock-community.de` wird abgeschaltet. Hauptseite, deren Redirect-URLs und Bot-Verhalten bleiben gleich. | Nutzer |
| E3 | `dead-lock.de/streamer` ist die englische Fassung der heutigen Streamer-Seite, vollständig auf Englisch. Umsetzung als Übersetzung derselben Codebasis, kein kopierter Klon. | Nutzer, Empfehlung |
| E4 | **Ein neuer gemeinsamer Discord-Bot („Discord-Hub“)** für alle Server statt einer öffentlichen Partner-API mit Schlüsseln. Server fügen ihn per Einladungslink hinzu. Keine Admin-Rechte. | Nutzer |
| E5 | Der Hub ist der **einzige interne Discord-Integrationspunkt** des Twitch-Bots. Für den deutschen Server leitet er alles an den bestehenden Haupt-Discord-Bot (Broker in Deadlock-Bots) weiter und tut selbst nichts. Für fremde Server handelt er selbst. | Nutzer |
| E6 | Der Hub lebt als neues Binary im Repo Deadlock-Twitch-Bot. Deadlock-Bots bleibt unangetastet. | Nutzer |
| E7 | Serverzugehörigkeit beim Onboarding über Discord-Login (Scopes `identify`, `guilds`). Abgleich mit den Servern, auf denen der Hub installiert ist. Der Streamer wählt seine Community. | Nutzer bestätigt |
| E8 | Ein Streamer kann mehreren Servern angehören. **Partner ist, wer mindestens eine aktive Guild hat.** Keine aktive Guild mehr heißt kein Partner mehr. | Nutzer |
| E9 | Fremde Server bekommen ein eigenes **Server-Admin-Dashboard**. Sie dürfen ihre Streamer für ihre Guild steuern, aber nichts Globales. | Nutzer |
| E10 | Standard pro Server: neue Streamer werden automatisch Partner des Servers (abschaltbar). | Nutzer |
| E11 | Raids über Servergrenzen innerhalb derselben Sprache: pro Server im Admin-Dashboard einstellbar, **Standard ja**. | Nutzer |
| E12 | Fremde Server zunächst kostenlos. | Nutzer |
| E13 | Übersetzt wird nur, was Nutzer sehen. Logs, Tracing und interne Texte bleiben deutsch. | Nutzer |
| E14 | **KI-Funktionen im Chat für fremde Server aus.** Nur regelbasierte Prüfungen (Spamfilter, Link- und Einladungs-Erkennung, globale Bot-Bans). Der deutsche Server behält alles unter Beobachtung des Nutzers. | Nutzer |
| E15 | Das Dashboard wird **sofort** zweisprachig gemacht, unabhängig von den übrigen Paketen. Sprache kommt aus dem Browser, manueller Umschalter bleibt. | Nutzer |
| E16 | Verworfen: öffentliche Partner-API mit Bearer-Schlüsseln nach Yoshi-Muster (`Deadlock--Patchnotes-Bot/rust/dev-tracker/src/partner.rs`). Kann später nachgerüstet werden, falls ein Server unbedingt seinen eigenen Bot nutzen will. | Nutzer |

## 3. Bestandsaufnahme (belegt am Code, Stand 0085b717f)

### 3.1 Dashboard
- Eigenes i18n ohne Framework in `bot/dashboard_v2/src/i18n/dictionary.ts`: Deutsch ist der Schlüssel, `useT()` aus `context/LanguageContext.tsx`, etwa 300 Einträge.
- Genutzt auf nur 12 von 42 Seiten und in 8 von 72 Komponenten. Rund 30 Seiten mit etwa 16.000 Zeilen sind unübersetzt (unter anderem Uplink, UplinkZiel, StreamReports, Coaching, PartnerProfile, Viewers, AIAnalysis, Monetization, Verwaltung, Schedule, Sessions, SessionDetail, Category, Comparison sowie die chatAnalytics- und coaching-Unterseiten).
- Sprache heute aus `localStorage`, Standard `de` (`readStoredLanguage`, dictionary.ts:622). Die Zeitzone kommt schon aus dem Browser (`api/core.ts:103`).
- Backend-Texte für das Dashboard (`rust/crates/tb-dashboard-api`): mindestens 730 deutsche Zeichenketten mit Umlaut, davon nur ein Teil sichtbar (Fehlermeldungen, Labels, Urteile).
- Weitere Oberflächen nur deutsch: `bot/admin_dashboard` (intern, bleibt deutsch), `website/` (Streamer-Landing, Onboarding, FAQ, Preise, `lang="de"`).

### 3.2 Discord-Kopplung an einen einzigen Server
- `rust/crates/tb-config/src/discord.rs:10`: Konstante `COMMUNITY` (Guild 1289721245281292288) und `STREAMER_ROLE` als Standard für Streamer-Link, OAuth-Nachfassen, Token-Lebenszyklus.
- `rust/crates/tb-config/src/operations.rs:59`: `live_ping_guild_id`, Scam-Guard- und Moderationskanal global.
- `rust/crates/tb-monitoring/src/announce/sink.rs:109`: ein globaler `notify_channel_id` für alle Go-Live-Posts.
- Partnerstatus (`twitch_streamers_partner_state`): `discord_user_id`, `is_on_discord`, `require_discord_link`, `live_ping_role_id` bedeuten stillschweigend „auf dem deutschen Server“. Keine Guild-Spalte.
- Alle Discord-Aktionen laufen über den Broker des Haupt-Bots: `rust/crates/tb-transport-discord/src/relay.rs` (`/internal/master/v1/discord/send-rich-message`, `edit-rich-message`, `delete-message`, `resolve-user`, `member/add-role`, `member/remove-role`, `role/create`, `create-invite`, `send-dm`, `members`, `roles`, `message-reactions`) hinter dem Trait `DiscordBackend`.
- Broker in Deadlock-Bots hat eine Guild-Allowlist (`rust/crates/dl-broker/src/lib.rs:69`). Deadlock-Bots enthält etwa 88 feste Verweise auf die Community-Guild. Durch E5/E6 muss dort nichts geändert werden.

### 3.3 Werbung
- Globale Ersatz-Einladung `DEFAULT_PROMO_DISCORD_INVITE` in `rust/crates/tb-chat/src/promos.rs:165` und `rust/crates/tb-internal-api/src/handlers/chat_command.rs:18`.
- Tabelle `twitch_streamer_invites` hält schon eine Einladung pro Streamer.

### 3.4 Raids und Sprache
- Partner haben kein Sprachfeld; der Partner-Pool wird nicht nach Sprache gefiltert.
- Kategorie-Ersatz fest `de`: `rust/crates/tb-raid/src/auto_raid_pipeline.rs:56` (`FALLBACK_LANGUAGE`).
- Sprachfilter Monitoring und Scout: `rust/crates/tb-config/src/global.rs:69` (`de`, `de-de`, `de-at`, `de-ch`).
- Kategorie-Rangliste nur deutsch: `rust/crates/tb-dashboard-api/src/handlers/category_leaderboard.rs:20`.
- Der Poller liest `stream.language` bereits (`tb-monitoring/src/poller/engine.rs:971`).

### 3.5 Domain und OAuth
- `dead-lock.de` zeigt per DNS auf den Server. Caddy-Snippet `dead_lock_domain_alias` (`/etc/caddy/Caddyfile:1354`) leitet GET-Seiten per 308 auf die Hauptdomain, API- und Callback-Pfade bekommen 404.
- Feste Hauptdomain im Rust-Code an etwa 70 Stellen in über 20 Dateien, unter anderem `tb-dashboard-api/src/handlers/spa.rs` (12), `handlers/plattform_connect.rs` (6), `bin/tb-bot/src/raid_oauth_impl.rs` (6), `tb-raid/src/token_lifecycle.rs` (4), `handlers/auth_login.rs` (4), `auth/discord_admin_login.rs` (4).
- Redirect-Konfiguration: `bot.raid_redirect_uri`, `bot.clip_raid_redirect_uri` (`operations.rs`), `dashboard.oauth_redirect_uri`, `dashboard.affiliate_oauth_redirect_uri`, `dashboard.admin_public_url` (`dashboard_options.rs`), `discord.raid_oauth.success_redirect_url`.

### 3.6 KI-Nutzung und Kosten (Ledger `llm_usage`, letzte 30 Tage)
- Nur Fireworks DeepSeek Flash, etwa 4,6 Mio. Eingabe- und 1,9 Mio. Ausgabe-Tokens bei rund 7.400 Aufrufen. Kein Kostenfeld im Ledger.
- Größte Zwecke: `title-insight`, `engagement`, `chat_message_type`, `channel_profile`, `conversation_scam_judge`, `community_sentiment`, `promo_pitch`.
- Ledger-Lücke: `channel_profile` (1.539 Aufrufe) und weitere Zwecke ohne Tokenzahlen.
- Server-Kosten sind laut Nutzer unerheblich.

## 4. Zielarchitektur

### 4.1 Discord-Hub
- Neues Binary `rust/bin/tb-discord-hub` mit eigener Discord-Anwendung und eigenem Bot-Token (Infisical).
- Bietet **dieselben internen Endpunkte wie der Broker** (`/internal/master/v1/discord/...`), jede Anfrage trägt zusätzlich die Guild-ID. Im Twitch-Bot ändert sich dadurch vor allem die Zieladresse; `BrokerRelay` bekommt das Guild-Feld.
- Weiche: Guild = deutscher Community-Server → unverändert an den Broker des Haupt-Bots weiterreichen. Jede andere Guild → Discord-REST direkt.
- Gateway-Verbindung für Slash-Befehle (`/setup`), Bot hinzugefügt/entfernt (`GUILD_CREATE`/`GUILD_DELETE`) und Mitglied verlassen (`GUILD_MEMBER_REMOVE`, braucht den Members-Intent).
- Rechte im Einladungslink: Kanäle sehen, Nachrichten senden, Links einbetten, Rollen verwalten. Kein Admin.
- Live-Pings an Rollen nur, wenn die Rolle „erwähnbar“ ist. Kein Recht „@everyone erwähnen“ anfordern; `/setup` prüft das und weist darauf hin.
- Rollen verwalten geht nur für Rollen unterhalb der Hub-Rolle; `/setup` prüft die Reihenfolge.
- Idempotenz und Wiederholungen wie im bestehenden Relay. Discord-Rate-Limits pro Route beachten.
- Ab 100 Servern verlangt Discord eine Bot-Verifizierung; für den Test unerheblich.

### 4.2 Datenmodell (neue Migrationen, keine Änderung angewandter Migrationen)
- `discord_guilds`: Guild-ID, Name, Sprache, Ankündigungskanal, Streamer-Rolle, Ping-Rolle, Moderations-/Alarmkanal, Discord-Einladung für Werbung, `auto_partner_default` (Standard an), `cross_server_raids` (Standard an), `ai_features` (Standard aus, für den deutschen Server an), installiert seit, entfernt am, Status.
- `streamer_guilds`: Streamer (Twitch-User-ID), Guild, aktiv, deaktiviert von, deaktiviert am, beigetreten am, Quelle (Onboarding, Migration, Admin).
- Streamer erhalten `broadcaster_language` (aus Helix, laufend aktualisiert).
- Migration: Alle bestehenden Partner bekommen eine aktive Zeile für den deutschen Server (E1). Der deutsche Server bekommt seine heutigen Kanal- und Rollenwerte aus der Konfiguration.
- Partnerstatus wird neu definiert: Partner = mindestens eine aktive `streamer_guilds`-Zeile (E8). `is_on_discord` und `require_discord_link` werden pro Guild ausgewertet.

### 4.3 Domains und OAuth
- Caddy: eigener Block für `dead-lock.de` ohne Weiterleitung. API, Callbacks und Dashboard gehen an dieselben Backends wie die Hauptdomain. Hauptdomain unverändert.
- Ein Backend bedient beide Domains. Jeder OAuth-Ablauf wählt die Redirect-URL nach der Domain, auf der er startet, **nur aus einer festen Allowlist** (die zwei Domains), nie aus einem freien Host-Header. Die gewählte URL wandert im `state` mit und wird beim Code-Tausch wiederverwendet. Session-Cookie wird auf dieser Domain gesetzt.
- Zentrale Funktion „öffentliche Basisadresse“ pro Anfrage beziehungsweise pro Streamer (Heimat-Server) ersetzt die etwa 70 festen Stellen.
- Links in Bot-Nachrichten zeigen je nach Heimat-Server auf die passende Domain.
- Vom Nutzer im App-Dashboard einzutragen (genaue Liste beim Bau aus der Konfiguration ableiten):
  - Twitch-App: `dead-lock.de`-Varianten aller bestehenden Callbacks (Raid-Freigabe `/callback/twitch`, Dashboard-Login, Affiliate, Clip-Raid). Twitch erlaubt bis zu zehn Redirect-URLs.
  - Neue Discord-App (Hub): Redirect für Onboarding (`identify`, `guilds`), Redirect für Server-Admin-Login, Bot-Einladungslink mit den Rechten aus 4.1.

### 4.4 Onboarding und Serverzugehörigkeit
- Onboarding auf `dead-lock.de`: Twitch-Login wie heute, zusätzlich Discord-Login.
- Discord liefert die Serverliste des Nutzers; Schnitt mit installierten Hub-Servern. Ein Treffer: automatisch. Mehrere: Auswahl. Keiner: Hinweis, den Server-Owner um Installation zu bitten.
- Niemand kann einen Server wählen, auf dem er nicht Mitglied ist.
- Bei `auto_partner_default` an wird der Streamer sofort Partner des Servers, sonst wartet er auf Freigabe im Server-Admin-Dashboard.
- Verlässt ein Streamer den Server, wird seine Zeile inaktiv. Ohne aktive Guild ist er kein Partner mehr und wird pausiert wie heute.

### 4.5 Server-Admin-Dashboard
- Login per Discord; Server-Admin ist, wer auf der Guild „Server verwalten“ hat.
- **Erlaubt:** Standard „automatisch Partner“, Streamer der eigenen Guild aktivieren und deaktivieren, Kanäle, Rollen, Sprache, Discord-Einladung, Schalter Raids über Servergrenzen.
- **Nicht erlaubt:** Tags ändern, globale Bot-Bans, Blacklist, Abrechnung, Daten anderer Server, KI-Schalter.
- Zentrale Rechte-Schranke: Jede Abfrage ist auf die Guild des Admins gefiltert, nicht pro Handler einzeln.
- Das bestehende Admin-Panel des Nutzers bleibt die globale Ebene; der deutsche Server ist darin eine Zeile.

### 4.6 Werbung pro Server
- Keine globale Ersatz-Einladung mehr für Streamer fremder Server.
- Einladung kommt aus `discord_guilds` des Heimat-Servers beziehungsweise `twitch_streamer_invites`.
- Gilt für Chat-Promo, LFG-Pitch, Paten-Angebot und Discord-Pitch-Engine.

### 4.7 Raids nach Sprache
- Sprachgruppen: `de` (de, de-de, de-at, de-ch), `en` (en, en-gb, en-us, …), weitere bei Bedarf.
- Partner-Pool nach Sprachgruppe des raidenden Streamers filtern. Bei `cross_server_raids` an: alle Partner derselben Sprachgruppe aus Servern mit ebenfalls aktivem Schalter. Sonst nur Partner desselben Servers.
- Kategorie-Ersatz nutzt die Sprache des raidenden Streamers statt fest `de`.
- Monitoring und Scout: Sprachfilter um `en` erweitern, sobald englische Partner existieren.
- Kategorie-Rangliste und Marktanteile bekommen eine Sprachdimension.

### 4.8 Sprache der Bot-Ausgaben
- Twitch-Chat: Sprache des Zielkanals (`broadcaster_language`).
- Discord: Sprache des Servers (`discord_guilds.language`).
- DMs: Sprache des Heimat-Servers des Empfängers.
- Betroffen: Go-Live-Embeds (`tb-monitoring/src/announce/template.rs`), Befehle und Standardantworten (`tb-chat`), Raid-Nachrichten und Begrüßungs-Erinnerungen (`tb-raid`, `bin/tb-bot/src/raid_greeting.rs`), Reauth- und OAuth-Nachfassen, Sub-Erinnerung, Scam-Warnungen.
- Regelbasierte Spam-Muster auf englische Scam-Texte prüfen und ergänzen.

### 4.9 KI-Schalter pro Server (E14)
- `discord_guilds.ai_features` aus für fremde Server.
- Aus im Chat: `promo_pitch`, `lfg_pitch`, `invite_question`, `conversation_scam_judge` und `_guidance`, Smalltalk-Loop.
- Dashboard-KI (Titel-Auswertung, Engagement, KI-Analyse, Dashboard-Assistent) für fremde Server vorerst ebenfalls aus und die Seiten dort ausgeblendet. **Offen, Empfehlung: aus** (siehe 7).
- Zentraler Guard vor jedem LLM-Aufruf mit Streamerbezug, nicht pro Aufrufer.

### 4.10 Website `dead-lock.de/streamer`
- Übersetzung der bestehenden React-Seite in `website/` (Streamer-Landing, Onboarding, FAQ, Preise), unter `dead-lock.de/streamer` fest Englisch.
- `hreflang` verknüpft beide Fassungen gegenseitig.
- Englische Datenschutzerklärung. Impressum kann deutsch bleiben.

## 5. Pakete und Reihenfolge

| Paket | Inhalt | Aufwand | Abhängigkeit |
|---|---|---|---|
| **D** | Dashboard zweisprachig (sofort, E15): Browsersprache als Standard, alle Streamer-Seiten und Komponenten übersetzt, sichtbare Backend-Texte des Dashboards | 4 bis 6 Tage | keine, läuft jetzt |
| H | Discord-Hub mit reiner Weiterleitung an den Broker (keine sichtbare Änderung) | 2 bis 3 Tage | keine |
| O | Caddy-Block `dead-lock.de`, zentrale Basisadresse, OAuth pro Domain | 2 bis 4 Tage | keine |
| G | Datenmodell Guilds, Migration bestehender Partner, neuer Partnerbegriff, `/setup`, Onboarding mit Serverwahl | 5 bis 7 Tage | H, O |
| A | Server-Admin-Dashboard | 3 bis 4 Tage | G |
| R | Raids nach Sprache, Netzwerk-Schalter, Werbung pro Server | 2 bis 3 Tage | G |
| K | KI-Schalter pro Server mit zentralem Guard | etwa 1 Tag | G |
| C | Englische Chat-, Discord- und DM-Texte, englische Spam-Muster | 1 bis 2 Tage | G |
| W | Website `dead-lock.de/streamer` englisch, Datenschutz EN | 2 bis 3 Tage | O |
| L | Ledger-Lücke schließen (fehlende Tokenzahlen) | unter 1 Tag | keine |

Zusätzlich etwa 20 bis 30 % für Review, Gate, Deploy und Live-Tests. Gesamt etwa 20 bis 28 Arbeitstage Agentenarbeit.

**Testversion für den ersten fremden Server:** H, O, G, R, K, C. Einstellungen zunächst per `/setup`. Danach A und W.

**Größtes Risiko:** Paket G. Der neue Partnerbegriff berührt Raids, Ankündigungen, Live-Ping, Rollen und Pausen des laufenden deutschen Betriebs. Vor dem Umschalten Schattenvergleich: alter und neuer Partnerstatus für alle bestehenden Streamer müssen identisch sein.

## 6. Abnahmekriterien

- Deutscher Server: Ankündigungen, Rollen, Pings, Raids und Chat verhalten sich nach jedem Paket unverändert (Vergleich vor/nach an echten Daten).
- Testserver: Bot ohne Admin-Rechte installiert, `/setup` abgeschlossen, ein englischer Teststreamer über `dead-lock.de` onboarded, Serverwahl korrekt, Go-Live-Embed auf Englisch im richtigen Kanal, Ping-Rolle funktioniert, Werbung nennt nur den Testserver, Raid geht an einen englischen Partner.
- Ein Server-Admin sieht und steuert nur seine Guild.
- Kein KI-Aufruf mit Bezug zu einem Streamer, dessen Server `ai_features` aus hat (Beleg im Ledger).
- OAuth-Abläufe auf beiden Domains live durchgespielt; Cookies und Rücksprung bleiben auf der Startdomain.
- Dashboard: Browser mit `en` zeigt alle Streamer-Seiten ohne deutsche Reste; Umschalter funktioniert; deutsche Ansicht unverändert.

## 7. Offene Punkte

1. Dashboard-KI für fremde Server: Empfehlung aus und Seiten ausblenden.
2. Name und Avatar des Hub-Bots.
3. Englische Datenschutzerklärung: Text muss juristisch passen.
4. Testserver und Owner: Zusage durch den Nutzer.
5. Pitch für fremde Server und Streamer (Entwurf): Go-Live-Feed ohne Arbeit, Raid-Netzwerk hält Zuschauer in der Community, Werbung für den eigenen Discord im Twitch-Chat, automatische Streamer- und Ping-Rollen, Deadlock-spezifische Analysen, Scam- und Spamschutz, `!rank` und `!connect`, kostenlos, keine Admin-Rechte. Schwäche: Das Raid-Netzwerk lebt von Masse; deshalb Standard „Raids über Servergrenzen an“.
