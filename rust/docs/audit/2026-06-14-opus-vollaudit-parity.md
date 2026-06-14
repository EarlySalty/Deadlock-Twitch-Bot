# Twitch-Bot Python→Rust — Opus-Vollaudit Paritäts- & Bug-Report (2026-06-14)

> Methode: 22 Subsystem-Cluster, je ein Opus-Vergleichsagent (volle Python-Oberfläche vs. Rust) + ein
> skeptischer Opus-Verifizierer pro kritischem/hohem Befund. 43 Agenten, ~5,6 Mio Tokens.
> Schweregrade unten sind die **verifizierten** Werte (False-Positives entfernt, Umstufungen angewandt).
> Quelle der Roh-Befunde + Verifikations-Notizen: Workflow-Run `wf_ac5e16b0-ab1`.

**Bilanz (verifiziert):** 4 KRITISCH · 39 HOCH · 102 MITTEL · 109 NIEDRIG. Crit+High nach Art: 17 Divergenz (Bug in portiertem Code), 15 fehlend, 7 Regression (neu in Rust), 4 nur-Proxy.

Drei Problemklassen ziehen sich durch alle Cluster:
- **A — Live-Regressionen:** neuer Bug in bereits nativ laufendem Rust-Code (Chat/Raid/Monitoring/Dashboard sind live). Wirkt JETZT.
- **B — Cutover-Blocker:** Funktion läuft heute nur, weil der Python-Prozess als Proxy/Owner mitläuft. Bricht beim Abschalten von Python.
- **C — Verlorene Subsysteme:** kein nativer Port UND kein Proxy → Feature ist im nativen Betrieb tot.

---

## Paritäts-Matrix je Cluster

| Cluster | Oberfläche~ | treu | divergent | fehlend | proxied | Befunde (K/H/M/N) |
|---|--:|--:|--:|--:|--:|---|
| monitoring-poller | 40 | 22 | 9 | 5 | 0 | 0/1/4/8 |
| monitoring-sessions-announce | 115 | 72 | 9 | 22 | 4 | 0/2/4/5 |
| chat-pipeline-mod | 48 | 30 | 11 | 3 | 1 | 0/1/4/9 |
| chat-safety | 95 | 70 | 11 | 2 | 0 | 0/3/7/4 |
| raid-scoring | 34 | 23 | 7 | 2 | 0 | 0/0/3/7 |
| raid-auth-arrival | 76 | 38 | 9 | 11 | 1 | 0/3/5/4 |
| analytics-overview-perf | 18 | 4 | 9 | 0 | 2 | 1/3/3/7 |
| analytics-audience-insights | 22 | 7 | 8 | 0 | 8 | 0/6/9/2 |
| analytics-admin-raids-market | 33 | 6 | 8 | 0 | 26 | 0/1/6/5 |
| analytics-poststream-coaching-ai | 25 | 0 | 1 | 1 | 8 | 0/0/3/5 |
| analytics-internalhome-public-misc | 34 | 8 | 9 | 0 | 12 | 0/0/5/6 |
| dashboard-auth-session | 34 | 6 | 9 | 3 | 16 | 0/1/7/4 |
| dashboard-billing-affiliate | 48 | 2 | 1 | 0 | 45 | 0/2/4/3 |
| dashboard-live-raids-legal | 32 | 7 | 6 | 1 | 18 | 0/1/6/6 |
| internal-api-routes | 32 | 18 | 6 | 1 | 0 | 0/0/3/1 |
| api-transport-token | 58 | 24 | 12 | 17 | 5 | 0/4/4/7 |
| storage-data | 55 | 9 | 4 | 18 | 14 | 1/1/4/5 |
| entitlements-crypto | 26 | 14 | 7 | 2 | 3 | 0/3/2/4 |
| social-media | 95 | 4 | 3 | 0 | 0 | 1/0/1/0 |
| engagement | 42 | 6 | 2 | 28 | 6 | 1/6/11/8 |
| community | 38 | 14 | 3 | 18 | 0 | 0/1/3/5 |
| clipper-title-coaching | 34 | 0 | 0 | 30 | 2 | 0/0/4/4 |

---

## Befunde je Cluster (verifiziert, nach Schwere)

### monitoring-poller

Der Poll-Loop-Kern selbst (tick/process_entries, live_state load/persist, tracked-Load, stats, extract_stream_start, Poll-Intervall) ist sehr treu portiert — die DB-Verträge (UNION-Tracked-Query, twitch_user_id-Conflict-Key mit DELETE-before-UPSERT, Live-State-Snapshot inkl. Partner-Raid-Flag-Lateral-Join, JSON-Tags, TEXT-ISO-Timestamps) stimmen 1:1 mit Python überein. Die Lücken liegen NICHT im Engine-Kern, sondern in den HOOKS und der Außenwirkung: Im verdrahteten Pfad (SubscriptionPollHooks) sind after_tick (Poll-Tick-Score-Refreshes), on_auto_archive und on_auto_unarchive Noop — d.h. die Auto-Archivierung inaktiver Partner (>10 Tage) und die Auto-Entarchivierung sind beim Rust-Poll faktisch tot, obwohl die Engine sie aufruft und die Kandidaten-Query existiert. Der Go-Live-Handler portiert nur die stream.offline-Subscription; Chat-Join, Werbefrei-Pitch-bei-Stream-Start (consume_stream_start_pitch ist toter Code) und der ReAuth-Chat-Reminder fehlen. Die komplette Broadcaster-Telemetrie- und Moderator-Subscription-Anlage (Bits/Subs/HypeTrain/ChannelPoints + channel.ban/unban/shoutout/follow/channel.moderate) ist nativ NICHT portiert und bleibt Python (harter Cutover-Blocker, u.a. für den Blacklist-Raid-Guard). Der Scout ist als eigener Crate da, weicht aber in Intervall (90s statt 300s), Sprache (4 DE-Varianten statt nur "de"), fehlendem Session-Priming und fehlendem Chat-Heal ab. Wichtig: Poll-Loop UND Rust-Scout sind per Flag default AUS — Python besitzt beide live, daher sind die meisten Befunde latente Cutover-Divergenzen, keine heutigen User-Brüche. Der bekannte mon-poll-1 Sprachfilter-Bug ist inzwischen gefixt (DE-Default in language_filters_from_env).

- **[HOCH · divergence · bestätigt]** Auto-Archivierung inaktiver Partner beim Rust-Poll tot (on_auto_archive ist Noop im verdrahteten Pfad)
  - Python: `bot/monitoring/monitoring.py:1964 _auto_archive_inactive_streamers (ruft _dashboard_archive(login,'archive'))`
  - Rust: `rust/bin/tb-bot/src/main.rs:584 SubscriptionPollHooks (überschreibt on_auto_archive NICHT) + poller/hooks.rs:121 NoopPollHooks-Default false; engine.rs:681 ruft on_auto_archive auf`
  - Wirkung: Inaktive Partner (>10 Tage kein Deadlock-Stream) werden beim Rust-Poll NIE archiviert; Python archiviert sie. Dashboard/Listen bleiben mit Karteileichen voll. Greift erst beim Cutover (Poll default AUS), ist aber ein fertig wirkender Engine-Pfad ohne Sink.
  - Verifikation: Selbst am Code verifiziert. Engine ruft on_auto_archive (engine.rs:681, in auto_archive_inactive ab 657, gedrosselt AUTO_ARCHIVE_THROTTLE) UND on_auto_unarchive (engine.rs:427) auf. Der einzige in den PollEngine verdrahtete Hook ist SubscriptionPollHooks (main.rs:583-602: match subscription_manager => SubscriptionPollHooks, sonst NoopPollHooks). SubscriptionPollHooks (main.rs:92-99) überschreibt NUR on_stream_went_live; on_auto_archive/on_auto_unarchive fallen auf die Trait-Defaults zurück (hook
  - Fix: SubscriptionPollHooks (oder einen kombinierten PollHooks) on_auto_archive implementieren: die Partner-Archive-Op nativ ausführen (DB-Update archived_at + ggf. Discord), analog Python _dashboard_archive. Bis dahin in 05-cleanup-decisions.md als bewusst zurückgestellt dokumentieren.
- **[MITTEL · divergence · unverif.]** Auto-Entarchivierung (archivierter Partner streamt wieder Deadlock) beim Rust-Poll tot
  - Python: `bot/monitoring/monitoring.py:1462-1468 (is_live && is_archived && is_deadlock → _dashboard_archive(login,'unarchive'))`
  - Rust: `rust/crates/tb-monitoring/src/poller/engine.rs:424-430 (ruft hooks.on_auto_unarchive); poller/hooks.rs:115 Default false; main.rs:584 SubscriptionPollHooks überschreibt nicht`
  - Wirkung: Ein archivierter Partner, der wieder Deadlock streamt, wird vom Rust-Poll nicht reaktiviert; Python entarchiviert ihn sofort. Latente Cutover-Divergenz (Poll default AUS).
  - Fix: on_auto_unarchive nativ implementieren (DB-Update archived_at=NULL) und true zurückgeben, sodass die Engine den Kanal ab sofort als aktiv behandelt.
- **[MITTEL · missing · unverif.]** Go-Live-Handler portiert nur stream.offline-Sub — Chat-Join, Werbefrei-Pitch-bei-Stream-Start und ReAuth-Reminder fehlen
  - Python: `bot/monitoring/eventsub_mixin.py:1471-1585 _handle_stream_went_live (60s-Debounce, chat_bot.join, consume_stream_start_pitch→Werbefrei-Pitch 90s, _is_fully_authed-Gate + _maybe_send_reauth_chat_reminder)`
  - Rust: `rust/bin/tb-bot/src/eventsub_hooks.rs:274-278 + main.rs:94-98 + chat_wiring.rs:453-455 (alle nur ensure_offline_subscription)`
  - Wirkung: Nach einem Bot-Timeout in einem Kanal sendet Python 90s nach Go-Live den Werbefrei-Pitch — nativ passiert das nie. ReAuth-Chat-Reminder bei needs_reauth-Partnern entfällt. Chat-Join läuft nativ über die Sub-Reconcile-Schleife statt event-getrieben, also potenziell verzögert für frisch live gegangene Kanäle ohne bestehende Chat-Sub.
  - Fix: Im go-live-Hook nach der Offline-Sub consume_stream_start_pitch prüfen und den Werbefrei-Pitch (verzögert) absetzen; ReAuth-Reminder bei needs_reauth nachziehen; ggf. event-getriebenen Chat-Join ergänzen.
- **[MITTEL · missing · umgestuft]** Broadcaster-Telemetrie- und Moderator-Subscriptions (inkl. channel.moderate) nativ nicht angelegt
  - Python: `bot/monitoring/eventsub_mixin.py:1623-1700 (broadcaster_subs: cheer/bits/hype_train/subscribe/gift/ad_break/channel_points) + 1702-1839 (moderator_subs: channel.ban/unban/shoutout.create/receive/follow/channel.moderate)`
  - Rust: `rust/crates/tb-monitoring/src/subscriptions.rs:21-25 CORE_SUBSCRIPTIONS (nur stream.online/offline + channel.update) + ensure_chat/ensure_raid`
  - Wirkung: Bei einem Python-Cutover fielen alle Telemetrie-Events (Bits/Subs/HypeTrain/ChannelPoints) UND der channel.moderate-getriebene Blacklist-Raid-Guard aus. Heute kein Bruch (Python legt sie an, Twitch liefert an dieselbe Callback-URL), aber harter Cutover-Blocker.
  - Verifikation: Faktisch korrekt, aber Schwere überzogen. Verifiziert: Der Rust-SubscriptionManager (subscriptions.rs) kennt nur CORE_SUBSCRIPTIONS (stream.online/offline, channel.update, Z.21-25), ensure_chat_subscriptions (Z.178) und ensure_raid_subscription/channel.raid (Z.136). KEIN ensure-/create-Pfad für die 12 Broadcaster-Telemetrie-Subs (cheer/bits/hype_train/subscribe/gift/ad_break/channel_points, eventsub_mixin.py:1623-1645) noch für die 6 Moderator-Subs (channel.ban/unban/shoutout.create/receive/foll
  - Fix: Telemetrie-/Moderator-Sub-Anlage nativ mit User-Token (twitch_raid_auth) + Scope-Filter + Bot-Token-Fallback portieren, bevor Python-EventSub abgeschaltet wird.
- **[MITTEL · divergence · unverif.]** Scout-Intervall 90s statt Python 300s → monitoring-only-Streamer werden ~3x früher entfernt
  - Python: `bot/base.py:1284 asyncio.sleep(300) + 1055 (missed_cycles>=2 → remove)`
  - Rust: `rust/crates/tb-monitoring/src/scout.rs:34 DEFAULT_INTERVAL=90s + 31 ABSENT_CYCLES_BEFORE_REMOVE=2`
  - Wirkung: Rust-Scout entfernt entdeckte Streamer bei kurzen Offline-Phasen/Stream-Restarts deutlich aggressiver (Datenverlust an Sessions/Sampling). Beide Scouts default AUS bzw. Python-Scout immer an (runtime_bootstrap.py:978, ungated) — also latent bis TB_SCOUT_ENABLED=1.
  - Fix: DEFAULT_INTERVAL auf 300s setzen ODER ABSENT_CYCLES auf den 300s-äquivalenten Wert anpassen, damit die Removal-Latenz zu Python passt.
- **[NIEDRIG · divergence · unverif.]** Poll-Tick-Score-Refreshes werden verworfen: after_tick ist Noop im verdrahteten Pfad
  - Python: `bot/monitoring/monitoring.py:1778-1782,1441-1444,1790 (_schedule_partner_raid_score_refreshes mit poll_stream_online/offline/restarted)`
  - Rust: `rust/crates/tb-monitoring/src/poller/engine.rs:244,272-277 (after_tick(TickReport{score_refreshes,...})); main.rs:584 SubscriptionPollHooks ohne after_tick-Override; hooks.rs:126 Default-Noop`
  - Wirkung: Score-Refreshes aus Poll-Transitions kommen beim Rust-Poll nicht an. Real abgemildert durch den separaten 300s-Voll-Reconcile (main.rs:380-420) + EventSub-online/offline-Refreshes — Scores aktualisieren sich also weiter, nur ggf. bis zu 300s verzögert statt beim nächsten Tick. category_streams für Partner-Recruiting bleibt ebenfalls ungenutzt (Recruiting ist bewusst pausiert).
  - Fix: after_tick im verdrahteten PollHooks implementieren und score_refreshes an den ScoreRefreshResolver durchreichen; Recruiting bleibt bewusst deferred.
- **[NIEDRIG · divergence · unverif.]** Scout fetcht 4 DE-Sprachvarianten statt Python nur "de"
  - Python: `bot/base.py:988-993 get_streams_for_game(language="de", limit=100)`
  - Rust: `rust/crates/tb-monitoring/src/scout.rs:261-281 (iteriert self.language_filters); main.rs:637 language_filters_from_env() → [de,de-de,de-at,de-ch]`
  - Wirkung: Rust-Scout registriert auch de-at/de-ch/de-de-Streamer, die Python ausschließt → breiterer monitoring-only-Pool. Latent (Scout default AUS).
  - Fix: Entscheiden, ob die DE-Varianten gewollt sind; falls Python-Parität, dem Scout nur ["de"] geben statt die 4-Varianten-Liste.
- **[NIEDRIG · divergence · unverif.]** Scout primt keine Sofort-Session für neu entdeckte Streamer (_prime_monitored_only_sessions fehlt)
  - Python: `bot/base.py:1039-1043 _prime_monitored_only_sessions(streams, new_logins)`
  - Rust: `rust/crates/tb-monitoring/src/scout.rs:296-308 (nur upsert_monitored, kein Session-Prime)`
  - Wirkung: Frisch entdeckte monitoring-only-Streamer bekommen ihr erstes Session-Sample verzögert; bei sehr kurzen Streams ggf. gar keins. Latent (Scout default AUS).
  - Fix: Im Scout für new_streamers ein Session-Open + erstes Sample anstoßen (SessionTracker.ensure_session), analog Python-Prime.
- **[NIEDRIG · divergence · unverif.]** Scout-Upsert setzt created_at nicht explizit (Python setzt now)
  - Python: `bot/base.py:1029-1035 INSERT ... (twitch_login, twitch_user_id, is_monitored_only, created_at) VALUES (...,%s) mit now`
  - Rust: `rust/crates/tb-monitoring/src/scout.rs:79-89 INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only) ohne created_at`
  - Wirkung: Ist kein Spalten-Default gesetzt, landet created_at NULL (oder Insert schlägt bei NOT NULL fehl). Bei vorhandenem Default ist es ein leicht abweichender Zeitstempel. Latent (Scout default AUS).
  - Fix: created_at im Rust-Insert explizit auf den aktuellen ISO-Zeitstempel setzen, um das Python-Verhalten zu spiegeln und NOT-NULL-Risiken auszuschließen.
- **[NIEDRIG · divergence · unverif.]** Capacity-Snapshots ohne Retention-Cleanup → unbegrenztes Wachstum
  - Python: `bot/monitoring/eventsub_mixin.py:677-685 (_record_eventsub_capacity_snapshot löscht stündlich Snapshots älter als retention_days)`
  - Rust: `rust/crates/tb-monitoring/src/subscriptions.rs:369-390 CapacitySnapshotStore.record (nur INSERT, kein DELETE)`
  - Wirkung: twitch_eventsub_capacity_snapshot wächst nativ unbegrenzt (DB-Bloat über Zeit). Tabelle ist aber niedrigfrequent befüllt → geringer Effekt.
  - Fix: Im CapacitySnapshotStore oder in der Sub-Maintenance-Schleife einen periodischen DELETE mit retention-Cutoff ergänzen, analog Python.
- **[NIEDRIG · divergence · unverif.]** Capacity-Snapshot ohne Sample-Interval-Throttle (Python drosselt)
  - Python: `bot/monitoring/eventsub_mixin.py:640-644 (Throttle via _eventsub_capacity_sample_interval_seconds, force-Bypass)`
  - Rust: `rust/crates/tb-monitoring/src/subscriptions.rs:350-355 record_capacity_snapshot (kein Zeitfenster-Guard)`
  - Wirkung: Bei vielen Go-Live-Wellen entstehen mehr Snapshot-Zeilen als bei Python; kombiniert mit fehlender Retention etwas mehr Bloat. Funktional unkritisch.
  - Fix: Optionalen Mindestabstand zwischen Snapshots einbauen, falls die Snapshot-Frequenz relevant wird.
- **[NIEDRIG · divergence · unverif.]** tick() ohne is_auth_blocked-Circuit-Breaker (Python bricht früh ab)
  - Python: `bot/monitoring/monitoring.py:1207-1213 (mehrfache if self.api.is_auth_blocked(): return)`
  - Rust: `rust/crates/tb-monitoring/src/poller/engine.rs:156-165 tick() (kein Auth-Block-Check); poller/source.rs hat kein is_auth_blocked`
  - Wirkung: Bei dauerhaft blockiertem App-Token verschwendet der Rust-Poll jeden Tick erfolglose Helix-Requests (Logspam, API-Last) statt zu pausieren. Korrektheit nicht betroffen, nur Effizienz/Lograuschen.
  - Fix: Im HelixStreamSource/Engine einen Auth-Block-Zustand abfragen und tick() früh überspringen, analog Pythons is_auth_blocked.
- **[NIEDRIG · divergence · unverif.]** stats-Tags JSON-Encoding ohne ensure_ascii (Python escapt non-ASCII)
  - Python: `bot/monitoring/monitoring.py:1054 json.dumps(clean_tags, ensure_ascii=True, separators=(",",":"))`
  - Rust: `rust/crates/tb-monitoring/src/stream.rs:39-51 tags_json() via serde_json::to_string`
  - Wirkung: Rein kosmetische Encoding-Differenz in der tags-Spalte von twitch_stats_*; bei Vergleich/Diff der Roh-JSON-Strings inkonsistent, semantisch identisch.
  - Fix: Falls byte-Parität gewünscht, beim Serialisieren non-ASCII escapen; sonst als bewusste Modernisierung dokumentieren.

### monitoring-sessions-announce

Der Kern dieser Einheit (Session-Lebenszyklus, Go-Live-Announce-Template + Broker-Sink, durable Processing-Inbox, Guard-Store, Telemetrie-Inserts, Webhook-Empfang, Poll-Loop, Offline-Seiteneffekte) ist überwiegend treu und teils härter als Python portiert: Sessions-Math/Retention/Dropoff, Inbox-Backoff/Dead-Letter, Guard-Claim, exp_sessions, channel.update und first_message (inkl. confirmed_first_ever, das im 14.6.-Audit noch als fehlend galt — ist inzwischen gefixt) stimmen feldgenau. Die gravierende Lücke liegt NICHT im Verarbeiten, sondern im Subscriben: unter aktivem TWITCH_RUST_MONITORING_TAKEOVER startet Python den EventSub-Listener nicht mehr (runtime_bootstrap.py:968), und der native Rust-Pfad legt nur stream.online/offline/channel.update + channel.raid + channel.chat.* als Subscriptions an. Die gesamte Broadcaster-/Moderator-Telemetrie (cheer/bits.use/hype_train/subscribe/subscription.*/ad_break/channel_points sowie channel.ban/unban/follow/shoutout/moderate und channel.chat.user_first_message) wird von NIEMANDEM mehr nativ subscribed — die Rust-Store-/Handler-Funktionen existieren, bekommen aber keine Events mehr (alte Twitch-Subs laufen nur weiter, solange nicht revoked). Dazu kommen mehrere kleinere Divergenzen: Retry rendert den aktuellen Tick statt der Erstversuch-Payload, fehlende Live-Ping-Rollen-Autoerstellung, fehlende Timestamp-Freshness-Prüfung im Webhook, nicht-native Post-Stream-Analyse und ein zusätzlich geschriebenes language-Stats-Feld.

- **[HOCH · missing · umgestuft]** Broadcaster-/Moderator-Telemetrie-Subscriptions werden nativ NICHT angelegt — Events kommen nicht mehr an
  - Python: `bot/monitoring/eventsub_mixin.py:1623-1712 (broadcaster_subs + moderator_subs in _handle_stream_went_live)`
  - Rust: `rust/crates/tb-monitoring/src/subscriptions.rs:21 (CORE_SUBSCRIPTIONS nur 3), rust/bin/tb-bot/src/eventsub_hooks.rs:274-278`
  - Wirkung: Sämtliche Bits-, Sub-, Hype-Train-, Ad-Break-, Channel-Points- und Follow/Ban/Shoutout-Telemetrie sowie first_message versiegen für alle Kanäle, deren Twitch-Subs nach dem Cutover ablaufen/revoked werden, und für JEDEN neuen Partner sofort. Die Rust-Telemetrie-Inserts (telemetry.rs) und der dispatch.store_telemetry-Pfad laufen ins Leere. Analytics-Dashboard verliert still ganze Metrik-Klassen.
  - Verifikation: Faktisch belegt: subscriptions.rs:21 CORE_SUBSCRIPTIONS = nur [stream.online, stream.offline, channel.update]; Grep über rust/ findet keine native Erstellung von channel.cheer/bits/hype_train/subscribe/subscription.*/ad_break/channel_points/ban/unban/shoutout/follow/moderate (nur channel.raid via raid_adapters + channel.chat.* via chat_wiring). runtime_bootstrap.py:967-969 belegt: unter rust_takeover startet Python den EventSub-Listener nicht (`if rust_takeover: pass`), also legt auch Python die
  - Fix: Native Subscription-Erstellung der broadcaster_subs/moderator_subs (mit Broadcaster-/Bot-Token + Scope-Filter) in den Go-Live-Hook bzw. die subscription_maintenance_loop ziehen; mindestens channel.chat.user_first_message und channel.moderate nachsubscriben, da Letzteres den Blacklist-Raid-Guard speist.
- **[HOCH · missing · bestätigt]** Go-Live-Followups stark verschlankt: Chat-Join, Werbefrei-Pitch, ReAuth-Reminder fehlen im nativen Pfad
  - Python: `bot/monitoring/eventsub_mixin.py:1471-1596 (_handle_stream_went_live), analytics/mixin.py:1814 (_run_stream_online_followups)`
  - Rust: `rust/bin/tb-bot/src/eventsub_hooks.rs:274-278 (on_stream_went_live nur ensure_offline_subscription)`
  - Wirkung: Werbefrei-Pitch nach Stream-Start und ReAuth-Reminder bei needs_reauth entfallen im EventSub-Go-Live-Pfad; betroffene Streamer bekommen keine Re-Auth-Aufforderung mehr beim Live-Gehen.
  - Verifikation: Bestätigt am Code: eventsub_hooks.rs:274-278 on_stream_went_live ruft ausschließlich ensure_offline_subscription. Pythons _handle_stream_went_live (eventsub_mixin.py:1493-1597) macht: Chat-Join, Werbefrei-Pitch nach 90s-Timeout (1530-1553), fully_authed-Check + _maybe_send_reauth_chat_reminder (1579-1591). Im nativen Pfad: Chat-Join läuft separat über chat_wiring.rs:453-454 (bestätigt — on_stream_went_live wird gewrappt, Join via ensure_chat_subscriptions), also NICHT verloren. Werbefrei-Pitch u
  - Fix: ReAuth-Reminder + Werbefrei-Pitch an den nativen Go-Live-Hook koppeln oder explizit als Python-Worker-Zuständigkeit dokumentieren und im Cutover-Plan als Lücke führen.
- **[MITTEL · missing · umgestuft]** channel.moderate wird gehandhabt aber nie subscribed — Blacklist-Raid-Guard hängt an Alt-Subs
  - Python: `bot/monitoring/eventsub_mixin.py:1711 (channel.moderate in moderator_subs)`
  - Rust: `rust/bin/tb-bot/src/eventsub_hooks.rs:331 (on_channel_moderate) — kein ensure_*moderate*`
  - Wirkung: Der Blacklist-Raid-Guard (Abbruch manueller Raids auf Blacklist-Ziele) feuert nur, solange die historisch von Python angelegte channel.moderate-Sub bei Twitch noch lebt. Nach Revocation/neuer Partner: Guard ist tot, Raids auf Blacklist-Ziele laufen durch.
  - Verifikation: Faktisch korrekt: eventsub_hooks.rs:331 on_channel_moderate → BlacklistRaidGuard.handle ist voll implementiert, dispatch.rs:248-257 routet channel.moderate, aber Grep über rust/ zeigt keine native channel.moderate-Subscription-Anlage. Python legt sie in moderator_subs an (eventsub_mixin.py:1711, version 1, channel:moderate, mit moderator_user_id-Retry 1750-1786). Dies ist jedoch derselbe Wurzel-Befund wie #1 und im Cutover-Plan (04-cutover-plan.md:104) namentlich genannt ('moderator-gated Subs w
  - Fix: channel.moderate-Subscription (mit moderator_user_id = Bot-ID) nativ im Go-Live-/Maintenance-Pfad anlegen, analog zur channel.raid-Sub im Raid-Subsystem.
- **[MITTEL · divergence · unverif.]** Live-Ping-Rolle wird im nativen Sink nicht auto-erstellt
  - Python: `bot/monitoring/embeds_mixin.py:505-561 (_ensure_live_ping_role: guild.create_role + persist)`
  - Rust: `rust/crates/tb-monitoring/src/announce/sink.rs:209-227`
  - Wirkung: Neue Partner ohne vorab gesetzte live_ping_role_id bekommen keine Live-Ping-Rolle und keinen Rollen-Ping im Go-Live-Posting; das Feature ist für sie still tot, bis die Rolle anderweitig angelegt wird.
  - Fix: Rollen-Erstellung über den Discord-Bridge/Broker anstoßen (z. B. ein ensure-role-Endpoint) oder beim Partner-Onboarding einmalig anlegen.
- **[MITTEL · regression · unverif.]** Webhook-Receiver prüft die Message-Freshness (Timestamp-Alter) nicht
  - Python: `bot/monitoring/eventsub_webhook.py:168-176, 563-570 (_is_message_too_old, 600s, 403 bei zu alt)`
  - Rust: `rust/crates/tb-monitoring/src/webhook_receiver.rs:111-132 (nur Signatur, kein Timestamp-Check)`
  - Wirkung: Eine korrekt signierte, aber alte Nachricht kann nach Ablauf des Message-ID-Guards (>600 s) erneut akzeptiert und verarbeitet werden — eng begrenzt, aber Python hatte hier eine zweite, unabhängige Schranke, die nun fehlt.
  - Fix: Timestamp-Header parsen und Nachrichten mit Alter > 600 s (oder Zukunfts-Skew) vor dem Dispatch mit 403 verwerfen.
- **[MITTEL · proxied · unverif.]** Post-Stream-Analyse läuft im nativen Offline-Pfad nicht (Python-Worker-abhängig)
  - Python: `bot/monitoring/eventsub_mixin.py:1953-1959 (trigger_post_stream_analysis bei stream.offline)`
  - Rust: `rust/bin/tb-bot/src/offline_side_effects.rs:11-12 (Doc: Post-Stream-Analyse bewusst NICHT portiert)`
  - Wirkung: Der unmittelbare Post-Stream-Analyse-Trigger hängt am weiterlaufenden Python-Worker; fällt der aus oder wird er abgeschaltet, bleibt nur der Backfill-/Retry-Job. Nicht nativ — zählt als Migrationslücke.
  - Fix: Post-Stream-Analyse-Trigger nativ in on_stream_offline aufnehmen oder die Python-Worker-Abhängigkeit verbindlich im Cutover-Plan festschreiben.
- **[NIEDRIG · divergence · unverif.]** Go-Live-Retry rendert den aktuellen Tick statt der Erstversuch-Payload
  - Python: `bot/monitoring/monitoring.py:382-469 (_resolve_live_announcement_retry_payload — cached stream + rendered_at)`
  - Rust: `rust/crates/tb-monitoring/src/announce/sink.rs:115-205 (RetryState hält nur tracking_token + render_now)`
  - Wirkung: Bei Retry kann der Embed-Inhalt (Viewerzahl/Titel) vom Erstversuch abweichen; Idempotenz gegen Doppel-Posting bleibt über den stabilen Tracking-Token gewahrt. Bereits im 14.6.-Audit als low notiert.
  - Fix: Im RetryState die Erstversuch-Stream-Felder mitführen und beim Retry statt des aktuellen Ticks rendern.
- **[NIEDRIG · divergence · unverif.]** ReAuth-Reminder-Dedupe-Guard wird im nativen Offline-Pfad nicht zurückgesetzt
  - Python: `bot/monitoring/eventsub_mixin.py:1870-1879 (_reauth_reminder_last_sent_ts.pop bei stream.offline)`
  - Rust: `rust/bin/tb-bot/src/offline_side_effects.rs:29-53 (run — kein Reminder-Guard-Reset)`
  - Wirkung: Da der ReAuth-Reminder ohnehin im Python-Worker lebt, ist der Effekt gering; wird der Reminder je nativ, würde ohne Reset pro Streamzyklus höchstens einmal erinnert.
  - Fix: Bei nativer ReAuth-Reminder-Portierung den Per-Broadcaster-Dedupe-Guard im Offline-Hook mit zurücksetzen.
- **[NIEDRIG · divergence · unverif.]** Stats-INSERT schreibt zusätzliches language-Feld, das die Python-Variante nicht setzt
  - Python: `bot/monitoring/monitoring.py:2089-2095, 2131-2137 (INSERT mit 7 Spalten ohne language)`
  - Rust: `rust/crates/tb-monitoring/src/stats.rs:60-75 (INSERT mit 8 Spalten inkl. language)`
  - Wirkung: Additive Abweichung für die DE-Markt-Sicht; nur konsistent, wenn die language-Spalte in den Stats-Tabellen existiert. Falls Schema sie nicht hat, schlägt der Rust-INSERT fehl (Stats-Verlust pro Tick). Andernfalls bloß ein Feld mehr als die hier vorliegende Python-Version.
  - Fix: Sicherstellen, dass twitch_stats_tracked/_category die language-Spalte führen; den Python-INSERT angleichen oder die Abweichung als bewusste Erweiterung dokumentieren.
- **[NIEDRIG · divergence · unverif.]** Inbox-Payload wird ohne sort_keys serialisiert (Schlüsselordnung weicht ab)
  - Python: `bot/monitoring/eventsub_processing_inbox.py:89 (json.dumps(..., sort_keys=True))`
  - Rust: `rust/crates/tb-monitoring/src/inbox_store.rs:89 (payload.to_string())`
  - Wirkung: Der gespeicherte payload_json-String unterscheidet sich in der Schlüsselreihenfolge; funktional irrelevant, solange niemand die serialisierte Form vergleicht (wird beim Verarbeiten zurückgeparst). Nur ein Repräsentations-Drift in der Inbox-/Dead-Letter-Tabelle.
  - Fix: Falls Determinismus gewünscht: ein BTreeMap-/sortiertes Serialisieren verwenden, sonst als harmlos dokumentieren.
- **[NIEDRIG · divergence · unverif.]** _coerce_bool für nicht-ganzzahlige Zahlen weicht im Template ab
  - Python: `bot/live_announce/template.py:30-41 (_coerce_bool: bool(float) für 0.5 → True)`
  - Rust: `rust/crates/tb-monitoring/src/announce/template.rs:40-51 (b(): Number via as_i64, 0.5 → None → default)`
  - Wirkung: Nur relevant, wenn Announcement-Config-Booleans als Float (z. B. 0.5) gespeichert wären — praktisch unwahrscheinlich. Edge-Case-Divergenz.
  - Fix: In b() für Value::Number auch f64 != 0 als true werten, um Pythons bool()-Semantik exakt zu treffen.

### chat-pipeline-mod

Die Chat-Verarbeitungspipeline ist überwiegend treu und nativ portiert: Die 15-Schritt-Pipeline (pipeline.rs), der zweistufige Spam-Score-Filter inkl. Homoglyph-Normalisierung, Hart/Weich-Signale, Mention-Scoring, Sus-Invite-Erkennung, Fun-Responses (korrekt default-aus), Global-Ban-Enforcement und der Auto-Ban-Pfad (Delete→Ban, 401-Retry, AlreadyBanned, silent_ban, Notice-/Reason-Texte) decken sich Zeichen- und schwellengenau mit Python. Die SPAM_PHRASES/FRAGMENTS/BRAND_TOKENS-Listen und WHITELISTED_BOTS sind identisch. Es gibt aber substanzielle Divergenzen: (1) Die KI-gelernten Spam-/Safe-Muster werden in Rust nur EINMAL beim Bootup geladen und nie aktualisiert — der selbstlernende Spam-Loop ist faktisch tot, weil der SpamAiReviewer zwar neue Muster in die DB schreibt, der laufende Filter sie aber bis zum Neustart ignoriert (Python: 2-min-TTL + Cache-Invalidierung). (2) Der Channel-Classifier fehlt der Session-Fallback für is_deadlock_live und hardcodet "deadlock" statt TWITCH_TARGET_GAME_NAME. (3) Mehrere Command-Divergenzen: !title/!titel und !lurkersteuer_off sind tot (return false), Super-Mod-Toggle ist NoopSuperMod (immer false), !raid_enable liefert keinen klickbaren OAuth-Link, !silentban/!silentraid lassen den Reauth-Precheck weg, und unberechtigte Mod-Commands sowie Engagement-Ablehnungen schlucken still jede Rückmeldung. Die meisten dieser Punkte sind im 2026-06-14-Coverage-Audit bekannt; die SpamFilter-Pattern-Reload-Lücke und der Classifier-Session-Fallback sind NEU.

- **[HOCH · regression · bestätigt]** KI-gelernte Spam-/Safe-Muster werden nie neu geladen — selbstlernender Filter tot bis Neustart
  - Python: `bot/chat/spam_ai_review.py:100 load_learned_patterns (TTL 120s) + :223 _invalidate_pattern_cache; bot/chat/moderation.py:543-545,572-574`
  - Rust: `rust/bin/tb-bot/src/chat_wiring.rs:254 (LearnedPatterns::load einmalig) + rust/crates/tb-chat/src/spam_filter.rs:443-451 (kein Reload/Interior-Mutability)`
  - Wirkung: Der gesamte Auto-Improving-Spam-Mechanismus ist im nativen Betrieb wirkungslos: neu gelernte Spam-Phrasen greifen erst nach Bot-Neustart, neu gelernte Safe-Muster korrigieren keine False-Positives. Im Python-Betrieb wirkten beide innerhalb von 2 Minuten.
  - Verifikation: Selbst verifiziert. Python moderation.py:543-545/572-574 ruft load_learned_patterns()/load_safe_patterns() PRO Nachricht; der Cache hat _PATTERN_CACHE_TTL (spam_ai_review.py:104,126) und wird nach jedem Lernschritt via _invalidate_pattern_cache() (Z.223) geleert → neue Muster greifen binnen Sekunden/TTL. In Rust gibt es ausserhalb von Tests genau EINEN LearnedPatterns::load — chat_wiring.rs:254 (Boot). SpamFilter (spam_filter.rs:443-445) hält `learned: LearnedPatterns` als plain owned Feld, ist 
  - Fix: SpamFilter auf ArcSwap<LearnedPatterns> o.ä. umstellen und einen periodischen Reload-Task (z.B. alle 120s, analog Python-TTL) in chat_wiring spawnen; idealerweise nach erfolgreichem SpamAiReviewer-Schreibvorgang aktiv neu laden.
- **[MITTEL · divergence · unverif.]** channel_classifier: is_deadlock_live ohne Session-Fallback aus twitch_stream_sessions
  - Python: `bot/chat/moderation.py:2051-2073 (_is_target_game_live_for_chat: live_state fehlt → Fallback auf twitch_stream_sessions per session_id)`
  - Rust: `rust/crates/tb-chat/src/channel_classifier.rs:135-146 (nur twitch_live_state, sonst false)`
  - Wirkung: Im Zeitfenster zwischen Stream-Start (Session existiert) und dem ersten twitch_live_state-Write klassifiziert Rust den Kanal fälschlich als nicht-deadlock-live → Fun-Responses, Activity-Promos und !invite werden in diesem Fenster unterdrückt, obwohl Python sie zugelassen hätte.
  - Fix: In classify_from_db einen Fallback ergänzen: bei fehlender live_state-Zeile die offene Session (twitch_stream_sessions WHERE streamer_login=? AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1) lesen und game_name vergleichen.
- **[MITTEL · missing · unverif.]** !title / !titel liefert keine Antwort (nicht portiert)
  - Python: `bot/chat/commands.py:770 cmd_title (name=title, aliases=titel)`
  - Rust: `rust/crates/tb-chat/src/commands.rs:339 ("!title" | "!titel" => false)`
  - Wirkung: Mod/Broadcaster, die im Chat !title nutzen, bekommen unter dem nativen Takeover keinerlei Reaktion. Bekannt aus dem 2026-06-14-Audit (medium, mod-only/low-traffic, Dashboard-Alternative existiert).
  - Fix: TitlePort-Trait andocken; MiniMax-M3-Client ist bereits in scam_pitch.rs vorhanden, nur der Title-Use-Case fehlt.
- **[MITTEL · divergence · unverif.]** !raid_enable sendet keinen klickbaren OAuth-Link
  - Python: `bot/chat/commands.py:94-99 (auth_url = self._raid_bot.auth_manager.generate_auth_url(twitch_login) + Link im Chat)`
  - Rust: `rust/crates/tb-chat/src/commands.rs:585-594 (Antwort: 'Kontaktiere einen Admin für den Auth-Link', Kommentar 'UNSICHER: auth_url-Generierung nicht im Trait abgebildet')`
  - Wirkung: Streamer können sich nicht mehr selbst per Chat autorisieren — der Self-Service-Onboarding-Pfad für Auto-Raid ist gebrochen, Admin-Eingriff nötig. Bekannt aus dem 2026-06-14-Audit (low; eher medium, da Onboarding-Blocker).
  - Fix: Einen AuthUrlPort/Trait andocken, der generate_auth_url(twitch_login) liefert (Raid-OAuth-Schicht existiert in tb-raid).
- **[MITTEL · missing · umgestuft]** Engagement-KI-Pipeline (Pipeline-Schritt 11) ist No-op — KI-Stammgast schweigt
  - Python: `bot/chat/bot.py:1757-1811 (Engagement get_pipeline().handle + stealth_sender für JEDE Partner-Nachricht)`
  - Rust: `rust/crates/tb-chat/src/pipeline.rs:536 (Schritt 11 als No-op kommentiert)`
  - Wirkung: Der MiniMax-KI-Stammgast antwortet seit dem Chat-Flip GAR NICHT mehr im Chat. Bereits im 2026-06-14-Audit als critical/high mit höchstem realem User-Impact markiert.
  - Verifikation: Code bestätigt: pipeline.rs:536 ist nur ein Kommentar ('Schritt 11: Engagement-AI — No-op bis Engagement-Phase'), keine Pipeline, kein stealth_sender. Python bot.py:1757-1811 führt get_pipeline().handle pro Partner-Nachricht aus und sendet via stealth_sender.send. Kein Proxy trägt das (Chat-Eingang nativ via EventSub). Echte Coverage-Lücke. Schwere aber von high auf medium: das Python-Feature war in Prod faktisch dormant — pipeline.py:250 bricht früh mit Decision.DISABLED ab, wenn settings None/
  - Fix: Engagement-KI-Kern nativ portieren (MiniMax-Pipeline + Threads + Persona) oder explizit als deferred kommunizieren; reiner No-op ohne Proxy = echte Funktionslücke.
- **[NIEDRIG · divergence · unverif.]** channel_classifier: Zielspiel hartkodiert 'deadlock' statt TWITCH_TARGET_GAME_NAME
  - Python: `bot/chat/moderation.py:2010-2012 + bot/chat/bot.py:170 (_target_game_lower aus TWITCH_TARGET_GAME_NAME; leer → immer True)`
  - Rust: `rust/crates/tb-chat/src/channel_classifier.rs:143 (game.trim().to_lowercase() == "deadlock")`
  - Wirkung: Solange TWITCH_TARGET_GAME_NAME='Deadlock' ist, identisch. Bei Änderung/Leeren der Konfiguration weicht das Live-Gate ab (Rust würde nie/immer matchen, wo Python umkonfigurierbar ist).
  - Fix: Zielspiel-String aus der Konfiguration in den ChannelClassifier injizieren; leerer Wert → is_deadlock_live immer True (Python-Semantik).
- **[NIEDRIG · missing · unverif.]** !lurkersteuer_off / _aus / lurker_tax_off nicht portiert
  - Python: `bot/chat/commands.py:535-537 cmd_lurkersteuer_off`
  - Rust: `rust/crates/tb-chat/src/commands.rs:341 (=> false)`
  - Wirkung: Streamer können die Lurker-Steuer nicht mehr per Chat-Command abschalten; stiller Funktionsausfall unter Takeover.
  - Fix: Schreibpfad auf streamer_plans portieren oder den Command bewusst als deferred dokumentieren; aktuell falsche-Stille (return false → kein Feedback).
- **[NIEDRIG · regression · umgestuft]** Super-Mod-Toggle für Engagement-Commands tot (NoopSuperMod → immer false)
  - Python: `bot/chat/engagement_commands.py:87-99 (_engagement_can_toggle via bot.engagement.admin.is_super_mod)`
  - Rust: `rust/bin/tb-bot/src/chat_wiring.rs:277,686-693 (NoopSuperMod) + rust/crates/tb-chat/src/commands.rs:980-986`
  - Wirkung: Ein Super-Mod ohne echten Twitch-Mod-Status kann !engagement_on/off nicht mehr auslösen — die Super-Mod-Rolle ist im nativen Chat wirkungslos. Bekannt aus dem 2026-06-14-Audit (high).
  - Verifikation: Code-Divergenz bestätigt: NoopSuperMod::is_super_mod gibt immer false zurück (chat_wiring.rs:690-692), und is_engagement_admin (commands.rs:995-1000) fällt für Nicht-Mod/Nicht-Broadcaster darauf zurück; Python nutzt echten DB-Lookup auf twitch_admin_roles role='super_mod' (admin.py:14-29). ABER Schwere übertrieben: (a) Broadcaster und Twitch-Mods können weiter toggeln (is_mod_or_broadcaster → true), nur die sehr spezielle super_mod-DB-Rolle verliert den Toggle. (b) Der Toggle steuert ausschliess
  - Fix: NoopSuperMod durch echte DB-Query (twitch_admin_roles role='super_mod' bzw. die in bot.engagement.admin genutzte Tabelle) ersetzen.
- **[NIEDRIG · divergence · unverif.]** Engagement-Command-Ablehnung sendet keine Rückmeldung
  - Python: `bot/chat/engagement_commands.py:104-108,130-134 (ctx.send 'Nur Broadcaster, Mods oder Super-Mod dürfen das.')`
  - Rust: `rust/crates/tb-chat/src/commands.rs:992-995,1033-1036 (if !is_engagement_admin → return ohne reply)`
  - Wirkung: Nutzer ohne Berechtigung erhalten kein Feedback, warum der Command nichts tut.
  - Fix: Vor dem Return eine Ablehnungs-Reply senden (analog Python-Text).
- **[NIEDRIG · divergence · unverif.]** !silentban / !silentraid ohne Reauth-Precheck
  - Python: `bot/chat/commands.py:445-452 (silentban) + :501-508 (silentraid): _is_fully_authed → sonst 'Neu-Autorisierung erforderlich'`
  - Rust: `rust/crates/tb-chat/src/commands.rs:834-867 (cmd_silentban) + :873-906 (cmd_silentraid) — kein is_fully_authed-Aufruf vor dem Toggle`
  - Wirkung: Streamer mit abgelaufener Autorisierung bekommen keinen Reauth-Hinweis mehr; der stille Toggle ist harmlos, aber der nudge zur Neu-Autorisierung fehlt.
  - Fix: Vor dem Toggle is_fully_authed(partner.twitch_user_id) aufrufen und bei false die Reauth-Reply senden.
- **[NIEDRIG · divergence · unverif.]** Unberechtigte Mod-Commands werden still geschluckt (keine Ablehnungs-Reply)
  - Python: `bot/chat/commands.py:60-63 (raid_enable), :430-433 (silentban), :486-489 (silentraid) — ctx.send 'Nur der Broadcaster oder Mods können den Bot steuern.'`
  - Rust: `rust/crates/tb-chat/src/commands.rs:275-308 (if event.is_mod_or_broadcaster() { handler } sonst kein else)`
  - Wirkung: Normale Chatter, die einen Mod-Command absetzen, erhalten keine Rückmeldung. Bekannt aus dem 2026-06-14-Audit (low).
  - Fix: Im else-Zweig die Ablehnungs-Reply senden (Python-Text), bevor true zurückgegeben wird.
- **[NIEDRIG · missing · unverif.]** VoiceReaction-Dispatch (Pipeline-Schritt 1) ist bewusster No-op
  - Python: `bot/chat/bot.py:1534-1546 (_voice_reaction_dispatch_message für JEDE Nachricht vor Whitelist-Check)`
  - Rust: `rust/crates/tb-chat/src/pipeline.rs:404 (Schritt 1 als No-op kommentiert)`
  - Wirkung: Streamer-Antworten in offenen Voice-Reaction-Outreach-Konversationen werden im nativen Chat nicht erkannt. Default-OFF auch in Python, daher gering — aber unter Takeover real abwesend.
  - Fix: Im Rahmen der Engagement/Voice-Reaction-Phase nachziehen; aktuell korrekt als deferred dokumentiert.
- **[NIEDRIG · divergence · unverif.]** Mention-Reason-Strings fließen nicht in has_hard_spam_signal des zweiten Evaluate ein (kosmetisch, aber Reason-Reihenfolge weicht ab)
  - Python: `bot/chat/bot.py:1611-1615 (mention_reasons werden VOR den Eskalatoren an spam_reasons angehängt)`
  - Rust: `rust/crates/tb-chat/src/pipeline.rs:597-602 (zweiter evaluate-Call erhält mention_reasons NICHT in matched; reasons.extend(mention_reasons) erst danach)`
  - Wirkung: Kein Score-/Aktions-Unterschied (die Eskalator-Gate-Bedingung wertet nur harte Signale, Mention-Reasons sind weich). Nur die Reihenfolge der Reason-Strings in Review-Log/Alert weicht minimal ab.
  - Fix: Belassen (im Code als bewusst kosmetisch dokumentiert) — kein Handlungsbedarf, nur zur Vollständigkeit gelistet.
- **[NIEDRIG · divergence · unverif.]** AI-Review nutzt eigenen Pattern-Cache, der vom live SpamFilter entkoppelt ist
  - Python: `bot/chat/spam_ai_review.py:223,276 (_invalidate_pattern_cache/_invalidate_safe_cache nach Schreiben → derselbe Cache, den load_learned_patterns nutzt)`
  - Rust: `rust/crates/tb-chat/src/scam_pitch.rs:1511-1520 (SpamAiReviewer hält Arc<patterns> separat) vs spam_filter.rs:443-451 (SpamFilter.learned)`
  - Wirkung: Verstärkt Befund #1: Selbst wenn der Reviewer seinen eigenen Cache aktualisierte, bliebe der Filter veraltet. Doppelte Quelle der Wahrheit für gelernte Muster.
  - Fix: Gemeinsame, hot-reloadbare Pattern-Quelle (ArcSwap) für Filter und Reviewer verwenden.

### chat-safety

Der chat-safety-Scope ist überwiegend nativ und sehr werkgetreu portiert. Scam-Pitch (scam_pitch.rs), Sus-Invite (sus_invite.rs), Global-Ban-Sweep (global_ban_sweep.rs), Global-Chatter-Ban (global_chatter_ban.rs), Chatter-Tracking (chatter_tracking.rs) und der Bot-Token-Manager (token.rs) sind verhaltensgleich umgesetzt; mehrere im 13.6.-Audit gemeldete schwere Scam-Pitch-Bugs (StrongTimeout-Delete, Follower-Gate tot) sind im aktuellen Code bereits gefixt. Die größten verbleibenden Lücken liegen im Promo-Pfad (promos.rs): (1) zwei Send-Ergebnis-Regressionen — Scam-Warnung und Targeted-Promo verbrauchen den Promo-Slot/Cooldown auch bei fehlgeschlagenem/gedropptem Send, weil das Ergebnis ignoriert wird; (2) der periodische Promo-Loop überspringt den Partner-Tracking-Gate, den Python für jeden Promo-Pfad durchläuft; (3) globale Admin-Promo-Overrides sind als Stub tot; (4) die Validierung streamer-eigener Promo-Texte fehlt; (5) Lurker-Tax-Settings/Entitlements lösen nicht über twitch_user_id auf und ignorieren Plan-Expiry sowie den Bot-Scope-Fallback. Account-Age-Caching im Scam-Pitch fehlt (mehr Helix-Calls), ist aber bereits als bekannt dokumentiert. Token-, Sweep- und Tracking-Subsysteme sind produktionsreif.

- **[HOCH · regression · bestätigt]** Scam-Warnung: Send-Ergebnis ignoriert — Slot/Cooldown auch bei Fehlsend verbraucht
  - Python: `bot/chat/promos.py:1084 (_maybe_send_scam_warning: if not warned: return False)`
  - Rust: `rust/crates/tb-chat/src/promos.rs:1122 (let _ = self.api.send_announcement(...))`
  - Wirkung: Bei gedroppter/fehlgeschlagener Announcement (AutoMod-Drop, Rate-Limit, Mod-Timeout) gilt die Warnung als gesendet: der Promo-Slot ist verbraucht, der 120-min-Scam-Cooldown läuft, und die reguläre Promo wird unterdrückt (true) — obwohl im Chat nichts ankam. Die Fake-Server-Warnung erreicht ihre Zielgruppe seltener.
  - Verifikation: Selbst am Code verifiziert. Rust promos.rs:1122 verwirft das Ergebnis mit `let _ = self.api.send_announcement(channel_id, &text, "orange").await;` und ruft danach BEDINGUNGSLOS mark_promo_sent (1127), setzt state.last_scam_warning_sent/last_scam_warning_text (1131-1132) und save_promo_cooldown (1134), gibt true zurück (1137). Python promos.py:1084-1095 prüft `warned = await self._send_announcement(...)` und kehrt bei `if not warned: return False` (1090-1091) zurück, BEVOR _mark_promo_sent / _las
  - Fix: Rückgabe von send_announcement auswerten; bei nicht-Sent früh `return false`, und erst bei Erfolg mark_promo_sent + last_scam_warning_sent + save_promo_cooldown setzen (wie Python).
- **[HOCH · regression · bestätigt]** Targeted-Promo: Send-Ergebnis ignoriert — Cooldown/Abwechslung auch bei Fehlsend gesetzt
  - Python: `bot/chat/targeted_promo.py:264 (if not ok: return False)`
  - Rust: `rust/crates/tb-chat/src/promos.rs:1508 und :1534 (let _ = self.api.send_message/send_announcement(...))`
  - Wirkung: Fehlgeschlagener/gedroppter Targeted-Send verbraucht trotzdem den 15-min-Kanal-Cooldown, kippt den global/user-Abwechslungs-Status und markiert den User als heute-gepitcht — der Promo-Slot ist verloren, obwohl nichts gesendet wurde.
  - Verifikation: Selbst am Code verifiziert, beide Pfade. User-Pfad: Rust promos.rs:1508 `let _ = self.api.send_message(...)`, danach unbedingt channel_last_targeted/channel_last_type="user"/user_last_pitched (1512-1514) + mark_promo_sent (1517) + return true (1519). Global-Pfad: 1534 `let _ = self.api.send_announcement(...,"purple")`, danach unbedingt channel_last_targeted/channel_last_type="global" (1538-1539) + mark_promo_sent (1542) + true (1544). Python targeted_promo.py:260-282 setzt `ok = await send_ann/s
  - Fix: Send-Ergebnis in beiden Zweigen prüfen; bei nicht-Sent `return false` ohne State-Mutation, sonst wie bisher fortfahren.
- **[HOCH · divergence · bestätigt]** Periodischer Promo-Loop überspringt is_partner_channel_for_chat_tracking-Gate
  - Python: `bot/chat/promos.py:1524 + 1533 (is_partner_channel_for_chat_tracking(login) für lurker_tax und activity/targeted/spike)`
  - Rust: `rust/crates/tb-chat/src/promos.rs:706 (send_promo_if_due — partner_check nirgends im Loop aufgerufen)`
  - Wirkung: Kanäle, die zwar deadlock-live + promo_disabled=0 sind, aber im Chat-Tracking nicht als Partner gelten, bekommen in Rust periodische Promos/Lurker-Tax/Scam-Warnungen, die Python für sie unterdrückt. Mögliche Werbung an nicht-freigegebenen Kanälen.
  - Verifikation: Befund am Code bestätigt, aber mit wichtiger Mitigations-Nuance. Trait PartnerChannelCheck (promos.rs:231-235) wird via set_partner_check (565) verdrahtet — und in der Bot-Binary mit einem ECHTEN DB-Check belegt: chat_wiring.rs:263-265 `.set_partner_check(Arc::new(DbPartnerCheck{...}))`, DbPartnerCheck (711-728) liest twitch_streamers_partner_state.is_partner_active. partner_check.is_partner_channel_for_chat_tracking wird aber NUR in on_message (589) aufgerufen, NICHT in send_promo_if_due (706-7
  - Fix: Im send_promo_if_due-Loop pro Kanal `if !self.partner_check.is_partner_channel_for_chat_tracking(login).await { continue; }` vor lurker_tax und vor dem activity/targeted-Block ergänzen (analog Python).
- **[MITTEL · missing · unverif.]** Globale Admin-Promo-Override-Nachricht ist Stub — Feature tot
  - Python: `bot/chat/promos.py:884 (_load_global_promo_message → load_global_promo_mode + evaluate_global_promo_mode), bot/promo_mode.py:231/300`
  - Rust: `rust/crates/tb-chat/src/promos.rs:892 (load_global_promo_message: "Hier Stub: immer None")`
  - Wirkung: Ein global gesetzter Admin-Promo-Override (z.B. zeitlich begrenzte Event-Werbung über alle Kanäle) wirkt in Rust nicht — es wird immer der Streamer-/Kategorie-Text genommen. Admin verliert die globale Promo-Steuerung.
  - Fix: promo_mode-Schema (promo_mode.py) lesen und load_global_promo_mode/evaluate_global_promo_mode nativ portieren; bei aktivem Override active_message zurückgeben.
- **[MITTEL · divergence · unverif.]** Streamer-Promo-Text wird ohne Validierung gesendet
  - Python: `bot/chat/promos.py:871 (validate_streamer_promo_message(message) → bei issues return None)`
  - Rust: `rust/crates/tb-chat/src/promos.rs:899 (load_streamer_promo_message — keine Validierung)`
  - Wirkung: Eine vom Streamer hinterlegte Promo-Nachricht, die Python wegen Regelverstoß (z.B. verbotene Inhalte/Platzhalter) ablehnt, wird in Rust ausgesendet — Umgehung der Promo-Text-Policy.
  - Fix: validate_streamer_promo_message (promo_mode.py:156) portieren und in load_streamer_promo_message bei Issues None zurückgeben (Fallback auf Pool wie Python).
- **[MITTEL · divergence · unverif.]** Lurker-Tax-Settings: keine twitch_user_id-Auflösung, keine Plan-Expiry-Prüfung
  - Python: `bot/chat/promos.py:212-296 (_load_lurker_tax_settings: twitch_streamer_identities→user_id, streamer_plans per user_id ODER login mit Priorität, manual_plan_expires_at, resolve_plan_snapshot_for_refs)`
  - Rust: `rust/crates/tb-chat/src/promos.rs:1289 (Lurker-Tax-Settings-Query nur LOWER(p.twitch_login)=$1)`
  - Wirkung: Streamer, deren streamer_plans-Eintrag nur per twitch_user_id (ohne login) geführt wird, finden ihren Plan in Rust nicht → Lurker-Tax bleibt still. Abgelaufene manuelle Pläne werden in Rust nicht als abgelaufen erkannt → Entitlement-Drift.
  - Fix: user_id über twitch_streamer_identities auflösen und streamer_plans per user_id-OR-login (Priorität user_id) abfragen; manual_plan_expires_at berücksichtigen, idealerweise über die Plan-Snapshot-Auflösung wie Python.
- **[MITTEL · divergence · unverif.]** Lurker-Tax: Bot-Token-Scope-Fallback fehlt — Feature feuert nie ohne streamer-eigenen Scope
  - Python: `bot/chat/promos.py:345-349 (has_chatters_scope = scope in streamer-scopes ODER in bot_scopes via token_manager)`
  - Rust: `rust/crates/tb-chat/src/promos.rs:1319-1335 (has_chatters_scope nur aus twitch_raid_auth.scopes des Streamers)`
  - Wirkung: Streamer, die auf den zentralen Bot-Token angewiesen sind (intendierter Migrationspfad), erhalten in Rust nie eine Lurker-Tax-Erinnerung — das Feature ist für sie dauerhaft tot. (Bereits im 13.6.-Audit als chat-promos dokumentiert, weiterhin offen.)
  - Fix: Bot-Scope-Quelle (BotTokenManager.scopes) durchreichen und has_chatters_scope = streamer_scope ODER bot_scope setzen.
- **[MITTEL · divergence · unverif.]** Promo-Entitlement (chat.promos.disable) ohne Plan-Snapshot/Expiry-Auflösung
  - Python: `bot/chat/promos.py:1122-1130 (_promo_blocked_by_plan_or_flag → resolve_plan_snapshot_for_refs → entitlements)`
  - Rust: `rust/crates/tb-chat/src/promos.rs:1733-1739 (plan_id_has_promos_disable auf manual_plan_id/plan_name ohne Expiry)`
  - Wirkung: Bei abgelaufenem manual_plan oder Multi-Ref-Konstellationen kann die Werbefrei-Sperre in Rust falsch (an/aus) ausfallen — werbefrei-bezahlte Kanäle bekommen ggf. Promos oder umgekehrt. Verwandte Entitlement-Drift ist im 13.6.-Audit dokumentiert.
  - Fix: Plan-Snapshot-Auflösung inkl. Expiry portieren und dieselbe Entitlement-Quelle wie Lurker-Tax/Billing nutzen.
- **[MITTEL · divergence · unverif.]** Account-Alter wird im Scam-Pitch nicht gecacht — jeder Score-Treffer ruft Helix
  - Python: `bot/chat/service_pitch_warning.py:653-698 (_get_account_age_days: 6h-Cache, id|login-Key, login-Fallback)`
  - Rust: `rust/crates/tb-chat/src/scam_pitch.rs:848 (observe ruft account_age direkt) + bin/tb-bot/src/chat_wiring.rs:489 (HelixAccountAge ohne Cache)`
  - Wirkung: Deutlich mehr Helix-Calls als Python bei aktiven Pitchern (kein 6h-Cache) → Rate-Limit/Latenz-Druck. Zusätzlich kein Login-basierter Lookup bei fehlender/nicht-numerischer user_id. Entscheidungslogik selbst identisch. (Bereits als chat-scam im 13.6.-Audit dokumentiert.)
  - Fix: account_age_cache-Feld im Fetch-Pfad nutzen (Read vor Port-Call, Write inkl. None, 6h-TTL) oder Cache im HelixAccountAge-Adapter; Login-Fallback ergänzen.
- **[MITTEL · divergence · unverif.]** Lurker-Tax-Kandidaten: Bot-Filter fehlt, Login- statt Identity-Key-Join
  - Python: `bot/chat/promos.py:451-524 (_get_lurker_tax_candidates: build_known_chat_bot_not_in_clause + chatter_identity_key id:/login:)`
  - Rust: `rust/crates/tb-chat/src/promos.rs:1402-1450 (get_lurker_tax_candidates ohne Bot-NOT-IN, Join über chatter_login)`
  - Wirkung: Bekannte Chat-Bots können fälschlich als Lurker mit @name gepingt werden. Login-only-Join ist bei Login-Wechseln / fehlender ID-Identität ungenauer als die id-priorisierte Identität. (Identity-Key-Drift bereits im 13.6.-Audit erwähnt.)
  - Fix: Known-Bot-NOT-IN-Klausel ergänzen und Join auf chatter_identity_key (id:/login:) umstellen wie Python.
- **[NIEDRIG · divergence · unverif.]** Scam-Warnung nicht abschaltbar — SCAM_WARNING_ENABLED-Flag nicht portiert
  - Python: `bot/chat/promos.py:1025 (if not SCAM_WARNING_ENABLED: return False), constants SCAM_WARNING_ENABLED`
  - Rust: `rust/crates/tb-chat/src/promos.rs:1046 (scam_warning_due_inner: kein ENABLED-Check)`
  - Wirkung: Die Fake-Server-Scam-Warnung lässt sich in Rust nicht zentral abschalten; ein Betreiber-Toggle, das Python kennt, fehlt. Bei Default (enabled) identisches Verhalten.
  - Fix: SCAM_WARNING_ENABLED (+ analog PROMO_VIEWER_SPIKE_ENABLED) als Konfig-Konstante/Env portieren und in scam_warning_due_inner bzw. maybe_send_viewer_spike_promo abfragen.
- **[NIEDRIG · divergence · unverif.]** Viewer-Spike-Schwelle ignoriert MIN_DELTA-Komponente
  - Python: `bot/chat/promos.py:1281-1284 (threshold = max(baseline*MIN_RATIO, baseline+MIN_DELTA))`
  - Rust: `rust/crates/tb-chat/src/promos.rs:1271 (threshold = baseline * PROMO_VIEWER_SPIKE_MIN_RATIO)`
  - Wirkung: Bei Default (MIN_DELTA=0, MIN_RATIO=1.0) identisch. Wird MIN_DELTA per Env >0 gesetzt, divergiert Rust (zu niedrige Schwelle → Spike-Promo feuert früher als in Python). Latent.
  - Fix: MIN_DELTA-Konstante ergänzen und threshold = max(baseline*MIN_RATIO, baseline+MIN_DELTA) bilden.
- **[NIEDRIG · divergence · unverif.]** stream_start_delay_ok: strikteres RFC3339-Parsing als Python
  - Python: `bot/chat/promos.py:104-109 (datetime.fromisoformat mit Datum-only-Toleranz, UTC-Annahme bei naive)`
  - Rust: `rust/crates/tb-chat/src/promos.rs:1672 (chrono::DateTime::parse_from_rfc3339)`
  - Wirkung: Liegt last_started_at in einem nicht-RFC3339-Format vor, fail-opens Rust (Promo direkt beim Go-Live möglich), während Python korrekt das Alter berechnet und die Anfangs-Promo-Sperre greift. Abhängig vom DB-Format.
  - Fix: Toleranteres Parsing (z.B. naive-Datetime + UTC-Annahme, Datum-only-Sonderfall) wie Python verwenden.
- **[NIEDRIG · divergence · unverif.]** spam_ai_review.review_worthwhile: leere-reasons-Pfad weicht ab
  - Python: `bot/chat/spam_ai_review.py:286-305 (_review_worthwhile: leere reasons → kein any-Treffer → return False)`
  - Rust: `rust/crates/tb-chat/src/scam_pitch.rs:1606-1623 (review_worthwhile: spam_reasons leer → return spam_domain.is_match(content))`
  - Wirkung: Latent/tot, da der Produktionspfad maybe_review_with_reasons mit gefüllten reasons verwendet. Würde maybe_review (leere reasons) je genutzt, triggerte Rust AI-Reviews bei reinem Domain-Match, die Python nicht macht.
  - Fix: Im is_empty()-Zweig false zurückgeben (Python-Parität) oder die ungenutzte maybe_review-Variante entfernen.

### raid-scoring

Die Einheit raid-scoring ist in Rust weitgehend nativ und überwiegend treu portiert: Die reine Score-Mathematik (scoring.rs), die Kandidaten-Auswahl (candidate_selection.rs/target_resolution.rs), Eligibility (eligibility.rs/offline_eligibility.rs), Outreach-Boost, Partner-Roster, die Auto-Raid-Pipeline und der Signal-Korrelations-Planer sind 1:1-Ports mit Tests, und der Score-Refresh (score_refresh.rs) ist live in main.rs verdrahtet. Es bleiben aber mehrere belegte Divergenzen, die teils Score-/Tracking-Daten verfälschen: Beim Confirm-Tracking schreibt Rust bei fehlendem Score-Cache NULL statt der Python-Defaults (0.0/0.5/1.0) und übernimmt gespeicherte readiness/fairness-Spalten statt sie wie Python aus duration/time/base neu abzuleiten; im Daily-Cap-Fallback und beim Outreach-Boost-Tiebreak weichen Reason-Label bzw. Sortier-Semantik (Sentinel statt leerer String) ab; die Recency-Grenze nutzt Sekunden-Truncation statt Float; die Rundung von avg_duration_sec ist half-away statt banker's; und der Fallback-Pfad reichert Follower bewusst nicht an (Tie-Break-Drift). Zwei nicht portierte Effekte (voice_reaction-Conversation nach Outreach-Boost, partner_scores._ensure_runtime_schema-Spalten-Guard) sind bewusste Lücken. Die früher gemeldete raid-scoring-1-Lücke (deadlock_continued/resolved nie gesetzt) ist inzwischen behoben.

- **[MITTEL · divergence · umgestuft]** ConfirmResolver schreibt bei fehlendem Score-Cache NULL statt Python-Defaults (0.0/0.5/1.0)
  - Python: `bot/raid/partner_raid_score_tracking.py:122-134 (_score_payload), :391-393`
  - Rust: `rust/bin/tb-bot/src/confirm_resolver.rs:113-122`
  - Wirkung: Bestätigte Partner-Raids ohne vorhandene Score-Cache-Zeile bekommen NULL-Scores statt der definierten neutralen Defaults. Analytik/Auswertungen über das Tracking lesen NULL statt 0.0/0.5/1.0; Aggregationen (AVG etc.) verzerren oder kippen auf NULL.
  - Verifikation: Divergenz am Code bestaetigt. Python track_confirmed_partner_raid (Z.390-393): ohne uebergebenen score_snapshot wird _score_payload(_load_cached_score_snapshot(...)) genutzt; _load_cached_score_snapshot gibt {} zurueck wenn keine Zeile existiert (Z.194-195), und _score_payload({}) (Z.122-135) liefert ueber _safe_float(..., default) feste Werte: final/base=0.0, duration/time/readiness/fairness=0.5, new_partner/raid_boost=1.0, today=0 (nur score_last_computed_at wird NULL). Rust confirm_resolver.r
  - Fix: In confirm_resolver.rs bei snapshot=None die Python-Defaults einsetzen (final/base 0.0, duration/time/readiness/fairness 0.5, multipliers 1.0, today 0) statt None durchzureichen — z.B. `snapshot.as_ref().map(|s| s.final_score).or(Some(0.0))` je Feld mit dem jeweils richtigen Default.
- **[MITTEL · divergence · unverif.]** ConfirmResolver nutzt gespeicherte readiness/fairness-Spalten, Python leitet sie neu aus duration/time/base ab
  - Python: `bot/raid/partner_raid_score_tracking.py:186-208 (_load_cached_score_snapshot, _derive_readiness_score/_derive_fairness_score)`
  - Rust: `rust/bin/tb-bot/src/confirm_resolver.rs:118-119; rust/crates/tb-raid/src/score_store.rs:122-140 (load)`
  - Wirkung: Weicht ab, sobald die gespeicherte readiness/fairness-Spalte nicht exakt der aus duration/time/base abgeleiteten entspricht — z.B. im offline+Cache-Freeze-Zweig oder nach Formel-/Rundungsdrift. Getrackte readiness/fairness pro Raid können von Pythons Wert abweichen.
  - Fix: In confirm_resolver.rs readiness/fairness aus den Snapshot-Feldern duration/time/base neu ableiten (gleiche Formeln wie scoring.rs compute_readiness_score + Python _derive_fairness_score) statt die gespeicherten Spalten zu übernehmen.
- **[MITTEL · missing · unverif.]** Outreach-Boost: voice_reaction_conversation nach erfolgreichem Boost-Raid nicht portiert
  - Python: `bot/raid/raid_pipeline.py:369-382 (open_voice_reaction_conversation nach is_outreach_boost-Erfolg)`
  - Rust: `rust/crates/tb-raid/src/auto_raid_pipeline.rs:255-259,471-486 (nur consume_outreach_boost)`
  - Wirkung: Nach einem Boost-Raid wird die (Discord-)Voice-Reaction-Conversation mit dem frisch geraideten Streamer nicht eröffnet — ein Outreach-Followup-Schritt fehlt gegenüber Python.
  - Fix: Voice-Reaction-Open über die Discord-Broker-Erweiterung als optionalen Sink in die Pipeline einhängen (analog consume_outreach_boost), oder die Lücke explizit im Cutover-Plan tracken.
- **[NIEDRIG · divergence · unverif.]** select_by_score: daily_cap_filtered vor Fallback berechnet → falsches Reason-Label im All-over-Cap-Fall
  - Python: `bot/raid/services/candidate_selection.py:285-310`
  - Rust: `rust/crates/tb-raid/src/candidate_selection.rs:173,193-197,217-221,237-241`
  - Wirkung: Nur das Log-/Reason-Label divergiert (kein DB-/Ziel-Unterschied), aber die im Doc-Kommentar 'exakt wie Python' zugesicherte Parität ist verletzt; Observability irreführend.
  - Fix: daily_cap_filtered erst nach Bilden von `pool` als `candidates.len() - pool.len()` berechnen (bei leerem under_cap = 0).
- **[NIEDRIG · divergence · unverif.]** is_recent_deadlock: Sekunden-Truncation (num_seconds) statt Float (total_seconds) am 360s-Grenzwert
  - Python: `bot/raid/services/raid_data_sources.py:86-97 (is_recent_deadlock, total_seconds() <= 360)`
  - Rust: `rust/crates/tb-raid/src/eligibility.rs:34-37`
  - Wirkung: Bei Differenzen zwischen 360.000 und 360.999s zählt ein Just-Chatting-Partner in Rust noch als Deadlock-eligible, in Python nicht — minimaler Grenzfall, kann aber Auto-Raid-Eligibility eines Kandidaten umkippen.
  - Fix: Float-Differenz nutzen: `(now-dt).to_std().map(|d| d.as_secs_f64()).unwrap_or(f64::INFINITY) <= cap_seconds as f64`, negativen Drift via unwrap_or(INFINITY) absichern.
- **[NIEDRIG · divergence · unverif.]** resolve_boost_target: fehlendes started_at sortiert ans Ende (Sentinel) statt nach vorn (Python leerer String)
  - Python: `bot/raid/raid_pipeline.py:167-170 (sort key (viewer, str(started_at or '')))`
  - Rust: `rust/crates/tb-raid/src/target_resolution.rs:215-217; rust/bin/tb-bot/src/raid_adapters.rs:46-55 (STARTED_AT_SENTINEL '9999-99-99')`
  - Wirkung: Outreach-Boost-Ziel-Auswahl kann bei gleichem viewer_count und fehlender Startzeit einen anderen Streamer wählen als Python — der Boost trifft potenziell den falschen frisch kontaktierten Kanal.
  - Fix: Leeres started_at als kleinsten Schlüssel führen (FairnessCandidate.started_at als Option<String> oder '' statt Sentinel) und im sort_by None/'' vor Some sortieren — exakt wie Pythons str(started_at or '').
- **[NIEDRIG · divergence · unverif.]** Fallback-/Boost-Kandidaten ohne Follower-Anreicherung → Tie-Break-Drift gegenüber Python attach_followers_totals
  - Python: `bot/raid/services/candidate_selection.py:329-331,377-378 (attach_followers_totals vor Tie-Break)`
  - Rust: `rust/bin/tb-bot/src/raid_adapters.rs:43-56 (followers_total: 0)`
  - Wirkung: Bei gleichem received_raids_total und viewer_count wählt der DE-Fallback/Boost ein anderes Ziel als Python (followers-Stufe übersprungen). Betrifft nur die seltenen Mehrfach-Gleichstände im Kategorie-Fallback.
  - Fix: Entweder Follower für die (kleine) Tie-Break-Menge nachladen wie im Partner-Pfad (auto_raid.rs assemble_eligible_partners) oder die bewusste Abweichung in 05-cleanup-decisions.md dokumentieren.
- **[NIEDRIG · divergence · unverif.]** avg_duration_sec: Rust .round() (half-away) vs Python round() (banker's) bei exakten .5-Mittelwerten
  - Python: `bot/raid/partner_scores.py:680-682 (int(round(sum/len)))`
  - Rust: `rust/bin/tb-bot/src/score_refresh.rs:469 (.round() as i64)`
  - Wirkung: Marginaler Effekt: avg_duration_sec ±1s nur bei exakten .5-Mittelwerten; Auswirkung auf duration_score ≈ 1/avg (≈0.0001) und in der Score-Pipeline praktisch unsichtbar.
  - Fix: Banker's Rounding nachbauen (round-half-to-even) für avg_duration_sec, falls bit-genaue Parität gewünscht; sonst als bewusste Abweichung dokumentieren.
- **[NIEDRIG · missing · unverif.]** partner_scores._ensure_runtime_schema (Spalten-Guard) nicht portiert
  - Python: `bot/raid/partner_scores.py:388-424 (_ensure_runtime_schema: ALTER TABLE ADD COLUMN für readiness/fairness/internal_*)`
  - Rust: `—`
  - Wirkung: Auf einer DB ohne diese Spalten (Alt-Schema) würde der Rust-Score-Refresh mit SQL-Fehler scheitern statt sich wie Python selbst zu migrieren. In Prod existieren die Spalten (Schema-Vertrag verifiziert), daher real geringes Risiko — reine Robustheits-/Bootstrap-Lücke.
  - Fix: Entweder eine sqlx-Migration als Single Source of Truth garantieren (vorhanden) oder bewusst dokumentieren, dass Rust kein Laufzeit-Schema-Guard nachbildet.
- **[NIEDRIG · infra · unverif.]** score_refresh/confirm_resolver tragen irreführendes #![allow(dead_code)] 'noch nicht aus main.rs aufgerufen'
  - Python: `—`
  - Rust: `rust/bin/tb-bot/src/score_refresh.rs:13-14; rust/bin/tb-bot/src/confirm_resolver.rs:13-14`
  - Wirkung: Kein Laufzeit-Effekt, aber irreführende Doku: Reviewer könnten annehmen, die Divergenzen in diesen Modulen seien noch inaktiv, obwohl sie live sind.
  - Fix: Stale-Kommentare entfernen und allow(dead_code) prüfen/streichen, damit der aktive Status der beiden Resolver klar ist.

### raid-auth-arrival

Der Raid-Kern ist solide portiert: Scope-Profile, OAuth-Flow/State-Store, verschlüsselter Token-Read/Write (AAD-genau, Advisory-Lock byte-identisch), Raid-Executor, Strikes, Raid-Blacklist, manuelle Suppression, Arrival-Tracking-Store und Score-Tracking sind verhaltens- und schema-treu. Der seit dem 13.6.-Audit gemeldete Sofort-Lockout (raid-auth-1) ist inzwischen in token_blacklist.rs gefixt. ABER: drei strukturell tote Lücken bleiben produktiv wirksam. (1) Re-Auth-Seiteneffekte: Pythons save_auth entfernt nach erfolgreicher Re-Autorisierung den Token-Blacklist-Eintrag und spiegelt den Partner-Status — der Rust-Callback tut beides nur teilweise (Partner-Sync nur mit Discord-ID im State) und löscht die Blacklist NIE, sodass ein einmal blacklisteter Streamer nach Re-Auth dauerhaft gesperrt bleibt. (2) Die komplette periodische Wartung fehlt: kein proaktiver Token-Refresh-Sweep (refresh_all_tokens/2h), keine Grace-Period-Verarbeitung (check_grace_periods/stündlich, Rolle entfernen + Reminder), kein State-/Blacklist-Cleanup, kein needs_reauth-Massensnapshot. (3) Die Arrival-Followups (raid-arrival-1) sind weiterhin tot: 5 Decision-Flags + silent_raid-Gate werden berechnet aber nicht konsumiert, und die ganze externe-Recruitment-Blacklist-Maschinerie (persist/schedule/process_due) existiert in Rust gar nicht. Discord-Token-Error-Notify ist ebenfalls nicht portiert.

- **[HOCH · regression · bestätigt]** Re-Auth entfernt Token-Blacklist-Eintrag nicht — blacklisteter Streamer bleibt nach Neu-Autorisierung dauerhaft gesperrt
  - Python: `bot/raid/auth.py:1374 save_auth (ruft token_error_handler.remove_from_blacklist); bot/api/token_error_handler.py:871 remove_from_blacklist (DELETE twitch_token_blacklist + technical_pause_reason token_error→NULL)`
  - Rust: `rust/crates/tb-raid/src/auth_writer.rs:84 store_new_auth; rust/bin/tb-bot/src/raid_oauth_impl.rs:977 oauth_callback (kein DELETE auf twitch_token_blacklist im gesamten Re-Auth-Pfad)`
  - Wirkung: Ein Streamer, dessen Refresh-Token revoked wurde (error_count erreichte ≥3), wird selbst nach erfolgreicher Re-Autorisierung weiterhin als token_blacklisted geführt: get_valid_token/get_valid_token_unrestricted liefern None (Blacklist-Check vor allem), resolve_integration_state meldet blocked. Raid-Bot bleibt für ihn tot, obwohl er gerade frisch autorisiert hat.
  - Verifikation: Selbst am Code verifiziert. Python bot/raid/auth.py:1374 ruft in save_auth explizit token_error_handler.remove_from_blacklist, das twitch_token_blacklist per DELETE leert (token_error_handler.py:891-894). Im Rust-Re-Auth-Pfad fehlt das: oauth_callback (raid_oauth_impl.rs:787-1058) ruft store_new_auth (auth_writer.rs:84-179) — dieses setzt nur needs_reauth=FALSE/reauth_notified_at=NULL (Z.170-176), KEIN DELETE auf twitch_token_blacklist. Die Followups complete_setup_for_streamer/sync_partner_stat
  - Fix: Im oauth_callback nach erfolgreichem store_new_auth (beide Pfade, unabhängig von Discord-ID) ein DELETE FROM twitch_token_blacklist WHERE twitch_user_id=$1 plus technical_pause_reason-Reset (='token_error'→NULL) ausführen — Äquivalent zu remove_from_blacklist; idealerweise direkt im AuthWriter in dieselbe Transaktion.
- **[HOCH · divergence · bestätigt]** Arrival-Followup-Flags + silent_raid-Gate berechnet aber nie konsumiert (Partner-Dank, Recruitment, Auto-Blacklist tot)
  - Python: `bot/raid/raid_arrival_runtime.py:265-416 (delete_external_blacklist_pending, record_confirmed_external_recruitment_raid, maybe_schedule_external_recruitment_blacklist_pending, send_partner_raid_message, send_recruitment_message, refresh_partner_score_cache, silent_raid-Return @395)`
  - Rust: `rust/crates/tb-raid/src/arrival_confirmation.rs:428-436 (Flags gesetzt); rust/bin/tb-bot/src/raid_arrival_wiring.rs:256-370 confirm_pending_raid (konsumiert NUR target_is_partner + should_track_confirmed_partner_raid)`
  - Wirkung: Bestätigte Partner-Raids senden keine Dank-/Shoutout-Nachricht; Recruitment-Funnel ist tot; externe Recruitment-Raids werden nicht persistiert und lösen keine Auto-Blacklist (Schwelle 4) aus; silent_raid wird ignoriert (Nachrichten würden bei aktivem Sink trotz Silent-Flag gesendet). Bekannter Befund raid-arrival-1, weiterhin unbehoben.
  - Verifikation: Am Code verifiziert. arrival_confirmation.rs:428-436 setzt sechs Flags (should_delete_external_recruitment_blacklist_pending, should_refresh_partner_score_cache, should_send_partner_raid_message, should_persist_confirmed_external_recruitment_raid, should_schedule_external_recruitment_blacklist_pending, should_send_recruitment_message). Grep über crates/+bin/ (ohne arrival_confirmation.rs/Tests) findet NULL Konsumenten dieser sechs Flags. Der einzige Confirm-Sink confirm_pending_raid (raid_arriva
  - Fix: Im confirm_pending_raid-Sink die 5 Flags in Python-Reihenfolge konsumieren (delete→persist[abort-bei-None]→schedule→silent_raid-Gate→partner/recruitment-message) plus refresh_partner_score_cache; Messaging ggf. hinter 6g, Daten-Seiteneffekte (delete/persist/schedule) sofort.
- **[HOCH · missing · bestätigt]** Externe-Recruitment-Blacklist-Maschinerie komplett abwesend (kein Store, kein Scheduler, keine due-Loops)
  - Python: `bot/raid/services/raid_blacklist.py:240-456 schedule_external_recruitment_blacklist_pending / delete_external_recruitment_blacklist_pending / process_due_external_recruitment_blacklist_pending / schedule_external_target_ban_check / process_due_external_target_ban_checks / reschedule_external_target_ban_check_pending; record_confirmed_external_recruitment_raid (twitch_confirmed_external_recruitment_raids)`
  - Rust: `—`
  - Wirkung: Der gesamte Auto-Schutz gegen wiederholte externe Recruitment-Raids ist weg: bestätigte externe Raids werden nirgends gezählt, ab Schwelle wird kein Ziel auf die Raid-Blacklist gehoben und kein Bot-Ban-Check eingeplant. Spam-/Bot-Raider werden nicht mehr automatisch geblockt.
  - Verifikation: Am Code verifiziert. Python bot/raid/services/raid_blacklist.py hat schedule_external_recruitment_blacklist_pending (Z.240), delete_... (Z.283), process_due_external_recruitment_blacklist_pending (Z.296), schedule_external_target_ban_check (Z.328), reschedule_... (Z.367), process_due_external_target_ban_checks (Z.387) plus record_confirmed_external_recruitment_raid + Tabelle twitch_confirmed_external_recruitment_raids. Grep über gesamtes rust/ (inkl. Tests) auf external_recruitment_blacklist_pen
  - Fix: twitch_confirmed_external_recruitment_raids-Insert + die pending-Tabellen + die zwei due-Verarbeitungs-Loops (recruitment_blacklist_pending, target_ban_checks) nativ portieren und in den Maintenance-Loop einhängen.
- **[MITTEL · divergence · unverif.]** Re-Auth-Partner-Mirror nur bei vorhandener Discord-ID — Login-basierte Dashboard-Re-Auth lässt Partner-Status ungespiegelt
  - Python: `bot/raid/auth.py:1327-1357 save_auth (bei activate_raid_features UNBEDINGT set_partner_raid_bot_enabled(True) + manual_partner_opt_out=0 + raid_bot_enabled=1)`
  - Rust: `rust/bin/tb-bot/src/raid_oauth_impl.rs:1008-1056 oauth_callback (Re-Auth-Zweig (Some(setup),true) läuft sync_partner_state_after_auth NUR wenn state_discord_user_id vorhanden; sonst (None,true)/kein-discord → no-op); rust/crates/tb-raid/src/auth_writer.rs:13-15 (Partner-Aktivierung bewusst ausgelagert)`
  - Wirkung: Re-autorisiert ein bestehender Partner über die login-basierte Dashboard-Flow (kein discord_user_id im OAuth-State), wird manual_partner_opt_out/raid_bot_enabled/technical_pause_reason NICHT zurückgesetzt — sein Partner-Eintrag bleibt evtl. auf pausiert/deaktiviert, obwohl die Auth frisch ist.
  - Fix: Im Re-Auth-Zweig sync_partner_state_after_auth (bzw. den Partner-Reaktivierungs-Write) unabhängig von state_discord_user_id ausführen, sobald had_existing_auth && activate_raid_features — discord_id nur als optionaler Zusatz.
- **[MITTEL · missing · unverif.]** Proaktiver Token-Refresh-Sweep (refresh_all_tokens, 2h-Schwelle) nicht portiert — Tokens werden nur lazy beim Raid erneuert
  - Python: `bot/raid/auth.py:1007 refresh_all_tokens (refresht alle raid_enabled-Tokens mit <7200s Restgültigkeit); bot/raid/bot.py:282-298 (Maintenance-Loop alle 30 min)`
  - Rust: `—`
  - Wirkung: Tokens von Streamern, die selten Raids auslösen, werden nie proaktiv erneuert. Läuft das Access-Token ab und wird zwischenzeitlich kein Raid versucht, kann das Twitch-Refresh-Fenster (Rotation) verstreichen → unnötige invalid_grant-Blacklists statt rechtzeitiger Erneuerung.
  - Fix: Periodischen Tokio-Loop (z.B. 30 min) in main.rs ergänzen, der alle WHERE raid_enabled IS TRUE AND needs_reauth IS NOT TRUE-Zeilen mit token_expires_at < now+2h über refresh_and_store erneuert (Cooldown/Blacklist-Check wie Python).
- **[MITTEL · missing · unverif.]** Grace-Period-Verarbeitung (check_grace_periods) fehlt komplett — Token-Error-Streamer durchlaufen keinen Reminder-/Rollen-Lifecycle
  - Python: `bot/api/token_error_handler.py check_grace_periods (stündlich aus bot/raid/bot.py:306-308): Reminder-DM senden, bei abgelaufenem grace_expires_at Streamer-Rolle entfernen + role_removed/reminder_sent setzen`
  - Rust: `—`
  - Wirkung: Streamer mit dauerhaftem Token-Fehler bekommen weder die geplante Reminder-DM noch wird nach Ablauf der 7-Tage-Gnadenfrist die Streamer-Rolle entzogen. Der gesamte Lifecycle (reminder_sent→role_removed) bleibt eingefroren; grace_expires_at-Werte veralten ungenutzt.
  - Fix: Stündlichen Loop portieren, der twitch_token_blacklist nach abgelaufenem grace_expires_at scannt, Reminder einmalig sendet (reminder_sent) und bei Ablauf Rolle entzieht (role_removed) — analog check_grace_periods.
- **[MITTEL · missing · unverif.]** Discord-Token-Error-Benachrichtigung (notify_token_error / _disable_raid_bot-Notify) nicht portiert
  - Python: `bot/raid/auth.py:987-994 (asyncio notify_token_error bei invalid_grant); bot/api/token_error_handler.py:743/754 add_to_blacklist→_disable_raid_bot (Discord-Notify ab count>=3)`
  - Rust: `rust/crates/tb-raid/src/token_blacklist.rs:21-23 (Doc: Partner-Mirror + Discord-Hinweis 'bleibt hier bewusst offen')`
  - Wirkung: Bei Token-Widerruf erhält der Streamer keine Discord-Benachrichtigung (Re-Auth-Aufforderung), und der Partner-Datensatz wird nicht auf technical_pause_reason='token_error'/raid_bot_enabled=0 gespiegelt. Das Dashboard/Analytics zeigt den token_error-Zustand evtl. inkonsistent.
  - Fix: Im invalid_grant-Pfad des Refreshers (bzw. in add_to_blacklist) den DmNotifier-Äquivalent über den Master-Broker auslösen und den Partner-Mirror (set_partner_raid_bot_enabled(false)+technical_pause_reason mit den Python-Guards) nachziehen.
- **[MITTEL · proxied · unverif.]** POST /raid/requirements liefert nativen 503-Stub statt DM zu senden (nicht proxied)
  - Python: `bot/raid/mixin.py/views _raid_requirements → generate_requirements_dm_embed + Discord-DM`
  - Rust: `rust/bin/tb-bot/src/raid_oauth_impl.rs:741-747 requirements (gibt hart RaidOAuthError::Upstream/503 zurück)`
  - Wirkung: Das Raid-Onboarding-Requirements-DM (Erklärung der nötigen Schritte) wird über die interne API nicht versendet; der Aufrufer bekommt 503. Mod/Onboarding-Flow degradiert.
  - Fix: Entweder die Route wirklich auf den Python-Legacy-Proxy mappen (8779) bis nativ portiert, oder den Requirements-DM-Pfad nativ über den Discord-Broker bauen.
- **[NIEDRIG · divergence · unverif.]** Auto-Raid-Eligibility prüft needs_reauth nicht — Massen-Reauth-Flag (snapshot_and_flag_reauth) wird beim Auto-Raid ignoriert
  - Python: `bot/raid/mixin.py:118-129 (Auto-Raid skip wenn _is_fully_authed false = needs_reauth=1); bot/raid/auth.py:1168-1183 snapshot_and_flag_reauth (setzt needs_reauth=TRUE OHNE raid_enabled zu ändern)`
  - Rust: `rust/crates/tb-raid/src/offline_eligibility.rs:80-96 load (liest nur raid_enabled, kein needs_reauth); rust/bin/tb-bot/src/auto_raid.rs:240-250`
  - Wirkung: Nach einem Massen-Reauth-Flag (Scope-Migration) feuert der Rust-Auto-Raid trotzdem die Pipeline an; der Token-Fetch scheitert dann zwar wegen needs_reauth (get_valid_token→None) und der Raid wird als failed protokolliert, statt wie Python sauber vorab übersprungen zu werden — verschwendete Arbeit + falsche failed-History-Zeilen, aber kein falscher Raid.
  - Fix: In offline_eligibility.load zusätzlich needs_reauth selektieren und raid_auth_enabled = raid_enabled AND NOT needs_reauth setzen (skip_reason 'needs_reauth').
- **[NIEDRIG · missing · unverif.]** Periodische Pending-Raid-/State-/Suppression-Cleanups nicht wired (cleanup_stale nur in Tests)
  - Python: `bot/raid/bot.py:276-348 (cleanup_states 30min, _cleanup_stale_pending_raids/_cleanup_recent_raid_arrivals/_cleanup_stale_raid_readiness_states 2min, _cleanup_expired_manual_raid_suppressions, cleanup_old_entries 3.5h)`
  - Rust: `rust/crates/tb-raid/src/pending_raids.rs:226 cleanup_stale (nur in Test Z.402 aufgerufen); rust/crates/tb-raid/src/state_store.rs:176 cleanup_expired (nirgends periodisch); rust/crates/tb-raid/src/manual_suppression.rs:59 cleanup_expired (nirgends periodisch)`
  - Wirkung: Nie-bestätigte Pending-Raids leaken in der In-Memory-Map (potenziell stale Matches), abgelaufene oauth_state_tokens und alte token_blacklist-Einträge sammeln sich als tote DB-Zeilen. Suppression self-expired beim Read (harmlos), Pending/State/Blacklist aber nicht.
  - Fix: Maintenance-Loop ergänzen: PendingRaidStore.cleanup_stale(300s) im 2-min-Tick, StateStore.cleanup_expired (30 min) und eine Blacklist-Alteintrags-Bereinigung (30 Tage).
- **[NIEDRIG · divergence · unverif.]** PendingRaid-Struct ohne target_stream_data — Score-Snapshot bei Confirm geht verloren
  - Python: `bot/raid/pending_raids.py:99-112 PendingRaid.target_stream_data; raid_arrival_runtime.py:388 score_snapshot=target_stream_data.get('_partner_score')`
  - Rust: `rust/crates/tb-raid/src/pending_raids.rs:59-85 PendingRaid (kein target_stream_data-Feld)`
  - Wirkung: Der bei Raid-Registrierung gecachte Partner-Score-Snapshot wird im Rust-Confirm-Pfad nicht durchgereicht. Rust löst den Score stattdessen frisch über confirm_resolver auf — meist gleichwertig, aber bei zwischenzeitlicher Score-Änderung weicht der getrackte Wert vom Python-Verhalten ab.
  - Fix: Falls Score-Snapshot-Parität gewünscht: target_stream_data (oder zumindest _partner_score: Option<f64>) ins Rust-PendingRaid aufnehmen und im ConfirmContext bevorzugt verwenden.
- **[NIEDRIG · divergence · unverif.]** State-Meta-Parsing ohne Legacy-Single-Token-Fallback und ohne Profil-Normalisierung auf 'base'
  - Python: `bot/raid/auth.py:155-171 _parse_state_meta (bei kaputtem JSON → parse_scope_profile_meta(raw) Legacy-Format; normalize_scope_profile defaultet auf BASE_SCOPE_PROFILE)`
  - Rust: `rust/crates/tb-raid/src/state_store.rs:63-80 from_row (scope_profile via get('scope_profile').unwrap_or_default() = '', kein Legacy-Format-Fallback)`
  - Wirkung: Sehr gering: scopes_for_profile('') normalisiert downstream ohnehin zu base. Ein aus altem Python stammender State im Legacy-Single-Token-Format würde sein Profil verlieren (→ base statt dashboard_reauth). In der Praxis selten, da neue States kompaktes JSON schreiben.
  - Fix: from_row: scope_profile leer → normalize zu 'base'; optional den Legacy-'scope_profile:<v>'-Fallback bei nicht-JSON-Meta nachziehen.

### analytics-overview-perf

Der Port dieser Einheit ist gemischt: Performance-Endpoints (monthly/weekly/hourly/calendar), retention-curve, loyalty-curve, follower-funnel, viewer-timeline und die drei viewer-Endpoints sind nativ in Rust vorhanden und die SQL-Aggregationen weitgehend strukturgleich. Aber es gibt einen kritischen Befund: der native /twitch/api/v2/overview-Handler liefert nur ein winziges 6-Feld-Summary (avgViewers, peakViewers, totalHoursWatched, totalAirtime, followersDelta, totalSessions) und lässt praktisch den gesamten Python-Payload weg — scores/Health, sessions-Liste, findings, actions, correlations, network, retention10m, uniqueChatters, followersGained/-PerHour, alle Trend-Indikatoren, categoryRank und das Free-Gate (window/windowLimited "Tagesform"). Da der Handler nativ registriert ist, schattet er den Python-Proxy aus, das v2-Dashboard bekommt also dauerhaft die Rumpf-Antwort. Zweiter schwerer Cluster: das Paywall-Gate (_require_extended_plan) fehlt in Rust bei retention-curve, loyalty-curve und allen drei viewer-Endpoints sowie viewer-timeline — nur follower-funnel nutzt extended_gate. Dazu kommen SQLX-Decode-Fallen (AVG(peak_viewers)/AVG(duration/3600) als numeric, f64-Read → still 0.0), die NULL-erhaltende Bot-Klausel die in Rust zu plain NOT IN/!= ALL degradiert (anonyme chatter_id-Zeilen + Streamer-Self + dynamische Bot-Logins werden falsch gefiltert), sowie mehrere bereits im 13.6.-Audit dokumentierte, weiterhin offene Detail-Divergenzen (ad_break-Typ, confidence-Schwelle, sort=first_seen). Einige 13.6.-Befunde (avg_watch curve[0], sessions_used) sind inzwischen gefixt.

- **[KRITISCH · divergence · bestätigt]** Overview-Handler liefert nur Rumpf-Summary statt vollem Python-Payload
  - Python: `bot/analytics/api_overview.py:_get_overview_data_sync (1118-1152)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/overview.rs:OverviewData/OverviewSummary (35-106)`
  - Wirkung: Das v2-Dashboard (/analyse) bekommt für die Overview-Seite eine fast leere Antwort: Health-Scores, Session-Liste, Insights/Actions, Korrelationen, Netzwerk-Kachel, Retention-/Chatter-KPIs, alle Trend-Pfeile und der Kategorie-Rang fehlen. Frontend zeigt 0/leer statt der eigentlichen Übersicht.
  - Verifikation: Selbst verifiziert. Rust OverviewData (overview.rs:36-56) hat nur streamer, days und summary mit exakt 6 Feldern (avgViewers, peakViewers, totalHoursWatched, totalAirtime, followersDelta, totalSessions; Serialisierung overview.rs:95-106). Python _get_overview_data_sync (api_overview.py:1118-1152) baut zusätzlich scores, ein summary mit 15 Feldern inkl. followersGained/followersPerHour/retention10m/uniqueChatters/streamCount + avgViewersTrend/followersTrend/retentionTrend, sowie sessions, finding
  - Fix: Entweder den vollen Payload in overview.rs nachbauen (overview_metrics um retention/chatter/scores/network/correlations/sessions/trends erweitern, prev-period-Trends, _calculate_health_scores portieren) oder den nativen /overview-Handler vorerst aus lib.rs:66 entfernen und über den Proxy an Python 8765 laufen lassen, bis er feature-komplett ist.
- **[HOCH · divergence · bestätigt]** Free-Gate 'Tagesform' (window/windowLimited) im Overview nicht portiert
  - Python: `bot/analytics/api_overview.py:_api_v2_overview (970-976), _window_since_dates (996)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/overview.rs:overview_handler (59-106)`
  - Wirkung: Free-User würden in Rust das volle Zeitfenster statt nur den letzten Stream sehen (Paywall-/Freemium-Bypass) und die UI bekommt windowLimited nicht, kann die Free-Schranke also nicht anzeigen.
  - Verifikation: Selbst verifiziert. Python _api_v2_overview liest window=self._resolve_read_window(request) (api_overview.py:970) und setzt data['window']=window + data['windowLimited']=(window=='last_stream') (975-976); _window_since_dates (996) staucht bei last_stream das Vorperioden-Fenster (Trend-Suppression). _resolve_read_window (api_v2.py:670-697) gibt für Free (nur analytics.daily) 'last_stream' zurück, für analytics.basic/extended sowie Admin/Localhost 'full'. Rust overview_handler kennt nur days.clamp
  - Fix: Read-Window (resolve_read_window) und _window_since_dates nach Rust portieren, window+windowLimited ins JSON aufnehmen und prev-Fenster bei last_stream leeren — Teil des Overview-Vollausbaus.
- **[HOCH · divergence · bestätigt]** Paywall-Gate _require_extended_plan fehlt bei retention-curve, loyalty-curve, viewer-timeline und allen viewer-Endpoints
  - Python: `bot/analytics/api_performance.py:1751; api_audience.py:1586; api_viewer_timeline.py:379,424; api_viewers.py:924,966,993`
  - Rust: `retention_curve.rs:require_auth(47); loyalty_curve.rs:29; viewer_timeline.rs:108,359; viewers.rs:353,599,921`
  - Wirkung: Partner ohne Extended-Plan/Trial bekommen erweiterte Analytics (Retention-Kurve, Loyalitäts-Kurve, Viewer-Verzeichnis/Detail/Segmente, Viewer-Timeline) frei statt 403 — Umsatzleck und Bruch des Freemium-Modells.
  - Verifikation: Selbst verifiziert. Python ruft _require_extended_plan vor retention_curve (api_performance.py:1751), loyalty_curve (api_audience.py:1586), viewer_timeline (api_viewer_timeline.py:379+424) und viewer_directory/detail/segments (api_viewers.py:924/966/993); die Funktion (api_v2.py:638-668) wirft 403 plan_required ohne Extended-Entitlement, mit Admin/Localhost-Bypass. In Rust prüfen retention_curve.rs:33-39 (require_auth), loyalty_curve.rs:29, viewer_timeline.rs:108+359 und viewers.rs:353/599/921 N
  - Fix: In allen genannten Handlern direkt nach Streamer-Parsing crate::auth::extended_gate(&pool,&auth).await einsetzen (analog follower_funnel.rs:53) und bei Some(resp) zurückgeben.
- **[HOCH · regression · bestätigt]** SQLX-Decode: AVG(peak_viewers)/AVG(duration/3600) als numeric mit f64-Read → still 0.0
  - Python: `bot/analytics/api_performance.py:_load_weekly_stats_payload (180,178,198,200), _load_hourly_heatmap_payload (75,90)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/performance.rs:163,161,195,193,227,251`
  - Wirkung: Im Wochentags-Chart sind avgPeak und avgHours konstant 0, im Stunden-Heatmap avgPeak konstant 0 — die Heatmap/Wochenanalyse zeigt falsche (0) Spitzen-/Stundenwerte. Python liefert dort echte Werte (float()).
  - Verifikation: Selbst verifiziert inkl. Typ- und Feature-Beleg. weekly_stats: AVG(s.duration_seconds/3600.0) AS avg_hours (performance.rs:161) und AVG(s.peak_viewers) AS avg_peak (163), gelesen try_get::<f64>('avg_hours') (193) bzw. 'avg_peak' (195). hourly_heatmap: AVG(s.peak_viewers) AS avg_peak (227), gelesen try_get::<f64>('avg_peak') (251) — alle ohne ::float8-Cast. peak_viewers ist INTEGER/BIGINT, duration_seconds BIGINT (belegt: Migrations- und Audit-Doku, Vorlauf-Audit 2026-06-13 nennt peak_viewers INT
  - Fix: AVG(s.peak_viewers)::float8 und AVG(s.duration_seconds/3600.0)::float8 casten (oder als numeric/BigDecimal lesen). avgViewers (avg_viewers ist DOUBLE PRECISION) ist nicht betroffen.
- **[MITTEL · regression · unverif.]** Bot-Exclusion-Klausel degradiert von NULL-erhaltend zu NOT IN / != ALL → anonyme chatter_id-Zeilen fallen weg
  - Python: `bot/core/chat_bots.py:build_known_chat_bot_not_in_clause (58-62)`
  - Rust: `viewer_timeline.rs:bot_not_in_sql(74); loyalty_curve.rs:46; follower_funnel.rs:118; viewers.rs:307,417,444,629,995,1044`
  - Wirkung: Viewer, die nur per chatter_id (ohne login) getrackt sind, werden in Rust aus Funnel-, Loyalty-, Timeline- und Viewer-Aggregaten herausgefiltert; Python zählt sie mit. Zahlen (unique_chatters, total_chatters, Funnel-Viewers) fallen in Rust niedriger aus.
  - Fix: Die Rust-Klauseln auf das NULL-erhaltende Muster umstellen: ((col) IS NULL OR (col)='' OR LOWER(col) <> ALL($n)) bzw. NOT IN, exakt wie build_known_chat_bot_not_in_clause.
- **[MITTEL · divergence · unverif.]** Viewer-Endpoints schließen Streamer-Self und dynamische Bot-Logins nicht aus
  - Python: `bot/analytics/api_viewers.py:_collect_viewer_exclusion_logins (32-52), _build_viewer_identity_not_in_clause (55-65)`
  - Rust: `viewers.rs:fetch_window_viewer_rows (296-312), 409, 617; viewer_timeline.rs:extra_excluded (162)`
  - Wirkung: Im Viewer-Verzeichnis/Segmenten erscheint der Streamer selbst und der eigene Moderations-/Chat-Bot als 'Viewer'; totalViewers, exclusiveViewers, avgSessionsPerViewer und Segment-Zahlen weichen ab. Bekannt aus Audit 2026-06-13 (dash-viewers-5), weiterhin offen.
  - Fix: Mindestens streamer-Login in viewers.rs an alle Aggregations-Queries binden (wie viewer_timeline.rs). Für volle Parität die dynamischen Bot-Logins beim Start auflösen und als ergänzbare Exclusion-Liste durchreichen.
- **[MITTEL · divergence · unverif.]** retention-curve: drop_events.type immer 'unknown' — ad_break-Korrelation fehlt
  - Python: `bot/analytics/api_performance.py:_load_retention_curve_payload_sync (1696-1730)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/retention_curve.rs:139`
  - Wirkung: Drop-Events durch Werbeunterbrechungen werden nicht als ad_break markiert; das Frontend/Coaching kann werbungsbedingte Drops nicht von echten Retention-Einbrüchen unterscheiden.
  - Fix: twitch_ad_break_events für die recent_sessions laden, Minuten-Offset FLOOR(EXTRACT(EPOCH FROM (a.started_at-s.started_at))/60) in ein HashSet sammeln und type entsprechend setzen.
- **[NIEDRIG · divergence · unverif.]** retention-curve: viewer_count IS NOT NULL filtert NULL-Ticks weg statt als 0 zu zählen
  - Python: `bot/analytics/api_performance.py:_load_retention_curve_payload_sync (1662-1665)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/retention_curve.rs:76`
  - Wirkung: Bei vorhandenen NULL-viewer_count-Ticks weichen sample_count und die Perzentile (p25/median/p75) zwischen Python und Rust ab; Rust ignoriert die NULL-Minuten statt sie als 0-Retention zu werten.
  - Fix: In der normalized-CTE COALESCE(sv.viewer_count,0) verwenden statt die Zeile per IS NOT NULL zu verwerfen, falls 1:1-Parität gewünscht ist.
- **[NIEDRIG · divergence · unverif.]** follower-funnel: confidence-'high'-Schwelle weicht bei kleinem session_count ab
  - Python: `bot/analytics/api_audience.py:715`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/follower_funnel.rs:207`
  - Wirkung: Bei wenigen Sessions/gültigen Followern zeigt das Funnel-Dashboard 'high confidence' obwohl Python 'medium' sagt — irreführende Datenqualitäts-Kennzeichnung.
  - Fix: Z.207 angleichen: follower_valid_samples >= (session_count as f64*0.6).floor().max(3.0) as i64.
- **[NIEDRIG · divergence · unverif.]** follower-funnel: dataQuality.estimatedFields fehlt im Rust-Output
  - Python: `bot/analytics/api_audience.py:748`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/follower_funnel.rs:230-237`
  - Wirkung: Die UI kann avgTimeToFollow und followersBySource nicht als 'geschätzt' kennzeichnen; Nutzer hält abgeleitete Werte für gemessen.
  - Fix: estimatedFields: ['avgTimeToFollow','followersBySource'] in den dataQuality-JSON-Block aufnehmen.
- **[NIEDRIG · divergence · unverif.]** viewer-directory: sort=first_seen degradiert still zu Sessions-Sortierung
  - Python: `bot/analytics/api_viewers.py:380-386`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/viewers.rs:363-367,544-550`
  - Wirkung: Wählt der Nutzer 'Sortierung nach Erstkontakt', liefert Rust die Sessions-Reihenfolge statt nach Datum — falsche, aber plausibel aussehende Sortierung.
  - Fix: In key_of einen firstSeen-Pfad ergänzen, der die DateTime/RFC3339-Werte vergleicht (None ans Ende), order_desc-Semantik wie bei den i64-Zweigen.
- **[NIEDRIG · divergence · unverif.]** loyalty-curve: leeres Ergebnis liefert zusätzliches window-Feld
  - Python: `bot/analytics/api_audience.py:1611`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/loyalty_curve.rs:63`
  - Wirkung: Minimaler Schema-Unterschied im Leerfall; nur relevant falls ein Client strikt auf Feldgleichheit prüft.
  - Fix: window aus dem Leerfall-JSON entfernen, um 1:1-Parität herzustellen.
- **[NIEDRIG · divergence · unverif.]** Performance: months→since_date trunkiert statt zu runden
  - Python: `bot/analytics/api_performance.py:_load_monthly_stats_payload (126)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/performance.rs:78`
  - Wirkung: In Randfällen liegt die since-Grenze einen Tag früher als in Python; eine Session genau an der Grenze kann ein-/ausgeschlossen werden.
  - Fix: (months as f64*30.44).round() as i64 verwenden.
- **[NIEDRIG · proxied · unverif.]** Legacy-v1-Backend (get_streamer_analytics_data/_overview/_session_detail/_comprehensive) nicht nativ in Rust
  - Python: `bot/analytics/backend.py:18,573,682,791; backend_extended.py:22`
  - Rust: `—`
  - Wirkung: Die v1-Dashboard-Übersicht läuft weiter über Python (Migration unvollständig, aber funktional). Solange v2 die produktive Oberfläche ist, geringe Auswirkung.
  - Fix: Bewusst als Legacy/proxied dokumentieren; nur portieren, falls /twitch/dashboard nativ bedient werden soll.

### analytics-audience-insights

Die Read-only-GET-Analytics meines Scopes sind zum großen Teil nativ portiert und im Kern verhaltensgleich (viewer-overlap, viewer-profiles, audience-sharing, audience-demographics inkl. Engagement/Peak-Hours, category-leaderboard, category-timings, retention-/loyalty-Kurve im Kern). Die bekannten SQLX-Typ-Drift-Befunde aus dem 13.6.-Audit (avg/peak viewer_count, SUM(duration), SUM(follower_delta), SUM(viewer_count) für viewerMinutes) sind inzwischen GEFIXT (explizite ::float8-Casts). ABER: Die gesamte Deep-Chat-/Monetarisierungs-/Social-Graph-Ebene (api_chat_deep, insights_monetization_loader, chat_social_graph_loader, chat-analytics aus api_insights, die eigenständige watch-time-distribution) ist NICHT nativ — sie läuft ausschließlich über den Strangler-Proxy auf Python 8765 (chat-analytics, chat-hype-timeline, chat-content-analysis, chat-deep-minimax, chat-social-graph, monetization, watch-time-distribution, category-activity-series). Ohne gesetzte TB_DASHBOARD_LEGACY_FALLBACK_URL liefern diese 404. Zusätzlich gibt es eine konsistente Klasse Auth-Gate-Drift: category-comparison ist in Rust ZU streng (extended_gate, obwohl Python nur v2-Auth verlangt → 403-Regression), während title-performance, retention-curve, loyalty-curve und lurker-analysis das in Python vorhandene Extended-Plan-Gate NICHT haben (Daten-Leak an Nicht-Extended-Nutzer). Dazu mehrere kleinere Feld-/Verhaltens-Divergenzen (gedroppte Felder estimatedFields/peerBenchmark/drop-type ad_break, fehlender last_seen-Backfill in audience-insights, geänderte Confidence-Schwelle im Funnel, NULLS-Reihenfolge im Ranking, avg_percentile-Default).

- **[HOCH · proxied · bestätigt]** watch-time-distribution-Endpoint nicht nativ (nur proxied), zugleich fehlt der last_seen-Backfill im nativen audience-insights
  - Python: `bot/analytics/api_audience.py:422 _api_v2_watch_time_distribution; Backfill api_audience.py:211 _backfill_last_seen_from_messages, aufgerufen in audience-insights api_audience.py:916`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/audience.rs:488-562 calc_watch_distribution (kein Backfill); kein Handler für /watch-time-distribution`
  - Wirkung: Ohne Backfill sind last_seen_at vieler Chatter NULL/veraltet → kürzere/fehlende watch_minutes-Samples → niedrigere Coverage und avgWatchTime → watchTimeMethod kippt häufiger auf 'low_coverage'/'no_data' und watchTimeChange wird unterdrückt. Der native audience-insights weicht damit systematisch vom proxied watch-time-distribution ab.
  - Verifikation: Beide Teile am Code bestaetigt. (1) Keine Route '/twitch/api/v2/watch-time-distribution' in lib.rs-Routenliste → proxied. (2) Datenkorrektheits-Divergenz im NATIVEN audience-insights: Python api_audience.py:916 ruft self._backfill_last_seen_from_messages(conn, current_ids+prev_ids) VOR _calc_watch_distribution; der Backfill (Def Z.211ff) setzt last_seen_at = MAX(message_ts) aus twitch_chat_messages. Rust audience.rs:700-701 ruft calc_watch_distribution direkt ohne Backfill; das Watch-SQL (Z.513-
  - Fix: Backfill _backfill_last_seen_from_messages vor calc_watch_distribution in audience-insights nativ nachbauen (UPDATE twitch_session_chatters.last_seen_at = MAX(message_ts) je session+chatter); watch-time-distribution-Route nativ ergänzen.
- **[HOCH · regression · bestätigt]** category-comparison: Rust erzwingt Extended-Plan-Gate, Python nicht → 403-Regression für Nicht-Extended-Partner
  - Python: `bot/analytics/api_performance.py:1043-1045 _api_v2_category_comparison (nur _require_v2_auth, KEIN _require_extended_plan)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/category_comparison.rs:68 crate::auth::extended_gate(&pool,&auth)`
  - Wirkung: Authentifizierte Streamer ohne Extended-Plan, die in Python die Kategorie-Vergleichsseite sehen konnten, bekommen in Rust 403 → Feature verschwindet für sie. Wahrscheinlich Überkorrektur aus dem dash-audience-4-Fix.
  - Verifikation: Bestaetigt. Python api_performance.py:1045 ruft im _api_v2_category_comparison-Body NUR self._require_v2_auth(request); grep des Bodys (1043-1110) zeigt kein _require_extended_plan. Rust category_comparison.rs:68 ruft crate::auth::extended_gate(&pool,&auth), das fuer DashboardAuthLevel::Partner ohne Entitlement 403 plan_required liefert (auth/mod.rs:122-145). Echte Verhaltens-Regression: nicht-Extended-Partner bekommen in Rust 403, in Python 200. high korrekt.
  - Fix: extended_gate in category_comparison.rs durch reinen None-Auth-Check ersetzen (analog rankings.rs require_auth), passend zu Python.
- **[HOCH · divergence · bestätigt]** title-performance: Extended-Plan-Gate fehlt in Rust (Python hat es) → Daten-Leak
  - Python: `bot/analytics/api_performance.py:714-717 _api_v2_title_performance (_require_v2_auth + _require_extended_plan)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/title_performance.rs:68 require_auth (nur DashboardAuthLevel::None)`
  - Wirkung: Jeder authentifizierte Nutzer (auch ohne Extended/Trial) erhält die Titel-Performance-Daten, die hinter dem Paywall-Gate liegen sollten.
  - Verifikation: Bestaetigt. Python api_performance.py:716-717 ruft _require_v2_auth UND _require_extended_plan. Rust title_performance.rs:33-39 require_auth prueft nur matches!(auth, None) (401), kein extended_gate; Aufruf Z.68. Folge: Extended-Analytics-Titel-Performance wird an nicht-zahlende authentifizierte Partner ausgeliefert. _require_extended_plan ist ein echtes 403-Gate (api_v2.py:638ff), kein No-op. Entitlement-Leak, high korrekt.
  - Fix: In title_performance_handler crate::auth::extended_gate(&pool,&auth) wie in category_leaderboard.rs:56 einsetzen.
- **[HOCH · divergence · bestätigt]** retention-curve: Extended-Plan-Gate fehlt in Rust (Python hat es)
  - Python: `bot/analytics/api_performance.py:1748-1751 _api_v2_retention_curve (_require_extended_plan)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/retention_curve.rs:47 require_auth (nur None-Check)`
  - Wirkung: Retention-Kurve (Paywall-Feature) ist für jeden authentifizierten Nutzer erreichbar.
  - Verifikation: Bestaetigt. Python api_performance.py:1751 self._require_extended_plan im _api_v2_retention_curve. Rust retention_curve.rs:33-39 require_auth nur None-Check (401), kein extended_gate (Aufruf Z.47). Gleiches Entitlement-Leak-Muster wie title-performance. high korrekt.
  - Fix: extended_gate in retention_curve_handler ergänzen.
- **[HOCH · divergence · bestätigt]** loyalty-curve: Extended-Plan-Gate fehlt in Rust (Python hat es)
  - Python: `bot/analytics/api_audience.py:1583-1586 _api_v2_loyalty_curve (_require_extended_plan)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/loyalty_curve.rs:29 (nur DashboardAuthLevel::None)`
  - Wirkung: Loyalty/Churn-Kurve (Paywall-Feature) ist für jeden authentifizierten Nutzer erreichbar.
  - Verifikation: Bestaetigt. Python api_audience.py:1586 self._require_extended_plan im _api_v2_loyalty_curve. Rust loyalty_curve.rs:29 prueft inline nur matches!(auth, DashboardAuthLevel::None) → 401, kein extended_gate. Loyalty/Churn-Verteilung leakt an nicht-Extended-Partner. high korrekt.
  - Fix: extended_gate in loyalty_curve_handler ergänzen.
- **[HOCH · divergence · bestätigt]** lurker-analysis: Extended-Plan-Gate fehlt in Rust + Fehlerverhalten 500 statt 200/honest-empty
  - Python: `bot/analytics/api_overview.py:1759-1760 (_require_extended_plan) und 1778-1786 (Exception → 200 mit dataAvailable:false)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/lurker_analysis.rs:44 (nur None-Check) und :115 (DB-Fehler → 500 internal_error)`
  - Wirkung: (1) Paywall-Feature ohne Gate erreichbar. (2) Bei DB-Fehler liefert Rust 500 statt des in Python bewusst gewählten 200+honest-empty — Frontend, das dataAvailable auswertet, bricht.
  - Verifikation: Beide Teile bestaetigt. (1) Plan-Gate: Python api_overview.py:1760 self._require_extended_plan; Rust lurker_analysis.rs:44 nur matches!(auth, None) → 401, kein extended_gate → Entitlement-Leak (das treibt high). (2) Fehlerverhalten-Divergenz: Python:1779-1786 faengt jede Exception und gibt web.json_response({dataAvailable:false,...}, status=200); Rust:113-116 gibt bei agg-Query-Fehler StatusCode::INTERNAL_SERVER_ERROR (500). Realer Unterschied, aber fuer sich nur low/medium (Frontend-Robustheit)
  - Fix: extended_gate ergänzen; DB-Fehler im agg-Pfad auf 200 mit {dataAvailable:false,message} mappen statt 500.
- **[MITTEL · proxied · umgestuft]** Deep-Chat-Analyse komplett nur proxied: chat-analytics, chat-hype-timeline, chat-content-analysis, chat-deep-minimax, chat-social-graph nicht nativ
  - Python: `bot/analytics/api_chat_deep.py:676,943,976,1005 (+ api_insights.py:602 chat-analytics); registriert api_overview.py:57,105,106,107,108`
  - Rust: `rust/crates/tb-dashboard-api/src/lib.rs (keine Route); rust/bin/tb-dashboard/src/main.rs:41-53 (Fallback-Proxy)`
  - Wirkung: Die gesamte Chat-Tiefenanalyse (Hype-Timeline, Inhalts-/Sentiment-Analyse, MiniMax-Deep-Insights, Social-Graph) läuft weiter auf Python; Migration unvollständig. Ohne gesetzte Fallback-URL liefern diese Routen 404 statt Daten.
  - Verifikation: Bestaetigt am Code: grep ueber rust/crates/tb-dashboard-api/src/lib.rs (build_authed_router) listet KEINE der Routen chat-analytics/chat-hype-timeline/chat-content-analysis/chat-deep-minimax/chat-social-graph; vollstaendige v2-Routenliste enthaelt sie nicht. main.rs:41-55 haengt dashboard_fallback_handler nur an, wenn TB_DASHBOARD_LEGACY_FALLBACK_URL gesetzt ist (Proxy nach Python 8765). Faktisch korrekt 'nicht nativ'. Reklassiziert auf medium: per dokumentierter Strangler-Architektur laufen die
  - Fix: api_chat_deep-Loader (_load_chat_hype_timeline_payload, _load_chat_content_analysis_payload_sync, chat-social-graph, chat-analytics-Snapshot) nativ nach tb-dashboard-api portieren; bis dahin Fallback-URL zwingend gesetzt halten und Status als bewusst proxied in 05-cleanup-decisions.md dokumentieren.
- **[MITTEL · proxied · umgestuft]** chat-social-graph nicht nativ (chat_social_graph_loader proxied)
  - Python: `bot/analytics/chat_social_graph_loader.py:16 load_chat_social_graph_payload; api_chat_deep.py:976 _api_v2_chat_social_graph; api_overview.py:108`
  - Rust: `— (keine Rust-Entsprechung, Proxy)`
  - Wirkung: Social-Graph-Tab (Mention-Hubs, Top-Paare, Mention-Distribution) ist vollständig Python-abhängig.
  - Verifikation: Bestaetigt: keine Rust-Route '/twitch/api/v2/chat-social-graph' und kein Handler in rust/crates/; grep ueber alle Crates auf chat-social findet 0 Treffer (nur 03-http-contract.md erwaehnt es als zu-portierend). Das Mention-Graph-Modell aus chat_social_graph_loader.py existiert in Rust nicht. Teilmenge von Befund 1 (in dessen Routenliste enthalten) — strenggenommen Doppelzaehlung. Selbe Begruendung: laeuft via Strangler-Proxy weiter, daher medium statt high.
  - Fix: load_chat_social_graph_payload inkl. _MENTION_RE-Logik (3-25 Zeichen, @-Erkennung, self-mention-skip) und build_raw_chat_status nativ portieren.
- **[MITTEL · proxied · umgestuft]** Monetarisierung (insights_monetization_loader) nicht nativ — nur proxied
  - Python: `bot/analytics/insights_monetization_loader.py:14 load_monetization_payload; api_insights.py:990 _api_v2_monetization; api_overview.py:90`
  - Rust: `— (keine Rust-Route /twitch/api/v2/monetization)`
  - Wirkung: Monetarisierungs-Analyse (Ad-Viewer-Drop, beste Ad-Zeit, Recovery, Bits/Subs/Hype) läuft komplett auf Python.
  - Verifikation: Bestaetigt: keine native Route '/twitch/api/v2/monetization' in rust/crates/tb-dashboard-api. Achtung Falle: grep auf 'monetization' trifft tb-internal-api/src/handlers/stats_native.rs:743 (fetch_monetization) — das ist aber die /stats-Aggregat-Sektion des INTERNEN API (anderer Dienst/Port), NICHT die v2-Dashboard-Route. Das Ad-Break-Drop/Recovery/Hype-Train-Modell aus insights_monetization_loader.py ist im Dashboard-API nicht portiert; die v2-Route wird proxied. Reklassiziert auf medium: via Fa
  - Fix: load_monetization_payload portieren; bis dahin proxied dokumentieren.
- **[MITTEL · divergence · unverif.]** follower-funnel: Feld estimatedFields gedroppt + abweichende Confidence-Schwelle
  - Python: `bot/analytics/api_audience.py:748 ('estimatedFields':['avgTimeToFollow','followersBySource']) und :715 (>= max(3, int(session_count*0.6)))`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/follower_funnel.rs:230-237 (kein estimatedFields) und :207 (>= session_count.max(3)*3/5)`
  - Wirkung: UI kann geschätzte Felder nicht mehr als 'estimated' markieren; Confidence-Stufe (high/medium) kippt bei kleinen Session-Zahlen anders als in Python.
  - Fix: estimatedFields-Array in die dataQuality-JSON aufnehmen; Confidence-Schwelle exakt als max(3, (session_count*3/5)) bzw. max(3, floor(session_count*0.6)) berechnen statt max(session_count,3)*3/5.
- **[MITTEL · divergence · unverif.]** title-performance: peerBenchmark dauerhaft null statt Peer-Group-Median
  - Python: `bot/analytics/api_performance.py:701-707 (peer_benchmark = {avgViewers, retention10m} aus _get_peer_group_stats)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/title_performance.rs:127 ('peerBenchmark': null)`
  - Wirkung: Der Vergleich der eigenen Titel-Performance gegen die Tier-Peer-Gruppe fehlt komplett im Frontend.
  - Fix: _get_peer_group_stats-Logik (bereits in category_comparison.rs als peer_group repliziert) extrahieren und für peerBenchmark wiederverwenden.
- **[MITTEL · divergence · unverif.]** retention-curve: drop_events.type immer 'unknown' statt 'ad_break'-Klassifikation
  - Python: `bot/analytics/api_performance.py:1696-1731 (ad_times aus twitch_ad_break_events; type='ad_break' if minute in ad_times else 'unknown')`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/retention_curve.rs:139 ('type':'unknown')`
  - Wirkung: Retention-Einbrüche durch Werbung werden im UI nicht mehr als Werbe-bedingt gekennzeichnet — Streamer kann Ad-Drops nicht von organischen unterscheiden.
  - Fix: Ad-Break-Minuten der letzten 50 Sessions laden (wie Python) und drop_events.type entsprechend setzen.
- **[MITTEL · divergence · unverif.]** category-comparison: avg_percentile-Default bei leerer Liste 50 statt 0 (percentile_of empty=50)
  - Python: `bot/analytics/api_performance.py:994 (avg_percentile = ... if sorted_avgs else 0) vs api_insights.py:160-166 (_percentile_of empty=0.5)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/category_comparison.rs:30-36 (percentile_of empty→50), :248`
  - Wirkung: percentiles.avgViewers meldet 50 statt 0, wenn keine Kategorie-Avgs vorliegen — irreführende 'Median'-Anzeige bei leerer Datenbasis.
  - Fix: Für avg_percentile den empty-Default 0 spiegeln (separater Pfad oder percentile_of-Aufruf mit explizitem if sorted_avgs.is_empty(){0}else{...}).
- **[MITTEL · divergence · unverif.]** category-leaderboard: yourTier aus gefiltertem Leaderboard statt aus ungefiltertem Peer-Group-Avg
  - Python: `bot/analytics/api_performance.py:1238-1240 (your_tier = _get_peer_group_stats(...)['tier'], eigene ungefilterte Query + Session-Fallback)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/category_leaderboard.rs:175-198 (your_tier aus your_avg_opt = gefilterte Leaderboard-Row, sonst Session-Fallback)`
  - Wirkung: Bei exclude_external=1 oder gesetztem tier-Filter (oder wenn Kategorie-Avg != Session-Avg) weicht yourTier von Python ab.
  - Fix: yourTier wie Python aus einer eigenen ungefilterten Kategorie-Avg-Query (mit Session-Fallback) ableiten, nicht aus den gefilterten Leaderboard-Rows.
- **[MITTEL · proxied · unverif.]** category-activity-series nicht nativ — nur proxied
  - Python: `bot/analytics/api_overview.py:312 (Demo) + zugehöriger Produktiv-Handler; registriert über api_overview`
  - Rust: `— (keine Rust-Route /twitch/api/v2/category-activity-series)`
  - Wirkung: Kategorie-Aktivitäts-Zeitreihe ist Python-abhängig.
  - Fix: Handler nativ portieren oder als bewusst proxied dokumentieren.
- **[NIEDRIG · divergence · unverif.]** rankings: ORDER BY value DESC NULLS LAST statt Python-Default (NULLS FIRST)
  - Python: `bot/analytics/api_performance.py:773,787,800 (ORDER BY value DESC — Postgres-Default NULLS FIRST)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/rankings.rs:74,85,98,111,123,134 (ORDER BY value DESC NULLS LAST)`
  - Wirkung: Streamer mit NULL-Wert (z.B. ohne retention/growth-Daten) erscheinen in Python am Anfang der Top-N-Liste, in Rust am Ende — die Top-N-Zusammensetzung kann sich unterscheiden.
  - Fix: NULLS LAST entfernen, um Python 1:1 zu spiegeln (oder bewusst als Verbesserung in 05-cleanup-decisions.md festhalten).
- **[NIEDRIG · divergence · unverif.]** loyalty-curve: Empty-Response enthält zusätzliches Feld 'window' (Python ohne)
  - Python: `bot/analytics/api_audience.py:1611 (web.json_response({'curve':[], 'one_time_rate':None, 'total_chatters':0}) — KEIN window)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/loyalty_curve.rs:63 ({'curve':[],'one_time_rate':null,'total_chatters':0,'window':'all_time'})`
  - Wirkung: Geringfügige Schema-Inkonsistenz nur im Leer-Fall; für die meisten Konsumenten irrelevant.
  - Fix: window im Empty-Branch weglassen, um Python exakt zu spiegeln.

### analytics-admin-raids-market

Die Einheit ist gespalten: read-only Analytics-Kern (Streamer-Liste, Raid-Retention/-Analytics-Mathematik, recent-raids/-bans, network) ist nativ in Rust und größtenteils verhaltensgleich portiert, alle Admin-Write-Routen (Config-Promo/Raid/Chat, Affiliate/Gutschriften, Billing, Announcements, Roadmap, Audit-Log) laufen ausschließlich via Strangler-Proxy gegen Python 8765 — sie sind also funktional, aber NICHT nativ migriert (Lücke). Die zentralen verifizierten Divergenzen: (1) Die beiden bezahlpflichtigen Raid-Endpoints raid-retention + raid-analytics setzen das in Python erzwungene Plan-Gate (_require_extended_plan) NICHT — der 06-13-Fix wurde nur auf follower_funnel angewandt, alle anderen extended-Plan-Routen inkl. Raid bleiben offen → Paywall-Leck. (2) Der Admin-Streamer-DETAIL-Handler droppt mehrere Top-Level-Felder (verified/archived/archivedAt/createdAt/isLive/planId) und liefert displayName=Login statt discord_display_name; der stats-Block hat eine andere Form (total_duration_seconds statt totalWatchHours, plus viewerCount/lastSeenAt/lastStartedAt/lastGame fehlen). (3) recent-bans weicht bewusst ab (event_type='ban'-Filter, channels_protected aus Partner-Tabelle statt Ban-Events, today via CURRENT_DATE statt UTC-Mitternacht, NULL→null statt ""). Die früher dokumentierte SQLX-i64/i32-Drift in raid_analytics ist inzwischen via ::bigint-Casts in der SQL behoben (jetzt faithful). Mehrere Punkte sind bereits im 06-13-Audit erfasst, der Detail-Feld-Drop und das fortbestehende Raid-Plan-Gate-Loch sind real noch offen.

- **[HOCH · divergence · bestätigt]** raid-analytics + raid-retention: Plan-Gate (_require_extended_plan) fehlt in Rust
  - Python: `bot/analytics/api_raids.py:32 (_require_extended_plan) + bot/analytics/api_overview.py:1950`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/raid_analytics.rs:209-211,368-373`
  - Wirkung: Jeder eingeloggte Streamer ohne bezahltes Extended-Analytics-Paket sieht die kompletten Raid-Analytics/-Retention-Daten (per-source-Performance, Follow-Attribution, Incoming-Boost). Paywall-Umgehung, Umsatzverlust.
  - Verifikation: Selbst am Code verifiziert. Python ruft in api_raids.py:32 (_api_v2_raid_analytics) und api_overview.py:1950 (_api_v2_raid_retention) jeweils _require_v2_auth + _require_extended_plan; ohne analytics.extended-Entitlement gibt es 403 plan_required (Admin/Localhost-Bypass). Beide Rust-Handler in raid_analytics.rs prüfen NUR `matches!(auth, DashboardAuthLevel::None)` → 401 (Z.209-211 raid_retention_handler, Z.368-370 raid_analytics_handler), kein extended_gate/require_extended_plan. Der Helper crat
  - Fix: In beiden Handlern direkt nach dem None-Auth-Check require_extended_plan(pool, streamer, auth) aufrufen und bei Err 403 zurückgeben — analog Python (streamer-Param, Session-Login-Fallback, leerer Kontext = skip).
- **[MITTEL · divergence · umgestuft]** Admin-Streamer-Detail: Top-Level-Felder gedroppt (verified/archived/archivedAt/createdAt/isLive/planId)
  - Python: `bot/analytics/admin_streamer_queries.py:458-475 (_admin_streamer_detail_payload)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:306-310 (AdminStreamerDetailResponse)`
  - Wirkung: Das Admin-Detail-Frontend erhält für diese Felder undefined; Verifiziert-Status, Archiv-Flag, Live-Status und der zusammengeführte planId (manual||billing||plan_name) werden nicht angezeigt oder sind kaputt.
  - Verifikation: Divergenz real, aber Befund leicht überzogen formuliert. AdminStreamerDetailResponse (admin_streamers.rs:77-86) hat auf Top-Level nur login, displayName, twitchUserId, partnerStatus, stats, sessions, settings, oauth — Python (admin_streamer_queries.py:458-475) zusätzlich verified, archived, archivedAt, createdAt, isLive, planId. ABER: createdAt und archivedAt sind NICHT komplett weg, sondern nur falsch platziert — sie stehen im verschachtelten settings-Block (Z.339 created_at, Z.340 archived_at)
  - Fix: Die sechs Top-Level-Felder in AdminStreamerDetailResponse ergänzen und aus row befüllen; planId = manual_plan_id || billing_plan_id || plan_name (trim, leer→None) wie Python:469-475.
- **[MITTEL · divergence · unverif.]** Admin-Streamer-Detail: displayName nutzt Login statt discord_display_name
  - Python: `bot/analytics/admin_streamer_queries.py:460-461`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:308`
  - Wirkung: Im Admin-Detail wird statt des Discord-Anzeigenamens immer der reine Twitch-Login gezeigt, inkonsistent zur Liste.
  - Fix: display_name aus row.discord_display_name (trim, !empty) mit Login-Fallback ableiten, analog zum LIST-Handler.
- **[MITTEL · divergence · umgestuft]** Admin-Streamer-Detail: stats-Block hat abweichende Form (totalWatchHours fehlt, raw Sekunden; viewerCount/lastSeenAt/lastStartedAt/lastGame fehlen)
  - Python: `bot/analytics/admin_streamer_queries.py:476-494`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:88-96,311-317`
  - Wirkung: Das Detail-Stats-Panel zeigt falsche/fehlende Werte: Watch-Hours fehlt (Frontend erwartet Stunden, bekommt Sekunden oder undefined), aktuelle Viewer/letzter Stream/letztes Spiel fehlen.
  - Verifikation: Am Code bestätigt: Python stats (admin_streamer_queries.py:476-494) = totalSessions, totalWatchHours(=round(dur/3600,2)), averageViewers(round 2), peakViewers, followerDelta, viewerCount, lastSeenAt, lastStartedAt, lastGame. Rust StreamerStats (admin_streamers.rs:90-96) = total_sessions, total_duration_seconds, avg_viewers, peak_viewers, follower_delta. Also: totalWatchHours ist durch totalDurationSeconds ersetzt (anderer Name UND Einheit: rohe Sekunden statt gerundeter Stunden), und viewerCount
  - Fix: StreamerStats um total_watch_hours (gerundet), viewer_count, last_seen_at, last_started_at, last_game erweitern (Quelle: row.last_viewer_count/last_seen_at/last_started_at/last_game) und avg_viewers auf 2 Stellen runden.
- **[MITTEL · divergence · unverif.]** recent-bans: Rust filtert event_type='ban', Python listet/zählt alle Events inkl. unban
  - Python: `bot/analytics/api_public.py:84-117`
  - Rust: `rust/crates/tb-analytics/src/bans.rs:52-67`
  - Wirkung: recent-bans-Feed und Statistik weichen vom Python-Original ab: unban-Events fehlen in Liste und Zählung. Wenn Python das Original-Verhalten ist, divergiert die öffentliche Statistik.
  - Fix: Produktentscheid: Für Parität event_type-Filter entfernen, sonst in cleanup-decisions als bewusste Verbesserung dokumentieren.
- **[MITTEL · divergence · unverif.]** recent-bans: channels_protected aus Partner-Tabelle statt DISTINCT twitch_user_id der Ban-Events
  - Python: `bot/analytics/api_public.py:107-122`
  - Rust: `rust/crates/tb-analytics/src/bans.rs:75-79`
  - Wirkung: Die öffentlich angezeigte Zahl geschützter Kanäle ändert sich (Rust meist deutlich höher = alle aktiven Partner statt nur Kanäle mit Ban-Events). Bewusste Verbesserung, aber Verhaltensänderung gegenüber Python.
  - Fix: Produktentscheid dokumentieren; falls Parität gewünscht, alte DISTINCT-Zählung wiederherstellen.
- **[MITTEL · proxied · unverif.]** Komplettes Admin-Write-Dashboard (Config/Affiliate/Billing/Announcements/Roadmap/Audit-Log) nur via Proxy, nicht nativ
  - Python: `bot/analytics/api_admin.py:684-732 (config/promo/raids/chat, affiliates/*, billing/*, announcements, roadmap, audit-log)`
  - Rust: `—`
  - Wirkung: Migration unvollständig: Admin-Config-Writes (Raid/Chat/Promo-Flags pro Scope), Affiliate-Verwaltung und Billing-Übersicht hängen weiter am Python-Prozess. Ein Python-Cutover würde diese Admin-Funktionen komplett abschalten.
  - Fix: admin_config_queries (update_admin_raid_config/chat/promo, load_admin_billing_*) und admin_affiliate_queries nach tb-analytics + tb-dashboard-api portieren; bis dahin als bekannte Strangler-Lücke führen.
- **[NIEDRIG · divergence · unverif.]** recent-bans: today via CURRENT_DATE (Server-TZ) statt UTC-Mitternacht
  - Python: `bot/analytics/api_public.py:103-116`
  - Rust: `rust/crates/tb-analytics/src/bans.rs:64`
  - Wirkung: Wenn die Postgres-Session-TZ nicht UTC ist, weicht der Tageszähler heute um bis zu einen Kalendertag ab (Grenzfälle um Mitternacht).
  - Fix: received_at >= date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' bzw. expliziter UTC-Mitternacht-Vergleich.
- **[NIEDRIG · divergence · unverif.]** recent-bans: NULL-Felder als null statt leerer String
  - Python: `bot/analytics/api_public.py:96-99`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/bans.rs:19-28`
  - Wirkung: JSON-Form-Drift: Konsumenten, die '' erwarten, bekommen null (und umgekehrt für target_login: Python '' bei NULL, Rust column NOT NULL). Frontend-Rendering kann abweichen.
  - Fix: Falls Parität nötig, im Handler None→"" mappen (.unwrap_or_default()) für moderator_login/reason.
- **[NIEDRIG · divergence · unverif.]** network: Login wird nicht lowercased/getrimmt und leere Logins nicht gefiltert
  - Python: `bot/analytics/api_public.py:218-227`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/network.rs:20-29`
  - Wirkung: Bei Großschreibung/Whitespace in twitch_streamers_partner_state.twitch_login weicht die ausgegebene login-Form ab; leere Logins würden (anders als Python) als Eintrag erscheinen.
  - Fix: Im From-Impl login = r.twitch_login.trim().to_lowercase() setzen und im Handler leere Logins überspringen.
- **[NIEDRIG · divergence · unverif.]** network: fehlende View löst 500 aus statt graceful {streamers: []}
  - Python: `bot/analytics/api_public.py:188-198`
  - Rust: `rust/crates/tb-analytics/src/network.rs:21-39`
  - Wirkung: Wenn die View (noch) nicht existiert, liefert Rust 500 statt der von Python garantierten leeren Liste — schlechtere Degradation.
  - Fix: sqlx-Fehler 'relation does not exist' abfangen und leere Liste zurückgeben, oder Existenz-Check wie Python.
- **[NIEDRIG · divergence · unverif.]** raid-analytics: Raids ohne parsebare target_session_id werden in Rust eingeschlossen (sid=0), Python schließt sie aus
  - Python: `bot/analytics/api_raids.py:113-118`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/raid_analytics.rs:402-417`
  - Wirkung: Raid-Zeilen mit NULL/ungültiger target_session_id fließen in Rust in per_source-Aggregation ein (mit session_id=0, also 0 Chat-Metriken), in Python gar nicht — leichte Zähl-/Durchschnittsabweichung. Praktisch selten (Spalte meist gesetzt durch JOIN).
  - Fix: target_session_id als Option<i64> lesen und Zeilen mit None überspringen (continue) statt 0.

### analytics-poststream-coaching-ai

Die gesamte Einheit (Post-Stream-Report-Builder, Coaching-Engine, AI-Analytics) ist in Rust NICHT nativ portiert. Sämtliche HTTP-Routen (8 Stück: /v2/coaching, /v2/ai/{analysis,chat,history}, /v2/stream-report + /rate + /ab-vote GET/POST) fallen durch den Strangler-Catch-all-Proxy (rust/crates/tb-dashboard-api/src/proxy.rs) transparent an Python 8765. Die einzige Rust-Datei im nominellen Scope, session_detail.rs, bedient /v2/session/{id} bzw. /v2/session/{id}/events aus api_v2.py — also gar nicht meinen Scope (kein Post-Stream-Report, kein Coaching, kein AI). tb-analytics enthält KEINE post_stream/coaching/ai-Module. Die schwerwiegendste Lücke ist NICHT, dass die Routen proxied sind (das ist in den Audits bekannt und funktioniert read-mäßig), sondern dass die EVENT-getriebene Report-Generierung strukturell tot ist: trigger_post_stream_analysis hängt am Python-EventSub-stream.offline-Handler (eventsub_mixin.py:1956), aber unter TWITCH_RUST_MONITORING_TAKEOVER startet Python EventSub gar nicht (runtime_bootstrap.py:968 `if rust_takeover: pass`), und der native Rust-Handler handlers.rs:261 führt nur drei Effekte aus (state/refresh/auto_raid) und triggert KEINE Post-Stream-Analyse. Reports entstehen dadurch nur noch verzögert über den ungated Backfill (letzte 3 Sessions bei Bot-Start) + Retry-Loop — nicht mehr zeitnah am Stream-Ende. Die in Audit 13.6. (#239) gemeldete Entitlement-Divergenz (ai_full bei Extended-Plänen) ist in plan.rs inzwischen korrigiert und deckt sich mit catalog.py; für diesen Scope ohnehin irrelevant, weil _plan_ai_model im Python-Prozess über catalog.py läuft. Der Proxy selbst ist sicher gebaut (Host-Header 1:1, redirect=none, Owner-Isolierung bleibt in Python erhalten).

- **[MITTEL · regression · umgestuft]** Post-Stream-Analyse wird unter Rust-Monitoring-Takeover am Stream-Ende NICHT mehr getriggert (verwaister Hook)
  - Python: `bot/monitoring/eventsub_mixin.py:1953-1959 (trigger_post_stream_analysis im stream.offline-Handler); bot/runtime_bootstrap.py:967-969`
  - Rust: `rust/crates/tb-monitoring/src/handlers.rs:261 handle_stream_offline`
  - Wirkung: Ein Stream, der endet, bekommt keinen zeitnahen Post-Stream-Report mehr. Reports entstehen nur noch verzögert über backfill_post_stream_reports (nur beim Python-Bot-Start, nur die letzten 3 Sessions/Streamer ohne done-Report) und den Retry-Loop (retried nur failed/stuck, nicht neue Sessions). Sessions zwischen zwei Restarts, die über die letzten 3 hinausgehen, erhalten nie einen Report. Auch die A/B-Compact/Full-Reports und Chat-Wortgruppen (twitch_chat_word_groups) fehlen entsprechend.
  - Verifikation: Kern stimmt: Der Echtzeit-Trigger trigger_post_stream_analysis ist NUR im Python-EventSub-Offline-Handler (eventsub_mixin.py:1956), und runtime_bootstrap.py:967-969 ueberspringt den eventsub_runner unter Takeover (`if rust_takeover: pass`). Der native Rust-Handler handle_stream_offline (handlers.rs:261-348) ruft tatsaechlich nur stream_offline_state/refresh/auto_raid und KEINEN Post-Stream-Trigger; offline_side_effects.rs:12 dokumentiert das sogar bewusst. ABER der erste Agent hat das DB-getrieb
  - Fix: Im Rust handle_stream_offline einen vierten Effekt ergänzen, der die Post-Stream-Analyse anstößt — entweder via Aufruf der internen Python-API (HTTP-Hop zu 8765, Endpoint für trigger_post_stream_analysis schaffen) oder den Trigger nativ portieren. Alternativ den Python-EventSub-stream.offline-Hook unter Takeover gezielt nur für diesen Trigger weiterlaufen lassen (ohne die Doppel-Write-Effekte).
- **[MITTEL · proxied · unverif.]** Background-Jobs backfill/retry laufen ungated im Python-Prozess, nicht in Rust — kein nativer Scheduler
  - Python: `bot/analytics/api_post_stream.py:857 backfill_post_stream_reports, :928 retry_failed_reports, :1016 schedule_report_retry_job; runtime_bootstrap.py:984-985`
  - Rust: `—`
  - Wirkung: Reports werden zwar nicht komplett tot (Backfill bei jedem Python-Start + Retry alle 30 Min für failed/stuck), aber die Generierung hängt am Python-Prozess und am Bot-Restart-Rhythmus statt am Stream-Ende. Verstärkt Finding #1: ohne zeitnahen Trigger ist Backfill der einzige Weg, und der deckt nur 3 Sessions ab.
  - Fix: Scheduler-Logik (backfill+retry) zusammen mit der Trigger-Portierung nach Rust ziehen oder bewusst im Python-Prozess belassen und dokumentieren. Falls belassen: sessions_per_streamer erhöhen oder Backfill häufiger als nur bei Bot-Start laufen lassen, solange der Echtzeit-Trigger fehlt.
- **[MITTEL · infra · unverif.]** Proxy-Abhängigkeit von TB_DASHBOARD_LEGACY_FALLBACK_URL — bei Fehlkonfiguration 404 für gesamten Scope
  - Python: `—`
  - Rust: `rust/bin/tb-dashboard/src/main.rs:41-53 (DashboardProxyExt), rust/crates/tb-dashboard-api/src/proxy.rs:128-134`
  - Wirkung: Single Point of Failure für Post-Stream-Report, Coaching und AI-Analytics auf dem tb-dashboard-Binary (8767). Fehlt die Env oder ist Python 8765 down (proxy.rs:212 → 502), ist der gesamte Scope nicht erreichbar — ohne dass es einen nativen Fallback gibt.
  - Fix: Beim Deploy sicherstellen, dass TB_DASHBOARD_LEGACY_FALLBACK_URL=http://127.0.0.1:8765 gesetzt ist; Health-Check/Alert auf 502/404 dieser Routen ergänzen. Mittelfristig durch native Ports (Findings #2-4) entschärfen.
- **[NIEDRIG · proxied · umgestuft]** Coaching-Engine (CoachingEngine.get_coaching_data, 12 Analyse-Module) nicht nativ — nur Proxy
  - Python: `bot/analytics/coaching_engine.py:26 get_coaching_data + _efficiency/_title_analysis/_schedule_optimizer/_duration_analysis/_cross_community/_tag_optimization/_retention_coaching/_double_stream_detection/_chat_concentration/_raid_network/_peer_comparison/_competition_density/_build_recommendations; Route api_overview.py:89 GET /twitch/api/v2/coaching → api_insights.py:963`
  - Rust: `—`
  - Wirkung: Das komplette Coaching-Tab (Effizienz, Titel-/Tag-/Schedule-Optimizer, Retention, Peer-Vergleich, Raid-Netzwerk, Konkurrenzdichte, Empfehlungen, Tagesform-Coaching) läuft weiter ausschließlich im Python-Prozess. Migration unvollständig; bei Proxy-Aus (TB_DASHBOARD_LEGACY_FALLBACK_URL unset → 404) ist das Tab tot.
  - Verifikation: Faktisch korrekt: /twitch/api/v2/coaching ist in KEINEM nativen Rust-Router registriert (verifiziert in lib.rs:33-247 build_public/authed/admin-Router + proxy.rs — kein coaching-Treffer; grep coaching in lib.rs/handlers/ = leer), tb-analytics hat keine coaching-Datei. Aber das ist der dokumentierte, gewollte Strangler-Fig-Zustand, kein Ausfall: tb-dashboard/main.rs:45-55 wired den dashboard_fallback_handler, der ueber proxy.rs:123 Method/Body/Header (inkl. Cookie, nur Hop-Header gestrippt) 1:1 a
  - Fix: Coaching-Engine als tb-analytics-Modul nativ portieren (12 SQL-Aggregations-Bereiche). Bei Portierung auf SQLX-Typ-Drift achten: viewer_hours/efficiency_ratio sind numeric/float-Ausdrücke (coaching_engine.py:79-82), avg_viewers/duration_seconds müssen korrekt als f64/i32 dekodiert werden, sonst stilles 0.
- **[NIEDRIG · proxied · umgestuft]** AI-Analytics-Routen (ai/analysis, ai/chat, ai/history) nicht nativ — nur Proxy; AI-Coach tot bei Proxy-Aus
  - Python: `bot/analytics/api_ai.py:167 _api_v2_ai_analysis, :1001 _api_v2_ai_chat, :1087 _api_v2_ai_history; Routen api_overview.py:115-117`
  - Rust: `—`
  - Wirkung: Deep-AI-Analyse (Opus/MiniMax), Follow-up-Chat und History laufen nur in Python. Bei Proxy-Aus tot. In-Memory-Chat-State ist nicht migrierbar ohne nativen Port — Konversationsfäden gingen bei einem Cutover verloren.
  - Verifikation: Faktisch korrekt: GET /v2/ai/analysis, POST /v2/ai/chat, GET /v2/ai/history sind nativ nicht registriert (lib.rs/handlers grep auf /ai/analysis|/ai/chat|/ai/history = leer) und laufen ueber den Fallback-Proxy an Python 8765 (api_overview.py:115-117 registriert sie dort). Der 'tot bei Proxy-Aus'-Frame beschreibt einen hypothetischen Zustand, der im Normalbetrieb nicht eintritt — der Proxy ist gewollt aktiv (main.rs:41-55). Der In-Memory-Chat-State (api_ai.py:29 _in_progress_analyses, :144 _chat_s
  - Fix: Bei nativem Port den Chat-Session-State (heute Prozess-lokal mit Cleanup) in DB/Redis verschieben; _plan_ai_model-Modellwahl (ai_full→Opus, ai_mini→MiniMax) aus den Entitlements übernehmen. Bis dahin als bewusst proxied dokumentieren.
- **[NIEDRIG · proxied · umgestuft]** Post-Stream-Report-API (GET stream-report, rate, ab-vote) + A/B-Generierung nicht nativ — nur Proxy
  - Python: `bot/analytics/api_post_stream.py:1060 _api_v2_stream_report, :1243 _api_v2_stream_report_rate, :1298 _api_v2_stream_report_ab_vote, :379 _generate_report_v2, :544 _generate_report, :503 _generate_word_groups; post_stream/report_builder.py:763 build_post_stream_snapshot; Routen api_overview.py:118-121`
  - Rust: `—`
  - Wirkung: Report-Anzeige, Bewertung (gut/schlecht/neutral, Upsert ON CONFLICT session_id/variant/rated_by) und A/B-Voting laufen ausschließlich in Python. Migration unvollständig; bei Proxy-Aus tot. Owner-Isolierung (api_post_stream.py:1075 streamer != session_login → 403) bleibt korrekt, weil Python sie selbst macht.
  - Verifikation: Faktisch korrekt: GET /v2/stream-report, POST /rate, GET+POST /ab-vote sind nativ nicht registriert (lib.rs grep stream-report|ab-vote = leer) und laufen ueber den Proxy; der proxy.rs-Test post_roundtrip_body_und_cookie (Zeile 307-340) mockt explizit /twitch/api/v2/stream-report/rate und beweist Body+Cookie-Durchreichung. report_builder.build_post_stream_snapshot (803 Zeilen) und _generate_report_v2 existieren tatsaechlich nur in Python. Aber wie bei Coaching/AI: gewollter Strangler-Zustand, Rou
  - Fix: Stream-Report-Lese-Routen + report_builder nativ portieren; A/B-Generierungs-Pipeline (trigger) zusammen mit Finding #1 lösen. Beim Port der numeric-Felder (retention_5m etc. * 100, avg_viewers) auf Option<f64>-Decode achten (vgl. session_detail.rs:212-216, dort bereits korrekt mit NULL-Erhalt).
- **[NIEDRIG · missing · unverif.]** session_detail.rs ist NICHT der Post-Stream-Report — nomineller Scope-Eintrag deckt andere Funktion ab
  - Python: `bot/analytics/api_v2.py:_api_v2_session_detail / _api_v2_session_events (außerhalb des Post-Stream-Scopes); bot/internal_api/routes/streamers.py:504`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/session_detail.rs:50 session_detail_handler, :233 session_events_handler; rust/crates/tb-internal-api/src/handlers/session_detail.rs`
  - Wirkung: Klarstellung statt Bug: die im Auftrag genannten Rust-Refs liegen außerhalb der eigentlichen Post-Stream/Coaching/AI-Funktion. Es darf nicht der Eindruck entstehen, der Post-Stream-Report sei via session_detail.rs portiert — er ist es nicht (0% nativ).
  - Fix: Bei künftigen Audit-Scopes session_detail.rs der api_v2-Session-Einheit zuordnen, nicht der Post-Stream-Einheit. Keine Code-Änderung nötig.
- **[NIEDRIG · divergence · unverif.]** Entitlement-Map ai_full/ai_mini in Rust inzwischen paritätisch — frühere Audit-Divergenz (#239) behoben
  - Python: `bot/entitlements/catalog.py:66-135 PLAN_ENTITLEMENTS_MAP; bot/analytics/api_ai.py:148-155 _plan_ai_model`
  - Rust: `rust/crates/tb-analytics/src/plan.rs:51-109 plan_entitlements`
  - Wirkung: Kein aktiver Bug mehr in diesem Bereich. Modellwahl (Opus vs MiniMax) für Post-Stream- und AI-Analyse ist korrekt, da sie über die Python-Entitlements läuft.
  - Fix: Audit-Eintrag #239 als erledigt markieren. Beim künftigen nativen Port von _plan_ai_model die Entitlement-Quelle bewusst auf eine einzige (Rust plan.rs) konsolidieren, um Doppel-Pflege catalog.py/plan.rs zu vermeiden.

### analytics-internalhome-public-misc

Die Einheit zerfällt in zwei klar getrennte Hälften. Nativ und weitgehend solide portiert ist nur der Public-API-Block (recent-bans, recent-raids, network in tb-analytics + tb-dashboard-api), die Plan/Trial-Gating-Schicht (plan.rs/trial.rs), der raw-chat-status (in viewers.rs eingebettet) sowie die Internal-API-Telemetry-Routen (live/active-announcements, live/link-click). Der komplette Internal-Home-Komplex (GET /internal-home mit Rate-Limit, Identity-Resolve, Changelog-Merge, Target-ID-Stripping; POST /internal-home/changelog mit CSRF-Origin-Check), das gesamte Roadmap-CRUD (GET/POST/PATCH/DELETE), die vier Experimental-Routen (/exp/*) und das Admin-Audit-Log laufen NICHT nativ — sie fallen durch den Strangler-Fallback-Proxy auf Python 8765. Das funktioniert, ist aber unportiert und bricht, sobald Python aus ist. Bei den nativen Teilen finde ich mehrere echte Verhaltensabweichungen: recent-bans filtert event_type='ban' und ändert channels_protected (beide bekannt), network hat die Missing-View-Schutzklausel und die Empty-Login/lowercase-Normalisierung verloren (neu), raw-chat-status hat die dreistufige lastMessageAt-Fallback-Kette gekappt (neu), und der Plan-Resolver matcht Manual-Override/Billing nur noch über den Login, nicht mehr zusätzlich über die twitch_user_id (neu). Die Public-Fehlerpfade verlieren CORS-Header und JSON-Body (neu). Plan-Entitlements (ai_full fehlt) und der raid_free-Override sind bereits dokumentiert.

- **[MITTEL · regression · unverif.]** network: verlorene Missing-View-Schutzklausel führt zu 500 statt leerer Liste
  - Python: `bot/analytics/api_public.py:185 _load_network_sync (has_partner_view try/except → return {"streamers": []})`
  - Rust: `rust/crates/tb-analytics/src/network.rs:21 network_streamers`
  - Wirkung: Wenn die View twitch_streamers_partner_state (noch) nicht existiert/migriert ist, antwortet die öffentliche Netzwerk-Karte der Website mit 500 statt der von Python garantierten leeren Liste.
  - Fix: In network_streamers analog Python einen View-Existenz-Check (to_regclass) voranstellen und bei fehlender Relation Ok(vec![]) zurückgeben, statt den sqlx-Fehler zu propagieren.
- **[MITTEL · divergence · unverif.]** raw-chat-status: dreistufige lastMessageAt-Fallback-Kette auf eine Stufe gekürzt
  - Python: `bot/analytics/raw_chat_status.py:281 (scope_raw["lastMessageAt"] or health_last_message_at or last_message_at)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/viewers.rs:179-188 (build_raw_chat_status json! lastMessageAt: last_message_at)`
  - Wirkung: Wenn im gewählten Zeitfenster keine Roh-Nachrichten liegen, zeigt Rust lastMessageAt=null, während Python den letzten bekannten Nachrichtenzeitpunkt (Health oder global) anzeigt — irreführende 'nie Nachrichten'-Darstellung im Ingest-Status.
  - Fix: Im Health-SELECT zusätzlich last_raw_chat_message_at lesen, einen globalen MAX(message_ts)-Fallback-Query ergänzen und die or-Kette scope → health → global nachbilden.
- **[MITTEL · divergence · umgestuft]** Plan-Resolver matcht Manual-Override/Billing nur über Login, nicht zusätzlich über twitch_user_id
  - Python: `bot/entitlements/repository.py:98 load_manual_override (WHERE twitch_user_id=ref OR twitch_login=ref, refs=login+user_id); :147 load_billing_subscription`
  - Rust: `rust/crates/tb-analytics/src/plan.rs:176 resolve_plan_snapshot (streamer_plans WHERE LOWER(twitch_login)=LOWER($1); billing WHERE LOWER(customer_reference)=LOWER($1))`
  - Wirkung: Ein Streamer, dessen Manual-Override oder Stripe-Abo per twitch_user_id (mit abweichendem/leerem Login) hinterlegt ist, wird im Rust-Pfad nicht gefunden → fällt auf raid_free zurück, verliert also sein bezahltes Paket/Entitlements. Umsatz-/Berechtigungsthema.
  - Verifikation: Divergenz real und am Code belegt: Rust resolve_plan_snapshot (plan.rs:176-247) bindet ausschliesslich den Login gegen LOWER(twitch_login)=LOWER($1) (Z.190) bzw. LOWER(customer_reference)=LOWER($1) (Z.229); twitch_user_id wird NIE als Match-Kriterium genutzt. Caller auth_status.rs:180 partner_response(pool, login, user_id) HAT die user_id, ruft aber Z.195 resolve_plan_snapshot(pool, login) auf — user_id wird verworfen. Python load_manual_override (repository.py:111-114) matcht 'twitch_user_id=re
  - Fix: resolve_plan_snapshot zwei Refs (Login + user_id) durchreichen und die WHERE-Klausel auf TRIM(twitch_user_id)=$uid OR LOWER(twitch_login)=LOWER($login) erweitern (Override und Billing), user_id-Treffer priorisieren wie Python.
- **[MITTEL · divergence · unverif.]** Plan normalize_plan_id: Rust akzeptiert Legacy-Namen als Override-IDs, Python verwirft sie
  - Python: `bot/entitlements/catalog.py:138 normalize_plan_id (nur KNOWN_PLAN_IDS, sonst raid_free); repository.py:81 manual_override_from_row (plan_id not in KNOWN_PLAN_IDS → None)`
  - Rust: `rust/crates/tb-analytics/src/plan.rs:114 normalize_plan_id (mappt 'free','werbefrei','analysis','bundle' etc. auf kanonische IDs)`
  - Wirkung: Steht in streamer_plans.manual_plan_id ein Legacy-Name statt einer kanonischen ID, vergibt Rust ein (ggf. höheres) Paket, das Python verworfen hätte — abweichende Entitlements/Tier.
  - Fix: Für die Override-/Billing-Plan-ID den strengen Python-normalize (nur KNOWN_PLAN_IDS, sonst raid_free) nachbilden; die Legacy-Namens-Mappings nur dort einsetzen, wo Python normalize_plan_id_from_legacy_name nutzt.
- **[MITTEL · divergence · unverif.]** recent-bans: event_type='ban'-Filter + geänderte channels_protected-Quelle (bekannt)
  - Python: `bot/analytics/api_public.py:84-117 (kein event_type-Filter; channels_protected=COUNT(DISTINCT twitch_user_id) über Ban-Events; today via UTC-Mitternacht)`
  - Rust: `rust/crates/tb-analytics/src/bans.rs:44-79 (WHERE event_type='ban'; channels_protected=COUNT twitch_partners_all_state.is_partner_active=1; today via CURRENT_DATE)`
  - Wirkung: Öffentlicher Ban-Feed und der 'geblockte Bans'-Zähler liefern andere Werte als Python; TZ-Drift an Tagesgrenzen bei today.
  - Fix: Entscheiden, ob Public-Endpoint Unbans zählt; channels_protected-Quelle und today=UTC-Mitternacht in 05-cleanup-decisions.md absegnen oder zurückbauen (bereits als Befund offen).
- **[NIEDRIG · proxied · umgestuft]** Internal-Home GET (/twitch/api/v2/internal-home) komplett proxied, nicht nativ
  - Python: `bot/analytics/api_v2.py:2005 _api_v2_internal_home; bot/analytics/services/internal_home.py:946 build_internal_home_payload`
  - Rust: `—`
  - Wirkung: Das Streamer-Dashboard-Landing (Tagesform, OAuth-Status, Bot-Impact-Feed) hängt vollständig an Python; bei Python-Down ist die Startseite tot. Migration unvollständig.
  - Verifikation: Faktisch korrekt: lib.rs build_authed_router (Z.57-193) registriert KEINE internal-home-Route; der Request faellt ueber den Catch-all dashboard_fallback_handler (proxy.rs:123, wired in bin/tb-dashboard/src/main.rs:47 via .fallback(...)) an Python 8765. Der Python-Handler _api_v2_internal_home (api_v2.py:2005) ist substanziell (Rate-Limit, Identity-Resolve, parallele Sub-Blocks via asyncio.gather, Changelog-Merge, Target-ID-Stripping fuer Nicht-Admins). ABER: das ist der DOKUMENTIERTE, gewollte S
  - Fix: build_internal_home_payload als nativen Handler in tb-dashboard-api portieren (KPIs/recent_streams/ban_events/raid_events aus DB; Autoban-/Service-Warning-Events aus den Logdateien; Health-Score/Week-Comparison). Bis dahin als bewusste Restschuld in 05-cleanup-decisions.md dokumentieren.
- **[NIEDRIG · proxied · umgestuft]** Internal-Home-Changelog POST + Roadmap-CRUD + Experimental + Audit-Log proxied statt nativ
  - Python: `api_v2.py:2077 _api_v2_internal_home_changelog_create; api_roadmap.py:75/109/172/267; api_experimental.py:178/206/238/270; api_admin.py:693 _api_admin_audit_log; audit_log.py:575 load_admin_audit_log`
  - Rust: `—`
  - Wirkung: Changelog-Spiegelung (CLAUDE.md-Workflow), Roadmap-Pflege, Labor-Analytics und das Admin-Audit-Log laufen nur solange Python lebt. Coverage-Eval 2026-06-14 listet ~24 Admin-Write-Routen inkl. audit-log bereits als high-Risiko-Proxy.
  - Verifikation: Verifiziert: grep ueber rust/crates findet 'roadmap' nur in proxy.rs-Tests (proxy.rs:608/630 als Durchreich-Test) und exp_sessions.rs (das ist tb-monitoring fuer die exp_*-TABELLEN, NICHT die /exp/-API-Routen). Keine native Registrierung von /internal-home/changelog, /roadmap, /exp/, /admin/audit-log in lib.rs. Python-Handler sind real (api_roadmap.py: _api_v2_roadmap_get/create/update/delete Z.75/109/172/267 mit _ensure_roadmap_table + admin-Gate; _api_v2_internal_home_changelog_create api_v2.p
  - Fix: Roadmap-CRUD und Changelog-POST als native Handler in tb-dashboard-api portieren (kleine, gut abgegrenzte Tabellen-CRUDs inkl. CSRF-Origin-Guard); Audit-Log-Aggregator und exp/*-Reads nachziehen. Bekannt aus 2026-06-14-port-coverage-eval.md:42.
- **[NIEDRIG · divergence · unverif.]** network: Empty-Login-Skip und Lowercase-Normalisierung fehlen im Rust-Output
  - Python: `bot/analytics/api_public.py:218-227 (login=strip().lower(); if not login: continue)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/network.rs:20 From<NetworkStreamerRow> (login: r.twitch_login direkt)`
  - Wirkung: Bei Datensätzen mit Groß-/Leerzeichen-/Leer-Login liefert Rust andere/zusätzliche Einträge als Python (gemischte Schreibweise statt kleingeschrieben, leere Login-Karten).
  - Fix: Im Query LOWER(sp.twitch_login) selektieren oder in der From-Impl trim().to_lowercase() anwenden und leere Logins vor der Serialisierung herausfiltern.
- **[NIEDRIG · divergence · unverif.]** raw-chat-status: session_ids-Modus nicht portiert (nur since_date-Pfad)
  - Python: `bot/analytics/raw_chat_status.py:166 build_raw_chat_status (Parameter session_ids; Zweige in _query_scope_presence_stats:40 und _query_scope_raw_stats:125)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/viewers.rs:59 build_raw_chat_status(pool, streamer, since)`
  - Wirkung: Callers, die den Status für eine konkrete Session-Auswahl statt eines Tagesfensters berechnen, können in Rust nicht abgebildet werden; sofern ein solcher Pfad gebraucht wird, weicht das Fenster ab.
  - Fix: Prüfen, ob ein Rust-Caller session_ids braucht; falls ja, einen zweiten Code-Pfad mit IN (...)-Filter ergänzen (inkl. Leerliste→0-Shortcut), sonst als bewusst weggelassen dokumentieren.
- **[NIEDRIG · divergence · unverif.]** Public-Endpoints: Rust-Fehlerpfad verliert CORS-Header und JSON-Fehlerbody
  - Python: `bot/analytics/api_public.py:46 _public_json_response (Access-Control-Allow-Origin:*); :134/:177/:238 (500 mit {"error":"internal_error"})`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/bans.rs:59-61; raids.rs Handler; network.rs:46-49 (bare StatusCode::INTERNAL_SERVER_ERROR)`
  - Wirkung: Bei einem DB-Fehler bekommt die Website-JS (cross-origin fetch) eine 500 ohne Fehlerbody statt der von Python gelieferten lesbaren JSON-Fehlermeldung; Frontend-Fehlerbehandlung weicht ab.
  - Fix: Im Err-Zweig (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))) zurückgeben; sicherstellen, dass der CorsLayer auch Fehlerantworten umfasst (liegt am Router, daher i.d.R. abgedeckt).
- **[NIEDRIG · divergence · unverif.]** self-explainer-log Relay: Float-channel_id wird zu 0 statt getrunkt (bekannt)
  - Python: `bot/internal_api/routes/discord_log.py self_explainer_log (int(channel_id) truncatet)`
  - Rust: `rust/crates/tb-internal-api/src/handlers/self_explainer_log.rs:230-234 (Value::Number(n) => n.as_i64().unwrap_or(0))`
  - Wirkung: Theoretischer Fehlrouting-Fall, wenn ein Float-channel_id ankommt; praktisch selten, da Caller Integer sendet.
  - Fix: n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)).unwrap_or(0).
- **[— · divergence · FALSE-POSITIVE]** ~~Plan-Entitlements weichen vom Python-Katalog ab (ai_full fehlt, raid.priority falsch) (bekannt)~~ (False-Positive)
  - Python: `bot/entitlements/catalog.py:66-135 PLAN_ENTITLEMENTS_MAP`
  - Rust: `rust/crates/tb-analytics/src/plan.rs:51-109 plan_entitlements`
  - Wirkung: Zahlende Extended-Kunden verlieren ai_full (Opus→MiniMax-Downgrade); Promo-Abschaltung/raid.priority je Plan falsch zugeteilt.
  - Verifikation: Gegen den AKTUELLEN Code widerlegt. plan.rs:51-109 (current) enthaelt analytics.ai_full sehr wohl: analysis_dashboard (Z.73), bundle_analysis_raid_boost (Z.79), bundle_werbefrei_analyse (Z.87), bundle_komplett (Z.95). Programmatischer Set-Diff Python (catalog.py:66-135) vs Rust ergibt fuer ALLE 9 Plaene exakte Gleichheit (ALL MATCH: True): ai_full in Rust vorhanden=True, analytics_trial hat raid.priority=False, bundle_analysis_raid_boost hat chat.promos.disable (vorhanden), bundle_komplett hat a
  - Fix: plan_entitlements 1:1 an PLAN_ENTITLEMENTS_MAP angleichen (ai_full einführen, Sets je Plan exakt setzen) — Detail-Fix steht im 13.6.-Audit.

### dashboard-auth-session

Der native Rust-Teil dieser Einheit deckt nur drei Oberflächen wirklich nativ ab: die SPA-Auslieferung (/analyse + /analyse/*), den Auth-Status (/twitch/api/v2/auth-status) und die darunterliegende Auth-Level-Kaskade (Fernet-Decrypt, Session-Lookup mit 5s-Cache + Sliding-Refresh, Localhost/Admin/Partner-Level). Fernet-Krypto und Session-Refresh sind sauber und gegen Python interop-getestet — das Fundament ist solide. Der GESAMTE Login-/Logout-/OAuth-Flow (Twitch-Login/Callback, Discord-Admin-Login/Complete/Link/Logout, Partner-Link/Login, shared-Discord-Callback, Fingerprint, validate_admin_session, auth/logout) ist NICHT portiert und läuft ausschließlich über den Strangler-Proxy nach Python 8765 — funktioniert, ist aber nicht nativ und damit ein harter Cutover-Blocker. Innerhalb des portierten Codes gibt es mehrere belegte Divergenzen: die Auth-Level-Kaskade lässt den dritten Session-Typ `partner_access` sowie die Admin-Login-Promotion (`_TWITCH_ADMIN_LOGINS`), die `X-Admin-Token`- und `_noauth`-Wege weg; die native auth-status-Antwort droppt `displayName` und `csrfToken` (entgegen der Behauptung „volle Parität" in der 2026-06-14-Eval); die nativen Routen haben keine Security-Header (X-Frame-Options etc.) und keine partner_status_gate-Middleware; der Partner-Gate ist durch eine zusätzliche Blacklist-Prüfung strenger als Python; und die SPA-Auth verzweigt nicht auf Admin-Host-Gate bzw. Discord-Admin-Login-URL. Der Proxy selbst ist sauber gebaut (Host-Header-Durchreichung, Redirect-Policy::none gegen SSRF).

- **[HOCH · divergence · bestätigt]** Admin-Login-Promotion fehlt: Twitch-Session eines Admin-Logins wird nicht zu Admin
  - Python: `bot/analytics/api_v2.py:1339-1342 (twitch_login in _TWITCH_ADMIN_LOGINS → return 'admin')`
  - Rust: `rust/crates/tb-dashboard-api/src/auth/level.rs:189-199 (twitch_dash_session ⇒ immer Partner); session.rs:205-315 (load_partner_session kennt keine Admin-Logins)`
  - Wirkung: Loggt sich der Admin per Twitch-OAuth statt per Discord ein, behandelt Rust ihn als normalen Partner: canViewAllStreamers=false, isAdmin=false, kein Zugriff auf fremde Streamer-Daten und Admin-Default-Streamer. Auth-status meldet abweichende Rechte.
  - Verifikation: Am Code belegt. Python api_v2.py:1339-1341: twitch_login der twitch-Session in _TWITCH_ADMIN_LOGINS (=frozenset{'earlysalty'}, api_v2.py:464) → return 'admin' (Vollzugriff, canViewAllStreamers). Rust level.rs:190-199 mappt JEDE gueltige twitch_dash_session bedingungslos auf Partner{twitch_login,...}; session.rs:205-315 (load_partner_session) kennt keine Admin-Login-Liste. Der auth_status-Handler bestaetigt die Folge: handlers/auth_status.rs:211-240 (partner_response) liefert fuer den Partner-Zwe
  - Fix: In der Partner-Auflösung den Login gegen die Admin-Login-Liste (entspr. _TWITCH_ADMIN_LOGINS) prüfen und in dem Fall DashboardAuthLevel::Admin zurückgeben — vor dem Partner-Mapping.
- **[MITTEL · divergence · umgestuft]** Auth-Level-Kaskade kennt den Session-Typ partner_access nicht
  - Python: `bot/analytics/api_v2.py:1344-1351 (_get_auth_level → _get_partner_access_session); bot/dashboard/auth/state_store.py:18,297 (_PARTNER_ACCESS_SESSION_TYPE='partner_access')`
  - Rust: `rust/crates/tb-dashboard-api/src/auth/level.rs:159-202 (FromRequestParts prüft nur master_dash_session + twitch_dash_session)`
  - Wirkung: Ein ausschließlich über den Partner-Access-Flow (Magic-Link/Login-Token, ohne Twitch-OAuth) authentifizierter Nutzer ist in Rust DashboardAuthLevel::None → wird auf /analyse zum Login umgeleitet und bekommt in auth-status authenticated=false, obwohl Python ihn als 'partner' führt.
  - Verifikation: Bestaetigt am Code: state_store.py:18 definiert _PARTNER_ACCESS_SESSION_TYPE='partner_access', state_store.py:291-309 (load/save_partner_access_session). api_v2.py:1344-1351 und 1382-1390 werten ueber _get_partner_access_session den partner_access-Cookie aus (Cookie-Name via partner_auth_mixin.py:63 _partner_access_cookie_name, gesetzt durch auth_partner_login). Der Rust-Extractor in level.rs:181-199 liest ausschliesslich master_dash_session (Admin) und twitch_dash_session (Partner) — partner_ac
  - Fix: Im Extractor nach twitch_dash_session auch den partner_access-Cookie laden (eigener session_type='partner_access', analog load_partner_session) und auf DashboardAuthLevel::Partner mappen.
- **[MITTEL · divergence · unverif.]** auth-status droppt displayName für Partner
  - Python: `bot/analytics/api_v2.py:2885 ("displayName": session.get("display_name")); services.py:243 (display_name im Payload gespeichert)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/auth_status.rs:221 ("displayName": null); auth/session.rs:48-53 (PartnerSession ohne display_name)`
  - Wirkung: Das Dashboard zeigt für eingeloggte Partner keinen Anzeigenamen mehr (fällt auf twitchLogin zurück). Die 2026-06-14-Eval (Zeile 121) behauptet hier fälschlich 'volle Parität'.
  - Fix: display_name aus dem entschlüsselten twitch-Payload in PartnerSession übernehmen und in partner_response durchreichen.
- **[MITTEL · divergence · unverif.]** auth-status liefert nie ein csrfToken
  - Python: `bot/analytics/api_v2.py:2833-2845,2891-2892 (csrf_token via _csrf_get_token/_csrf_generate_token → csrfToken/csrf_token)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/auth_status.rs:227-228 ("csrfToken": null, "csrf_token": null) — auch im Admin- und Partner-Zweig`
  - Wirkung: Das Frontend zieht den CSRF-Token aus auth-status, um ihn bei Mutations-POSTs (die per Proxy nach Python gehen) mitzusenden. Liefert die native Route immer null, fehlt dem Frontend der Token → proxied POST-Formulare (discord_link, billing etc.) können am Python-CSRF-Check scheitern.
  - Fix: Entweder einen CSRF-Token nativ erzeugen/auslesen (kompatibel zu Pythons _csrf-Schema) oder die auth-status-Route bewusst proxien, statt sie nativ mit null zu beantworten.
- **[MITTEL · regression · unverif.]** Native Routen ohne Security-Header (X-Frame-Options etc.)
  - Python: `bot/dashboard/server_v2.py:1067-1082 (_security_headers_middleware: X-Frame-Options DENY, X-Content-Type-Options nosniff, Referrer-Policy, COOP auf JEDER Antwort)`
  - Rust: `rust/crates/tb-dashboard-api/src/lib.rs:30-247 (kein Security-Header-Layer); handlers/spa.rs:64-69 + auth_status.rs:246-257 (setzen nur Content-Type/Cache-Control)`
  - Wirkung: Nativ ausgelieferte Seiten (insb. /analyse-HTML) verlieren den Clickjacking-Schutz (X-Frame-Options: DENY) und nosniff. Proxied Routen behalten Pythons Header; nur native Routen sind betroffen.
  - Fix: Einen SetResponseHeaderLayer auf den Dashboard-Router legen, der die vier Header analog _security_headers_middleware als Default setzt.
- **[MITTEL · missing · unverif.]** partner_status_gate-Middleware (passive Partner) fehlt nativ
  - Python: `bot/dashboard/auth/auth_mixin.py:1836-1905 (build_partner_status_gate_middleware: passive Partner → 403/Redirect auf Active-Only-Routen)`
  - Rust: `rust/crates/tb-dashboard-api/src/lib.rs (keine Middleware); auth/level.rs (kennt keinen active/passive-Status)`
  - Wirkung: Ein passiver Partner (z.B. manual_partner_opt_out oder token_error) wird auf nativen Routen nicht gegated; auf /analyse greift nur der gröbere analytics_access_allowed-Check. Verhaltensabweichung, im Worst Case Zugriff auf Bereiche, die Python sperrt.
  - Fix: Eine axum-Middleware analog partner_status_gate ergänzen (active-Status via _resolve_partner_active_status-Äquivalent), oder dokumentieren dass dieses Gating bewusst Python-only bleibt bis Cutover.
- **[MITTEL · divergence · unverif.]** Partner-Gate strenger als Python: zusätzliche token_blacklist-Sperre beim Session-Load
  - Python: `bot/dashboard/auth/auth_mixin.py:741-780 (_is_partner_allowed: NUR twitch_partners, KEINE twitch_token_blacklist-Prüfung)`
  - Rust: `rust/crates/tb-dashboard-api/src/auth/session.rs:285-301 (zusätzlicher EXISTS-Check auf twitch_token_blacklist → None)`
  - Wirkung: Ein Partner mit einem (ggf. nur token_error-bedingten) Blacklist-Eintrag wird in Rust komplett ausgeloggt (DashboardAuthLevel::None → /analyse-Redirect zum Login), während Python ihn einlässt und nur den token_error-Status mit Gnadenfrist anzeigt. Falsch-Aussperrung möglich.
  - Fix: Die Blacklist-Prüfung im Session-Load entfernen oder auf die echte Python-Semantik bringen (Blacklist beeinflusst partner_status/grace, nicht die Session-Gültigkeit) — Blacklist-Logik gehört in partner_access-State, nicht ins Gate.
- **[MITTEL · divergence · unverif.]** SPA-Handler ohne Admin-Host-Gate
  - Python: `bot/analytics/api_overview.py:544-546,809-811 (_serve_dashboard_v2/_assets: _admin_dashboard_host_page_gate(request) zuerst)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/spa.rs:43-83 (analyse_handler/assets ohne Host-Gate)`
  - Wirkung: Auf dem Admin-Dashboard-Host verhält sich /analyse abweichend von Python (Python kann dort gaten/umleiten). Topologie-/Host-abhängige Auslieferung weicht ab.
  - Fix: Den Admin-Host-Gate (Host-Header-Erkennung wie _is_admin_dashboard_host_request) vor check_spa_auth in analyse_handler/assets nachziehen, oder bewusst als Host-agnostisch dokumentieren.
- **[NIEDRIG · proxied · umgestuft]** Gesamter Login-/Logout-/OAuth-Flow nicht nativ — nur via Proxy
  - Python: `bot/dashboard/routes_mixin.py:626-636 (auth_login, auth_callback, discord_auth_login/complete/link/logout); routes_entry.py:48-51 (auth_logout, validate_admin_session, fingerprint); partner_auth_mixin.py:102,150 (auth_partner_link/login)`
  - Rust: `rust/bin/tb-dashboard/src/main.rs:41-55 (nur Strangler-Fallback); rust/crates/tb-dashboard-api/src/lib.rs:54-193 (keine Login-Routen)`
  - Wirkung: Funktioniert solange Python läuft, ist aber ein harter Cutover-Blocker: bei Abschaltung des Python-Prozesses kann sich niemand mehr am Dashboard an- oder abmelden. Die komplette Session-Erzeugung (Fernet-Encrypt, DB-Insert) lebt nur in Python.
  - Verifikation: Faktisch korrekt: In rust/crates/tb-dashboard-api/src/lib.rs sind nur die read-only v2-Analytics + /twitch/api/v2/auth-status nativ registriert. Grep ueber die ganze Crate nach .route(...login|callback|logout|auth/discord|auth/partner) liefert 0 Treffer; einzige auth-bezogene native Route ist auth-status (reiner Status, kein Login). Alle Login/OAuth/Logout/partner/validate/fingerprint-Pfade fallen ueber app.fallback(dashboard_fallback_handler) (main.rs:47) an Python 8765. ABER: Das ist die bewus
  - Fix: Login/Callback/Logout für mindestens den Twitch- und Discord-Admin-Flow nativ portieren (Fernet-encrypt existiert bereits in fernet.rs::encrypt), oder explizit als bewusst-proxied in der Cutover-Doku führen.
- **[NIEDRIG · divergence · unverif.]** SPA-Login-Redirect wählt nicht zwischen Twitch- und Discord-Admin-Login
  - Python: `bot/analytics/api_overview.py:548-557 (_should_use_discord_admin_login → DASHBOARD_V2_DISCORD_LOGIN_URL); _dashboard_auth_redirect_or_unavailable (503 wenn OAuth aus)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/spa.rs:28,90 (LOGIN_URL hart = /twitch/auth/login?next=%2Fanalyse)`
  - Wirkung: Auf Admin-Kontext-Pfaden wird der falsche Login-Weg angeboten; bei deaktiviertem OAuth zeigt Rust einen Redirect ins Leere statt der 503-Meldung. Geringer Impact, da /analyse primär der Streamer-Pfad ist.
  - Fix: Login-URL-Auswahl analog _should_use_discord_admin_login bzw. _dashboard_auth_redirect_or_unavailable ergänzen (Discord-Login-URL + 503-Fall).
- **[NIEDRIG · divergence · unverif.]** X-Admin-Token-Header und _noauth-Modus in der Level-Kaskade nicht abgebildet
  - Python: `bot/analytics/api_v2.py:1331-1332 (_noauth → localhost), 1353-1362 (X-Admin-Token → admin)`
  - Rust: `rust/crates/tb-dashboard-api/src/auth/level.rs:166-202 (nur Localhost-Host+Peer, master_dash_session, twitch_dash_session)`
  - Wirkung: Interne/Test-Konsumenten, die heute per X-Admin-Token oder im _noauth-Modus auf die cookie-gegateten v2-Routen zugreifen, bekommen in Rust None. In der Praxis gering, da der separate ExpectedToken-Pfad (tb_http_core::AuthLevel) für die token-basierten Analytics-Routen existiert; betrifft nur die DashboardAuthLevel-Routen (auth-status, /analyse).
  - Fix: Falls X-Admin-Token/_noauth für die DashboardAuthLevel-Routen real gebraucht werden, im Extractor ergänzen; sonst die bewusste Auslassung in der Auth-ADR dokumentieren.
- **[NIEDRIG · divergence · unverif.]** Unauth-auth-status ohne Rate-Limit
  - Python: `bot/analytics/api_v2.py:2772-2781 (_check_rate_limit auf dem unauth-Zweig von _api_v2_auth_status)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/auth_status.rs:83-97 (unauth_response: nur 5s-Cache, kein Rate-Limit)`
  - Wirkung: Anonyme auth-status-Requests werden nativ nicht gedrosselt; der 5s-Cache liefert zwar billige Antworten, eine missbräuchliche Flut wird aber nicht wie in Python rate-limitiert. Geringer Impact (Antwort ist statisch/gecached).
  - Fix: Optional einen leichten Rate-Limit-Layer für den unauth-Pfad ergänzen oder die Auslassung als bewusst dokumentieren (Cache deckt den Hauptzweck ab).

### dashboard-billing-affiliate

Der Port dieser Einheit ist bewusst minimal: von der gesamten Billing-/Abo-/Affiliate-Oberfläche ist in Rust EINZIG der Self-Claim-Trial nativ (`POST /twitch/api/billing/trial/start` → `tb-analytics/trial.rs`). Diese eine Route ist verhaltensgleich portiert (Status-Mapping, Paid-Plan-Liste, trial_ever_granted-Einmaligkeit, 30-Tage-Ablauf alle korrekt). Auch der statische Entitlement-Katalog (`tb-analytics/plan.rs`) wurde seit dem 13.6.-Audit korrigiert und stimmt jetzt 1:1 mit `bot/entitlements/catalog.py` überein (per Diff verifiziert — der frühere [ana-crate-1]-Befund ist behoben). Alles Übrige — abbo-Dashboard (Pläne, Rechnungsdaten, Kündigung, Lurker-Tax/Promo-Toggles), Stripe-Checkout/Invoice-Preview/Webhook/Product-Sync, der komplette Affiliate-Stack (OAuth, Stripe-Connect, 30%-Provision mit Lock+Replay, verschlüsselte PII, Gutschrift-PDF/VAT/Nummernkreis, der 6h-Hintergrundjob) — ist NICHT nativ und läuft ausschließlich über den Strangler-Proxy (`tb-dashboard/main.rs` → Python 8765). Das funktioniert solange der Python-Prozess lebt, ist aber keine echte Migration: bei Python-Aus werden Abos nie aktiviert/gekündigt und keine Provisionen/Gutschriften erzeugt. Zwei echte Verhaltensbefunde im nativen Code: der manuelle raid_free-Downgrade wird in Rust von einem aktiven Stripe-Abo überschrieben (Geld-/Berechtigungs-Divergenz, bekannt), und der 24h-Auto-Grant-Trial existiert nur im (proxied) Python-Pfad nativ nicht.

- **[HOCH · proxied · bestätigt]** Gesamter Stripe-Billing-Schreibpfad nur via Proxy, nicht nativ (Webhook/Checkout/Invoice/Sync)
  - Python: `bot/dashboard/routes_billing.py:132 api_billing_stripe_webhook, :233 api_billing_checkout_preview, :302 api_billing_invoice_preview, :375 api_billing_stripe_sync_products, :116 api_billing_readiness, :75 api_billing_catalog`
  - Rust: `—`
  - Wirkung: Bei Python-Ausfall werden Stripe-Events nie verarbeitet: Abos werden nicht aktiviert, Upgrades/Downgrades/Kündigungen nicht synchronisiert, Bonus-Monate nicht gewährt. Migration läuft, ist aber nicht nativ.
  - Verifikation: Am Code bestätigt. In rust/crates/tb-dashboard-api/src/lib.rs (build_authed_router) ist als einzige Billing-Route POST /twitch/api/billing/trial/start (billing::start_trial_handler) registriert; handlers/billing.rs ist laut eigenem Doc-Kommentar nur ein Port von api_billing_trial_start. Alle uebrigen Python-Routen existieren (routes_billing.py: api_billing_catalog:75, api_billing_readiness:116, api_billing_stripe_webhook:132, api_billing_checkout_preview:233, api_billing_invoice_preview:302, api
  - Fix: Stripe-Webhook-Eingang nativ portieren (Signatur-Verify + Idempotenz-Insert + Plan-Sync nach twitch_billing_subscriptions/streamer_plans) — laut Welle-C-Plan als einziges Billing-Stück explizit als 'nötig' deklariert, aber bis heute nicht gebaut.
- **[HOCH · proxied · bestätigt]** Kompletter Affiliate-Stack nicht nativ: 16 Routen + Provisions-Engine + Gutschrift-PDF + 6h-Job nur via Proxy/Python
  - Python: `bot/dashboard/affiliate/affiliate_mixin.py:1457 _affiliate_register_routes (16 Routen), :1352 _affiliate_process_commission, :781 _affiliate_transfer_commission, :551 _affiliate_run_gutschrift_job, :598 _affiliate_background_context`
  - Rust: `—`
  - Wirkung: Ohne Python keine Affiliate-Anmeldung, keine 30%-Provisionsberechnung/-auszahlung, keine Gutschrift-Generierung. Das ist die größte funktionale Lücke der Einheit — finanzwirksam.
  - Verifikation: Am Code bestaetigt. grep ueber rust/crates nach affiliate|gutschrift|commission|stripe liefert NULL Affiliate-Implementierung: die einzigen Treffer sind Fehlalarme — tb-crypto/aad.rs (Doc-Kommentar nennt affiliate_pii-AAD-Format), tb-analytics/plan.rs+trial.rs (Stripe-Abo-Query), und tb-chat/scam_pitch.rs:226/249 (Regex-Pattern \baffiliate\b / \bcommissions?\b zur Scam-Erkennung, kein Billing). Keine Affiliate-Route in lib.rs registriert → alle /twitch/affiliate*-Pfade laufen ueber dashboard_fal
  - Fix: Bewusst-gedropped laut 13.6.-Audit (Welle C eingedampft). Wenn Python-Bot dauerhaft Affiliate trägt, in 05-cleanup-decisions.md als 'bleibt Python' festschreiben; sonst Provisions-Engine + Gutschrift-Job nativ portieren.
- **[MITTEL · proxied · unverif.]** Verschlüsselter Affiliate-PII-Store (DSGVO) nicht nativ portiert
  - Python: `bot/dashboard/affiliate/affiliate_pii.py:11 AffiliatePII (save_pii/load_pii/migrate_from_plaintext, AAD-gebundene Feldverschlüsselung), :158 save_pii, :262 load_pii, :303 migrate_from_plaintext`
  - Rust: `—`
  - Wirkung: PII-Verschlüsselung/Entschlüsselung der Affiliate-Steuerdaten ist an Python gebunden; der Rust-Pfad kann diese DSGVO-relevanten Daten nicht lesen/schreiben. Bei Migration des Affiliate-Stacks muss AAD-Schema 1:1 übernommen werden, sonst werden Bestandsdaten unlesbar.
  - Fix: Falls Affiliate später nativ wird: AAD-Konstruktion (field + normalisierter Login) und Tax-Bundle-Serialisierung byte-genau gegen affiliate_pii.py spiegeln, sonst Entschlüsselungs-Fehler auf Altdaten.
- **[MITTEL · proxied · unverif.]** Gutschrift-PDF/VAT/Nummernkreis komplett Python (fpdf2, ROUND_HALF_UP, fortlaufende Nummer)
  - Python: `bot/dashboard/affiliate/gutschrift.py:29 AffiliateGutschriftService, :100 _vat_amount_cents (19% nur bei 'regelbesteuert', Decimal ROUND_HALF_UP), :312 _next_gutschrift_number, :458 generate_gutschrift_pdf, :782 generate_for_period, :960 generate_monthly_gutschriften`
  - Rust: `—`
  - Wirkung: Rechtsverbindliche Gutschriften (Steuerbeleg) werden nur von Python erzeugt. Eine spätere Rust-Portierung muss die VAT-Rundung (ROUND_HALF_UP auf Cent) und den ust_status-Sonderfall exakt treffen, sonst Steuerbetrags-Abweichungen.
  - Fix: Bei Portierung: Decimal-Semantik (ROUND_HALF_UP, Cent-Quantisierung) und Nummernkreis-Vergabe unter Lock nachbilden; latin-1-PDF-Encoding (_pdf_safe) beachten.
- **[MITTEL · missing · unverif.]** 24h-Auto-Grant des Analytics-Trials existiert nativ nicht (nur Self-Claim portiert)
  - Python: `bot/dashboard/billing/billing_mixin.py:1110 _billing_check_and_grant_trial_eligibility (24h-Grace nach first_login_at, dann Trial gewähren), aufgerufen aus :1321 _billing_current_plan_for_request`
  - Rust: `rust/crates/tb-analytics/src/trial.rs (nur start_trial_for_user + grant_trial_at_onboarding)`
  - Wirkung: Solange der Python-Proxy lebt, wird der Auto-Grant beim Abbo-/Pricing-Seitenaufruf weiterhin ausgelöst. Bei reinem Rust-Betrieb (Python aus) bekämen passive Neu-Streamer den Trial nicht automatisch. Bekannt aus 14.6.-Coverage-Eval.
  - Fix: Falls Python später wegfällt: 24h-Grace-Auto-Grant (first_login_at-Parse inkl. Date-only→T00:00:00, paid_plan_ids={raid_boost,analysis_dashboard,bundle_analysis_raid_boost}, manual!=raid_free-Guard) in den nativen Plan-Resolver-Pfad ziehen.
- **[MITTEL · proxied · unverif.]** abbo-Dashboard-Seiten und Mutationen (Promo/Lurker-Tax-Toggles, Profil, Kündigung) nur via Proxy
  - Python: `bot/dashboard/abbo_routes.py:630 abbo_promo_settings, :664 abbo_lurker_tax_settings, :703 abbo_promo_message; bot/dashboard/abbo_billing_routes.py:21 abbo_pay, :126 abbo_profile_save, :180 abbo_cancel, :243 abbo_invoices, :378 abbo_stripe_settings, :501 abbo_invoice`
  - Rust: `—`
  - Wirkung: Streamer-Self-Service (Lurker-Tax aktivieren, Promo abschalten/anpassen, Rechnungsdaten speichern, Abo kündigen, Rechnungen herunterladen) ist vollständig Python-abhängig. Migration läuft, aber nicht nativ.
  - Fix: Bei Portierung: CSRF-Token-Verifikation, Entitlement-Gates und die ON-CONFLICT(twitch_user_id)-Upserts auf streamer_plans (promo_disabled/promo_message/lurker_tax_enabled) 1:1 übernehmen; promo_message-Validierung (validate_streamer_promo_message) mitportieren.
- **[NIEDRIG · proxied · unverif.]** Stripe-Product-Sync (Preis-/Produkt-ID-Map-Schreibpfad) nicht nativ
  - Python: `bot/dashboard/routes_billing.py:375 api_billing_stripe_sync_products; billing_mixin.py:202 _billing_set_price_id_map, :213 _billing_set_product_id_map, :237 _billing_price_mapping_stats`
  - Rust: `—`
  - Wirkung: Das Pflegen der Stripe-Preis-IDs (Admin-Aktion) geht nur über Python. Geringer User-Impact (selten, Admin-only), aber Teil des unmigrierten Schreibpfads.
  - Fix: Niedrige Priorität; bei vollständiger Billing-Migration mitnehmen, sonst als 'bleibt Python' dokumentieren.
- **[NIEDRIG · divergence · unverif.]** Trial-Handler: Localhost/Admin-Auth liefert 401 statt Session-Plan zu lesen (subtile Auth-Divergenz)
  - Python: `bot/dashboard/routes_billing.py:596-606 api_billing_trial_start (liest twitch_user_id/twitch_login aus _billing_auth_sessions_for_request, unabhängig von Localhost)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/billing.rs:20-35 + auth/level.rs:170-173`
  - Wirkung: Sehr eng: nur ein lokaler/Admin-Request, der gleichzeitig einen gültigen Partner-Session-Cookie trägt, würde abweichen (401 statt Trial-Grant). Praktisch kaum relevant (Trial-Start kommt vom Browser über Caddy = nicht-Loopback-Host → Partner-Pfad greift). Dokumentiert als Kantenfall.
  - Fix: Falls Parität gewünscht: im Trial-Handler bei Localhost/Admin zusätzlich den twitch_dash_session-Cookie auf eine Partner-Session auflösen, bevor 401 zurückgegeben wird — oder bewusst als 'akzeptierte Abweichung' in 05-cleanup-decisions.md festhalten.
- **[NIEDRIG · proxied · unverif.]** Affiliate-Gutschrift-Hintergrundjob (6h-Loop) hat kein natives Scheduler-Pendant
  - Python: `bot/dashboard/affiliate/affiliate_mixin.py:598 _affiliate_background_context, :551 _affiliate_run_gutschrift_job; gutschrift.py:1025 run_pending, :384 due_periods`
  - Rust: `—`
  - Wirkung: Die periodische Gutschrift-Erzeugung ist an den laufenden Python-Bot gekoppelt; im reinen Rust-Betrieb würde sie ausbleiben. Niedriger akuter Impact (Python läuft), aber strukturelle Lücke.
  - Fix: Bei Affiliate-Migration als nativen Timer/Job (due_periods → generate_for_period → run_pending) nachbauen; Periodenlogik (Monats-Start/Next-Period-Start) exakt aus gutschrift.py übernehmen.
- **[— · divergence · FALSE-POSITIVE]** ~~Manueller raid_free-Downgrade wird in Rust von aktivem Stripe-Abo überschrieben (Geld/Berechtigung)~~ (False-Positive)
  - Python: `bot/entitlements/repository.py:84-95 + 206-228 (jeder nicht abgelaufene Manual-Override gewinnt, auch raid_free)`
  - Rust: `rust/crates/tb-analytics/src/plan.rs:199-222 (resolve_plan_snapshot)`
  - Wirkung: Ein Admin-Downgrade auf raid_free wird ignoriert, solange ein aktives/trialing/past_due-Stripe-Abo existiert — der Streamer behält das bezahlte Paket inkl. Entitlements gegen den Admin-Willen. Bekannt aus 13.6.-Audit ([ana-crate]), bis heute nicht gefixt.
  - Verifikation: Adversariell widerlegt am Code. Das konstruierte Szenario stimmt nicht. plan.rs:202 lautet `if pid != "raid_free" || pid_raw == "raid_free"`. Python prueft in manual_override_from_row (repository.py:81-82) den ROHEN manual_plan_id direkt gegen KNOWN_PLAN_IDS (KEINE Legacy-Normalisierung an dieser Stelle) — nur Rohwerte aus {raid_free, chat_quiet, raid_boost, bundle_*, analysis_dashboard, analytics_trial} aktivieren den Override. Truth-Table-Vergleich (selbst durchgerechnet): (a) Rohwert 'raid_fr
  - Fix: Den raid_free-Sonderfall in plan.rs:202 entfernen: jeden nicht-abgelaufenen, explizit gesetzten Manual-Override anwenden (auf Override-Existenz + nicht-abgelaufen prüfen, nicht auf pid!=raid_free) und den Billing-Zweig überspringen — wie build_plan_snapshot.

### dashboard-live-raids-legal

Der Port dieser Einheit ist gespalten: Nur die Legal-Seiten (Impressum/Datenschutz/AGB/Sicherheit + Turnstile-Human-Gate + robots.txt) sind nativ in tb-dashboard-api (8769) portiert und überschatten den Proxy. Praktisch der GESAMTE Rest des Live-Dashboards meines Scopes läuft über den Strangler-Fallback-Proxy nach Python 8765 und ist damit NICHT nativ: die Live-Dashboard-HTML-Seiten (index/admin/partner_stats), alle Streamer-Admin-Mutationen (verify/remove/archive/discord_flag/discord_link/add_*/chat_action), die 5 Live-Announcement-Config-Routen, die 6 Raid-Dashboard-Routen (auth/go/requirements/history/analytics/callback) sowie die Markt-HTML-Seite + market_data. Markt-Anteil (market-share) ist ein Sonderfall: Python proxied nur durch, die Berechnung lebt nativ in tb-analytics::market (sauber). Wo nativer Code existiert, fand ich mehrere belegte Feld-Divergenzen: timestamp-Format-Drift (::text statt isoformat) in recent-raids, viewers null-vs-0, fehlende lowercase/empty-Normalisierung im network-Handler, sowie eine harte 503-Lücke im nativen verify-Pfad (clear/failed nicht portiert). Schwerwiegendster Befund: die native Sicherheits-Seite enthält ein in Python NICHT existierendes öffentliches, unauthentifiziertes Report-Formular, dessen POST-Handler im Hintergrund die claude-CLI mit --dangerously-skip-permissions gegen das Repo startet (Prompt-Injection-Risiko). Der in früheren Audits gemeldete discord-profile-Rollensync-Regress ist inzwischen behoben (Helix-Lookup + DiscordRolePort sind verdrahtet).

- **[HOCH · regression · bestätigt]** Öffentlicher Security-Report-POST startet claude-CLI mit --dangerously-skip-permissions (kein Python-Pendant)
  - Python: `bot/dashboard/admin/legal_mixin.py:1060 (abbo_sicherheit nur GET, statische Seite, KEIN Report-Formular/Handler)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/legal.rs:1513 security_report_handler + :1532 spawn_opus_analysis + :1393 run_opus_analysis_blocking`
  - Wirkung: Anonyme Internet-Eingabe triggert einen autonom code-editierenden Agenten mit Vollzugriff auf das Repo (Prompt-Injection → ungewollte Commits/Datenexfiltration über die Analyse). Zusätzlich Divergenz: die Rust- und Python-Sicherheitsseite liefern unterschiedliches HTML, entgegen dem im Modul-Docblock erklärten Ziel byte-identischer Live-Diff-Antworten.
  - Verifikation: Selbst am Code verifiziert. legal.rs:1560 registriert POST /twitch/sicherheit/report in build_legal_router; lib.rs:246 merged diesen Router OHNE Auth-Layer (anders als die admin-Router, die .layer(Extension(ExpectedToken)) tragen). Der Handler security_report_handler (legal.rs:1513) prüft nur title nicht leer + description.len()>=100, dann spawn_opus_analysis → run_opus_analysis_blocking (legal.rs:1393): std::process::Command::new('/home/naniadm/.local/bin/claude').args(['-p','--model','opus','-
  - Fix: Report-Formular + Route hinter Auth/Turnstile/Rate-Limit legen ODER den Opus-Auto-Fix-Pfad entfernen (nur DM ohne Code-Ausführung). Mindestens --dangerously-skip-permissions streichen und den Commit-Auftrag aus dem Prompt nehmen. Parität zu Python wiederherstellen (Form raus oder bewusst dokumentieren).
- **[MITTEL · divergence · umgestuft]** Nativer verify-Pfad clear/failed liefert 503 statt Departnering (Partner-Lifecycle nicht portiert)
  - Python: `bot/dashboard/streamer_admin_mixin.py:354 (_dashboard_verify_storage_step mode='clear') + :369 (mode='failed'): storage.departner_active_partner + Rollen-Entzug + DM`
  - Rust: `rust/crates/tb-internal-api/src/handlers/streamers.rs:415 (VerifyStreamerResult::RequiresPartnerLifecycle => ApiError::unavailable())`
  - Wirkung: Solange Python mitläuft kein User-Bruch (Proxy); bei Python-Abschaltung kann ein Admin Partner nicht mehr über das Dashboard zurückstufen (clear) oder als gescheitert markieren (failed) inkl. Rollen-Entzug und DM.
  - Verifikation: Code-Divergenz bestätigt, aber Schwere überzogen. verify_streamer (streamers_crud.rs:334) gibt für 'clear'|'failed' VerifyStreamerResult::RequiresPartnerLifecycle zurück — KEIN Departnering. Der Handler (streamers.rs:415) mappt das bewusst auf ApiError::unavailable() (503). Das Departnering existiert nur in Python: _dashboard_verify_storage_step (streamer_admin_mixin.py:354 clear / :369 failed) ruft storage.departner_active_partner in-process auf, plus Rollen-Entzug/DM in _dashboard_verify (:480
  - Fix: departner_active_partner + Rollen-Entzug (DiscordRolePort revoke) + optionale DM nativ in verify_streamer/verify_handler nachziehen; danach den 503-Zweig entfernen.
- **[MITTEL · divergence · unverif.]** recent-raids: executed_at-Format-Drift (Postgres ::text statt ISO isoformat)
  - Python: `bot/analytics/api_public.py:165 + :67 _serialize_timestamp (value.isoformat() → '2026-06-14T12:30:00+00:00')`
  - Rust: `rust/crates/tb-analytics/src/raids.rs:35 (executed_at::text AS executed_at)`
  - Wirkung: Frontend-Datumsparser (Date.parse/Intl) können das Postgres-Format teils nicht/abweichend interpretieren; Zeitanzeige auf der öffentlichen Raid-Liste kann brechen oder als invalid erscheinen.
  - Fix: to_char(executed_at, 'YYYY-MM-DD"T"HH24:MI:SS+00:00') oder in Rust chrono::DateTime laden und .to_rfc3339() — wie es session_detail.rs/raid_analytics.rs bereits korrekt machen.
- **[MITTEL · proxied · unverif.]** Markt-Dashboard: /twitch/market + /twitch/api/market_data nicht nativ (Proxy → Python 8765)
  - Python: `bot/dashboard/routes_market.py:20 (market_research HTML) + :21/:71 (api_market_data: Chat-Health/Lurker/Sentiment/Meta/Overlap)`
  - Rust: `—`
  - Wirkung: Migration unvollständig: ganze Markt-Research-Auswertung hängt am Python-Prozess; bei Python-Down nicht verfügbar.
  - Fix: Bei Bedarf nach tb-analytics portieren; aktuell bewusst proxied — als Lücke führen.
- **[MITTEL · proxied · umgestuft]** Live-Announcement-Config-Dashboard (5 Routen) nicht nativ — Proxy → Python 8765
  - Python: `bot/dashboard/live/live_announcement_mixin.py:207/248/282/358/419 (page, config GET/POST, test, preview) + routes_mixin.py:638-641`
  - Rust: `—`
  - Wirkung: Migration unvollständig: gesamte Live-Announcement-Konfiguration inkl. Auto-Erstellung der Ping-Rolle hängt am Python-Prozess; bei Python-Down kann kein Streamer die Ankündigung konfigurieren/testen.
  - Verifikation: Fakten bestätigt, Schwere überzogen. In tb-dashboard-api existiert keine Live-Announcement-Config-UI; die Treffer in tb-internal-api/tb-analytics telemetry_routes sind nur der Delivery-Pfad (live_active_announcements_handler liest Configs zum Ausspielen, live_link_click_handler trackt Klicks), nicht Config-Schreiben/Preview/Test/Ping-Rolle. Python hat die volle Logik: live_announcement_mixin.py:8 page (+_la_ensure_streamer_ping_role mit Rollen-Auto-Create + CSRF), api_live_announcement_config GE
  - Fix: Config-CRUD + Ping-Rollen-Autocreate (via Broker) nativ nachziehen, falls Cutover geplant; bis dahin als Lücke führen.
- **[MITTEL · proxied · unverif.]** Raid-Dashboard-Routen (auth/go/requirements/history/analytics/callback) nicht nativ — Proxy → Python 8765
  - Python: `bot/dashboard/raids/raid_mixin.py:182/258/295/409/425/522 (raid_auth_start/go/requirements/history/analytics/oauth_callback) + routes_mixin.py:621-625,637`
  - Rust: `rust/crates/tb-internal-api/src/handlers/raid_oauth.rs:414ff (auth-url/go-url/requirements/callback nativ als INTERNE API, nicht als Dashboard-HTML-Route)`
  - Wirkung: Solange Python läuft kein Bruch; Raid-Onboarding-Seiten + History/Analytics-HTML hängen am Python-Prozess.
  - Fix: Bewusst proxied; falls nötig HTML-Seiten nativ rendern. raid/requirements-Discord-DM braucht zusätzlich die in der internen API noch fehlende DM-Bridge (separat dokumentiert).
- **[MITTEL · proxied · umgestuft]** Streamer-Admin-Mutationen + Live-Dashboard-HTML (index/admin/partner_stats/add_*/verify/remove/archive/discord_flag/discord_link/chat_action) nicht nativ — Proxy → Python 8765
  - Python: `bot/dashboard/live/live.py:506 (index) / :1680-2086 (add_*/verify/remove/archive/discord_flag/chat_action) + streamer_admin_mixin.py:475 (_dashboard_verify macht ALLES in Python, ruft die interne Rust-API NICHT)`
  - Rust: `rust/crates/tb-dashboard-api/src/lib.rs:221 (nur GET admin/streamers list/detail nativ; keine Mutations-/HTML-Routen)`
  - Wirkung: Kern des Partner-Admin-Workflows ist nicht nativ; bei Python-Abschaltung bräche das Streamer-Management komplett. Aktuell via Proxy transparent.
  - Verifikation: Fakten bestätigt, Schwere überzogen. tb-dashboard-api registriert in build_admin_streamers_router (lib.rs:221-235) nur die zwei read-only GETs (/twitch/api/admin/streamers list + :login detail); keine Mutations-/HTML-Route. Die Python-Pendants existieren alle (live.py: index :506, add_any :1680, add_url :1696, add_login :1712, add_streamer :1728, admin_partner_chat_action :1790, discord_flag :1968, remove :2031, verify :2050, archive :2070) und führen Promotion/Departnering+Rollen-Sync+DM direkt
  - Fix: Als große offene Migrationslücke führen; vor Cutover die Python-internen Storage-Schritte (promote/departner/backfill) nativ + die HTML-Seiten portieren.
- **[NIEDRIG · divergence · unverif.]** recent-raids: viewers serialisiert null statt 0 bei NULL-Spalte
  - Python: `bot/analytics/api_public.py:164 (int(row[2] or 0) → immer Integer)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/raids.rs:16 (viewers: Option<i32>) + tb-analytics/src/raids.rs:21`
  - Wirkung: Geringfügig: Frontend muss null tolerieren; Zahlenoperationen (Summen/Render) können auf null straucheln statt auf 0.
  - Fix: In raids.rs COALESCE(viewer_count,0) bzw. Handler unwrap_or(0), um Pythons Integer-Vertrag zu treffen.
- **[NIEDRIG · divergence · unverif.]** network-Handler: keine lowercase/empty-Normalisierung des login + 500 statt Leerliste bei fehlender View
  - Python: `bot/analytics/api_public.py:219 (login=str(row[0] or '').strip().lower(); if not login: continue) + :189 has_partner_view-Fallback → {'streamers':[]}`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/network.rs:23 (login: r.twitch_login unverändert) + :48 (Err → 500)`
  - Wirkung: Bei gemischter Schreibweise/leeren Logins weicht das login-Feld ab (Frontend-Matching kann fehlschlagen); auf einem Schema ohne die Partner-View 500 statt sanfter Leerliste.
  - Fix: login im Handler .trim().to_lowercase() und leere überspringen; Query-Fehler tolerant auf leere streamers-Liste mappen (wie Python).
- **[NIEDRIG · divergence · unverif.]** Turnstile-remoteip wird ungefiltert aus CF-Connecting-IP gesetzt (Python nur bei Trusted-Proxy)
  - Python: `bot/dashboard/admin/legal_mixin.py:748 _legal_turnstile_remote_ip (CF-Connecting-IP nur wenn _is_trusted_proxy_host(peer), sonst request.remote)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/legal.rs:1021 (CF-Connecting-IP wird ohne Trusted-Proxy-Prüfung als remoteip übernommen)`
  - Wirkung: Sehr gering: remoteip ist bei Cloudflare-siteverify nur advisory; ein gefälschter Header kann die IP-Heuristik der Turnstile-Validierung stören, kippt das Gate aber nicht (success/action/hostname werden weiterhin geprüft).
  - Fix: CF-Connecting-IP nur bei vertrauenswürdigem Peer übernehmen, sonst die echte Client-IP (ConnectInfo) — Parität zu _legal_turnstile_remote_ip.
- **[NIEDRIG · proxied · unverif.]** market-share läuft via Doppel-Proxy (Python 8765 → internal Rust 8776), nicht nativ in tb-dashboard-api
  - Python: `bot/dashboard/routes_market.py:26 api_market_share (Admin-Gate + aiohttp-Proxy auf 127.0.0.1:8776/internal/twitch/v1/market-share)`
  - Rust: `rust/crates/tb-internal-api/src/handlers/market_share.rs:145 (Berechnung) + rust/crates/tb-analytics/src/market.rs:38 (market_share_series)`
  - Wirkung: Funktioniert; nur Architektur-Schuld (zwei Hops, Admin-Gate liegt in Python). Kein User-Bruch.
  - Fix: Optional: /twitch/api/v2/market-share direkt in tb-dashboard-api gegen tb-analytics::market verdrahten, um den Python-Doppel-Hop zu sparen.
- **[NIEDRIG · proxied · unverif.]** Admin-Announcements + Roadmap-Seiten (announcement_mode_mixin) nicht nativ — Proxy → Python 8765
  - Python: `bot/dashboard/admin/announcement_mode_mixin.py:227 (admin_announcements_page) + :344 (admin_announcements_save); routes_entry.py:28/30/31`
  - Rust: `—`
  - Wirkung: Admin-Announcement-Verwaltung hängt am Python-Prozess; kein User-sichtbarer Bruch solange Python läuft.
  - Fix: Bewusst proxied; als Lücke führen.
- **[NIEDRIG · divergence · unverif.]** discord-profile-Rollensync-Regress aus dem 2026-06-14-Audit ist behoben (Helix + DiscordRolePort verdrahtet) — Gegenprüfung
  - Python: `bot/dashboard/streamer_admin_mixin.py:212 _dashboard_save_discord_profile (raid_auth→Helix get_users + sync_streamer_role)`
  - Rust: `rust/crates/tb-internal-api/src/handlers/streamers.rs:539 discord_profile_handler (:589 load_twitch_user_id_from_raid_auth, :593 Helix get_users, :617 port.grant_streamer_role) + rust/bin/tb-bot/src/main.rs:686 (echter DiscordRolePort=BrokerDiscordDirectory) + oauth_followups.rs:106`
  - Wirkung: Kein Regress mehr bei discord-profile. Hinweis: dieser Handler wird aktuell vom Python-Dashboard ohnehin nicht aufgerufen (Python schreibt direkt), ist also nur für einen späteren Cutover relevant.
  - Fix: Audit-Befund als erledigt markieren; beim Cutover die Verifizierungs-Erfolgs-DM (verify permanent/temp) im nativen Pfad ergänzen, falls dort gewünscht.

### internal-api-routes

Interne API nahezu vollstaendig nativ; Hauptluecken in Findings.

- **[MITTEL · missing · unverif.]** GET debug/eventsub-processing fehlt nativ komplett
  - Python: `telemetry.py:274,329`
  - Rust: `-`
  - Wirkung: Debug-Endpoint im Rust-Betrieb tot (404/502).
  - Fix: Route registrieren, nativ befuellen oder Stub liefern.
- **[MITTEL · divergence · unverif.]** Idempotenz fuer streamer-CRUD und raid/requirements nativ nicht verdrahtet
  - Python: `streamers.py:39-326; raid.py:166`
  - Rust: `streamers.rs:207-539`
  - Wirkung: Retry mit gleichem Key fuehrt Mutation erneut aus statt Replay; kein 409 bei abweichendem Body.
  - Fix: IdempotencyState wie bei link-click verdrahten.
- **[MITTEL · divergence · unverif.]** Drei Routen nativ 503-Stub statt Funktion/Proxy: raid/requirements, chat-action, verify clear/failed
  - Python: `raid.py:166; streamers.py:470,147`
  - Rust: `python_stubs.rs:85,58; streamers.rs:415`
  - Wirkung: Raid-DM, Admin-Chat-Aktion, Departnering schlagen hart fehl.
  - Fix: Nativ implementieren oder Stub-Routen entfernen damit Proxy greift.
- **[NIEDRIG · divergence · unverif.]** Low-Drifts: Global-Ban-Mirror weg, require_link ignoriert, debug-Envelope, stale-Inflight 500
  - Python: `pg.py:4192; streamers.py:62; telemetry.py:27; app.py:515`
  - Rust: `global_ban.rs:62; streamers.rs:218; python_stubs.rs:11; idempotency.rs:240`
  - Wirkung: Je gering: Mirror bewusst weg; require_link wirkungslos; Shape-Drift; selten 500 statt 503.
  - Fix: Mirror optional; require_link lesen; Envelope spiegeln; stale 503 senden.

### api-transport-token

Der reine Helix-/OAuth-Transport ist in Rust (tb-transport-twitch + tb-chat/token.rs + tb-raid/token_*) sehr sauber und verhaltensnah portiert: App-Token-Client-Credentials, User-Token-Refresh/Exchange/Owner-Lookup, Bot-Token-Manager (Validate→Refresh, 30-min-Loop, 1h-Schwelle), Streams/Kategorien/Followers/EventSub-Webhook/Clip/Ban/Announcement/Raid und der verschlüsselte Raid-Token-Lese-/Schreibpfad inkl. Advisory-Lock (byte-identisch zu Python). Die InvalidClient/InvalidGrant-Klassifikation und der Sofort-Lockout (raid_enabled=FALSE/needs_reauth=TRUE ab dem ersten invalid_grant, früher ein Audit-Befund) sind nachgezogen. ABER: Der gesamte TokenErrorHandler-Lifecycle aus bot/api/token_error_handler.py (1402 Zeilen) ist NICHT portiert — Grace-Period-Handling, Discord-Admin-Notify, Streamer-DMs (Token-Fehler + Bot-Ban-Recovery), Rollen-Sync, Partner-Mirror (technical_pause_reason), Bot-Ban-Opt-out + Restore, cleanup_old_entries. In Rust existiert nur der nackte Blacklist-Counter (tb-raid/token_blacklist.rs). Diese Funktionen laufen heute noch im Python-Prozess (raid/bot.py-Wartungsloop bzw. chat/connection.py), wobei der notify-Pfad im Chat-Send durch den Chat-Flip in Python tot ist. Zusätzlich fehlen dem App-Token-Pfad mehrere Robustheits-Eigenschaften des Python-TwitchAPI (15-min-Auth-Cooldown bei invalid_client, Credential-Pre-Check, Retry-Backoff mit 3 Versuchen, 500-5xx-Retry, structured _helix_result mit error_code-Mapping, expires_in-Default 3600). Die strukturierten *_result-Methoden (followers/subscriptions/ads/chatters) mit error_code-Mapping sind vereinfacht oder fehlen ganz.

- **[HOCH · missing · bestätigt]** Grace-Period-Handling (check_grace_periods) komplett unportiert
  - Python: `bot/api/token_error_handler.py:1225 check_grace_periods`
  - Rust: `—`
  - Wirkung: Nach Token-Ablauf bekommt der Streamer im reinen Rust-Betrieb keine Erinnerung, behält dauerhaft die Streamer-Rolle und wird nie als manuelles Opt-out markiert; der Lifecycle (Rolle entziehen nach 7 Tagen) findet nicht statt. Hängt heute am Python-Wartungsloop.
  - Verifikation: Selbst verifiziert. Python token_error_handler.py:1225 check_grace_periods liest stündlich abgelaufene Grace-Periods (error_count>=3 AND grace_expires_at<=now AND role_removed=0), schickt Reminder-DM + _notify_admin_grace_expired, entfernt via schedule_streamer_role_sync die Rolle und schreibt twitch_partners manual_partner_opt_out=1/technical_pause_reason='token_error_expired'/raid_bot_enabled=0 + twitch_raid_auth needs_reauth=TRUE + role_removed=1 (Zeilen 1234-1306). Aufrufer raid/bot.py:307 l
  - Fix: Stündlichen Maintenance-Task in tb-raid/tb-bot bauen, der die Grace-Logik aus token_error_handler.py:1225-1322 nachbildet (Query + Rollen-Sync via Broker + Partner-/Auth-Mirror + role_removed-Flag). Discord-Sends über den Master-Broker-Relay.
- **[HOCH · missing · bestätigt]** Discord-Token-Fehler-Notify (notify_token_error) unportiert
  - Python: `bot/api/token_error_handler.py:902 notify_token_error / :1035 _send_user_dm_token_error`
  - Rust: `—`
  - Wirkung: Bei widerrufenem Streamer-Token erfährt weder Admin noch Streamer etwas; die Reauth-Aufforderung (DM mit /traid-Hinweis) entfällt. Der connection.py:1054-Pfad ist durch den Chat-Flip in Python ohnehin tot.
  - Verifikation: Selbst verifiziert. Python token_error_handler.py:902 notify_token_error baut Admin-Embed (TOKEN_ERROR_CHANNEL_ID 1374364800817303632) + _send_user_dm_token_error, entprellt via notified-Flag (Z.922-934). Der native Rust-InvalidGrant-Pfad token_refresher.rs:213-219 ruft ausschließlich self.blacklist.add_to_blacklist(...) auf — kein Notify, keine DM, keine Discord-Bridge. grep über tb-raid/tb-chat: token_refresher.rs und token_blacklist.rs enthalten 0 Treffer für notify/discord/bridge/DM. Der Rai
  - Fix: Nach RefreshError::InvalidGrant im token_refresher (oder im Blacklist-add) einen DiscordNotify-Port aufrufen, der Admin-Embed + Streamer-DM einmalig (notified-Spalte) sendet — analog notify_token_error/_send_user_dm_token_error.
- **[HOCH · missing · bestätigt]** Bot-Ban-Recovery (handle_bot_banned_channel + DM + restore) unportiert
  - Python: `bot/api/token_error_handler.py:323 handle_bot_banned_channel / :457 restore_bot_banned_channel / :342 _send_user_dm_bot_banned`
  - Rust: `—`
  - Wirkung: Ein im Kanal gebannter Bot wird in Rust nicht als technisches Opt-out geführt, der Streamer bekommt keine Anleitung zum Entbannen, und der Auto-Restore nach Health-Wiederherstellung fehlt. Hängt am Python-Chat-/Mod-Pfad, der teils geflippt ist.
  - Verifikation: Selbst verifiziert. Python token_error_handler.py:323 handle_bot_banned_channel ruft _mark_partner_opt_out_only (raid_enabled=FALSE + technical_pause_reason='bot_banned' + Partner-Mirror) + _send_user_dm_bot_banned (konkrete /unban + /mod Recovery-DM, Z.342-455) und :457 restore_bot_banned_channel stellt den Zustand bei Bot-Health-Recovery wieder her. Aufrufer chat/moderation.py:1259, chat/connection.py:947, analytics/mixin.py. Adversarial geprüft: der Rust-tb-chat hat NUR TimeoutGuard (moderati
  - Fix: _mark_partner_opt_out_only + restore_bot_banned_channel als nativen Partner-/Auth-Store-Pfad portieren und an die Rust-Moderation/Connection-401-Erkennung hängen; Recovery-DM über Discord-Port.
- **[HOCH · divergence · bestätigt]** Partner-Mirror in add_to_blacklist nur teilweise portiert (_disable_raid_bot/_mark_reauth_required Partner-Teil fehlt)
  - Python: `bot/api/token_error_handler.py:161 _mark_reauth_required / :767 _disable_raid_bot`
  - Rust: `rust/crates/tb-raid/src/token_blacklist.rs:199-209 add_to_blacklist_inner`
  - Wirkung: twitch_partners bleibt nach invalid_grant auf raid_bot_enabled=1/ohne technical_pause_reason. Dashboard/Analytics-Gates, die auf technical_pause_reason='token_error' reagieren, greifen nicht; der Streamer erscheint weiter als aktiv obwohl sein Token tot ist.
  - Verifikation: Selbst verifiziert am Code. Rust token_blacklist.rs:199-209 add_to_blacklist_inner schreibt ausschließlich twitch_raid_auth SET raid_enabled=FALSE, needs_reauth=TRUE, twitch_login. Python _mark_reauth_required (token_error_handler.py:203-238) spiegelt zusätzlich set_partner_raid_bot_enabled(conn, enabled=False) UND twitch_partners SET technical_pause_reason=CASE (Guards: manual_partner_opt_out=1 bzw. 'bot_banned' nicht überschreiben, sonst 'token_error'), raid_bot_enabled=0. Der Rust-Doc-Komment
  - Fix: Im add_to_blacklist_inner (oder einem Partner-Port) die zwei Partner-UPDATEs aus _mark_reauth_required nachziehen, inkl. der manual_partner_opt_out/'bot_banned'-Guards.
- **[MITTEL · divergence · unverif.]** App-Token-Client ohne 15-min invalid_client-Cooldown / Credential-Pre-Check
  - Python: `bot/api/twitch_api.py:130 _ensure_token (block_auth 900s) / :117 _ensure_client_credentials`
  - Rust: `rust/crates/tb-transport-twitch/src/token.rs:79 fetch_app_token, client.rs:78 access_token`
  - Wirkung: Bei kaputtem TWITCH_CLIENT_SECRET hämmert der Rust-App-Token-Pfad ungebremst gegen id.twitch.tv (Rate-Limit/Sperre-Risiko) statt 15 min zu pausieren wie Python.
  - Fix: In HelixClient einen auth_blocked_until-Zustand + invalid_client-Erkennung (analog user_token::is_invalid_client) ergänzen; bei 400/invalid client 900s suppress, bei Erfolg zurücksetzen; leere Credentials vorab abfangen.
- **[MITTEL · divergence · unverif.]** App-Token-/GET-/POST-Pfad ohne Retry-Backoff (3 Versuche, 5xx-Retry)
  - Python: `bot/api/twitch_api.py:248 _post (max_attempts, 0.5*(n+1)) / :356 _get (500/502/503/504-Retry) / :141 _ensure_token (3 Versuche)`
  - Rust: `rust/crates/tb-transport-twitch/src/client.rs:104 get/116 post, check_status_and_json:227`
  - Wirkung: Transiente Twitch-5xx/Timeouts führen in Rust zu sofortigem Fehler statt zum Python-Retry; mehr Fehlschläge bei Go-Live-Spikes/Twitch-Wackler.
  - Fix: Einen schmalen Retry-Wrapper (3 Versuche, 0.5/1.0/1.5s, nur auf 5xx + reqwest-Transport-Fehler) um die Helix-Sends legen; idempotente GETs primär.
- **[MITTEL · divergence · unverif.]** Strukturierte *_result-Methoden mit error_code-Mapping fehlen (followers/subscriptions/ads/chatters)
  - Python: `bot/api/twitch_api.py:222 _map_helix_error_code / :728 get_followers_total_result / :836 subscriptions_result / :927 ad_schedule_result / :1021 get_chatters_result`
  - Rust: `rust/crates/tb-transport-twitch/src/streams.rs:250 get_followers_total (nur Option<i64>)`
  - Wirkung: Aufrufer können in Rust nicht zwischen 'kein Scope', 'rate-limited', 'token mismatch' unterscheiden (alles wird zu None/Fehler kollabiert); subscriptions/ads/chatters sind als nativer Helix-Aufruf nicht verfügbar — die zugehörigen Features müssen Python nutzen.
  - Fix: get_broadcaster_subscriptions/get_ad_schedule/get_chatters (mit Pagination) nativ ergänzen und ein error_code-Mapping analog _map_helix_error_code einführen, damit Aufrufer die Fehlerklasse kennen.
- **[MITTEL · missing · unverif.]** schedule_streamer_role_sync (Discord-Rollen-Sync) im Token-Fehler-Pfad unportiert
  - Python: `bot/api/token_error_handler.py:137 schedule_streamer_role_sync (→ discord_role_sync)`
  - Rust: `—`
  - Wirkung: Streamer mit dauerhaft totem Token behalten ihre Discord-Rolle, weil der Entzug nur im Python-Grace-Loop passiert; im nativen Pfad gibt es keinen Rollen-Sync.
  - Fix: Beim nativen Grace-Handling den Broker-/Discord-Rollen-Sync-Port aufrufen (should_have_role=false) analog schedule_streamer_role_sync.
- **[NIEDRIG · divergence · unverif.]** create_clip: title/duration-Parameter gedroppt
  - Python: `bot/api/twitch_api.py:599 create_clip (title 60-Char-Trim, duration 15-60s-Clamp, has_delay)`
  - Rust: `rust/crates/tb-transport-twitch/src/client.rs:180 create_clip`
  - Wirkung: Gewünschte Clip-Dauer/Titel werden nie an Twitch übergeben; has_delay ist hart false. Praktisch gering, da Helix den Clip aus dem Buffer schneidet — aber Verhaltensabweichung zur Python-Referenz.
  - Fix: Falls Parität gewünscht: broadcaster_id über query-Builder setzen und optional title (60-Char-Trim) + duration (15-60-Clamp) + has_delay als Query-Param ergänzen; sonst die Python-Seite ebenfalls auf 'nur buffer' angleichen und dokumentieren.
- **[NIEDRIG · divergence · unverif.]** App-Token TokenResponse.expires_in ohne Default (Parse-Fehler bei fehlendem Feld)
  - Python: `bot/api/twitch_api.py:166 expires = js.get('expires_in', 3600)`
  - Rust: `rust/crates/tb-transport-twitch/src/token.rs:64-68 TokenResponse`
  - Wirkung: Theoretischer Edge-Case (Twitch liefert expires_in praktisch immer); bei einer abweichenden Token-Antwort bräche der Rust-App-Token-Abruf hart statt zu defaulten.
  - Fix: #[serde(default = "default_expires")] mit 3600 auf TokenResponse.expires_in setzen, analog Python.
- **[NIEDRIG · divergence · unverif.]** get_streams_for_game game_name-Fallback (ohne game_id) nicht portiert
  - Python: `bot/api/twitch_api.py:517 get_streams_for_game (else-Zweig: ohne game_id scannen + nach game_name filtern)`
  - Rust: `rust/crates/tb-transport-twitch/src/streams.rs:134 get_streams_by_category`
  - Wirkung: Falls die Kategorie-ID nicht auflösbar ist, liefert Rust gar nichts statt des Python-Fallback-Scans. Geringe Auswirkung, da search_category_id i.d.R. die ID liefert.
  - Fix: Optional: einen Fallback ergänzen, der bei fehlender game_id Streams scannt und nach game_name filtert; oder bewusst als entfallen dokumentieren.
- **[NIEDRIG · missing · unverif.]** cleanup_old_entries (30-Tage-Blacklist-Cleanup) unportiert
  - Python: `bot/api/token_error_handler.py:1373 cleanup_old_entries`
  - Rust: `—`
  - Wirkung: twitch_token_blacklist wächst im reinen Rust-Betrieb monoton, alte Einträge werden nicht aufgeräumt. Geringe Tabelle, kosmetisch.
  - Fix: Im Rust-Maintenance-Loop ein periodisches DELETE WHERE last_error_at < now-30d ergänzen.
- **[NIEDRIG · missing · unverif.]** _migrate_db (idempotente Schema-Migration) unportiert
  - Python: `bot/api/token_error_handler.py:55 _migrate_db`
  - Rust: `—`
  - Wirkung: Rust verlässt sich darauf, dass Python die Migration schon gemacht hat; auf einem frischen Schema ohne Python-Init würden die ALTER-TABLEs fehlen und Rust-Inserts/Reads auf die neuen Spalten brechen.
  - Fix: Migration in den Rust-Bootstrap (migrations/) aufnehmen, damit der native Pfad nicht von Pythons _migrate_db abhängt.
- **[NIEDRIG · missing · unverif.]** _post 202-Akzeptanz + EventSub-WebSocket-Subscription nicht nativ
  - Python: `bot/api/twitch_api.py:1130 subscribe_eventsub_websocket`
  - Rust: `—`
  - Wirkung: Kein funktionaler Verlust im aktuellen Webhook-only-Betrieb (bewusste Architekturentscheidung), aber die Python-Oberfläche subscribe_eventsub_websocket hat in Rust keine Entsprechung — relevant nur falls je auf WS umgestellt würde.
  - Fix: Als bewusst entfallen (ADR-0004) dokumentieren; nur portieren, falls WS-Transport reaktiviert wird.
- **[NIEDRIG · divergence · unverif.]** TwitchTokenClient.exchange_code ohne redirect_uri-Parameter (fest verdrahtet im Adapter)
  - Python: `bot/raid/auth.py:864 exchange_code_for_token (redirect_uri=self.redirect_uri)`
  - Rust: `rust/crates/tb-raid/src/token_refresher.rs:63 exchange_code(code) / rust/bin/tb-bot/src/raid_adapters.rs:118 HelixTokenClient.redirect_uri`
  - Wirkung: Kein Defekt, solange die Adapter-redirect_uri korrekt konfiguriert ist; aber die feste Verdrahtung verbirgt eine potenzielle Fehlkonfigurationsquelle, die Twitch beim authorization_code-Grant hart ablehnt.
  - Fix: Belassen; sicherstellen dass HelixTokenClient.redirect_uri 1:1 dem in der OAuth-URL verwendeten redirect_uri entspricht (Test/Assertion beim Wiring).

### storage-data

Die Behauptung des Vor-Audits ("bot/storage = ported") hält einer genauen Prüfung nicht stand. Faktisch portiert ist nur die *Zugriffs*-Infrastruktur in einzelnen Feature-Crates (sqlx-Pool, DB-Identitäts-Fingerprint, Fernet-Session-Krypto, Offline-Raid-Eligibility, Promo-Cooldowns, Global-Ban). Die eigentlichen *Storage-Layer-Verantwortlichkeiten* aus pg.py fehlen weitgehend: die komplette Schema-Verwaltung (ensure_schema, Billing-Schema, Runtime-Migrationen v1–v7, schema_version-Tracking, Startup-Maintenance wie Sequenz-Alignment und Boolean-Coercion) ist in keinem Rust-Binary verdrahtet — weder tb-bot noch tb-dashboard rufen run_migrations/MIGRATOR auf; der Migrator ist ein dokumentierter No-op mit eigener _sqlx_migrations-Tabelle, der das Python-Schema bewusst NICHT anfasst. Folge: Python (prepare_runtime_storage) bleibt der alleinige Schema-Owner; läuft der Python-Prozess nicht, wird kein Schema angelegt/migriert und keine Startwartung ausgeführt. tb-db selbst ist dünn (Pool + No-op-Migrator + 3 Row-Structs, davon 1 komplett ungenutzt, 2 nur in Tests); der konfigurierte connect_timeout wird geparst, aber nie auf den Pool angewandt. tb-domain ist zu ~75% Gerüst-Leiche: PartnerStatus, StreamerLogin und TwitchUserId werden nirgends konsumiert, nur normalize_twitch_login ist live. Der Observability-Event-Writer ist ein Tracing-Stub (insert_observability_event unportiert). Sämtliche Partner-Registry-Mutationen laufen weiter über Python (Proxy bzw. 503). Mehrere exportierte Python-Helfer (auto_raid_pause, Transaktions-Retry/Isolation) sind allerdings auch in Python schon tot und daher nur formal "fehlend".

- **[KRITISCH · missing · bestätigt]** Schema-Verwaltung (ensure_schema + Runtime-Migrationen v1–v7) komplett unportiert — kein Rust-Binary migriert
  - Python: `bot/storage/pg.py:1636 ensure_schema, :974 _apply_runtime_schema_migrations, :1079 prepare_runtime_storage, :1482 ensure_billing_entitlement_schema`
  - Rust: `rust/crates/tb-db/src/migrate.rs:14 run_migrations, bin/tb-bot/src/main.rs:134, bin/tb-dashboard/src/main.rs:21`
  - Wirkung: Python-Prozess ist alleiniger Schema-Owner. Ein reiner Rust-Betrieb (oder Python-Down beim Boot gegen eine frische/teilmigrierte DB) legt Tabellen/Indizes nicht an und führt keine Schema-Migration aus → die nativen Rust-Reads/Writes laufen gegen ein nicht garantiert vorhandenes Schema. Migration ist hier strukturell unvollständig.
  - Verifikation: Am Code bestätigt. migrate.rs:14-18 ist ein No-op (MIGRATOR.run, Doku Z.1-4 'wendet nichts an'). grep über bin/tb-bot + bin/tb-dashboard nach run_migrations/MIGRATOR/ensure_schema/migrate/prepare_runtime = 0 Treffer; run_migrations wird AUSSCHLIESSLICH in tb-db/tests/hermetic.rs:62 aufgerufen. Beide main.rs rufen nur tb_db::connect(). Die einzige existierende SQL-Migration (migrations/20260612120000_add_stats_leaderboard_indexes.sql, nur 2 Indizes) wird damit produktiv NIE angewandt. Python-Pend
  - Fix: Schema als SSOT in rust/migrations/ als .sql nachziehen (ensure_schema + Billing + Drops als versionierte Migrationen) und run_migrations in beiden Bins vor dem Serven aufrufen; bis dahin ehrlich dokumentieren, dass Python den Schema-Bootstrap besitzt.
- **[HOCH · missing · bestätigt]** Startup-Maintenance (Sequenz-Alignment, Boolean-Coercion, Live-State-Dedup, Unique-Indizes) nicht portiert
  - Python: `bot/storage/pg.py:522 _run_startup_maintenance (+ _align_serial_sequence:295, _coerce_column_to_boolean:317, _cleanup_duplicate_live_state_rows:618, _ensure_unique_live_state_login_index:631)`
  - Rust: `—`
  - Wirkung: Sequenzen können nach manuellen Inserts/Restores driften (ID-Kollision bei nativen sqlx-INSERTs in twitch_stream_sessions/twitch_raid_history), und nicht-koerzierte Boolean-Spalten könnten native FromRow-Decodes brechen. Solange Python bootet, wird das verdeckt — nativer Alleinbetrieb verliert die Selbstheilung.
  - Verifikation: Bestätigt. pg.py:522 _run_startup_maintenance richtet SERIAL-Sequenzen (twitch_stream_sessions/twitch_raid_history/clip_fetch_history/twitch_clips_social_media), Boolean-Spalten (twitch_session_chatters.*, twitch_chat_messages.is_command) und Live-State-Eindeutigkeit (:618 _cleanup_duplicate_live_state_rows, :631 _ensure_unique_live_state_login_index = UNIQUE INDEX idx_twitch_live_state_login_lower) gerade. grep in crates/+bin/ nach align_serial/coerce_column_to_boolean/cleanup_duplicate_live_st
  - Fix: Maintenance-Schritte als idempotente Migration/Boot-Task in Rust nachbauen oder explizit dokumentieren, dass Python diesen Wartungslauf weiterhin exklusiv ausführt.
- **[MITTEL · proxied · umgestuft]** Partner-Registry-Mutationen (promote/departner/reactivate/archive/block/flags/discord-profile) nicht nativ — Proxy bzw. 503
  - Python: `bot/storage/partner_registry.py:782 promote_streamer_to_partner, :1130 departner_active_partner, :1283/:1355 reactivate_*, :1447 set_streamer_archive_state, :1558 set_streamer_block_state, :1808 set_partner_silent_flags, :1847 set_partner_live_ping_settings, :499 upsert_streamer_identity, :563 upsert_non_partner_streamer, :1885 bulk_update_partner_flags, :1957 migrate_legacy_partner_registry`
  - Rust: `rust/crates/tb-internal-api/src/handlers/streamers.rs:25-32 (Doku), :399-405 verify clear/failed → 503`
  - Wirkung: Der gesamte schreibende Partner-Lebenszyklus (Aufnahme/Departnering/Archivierung/Block/Flags) ist nicht in Rust; bei Python-Ausfall blockieren diese Admin-Aktionen (503) bzw. hängen am Proxy. Migration der Storage-Schreibschicht ist hier unvollständig.
  - Verifikation: Teilweise falsch — der erste Agent hat die nativen Implementierungen übersehen. NATIV vorhanden UND im Router verdrahtet (lib.rs:232-253): add/remove (streamers_crud.rs:114/230), verify permanent/temp (verify_streamer :300), archive/block (archive_streamer :398, ArchiveMode block/unblock), discord-flag (:534), discord-profile (:592), upsert_non_partner (über add, Doku :109). promote_streamer_to_partner ist nativ in tb-raid/src/partner_setup.rs:485 implementiert UND produktiv verdrahtet über die 
  - Fix: Partner-Lifecycle-Writes in tb-analytics/streamers_crud + tb-internal-api nativ nachziehen (inkl. Discord-Rollen-Sync-Bridge) und 503-Stubs ersetzen.
- **[MITTEL · missing · umgestuft]** Observability-Event-Writer (insert_observability_event + Batch-Queue) ist ein Tracing-Stub
  - Python: `bot/storage/pg.py:1363 insert_observability_event, :1252 _observability_writer_loop, :1219 _flush_observability_batch (→ twitch_observability_events)`
  - Rust: `rust/crates/tb-observability/src/lib.rs:4 (Doku-Stub), :11 init_tracing`
  - Wirkung: Die strukturierten Flow-/Decision-Events (flow_type/step/decision/details_json), die Python für Diagnose/Dashboards in twitch_observability_events schreibt, werden im nativen Pfad gar nicht persistiert. Stille Beobachtbarkeitslücke für alle nativ portierten Flows (Chat/Raid/Monitoring).
  - Verifikation: Am Code bestätigt, aber Severity überzogen. tb-observability/src/lib.rs enthält NUR init_tracing (fmt+EnvFilter), keinen mpsc-Writer; grep über crates/+bin/ nach twitch_observability_events/insert_observability/observability_writer/flush_observability trifft NUR die Doku-Kommentarzeile in lib.rs:4 — kein Crate schreibt Observability-Events in die DB. Python-Pendant vorhanden: pg.py:1363 insert_observability_event, :1252 _observability_writer_loop, :1219 _flush_observability_batch, Tabelle twitch
  - Fix: Batched mpsc-Writer (skip decision=='failed', Längen-Truncation 40/80, JSON sort_keys) in tb-observability implementieren und in den nativen Pipelines verdrahten.
- **[MITTEL · divergence · unverif.]** connect_timeout wird in DbConfig geladen, aber nie auf den Pool angewandt
  - Python: `bot/storage/pg.py:689 _dsn_with_connect_timeout (injiziert connect_timeout in DSN), :142 _CONNECTION_CONNECT_TIMEOUT_SECONDS (Default 5)`
  - Rust: `rust/crates/tb-db/src/pool.rs:9-15 connect, rust/crates/tb-config/src/lib.rs:61 connect_timeout`
  - Wirkung: Beim DB-Verbindungsaufbau gilt nicht das konfigurierte 5s-Limit, sondern das sqlx/libpq-Default-Verhalten. Bei hängendem DB-Host blockiert ein Connect potenziell länger als in Python, statt schnell zu scheitern.
  - Fix: In pool.rs PgConnectOptions aus dem DSN bauen und .connect_timeout via PgConnectOptions::… bzw. eine after_connect/timeout-Strategie setzen, oder den Wert (wie Python) in die DSN-Conninfo einfügen.
- **[MITTEL · divergence · unverif.]** verify-Flow: native permanent/temp promotet — anders als Python — keine Nicht-Partner; clear/failed gar nicht nativ
  - Python: `bot/storage/partner_registry.py:2188 verification_payload (permanent/temp/clear/failed) + promote_streamer_to_partner-Aufruf für Nicht-Partner`
  - Rust: `rust/crates/tb-analytics/src/streamers_crud.rs:297 (Kommentar 'Teilport: Python promotet bei permanent/temp auch Nicht-Partner'), :307 temp, :321 permanent`
  - Wirkung: Manuelle Verifizierung eines noch-nicht-Partners hat in Rust keinen Effekt (Python hätte den Streamer befördert); clear/failed (Verifizierung zurücknehmen/Departnering) schlägt nativ fehl. Verhaltensabweichung im Admin-Verify-Pfad.
  - Fix: permanent/temp um Promote-Pfad für Nicht-Partner ergänzen und clear/failed nativ implementieren (verification_payload-Semantik 1:1).
- **[NIEDRIG · divergence · unverif.]** tb-domain PartnerStatus/StreamerLogin/TwitchUserId sind toter Code; PartnerStatus modelliert 'departnered' nicht
  - Python: `bot/storage/partner_registry.py:9-11 PARTNER_STATUS_* , :49-60 _is_departnered_status/_is_inactive_partner_status`
  - Rust: `rust/crates/tb-domain/src/partner.rs:5 PartnerStatus, src/ids.rs:7/11 StreamerLogin/TwitchUserId`
  - Wirkung: Domänen-Typisierung bringt keinen Nutzen (vestigiales Gerüst); die Status-Semantik (insb. departnered als eigener Zustand) ist nur implizit über verstreute String-Vergleiche abgedeckt, nicht zentral typisiert. Kein Laufzeit-Bug, aber irreführende 'native Domäne'.
  - Fix: Entweder PartnerStatus um Departnered erweitern und in den Feature-Crates verwenden, oder die ungenutzten Typen als bewusst-deferred kennzeichnen statt als 'portiert'.
- **[NIEDRIG · missing · unverif.]** Admin-Auto-Raid-Pause-Storage (twitch_auto_raid_pause) nicht portiert
  - Python: `bot/storage/auto_raid_pause.py:28 set_auto_raid_pause, :77 get_auto_raid_pause, :95 is_auto_raid_paused, :65 clear_auto_raid_pause; Tabelle in pg.py:2969`
  - Rust: `—`
  - Wirkung: Storage-Oberfläche fehlt in Rust, aber kein aktueller User-Bruch, da der Pause-Gate in Python aktuell tot ist. Würde der Gate reaktiviert, fehlte die native Durchsetzung.
  - Fix: Bei späterer Aktivierung der Admin-Pause: Tabelle + set/clear/get/is_paused nativ und den Gate im offline-Raid-Pfad portieren. Vorerst als bekannte dormant-Lücke führen.
- **[NIEDRIG · missing · unverif.]** Transaktions-Retry (40001/40P01) + REPEATABLE READ/SERIALIZABLE-Transaktionen nicht portiert
  - Python: `bot/storage/pg.py:769 _run_transaction_operation (Retry), :1168 repeatable_read_transaction, :1176 serializable_transaction, :163 _RETRYABLE_SQLSTATES`
  - Rust: `—`
  - Wirkung: Kein aktueller Funktionsverlust, da die höher-isolierten/retry-fähigen Transaktionspfade in Python nicht aufgerufen werden. Falls künftig serialisierbare Transaktionen nötig werden, fehlt in Rust die Retry-Schicht.
  - Fix: Nicht dringend; bei Bedarf einen with_serialization_retry-Helper in tb-db ergänzen. Vorerst als nicht-genutzte Surface dokumentieren.
- **[NIEDRIG · divergence · unverif.]** tb-db Row-Structs: TwitchPartnerRow ungenutzt, übrige nur in Tests — kaum reale Lesepfad-Nutzung
  - Python: `bot/storage/_rows.py:10 StorageRow / storage_row_factory (universelle Row-Abstraktion)`
  - Rust: `rust/crates/tb-db/src/rows.rs:8 TwitchStreamerRow, :19 TwitchPartnerRow, :31 StreamerPlanRow`
  - Wirkung: Kein Korrektheitsbug (Typmapping ist faithful), aber die zentrale Row-Schicht ist weitgehend vestigial — die Storage-Konsolidierung ('Repositories in tb-db') hat nicht stattgefunden; jeder Feature-Crate definiert eigene Row-Typen.
  - Fix: Ungenutzte Row-Structs entfernen oder zu echten Repository-Funktionen ausbauen; Entscheidung dokumentieren, dass Reads dezentral je Feature-Crate leben.
- **[NIEDRIG · divergence · unverif.]** DB-Fingerprint-DSN-Parsing schmaler als psycopg conninfo_to_dict (Edge-DSN-Formen)
  - Python: `bot/storage/pg.py:219 _dsn_conninfo (conninfo_to_dict), :252 _analytics_db_identity_fields`
  - Rust: `rust/crates/tb-internal-api/src/handlers/healthz.rs:63 analytics_identity_fields`
  - Wirkung: Nur für ungewöhnliche DSN-Formen relevant; der Fingerprint ist nicht load-bearing für Korrektheit (Diagnose/Health-Anzeige). Bei abweichendem Fingerprint könnten Health-/Safety-Checks, die Python- und Rust-Fingerprint vergleichen, fälschlich Mismatch melden.
  - Fix: Für die in Prod genutzte URL-DSN-Form ist die Parität gegeben; bei breiterer DSN-Unterstützung conninfo-äquivalentes Parsing nachziehen.

### entitlements-crypto

Der Krypto-Kern ist exzellent portiert: AES-256-GCM-Feldverschlüsselung (tb-crypto/field.rs) ist byte-identisch zum Python-FieldCrypto inkl. Blob-Framing (version|kid_len|kid|nonce|ct+tag), und die Fernet-Implementierung (auth/fernet.rs) ist mit echten Python-Interop-Testvektoren und einem Rust-encrypt→Python-decrypt-Test belegt — beide faithful. Auch die AAD-Builder stimmen exakt. Der Entitlement-Katalog (plan.rs) ist seit dem 13.6.-Audit erheblich verbessert: die damals kritische plan_entitlements-Divergenz (fehlendes ai_full, zusammengefasste Match-Arme) ist GEFIXT — alle 9 Pläne stimmen jetzt 1:1 mit catalog.py (verifiziert). Verbleibende Divergenzen liegen im DB-Resolver: (1) Rust matched streamer_plans/Overrides nur per twitch_login, Python per twitch_user_id ODER twitch_login — ein nur per user_id eingetragener Override/Plan wird in Rust nicht gefunden (relevant, weil Admin-Writes primär auf twitch_user_id konflikt-upserten und twitch_login leer sein kann); (2) das Extended-Gate (has_extended_entitlement) liest gar kein Stripe-Abo, nur streamer_plans — ein Partner mit reinem Stripe-Abo ohne manual_plan-Zeile bekommt fälschlich plan_required; (3) der Trial-Onboarding-Pfad ist gegensätzlich gebaut: Rust grantet sofort beim OAuth-Callback ohne 24h/first_login-Gate, Python grantet NUR nach 24h-Grace beim Plan-Resolve und hat keinen Sofort-Grant. Dazu kleinere Feld-/Parsing-Lücken (fehlende manual_override/billing_subscription-Objekte im auth-status-Plan, offset-loser Timestamp-Parse in plan.rs inkonsistent zu level/mod.rs, kein current_period_end-Spaltenfallback).

- **[HOCH · divergence · bestätigt]** Plan-Resolver matched streamer_plans-Overrides nur per twitch_login, nicht per twitch_user_id
  - Python: `bot/entitlements/repository.py:load_manual_override (Z.98-144, WHERE twitch_user_id=%s OR LOWER(twitch_login)=LOWER(%s))`
  - Rust: `rust/crates/tb-analytics/src/plan.rs:186-197 (resolve_plan_snapshot, WHERE LOWER(twitch_login)=LOWER($1))`
  - Wirkung: Ein Manual-Override oder Plan, der nur unter twitch_user_id eingetragen ist (oder dessen twitch_login sich nach Namensaenderung unterscheidet), wird im Rust-Dashboard nicht gefunden -> Streamer sieht raid_free/default_basic statt seines bezahlten/gecompten Plans, verliert Entitlements und Dashboard-Zugang.
  - Verifikation: Selbst am Code verifiziert. Python load_manual_override (repository.py:98-144) matched 'TRIM(COALESCE(twitch_user_id,''))=%s OR LOWER(COALESCE(twitch_login,''))=LOWER(%s)' und priorisiert per CASE den user_id-Treffer. Rust resolve_plan_snapshot (plan.rs:186-197) hat nur 'WHERE LOWER(twitch_login)=LOWER($1)' und bekommt vom einzigen Caller auth_status.rs:195 ausschliesslich 'login' uebergeben, obwohl der Partner-AuthLevel laut level.rs:31/194 sowohl twitch_login als auch twitch_user_id traegt. Wr
  - Fix: resolve_plan_snapshot um den twitch_user_id-Pfad erweitern: Signatur auf (login, user_id) bringen (auth_status.rs hat user_id bereits aus der Partner-Session) und WHERE auf `TRIM(COALESCE(twitch_user_id,''))=$2 OR LOWER(twitch_login)=LOWER($1)` mit der Python-ORDER-BY-Prioritaet (user_id-Match zuerst) angleichen.
- **[HOCH · divergence · bestätigt]** Extended-Plan-Gate ignoriert Stripe-Abo komplett (nur streamer_plans gelesen)
  - Python: `bot/entitlements/repository.py:resolve_plan_snapshot + load_billing_subscription (Z.147-191, 245-264)`
  - Rust: `rust/crates/tb-dashboard-api/src/auth/mod.rs:82-114 (has_extended_entitlement) + 122-145 (extended_gate)`
  - Wirkung: Ein zahlender Partner mit reinem Stripe-Abo (kein manueller streamer_plans-Eintrag) wird vom extended_gate mit 403 plan_required abgewiesen, obwohl er den Extended-Plan bezahlt -- Erweitert-Analytics sind fuer echte Zahler gesperrt.
  - Verifikation: Am Code bestaetigt. Rust has_extended_entitlement (auth/mod.rs:82-114) liest ausschliesslich 'SELECT manual_plan_id, manual_plan_expires_at FROM streamer_plans WHERE LOWER(twitch_login)=LOWER($1)' und prueft plan_is_extended darauf; weder twitch_billing_subscriptions noch streamer_plans.plan_name werden gelesen. Der Stripe-Webhook-Sync (_billing_sync_plan_to_streamer_plans, billing_mixin.py:539-561) schreibt den aktiven Plan in die Spalte plan_name (NICHT manual_plan_id) plus in twitch_billing_s
  - Fix: has_extended_entitlement an resolve_plan_snapshot koppeln (denselben Resolver verwenden statt einer separaten verkuerzten Query), sodass Manual-Override UND Stripe-Abo wie in Python beruecksichtigt werden; danach plan_is_extended auf das aufgeloeste plan_id anwenden.
- **[HOCH · divergence · bestätigt]** Onboarding-Trial wird sofort gewaehrt statt erst nach 24h-Grace + first_login-Gate
  - Python: `bot/raid/services/partner_setup_service.py:check_and_grant_trial_eligibility (Z.237-381) + billing_mixin.py:1110-1237`
  - Rust: `rust/crates/tb-analytics/src/trial.rs:78-187 (grant_trial_at_onboarding/grant_trial_inner) + raid_oauth_impl.rs:1027`
  - Wirkung: Der einmalige Trial wird in Rust sofort beim ersten Auth verbraucht (trial_ever_granted=1) -- der 24h-Bedenkzeitraum entfaellt, und Streamer, die innerhalb der ersten 24h einen Bezahlplan kaufen wuerden, verbrennen ihren Trial-Slot vorzeitig.
  - Verifikation: Verifiziert, mit Praezisierung. Rust grant_trial_inner (trial.rs:113-187) prueft nur trial_ever_granted + paid-Plan (Billing/manual), aber KEIN first_login_at und KEIN 24h-Fenster, und wird via grant_trial_at_onboarding direkt im OAuth-Callback raid_oauth_impl.rs:1027 sofort beim Erst-Auth (had_existing_auth=false) getriggert. Python-Seite: complete_setup_for_streamer (partner_setup_service.py:391-415) RECORDET beim Onboarding nur first_login_at, gewaehrt KEINEN Trial. Die grace-gated check_and_
  - Fix: grant_trial_inner um first_login_at-Lookup + 24h-Grace-Check erweitern (analog Python) und den Sofort-Grant aus raid_oauth_impl.rs:1027 entfernen ODER bewusst als Produktentscheidung dokumentieren; paid_plan-Set auf {raid_boost, analysis_dashboard, bundle_analysis_raid_boost} pruefen.
- **[MITTEL · missing · unverif.]** Periodischer Trial-Auto-Grant beim Plan-Resolve fehlt in Rust nativ
  - Python: `bot/dashboard/billing/billing_mixin.py:_billing_current_plan_for_request -> _billing_check_and_grant_trial_eligibility (Z.1299-1323)`
  - Rust: `—`
  - Wirkung: Streamer, die nie den nativen Onboarding-Pfad durchlaufen (Alt-Partner, Re-Auth, oder wenn der Onboarding-Grant fehlschlaegt), bekommen den 24h-Auto-Trial im Rust-Pfad nie -- sie bleiben dauerhaft auf raid_free, obwohl Python sie automatisch hochgestuft haette. Steht bereits im Coverage-Eval vom 14.6. (medium).
  - Fix: Eligibility-Check (trial_ever_granted=0 AND first_login_at>24h AND kein paid plan) in resolve_plan_snapshot oder partner_response als separate Transaktion vor dem Read einhaengen, wie Python in _billing_current_plan_for_request.
- **[MITTEL · proxied · unverif.]** Reiche Billing-/Plan-Mutationen (Checkout, Stripe-Webhook, Admin-Plan-Setzen) laufen weiter ueber Python-Proxy
  - Python: `bot/dashboard/routes_billing.py (Checkout/Webhook/Portal) + billing_mixin.py:1342ff + abbo_routes.py:190`
  - Rust: `rust/bin/tb-dashboard/src/main.rs:47 (proxy::dashboard_fallback_handler) -- nur trial/start nativ`
  - Wirkung: Migration der Entitlement/Billing-Schicht ist nur lesend (Resolver) teilweise nativ; alle schreibenden Pfade haengen an Python. Laeuft via Proxy, zaehlt aber als offene Luecke -- Rust-Cutover dieser Subsysteme steht aus (deckt sich mit Welle-D-Plan).
  - Fix: Im Cutover-Plan als bewusst-proxied markieren; bei nativer Portierung den Resolver (plan.rs) als Single Source nutzen, damit Read- und Write-Seite konsistent bleiben.
- **[NIEDRIG · divergence · unverif.]** auth-status-Plan emittiert manual_override/billing_subscription/status-Felder nicht
  - Python: `bot/entitlements/repository.py:build_plan_snapshot (Z.230-242: status, manual_override, billing_subscription, customer_reference)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/auth_status.rs:198-208 (plan-JSON nur planId/planName/tier/isExtended/expiresAt/source/entitlements)`
  - Wirkung: Frontend-Komponenten oder proxied Endpoints, die manual_override/billing_subscription/status erwarten, bekommen sie aus der nativen Route nicht. Im Coverage-Eval 14.6. als low eingestuft (native auth-status hat die 7 Kernfelder).
  - Fix: Falls Konsumenten die Felder brauchen: PlanSnapshot um status/customer_reference/manual_override/billing_subscription erweitern und in partner_response ausgeben; sonst als bewusste Reduktion dokumentieren.
- **[NIEDRIG · divergence · unverif.]** Offset-loser ISO-Timestamp wird in plan.rs:is_expired_timestamp nicht geparst (inkonsistent zu level/mod.rs und Python)
  - Python: `bot/entitlements/repository.py:parse_datetime_value (Z.44-62, naive -> tzinfo=UTC)`
  - Rust: `rust/crates/tb-analytics/src/plan.rs:253-271 (is_expired_timestamp) vs rust/crates/tb-dashboard-api/src/auth/mod.rs:150-171 (expiry_in_future mit NaiveDateTime-Fallback)`
  - Wirkung: Ein abgelaufener Manual-Override mit offset-losem Timestamp wuerde von plan.rs faelschlich als aktiv behandelt (Streamer behaelt Plan ueber Ablauf hinaus). In der Praxis selten, weil Python Overrides mit .isoformat() inkl. Offset schreibt; betrifft nur manuell editierte DB-Werte. Zwei Rust-Stellen mit unterschiedlicher Parse-Logik sind zudem ein Drift-Risiko.
  - Fix: is_expired_timestamp an expiry_in_future angleichen (denselben NaiveDateTime-Fallback ergaenzen) oder beide auf eine gemeinsame parse_plan_datetime-Helper-Funktion konsolidieren.
- **[NIEDRIG · divergence · unverif.]** Kein current_period_end-Spaltenfallback bei Alt-Schema (Python fail-open, Rust fail-closed)
  - Python: `bot/entitlements/repository.py:load_billing_subscription (Z.147-191, is_missing_current_period_end_error -> spaltenlose Re-Query)`
  - Rust: `rust/crates/tb-analytics/src/plan.rs:225-238 (SELECT current_period_end::text, kein Fallback)`
  - Wirkung: Auf einem DB-Schema ohne current_period_end-Spalte liefert Rust keinen Plan (plan:null im auth-status) statt fail-open wie Python. Steht im Coverage-Eval 14.6. (low); aktuelle Migrations duerften die Spalte garantieren, daher geringe Praxis-Relevanz.
  - Fix: Entweder per Migration garantieren, dass current_period_end existiert (dann Befund obsolet), oder einen analogen Spalten-Fallback in plan.rs einbauen.
- **[NIEDRIG · divergence · unverif.]** Onboarding-Trial-Grant nutzt breitere PAID_PLAN_IDS-Liste als Python-Eligibility-Check
  - Python: `bot/raid/services/partner_setup_service.py:251 (paid_plan_ids = {raid_boost, analysis_dashboard, bundle_analysis_raid_boost})`
  - Rust: `rust/crates/tb-analytics/src/trial.rs:24-32 (PAID_PLAN_IDS: 7 Plaene) + Z.149-152/203`
  - Wirkung: Fuer den Onboarding-Grant weicht Rust von der Python-Eligibility-Liste ab: Ein Streamer mit z.B. chat_quiet wuerde in Rust als has_paid_plan abgewiesen, in Pythons Onboarding-Check aber den Trial bekommen. Da der Onboarding-Grant ohnehin gegensaetzlich gebaut ist (siehe high-Befund), niedrige Prioritaet.
  - Fix: Beim Angleichen des Onboarding-Trials die paid-Set-Wahl je nach Pfad an Python anpassen: Onboarding/Auto = 3er-Set, Self-Claim = 7er-Set.

### social-media

Social Media in Rust unportiert nur Clip Fetcher default aus kein Proxy Audit 2026 06 14

- **[KRITISCH · missing · bestätigt]** Social-Media-Stack unportiert kein Proxy
  - Python: `upload_worker.py:28 dashboard.py:531-631`
  - Rust: `none`
  - Wirkung: Reiner Rust Betrieb keine Uploads DMs OAuth Captions Frontend
  - Verifikation: Kernaussage am Code bestätigt: tb-social-media/src/lib.rs enthält NUR das clip-Modul (repository/helix/service/task) und ist laut Doku-Kommentar standardmäßig deaktiviert ('Der Task ist standardmäßig deaktiviert ... bis die Social-Media-Pipeline bereit ist'); im tb-bot-Binary nur build_clip_fetch_task hinter TB_CLIP_FETCHER_ENABLED (default aus). Der restliche Python-Stack (~7000 Zeilen, ~30 Module unter bot/social_media/) ist NICHT in Rust: upload_worker.py:28 = commands.Cog mit asyncio-Loop (k
  - Fix: Nativ portieren oder Proxy gaten
- **[MITTEL · divergence · unverif.]** Clip Fetcher Divergenzen
  - Python: `clip_manager.py:67 clip_fetcher.py:80`
  - Rust: `repository.rs:62 service.rs:64`
  - Wirkung: Layout fehlt andere Partner Menge Statistik abweichend
  - Fix: Layout nachziehen View nutzen Zaehlung angleichen

### engagement

Der Engagement-Layer (KI-Stammgast/MiniMax) ist in Rust nahezu vollständig nicht portiert. Nativ und verhaltensgleich existieren nur die Ränder: die 5 Chat-Steuerbefehle (!engagement_on/off/status/ignore_me/remember_me) in tb-chat/src/commands.rs und das Engagement-Auto-Off beim Stream-Offline (offline_side_effects.rs). Der gesamte KI-Kern — die EngagementPipeline (settings/opt-out/partner-gate/deadlock-live-gate, Anti-Flood/Burst-Rhythmus, ~12 Prompt-Fragmente, MiniMax-generate, Starter-Repeat-Guard, Decision-Log) plus ~18 Stützmodule (threads, lurker_signal, minimax_chat, persona, rhythm, soul_store, conversation, channel_background, style_examples, match_context, deadlock_wiki/patches/stats, stream_transcripts, irc_reader, global_sentiment, stream_state, stealth_sender, sender_auth) — hat NULL Rust-Port. Pipeline-Schritt 11 in tb-chat/src/pipeline.rs:536 ist ein leerer No-op-Kommentar, kein Proxy fängt EventSub-Chat ab. Da unter dem aktiven TWITCH_RUST_CHAT_TAKEOVER=1 der Python-Chat-Bot gar nicht mehr gestartet wird (runtime_bootstrap.py:910), läuft auch der Python-Pfad nicht: der KI-Stammgast schweigt real komplett. Die 6 Dashboard-v2-Routen (settings/toggle/update/log/sender-auth/-callback) sind nicht nativ und laufen über den Strangler-Fallback-Proxy → Python 8765; sie funktionieren, schalten aber nur ein Feature scharf, das nicht mehr antwortet. Zwei echte Divergenzen im portierten Command-Code: NoopSuperMod liefert immer false (Super-Mod-Toggle tot) und unberechtigte Toggle-Versuche werden still verworfen statt mit Ablehnungs-Reply. Migration der Schreib-Felder enabled_at ist korrekt. Befunde decken sich überwiegend mit dem Vor-Audit vom 14.6.; einzelne sind hier neu präzisiert.

- **[KRITISCH · missing · bestätigt]** Engagement-KI-Pipeline (Schritt 11) ist No-op — KI-Stammgast antwortet GAR NICHT
  - Python: `bot/engagement/pipeline.py:209 EngagementPipeline.handle / bot/chat/bot.py:1765`
  - Rust: `rust/crates/tb-chat/src/pipeline.rs:536`
  - Wirkung: Der MiniMax-KI-Stammgast schweigt produktiv vollständig: kein nativer Port, kein Proxy, Python-Pfad durch Chat-Takeover deaktiviert. Komplettes Kernfeature des Engagement-Layers tot.
  - Verifikation: Selbst verifiziert: pipeline.rs:536 enthält nur den Kommentar 'Schritt 11: Engagement-AI — No-op bis Engagement-Phase' ohne jeden Code, identisch die Modul-Doku Z.10/20 ('Outreach-Konversationen laufen weiter über Python'). grep 'engagement' über rust/ findet KEINEN Port von EngagementPipeline.handle (Python pipeline.py:209 existiert real). Python-Pfad ist unter Takeover tot: runtime_bootstrap.py:910 _rust_chat_takeover_active() überspringt _init_twitch_chat_bot() (Z.917), event_message/bot.py:1
  - Fix: Pipeline-Kern nativ portieren (Settings/Opt-out/Partner-Gate/Deadlock-Live-Gate, Rhythmus, Prompt-Aufbau, MiniMax-Call, Decision-Log, Stealth-Send) und in pipeline.rs:536 verdrahten — oder bewusst als deferred kennzeichnen. Bekannt aus Vor-Audit (critical).
- **[HOCH · missing · bestätigt]** Stealth-Send über separaten Engagement-Sende-Account fehlt nativ
  - Python: `bot/engagement/stealth_sender.py:28 send / bot/chat/bot.py:1774-1809`
  - Rust: `—`
  - Wirkung: Selbst bei reaktiviertem Kern könnte die KI nicht als unauffälliger Zuschauer antworten — die gewünschte Trennung Bot-Identität vs. Stealth-Account ist nicht reproduziert.
  - Verifikation: Verifiziert: bot/engagement/stealth_sender.py:28 send() existiert, postet via Helix /chat/messages mit access_token+sender_id des Smoke-Accounts (eigene Identität, nicht Bot). grep 'stealth' über rust/ liefert NUR Testdaten ('lurker') — kein Port des Sendepfads. bot.py:1774-1809 ruft _engagement_stealth_send nur aus dem (toten) Python-Chat. Selbst bei portierter Pipeline fehlt der zweite Identitäts-Sendeweg nativ komplett. high angemessen (Folgelücke von Befund 1, aber eigenständiger Sendeweg).
  - Fix: Helix-Send mit get_valid_access_token()/sender_id portieren, inkl. is_sent/drop_reason-Auswertung (200-mit-Drop = False).
- **[HOCH · missing · bestätigt]** Engagement-Sender-OAuth (sender_auth) + AAD-Crypto-Format fehlt nativ
  - Python: `bot/engagement/sender_auth.py:97 build_authorize_url / :236 handle_callback / :154 _store_tokens`
  - Rust: `rust/crates/tb-crypto/src/aad.rs:7 (explizit deferred)`
  - Wirkung: Der separate Sende-Account kann nativ weder onboarded noch entschlüsselt/verwendet werden; Voraussetzung für den Stealth-Send ist nicht erfüllt.
  - Verifikation: Verifiziert: aad.rs:7-8 sagt wörtlich, dass 'engagement_sender_auth' ein abweichendes AAD-Format nutzt und 'mit ihren jeweiligen Feature-Crates ergänzt' wird (nicht Phase 0a) — aad.rs bietet nur raid_auth() und social_media(), kein engagement-Builder. Python sender_auth.py:97 build_authorize_url, :154 _store_tokens, :236 handle_callback und Tabelle twitch_engagement_sender_auth (Z.82) existieren real, nutzen Field-Crypto AES-256-GCM AAD-gebunden. grep über rust/ findet keinen sender_auth-Port. O
  - Fix: Engagement-AAD-Builder (access_aad/refresh_aad) + Token-Store/Refresh nativ ergänzen analog raid_auth in aad.rs.
- **[HOCH · missing · bestätigt]** Threads/Beziehungsführung-Subsystem (Langzeitgedächtnis) nicht portiert
  - Python: `bot/engagement/threads.py:59 load_open_threads_for_user / :116 mark_referenced / :156 auto_close_stale / :243 extract_threads`
  - Rust: `—`
  - Wirkung: Konversationsfäden/Follow-ups (das zentrale Beziehungsführungs-Modell) fehlen vollständig; kein Langzeitgedächtnis.
  - Verifikation: Verifiziert: bot/engagement/threads.py:59 load_open_threads_for_user, :116 mark_referenced, :156 auto_close_stale, :243 extract_threads existieren real. pipeline.py:347 lädt offene Threads pro User (limit=5) und baut threads_to_prompt_fragment — bestätigt. grep 'thread' über rust/ liefert nur tokio/std-Treffer und eine Tabellenspalte referenced_thread_ids (commands.rs:1682, ungenutzter Schema-Rest) — KEIN Port der Thread-Logik oder des Hintergrund-Extractors. high angemessen, da Teil des No-op-S
  - Fix: Bei Kern-Portierung mitnehmen: load_open_threads_for_user + mark_referenced + Hintergrund-Extractor (15-min) + auto_close_stale (60-min).
- **[HOCH · missing · bestätigt]** MiniMax-Engagement-Client (Sanitizing 480-Zeichen, Baseline-Prompt) nicht portiert
  - Python: `bot/engagement/minimax_chat.py:91 generate / :153 _sanitize_chat_text / :194 build_baseline_system_prompt`
  - Rust: `—`
  - Wirkung: Selbst-erzeugung der Antworten (inkl. think-Strip, Längen-Cap, Baseline-Persona) fehlt — Kern des Sprachverhaltens.
  - Verifikation: Verifiziert: bot/engagement/minimax_chat.py:91 generate (max_answer_len=480), :153 _sanitize_chat_text (max_len=480, think-Strip), :194 build_baseline_system_prompt existieren real. Die MiniMax-Treffer in rust/ gehören AUSSCHLIESSLICH zu scam_pitch.rs (Spam-AI-Review) und promos.rs (Targeted-Promo-Preset-Picker) — kein Engagement-Chat-Use-Case. Kein Port der Persona-/Sanitize-Logik. high korrekt.
  - Fix: Bestehenden Rust-MiniMax-Client wiederverwenden, aber Engagement-Baseline-Prompt + 480-Cap + think-Strip nachbilden.
- **[HOCH · divergence · bestätigt]** Super-Mod-Berechtigung ist NoopSuperMod → immer false (Toggle für Super-Mod tot)
  - Python: `bot/engagement/admin.py:25 is_super_mod (SELECT twitch_admin_roles role='super_mod')`
  - Rust: `rust/bin/tb-bot/src/chat_wiring.rs:686-693 NoopSuperMod`
  - Wirkung: Berechtigungs-Regression: Super-Mods verlieren das Recht, !engagement_on/off auszuführen. Bekannt aus Vor-Audit (high).
  - Verifikation: Verifiziert am Code: chat_wiring.rs:686-693 NoopSuperMod::is_super_mod gibt hart false zurück; wird bei chat_wiring.rs:277 als SuperModPort eingehängt (Arc::new(NoopSuperMod)). commands.rs:999 is_engagement_admin ruft self.super_mod.is_super_mod() — fällt also immer auf false zurück, wenn nicht Mod/Broadcaster (Z.996). Python admin.py:is_super_mod prüft real twitch_admin_roles role='super_mod'. Ein Super-Mod ohne Twitch-Mod-Status kann !engagement_on/off/status in Rust nicht auslösen. high korre
  - Fix: NoopSuperMod durch echte Query 'SELECT 1 FROM twitch_admin_roles WHERE twitch_user_id=$1 AND role=''super_mod''' ersetzen.
- **[HOCH · proxied · bestätigt]** Sender-Auth-OAuth-Routen (/engagement/sender-auth, /sender-callback, /callback/engagement-sender) nur via Proxy/Python
  - Python: `bot/engagement/dashboard_api.py:470-475 / :386 _handle_sender_auth_start / :411 _handle_sender_auth_callback`
  - Rust: `— (proxy.rs:123 Fallback → 8765)`
  - Wirkung: Der Stealth-Sende-Account kann nur über Python onboarded werden; ohne Python kein Onboarding und kein Token-Refresh. Hard-Blocker für jeden Python-Cutover dieses Features.
  - Verifikation: Verifiziert: dashboard_api.py:470-475 registriert sender-auth, sender-callback und /callback/engagement-sender (Start/Callback → Token-Store). grep über rust/ findet KEINE native Route; tb-dashboard-api/lib.rs hat 0 engagement-Routen, alle laufen über den Strangler-Fallback-Proxy (proxy.rs → http://127.0.0.1:8765, dashboard_fallback_handler in tb-dashboard/main.rs:47, nur aktiv wenn TB_DASHBOARD_LEGACY_FALLBACK_URL gesetzt — Default-Doku 8765). Onboarding funktioniert also via Proxy, aber rein ü
  - Fix: OAuth-Start/Callback + verschlüsselten Token-Store nativ portieren (mit Engagement-AAD-Format), bevor Python abgeschaltet wird.
- **[MITTEL · missing · unverif.]** Lurker-Signal (bekannte Stammgäste die gerade lurken) nicht portiert
  - Python: `bot/engagement/lurker_signal.py:72 known_regulars_currently_lurking / :110 lurker_hint_to_prompt_fragment`
  - Rust: `—`
  - Wirkung: Die KI kann lurkende Stammgäste nicht gezielt ansprechen; Funktion fehlt.
  - Fix: Teil der Kern-Portierung; reine SELECT-Query + Prompt-Fragment, geringer Aufwand.
- **[MITTEL · missing · unverif.]** Persona/Style-Fragmente (Tonprobe, Stil-Few-Shot) nicht portiert
  - Python: `bot/engagement/persona.py:171 sample_tone / bot/engagement/style_examples.py:171 build_style_fragment`
  - Rust: `—`
  - Wirkung: Antworten klingen ohne Persona/Stil-Few-Shot generischer/AI-isch — direkter Widerspruch zur Stil-Vorgabe.
  - Fix: Teil der Kern-Portierung; SELECT auf jüngste Chat-Texte + Fragment-Bau.
- **[MITTEL · missing · unverif.]** Anti-Flood/Anti-Burst-Rhythmus (RhythmGuard) nicht portiert
  - Python: `bot/engagement/rhythm.py:48 RhythmGuard (anti_flood_ok/anti_burst_ok, Defaults 5s/3/60s)`
  - Rust: `—`
  - Wirkung: Ohne Rhythmus-Gate würde eine reaktivierte KI fluten/spammen; Schwellen+Logik fehlen.
  - Fix: In-Memory-State pro Channel mit identischen Env-Defaults (ENGAGEMENT_MIN_PAUSE_SEC/BURST_LIMIT/BURST_WINDOW_SEC) nachbauen.
- **[MITTEL · missing · unverif.]** Match-Kontext (Steam-Last-Match-Poll für Live-Bezug) nicht portiert
  - Python: `bot/engagement/match_context.py:87 get_match_state / :203 poll_match_state`
  - Rust: `—`
  - Wirkung: KI kann sich nicht aufs laufende Spiel beziehen (Held/Score) — Verlust an Andock-Substanz.
  - Fix: Bei Kern-Portierung: Hero-Cache + last-match-Fetch + 30s-Poller.
- **[MITTEL · missing · unverif.]** Deadlock-Grounding (Wiki/Patches/Stats) nicht portiert
  - Python: `bot/engagement/deadlock_wiki.py:221 build_grounding_fragment / deadlock_patches.py:135 build_patch_fragment / deadlock_stats.py:119 build_stats_fragment`
  - Rust: `—`
  - Wirkung: Ohne Grounding würde eine reaktivierte KI wieder Spielfakten halluzinieren (genau der Bug, der das Feature einst stoppte).
  - Fix: Grounding zwingend mitportieren, bevor der Kern wieder scharf geschaltet wird.
- **[MITTEL · missing · unverif.]** Engagement-Hintergrund-Scheduler (8 Loops) nicht portiert
  - Python: `bot/engagement/background.py:260 ensure_started (Thread-Extractor 15m, Match-Poll 30s, Auto-Closer 60m, Conv-Trim 24h, Transcript 15m, Sentiment 20m, Soul 3h, Channel-Profile 4h)`
  - Rust: `—`
  - Wirkung: Selbst bei Kern-Portierung blieben Threads/Profile/Sentiment/Conv-Trim ohne periodische Pflege — Datengrundlage veraltet/fehlt.
  - Fix: Loops als tokio-Tasks mit identischen Intervallen + Jitter portieren; Conversation-Trim nicht vergessen (DB-Wachstum).
- **[MITTEL · proxied · unverif.]** Dashboard-Route GET /twitch/api/v2/engagement/settings nur via Proxy (nicht nativ)
  - Python: `bot/engagement/dashboard_api.py:465 / :228 _handle_get_settings`
  - Rust: `— (proxy.rs:123 dashboard_fallback_handler → 8765)`
  - Wirkung: Settings-Anzeige im Streamer-Dashboard hängt vollständig am Python-Prozess; bricht bei Python-Down. Migration unvollständig.
  - Fix: Native Read-Route mit Session-Actor-Resolve + Serialisierung portieren (analog anderer v2-Read-Routen).
- **[MITTEL · proxied · unverif.]** Dashboard-Route POST /engagement/toggle + /engagement/update nur via Proxy
  - Python: `bot/engagement/dashboard_api.py:466-467 / :258 _handle_post_toggle / :293 _handle_post_update`
  - Rust: `— (proxy.rs:123 Fallback → 8765)`
  - Wirkung: Schreib-Pfad fürs Engagement-Setup (Persona/Tabu/Steam-ID/Toggle) ist nicht nativ; bei Python-Down kein Dashboard-Toggle/Update.
  - Fix: Native UPSERT-Handler mit Actor-Auth portieren; Schema enabled/steam_id/persona_override/tabu_topics.
- **[MITTEL · missing · unverif.]** Conversation-Buffer (User/Assistant-Turns, History-Load) nicht portiert
  - Python: `bot/engagement/conversation.py:73 ConversationBuffer (append_user_turn/append_assistant_turn/load_recent_buffer)`
  - Rust: `—`
  - Wirkung: Kein Konversationsgedächtnis pro Kanal — die KI hätte ohne Buffer keinen Gesprächsverlauf; zudem keine native Pflege/Trim der Tabelle.
  - Fix: Buffer-Schreib/Lese-Pfad + 24h-Trim mitportieren; Schema-Treue zu twitch_engagement_conversation beachten (role TEXT, ts TIMESTAMPTZ).
- **[MITTEL · missing · unverif.]** Decision-Logging (twitch_engagement_log Insert mit Kosten/Tokens/Latenz) nicht portiert
  - Python: `bot/engagement/pipeline.py:60 _sync_log_decision / :229 _calc_cost_usd`
  - Rust: `—`
  - Wirkung: Solange der Kern fehlt, bleibt das Log leer → !engagement_status meldet stets 'Noch keine Aktionen geloggt'; auch Kosten-Transparenz fehlt. Bei Kern-Port muss das Logging mit.
  - Fix: Insert + Kosten-Schätzung (MINIMAX_PRICE_INPUT/OUTPUT_PER_1K, Defaults 0.0008/0.0024) bei Port mitnehmen.
- **[MITTEL · missing · unverif.]** Partner-/Deadlock-Live-Gate der Pipeline nicht portiert (stream_state.is_streaming_deadlock)
  - Python: `bot/engagement/pipeline.py:257 _sync_is_operational_partner / :265 is_streaming_deadlock / bot/engagement/stream_state.py:32`
  - Rust: `—`
  - Wirkung: Bei Reaktivierung ohne diese Gates würde die KI in nicht-Partner-Kanälen oder bei Nicht-Deadlock-Streams antworten — Fehlverhalten/Scope-Bruch.
  - Fix: Beim Andocken von Schritt 11 die vorhandenen Klassifizierer (is_deadlock_live) + Partner-Check wiederverwenden und das operational-partner-Gate ergänzen.
- **[NIEDRIG · missing · unverif.]** Soul-Store (Selbst-Reflexion/Anker-Extension) nicht portiert
  - Python: `bot/engagement/soul_store.py:78 get_soul_extension_fragment / :143 reflect_and_store_anchor`
  - Rust: `—`
  - Wirkung: Persönlichkeits-Anker/Selbstbild der KI fehlen; geringe Außenwirkung.
  - Fix: Nur bei vollständiger Paritäts-Portierung relevant.
- **[NIEDRIG · missing · unverif.]** Channel-Background-Profil (kanalspezifischer Kontext) nicht portiert
  - Python: `bot/engagement/channel_background.py:97 rebuild_channel_profile / :136 get_channel_profile_fragment`
  - Rust: `—`
  - Wirkung: KI kennt Kanal-Eigenheiten nicht; geringe Wirkung.
  - Fix: Teil der Kern-Portierung, nachrangig.
- **[NIEDRIG · missing · unverif.]** Stream-Transkript-Kontext + IRC-Reader nicht portiert
  - Python: `bot/engagement/stream_transcripts.py:154 load_recent_segments / bot/engagement/irc_reader.py:69 EngagementIrcReader`
  - Rust: `—`
  - Wirkung: KI hört nicht zu, was der Streamer gerade sagt; IRC-Read-Pfad fehlt. Geringe Außenwirkung, aufwändig (Audio/Whisper).
  - Fix: Bewusst zurückstellen; nur bei voller Parität.
- **[NIEDRIG · missing · unverif.]** Global-Sentiment-Kontext nicht portiert
  - Python: `bot/engagement/global_sentiment.py:153 get_sentiment_fragment / :105 rebuild_global_sentiment`
  - Rust: `—`
  - Wirkung: KI hat keinen kanal-übergreifenden Stimmungs-Kontext; minimal.
  - Fix: Nachrangig.
- **[NIEDRIG · divergence · unverif.]** Unberechtigter Toggle-Versuch wird in Rust still verworfen (kein Ablehnungs-Reply)
  - Python: `bot/chat/engagement_commands.py:104-107 (sendet 'Nur Broadcaster, Mods oder Super-Mod dürfen das.')`
  - Rust: `rust/crates/tb-chat/src/commands.rs:1007-1009 / 1048-1050`
  - Wirkung: Nutzer ohne Recht bekommt keinerlei Rückmeldung — wirkt wie ein toter Befehl statt 'keine Berechtigung'. Kleine UX-Regression.
  - Fix: Vor dem return ein reply mit der Ablehnungsnachricht senden, wie Python.
- **[NIEDRIG · divergence · unverif.]** !engagement_status: 'nie konfiguriert'-Pfad bei DB-Fehler weicht ab (still .unwrap_or(None))
  - Python: `bot/chat/engagement_commands.py:155-162 (DB-Exception → 'Fehler beim Status-Abruf')`
  - Rust: `rust/crates/tb-chat/src/commands.rs:1097-1108`
  - Wirkung: Bei DB-Problemen meldet die KI fälschlich 'nie konfiguriert' statt Fehler — irreführend für Mods. Geringe Häufigkeit.
  - Fix: DB-Fehler von 'kein Eintrag' unterscheiden: bei Err eine Fehlermeldung senden statt None-Fallthrough.
- **[NIEDRIG · proxied · unverif.]** Dashboard-Route GET /engagement/log nur via Proxy
  - Python: `bot/engagement/dashboard_api.py:468 / :360 _handle_get_log`
  - Rust: `— (proxy.rs:123 Fallback → 8765)`
  - Wirkung: Log-Ansicht hängt am Python-Prozess; bei Down leer. Geringer Impact (read-only Transparenz).
  - Fix: Native SELECT-Route auf twitch_engagement_log mit Serialisierung + Limit.
- **[NIEDRIG · missing · unverif.]** Pre-Filter _should_skip_trigger + Starter-Repeat-Guard fehlen (Teil des fehlenden Kerns)
  - Python: `bot/engagement/pipeline.py:136 _should_skip_trigger / :452 Starter-Repeat-Guard`
  - Rust: `—`
  - Wirkung: Bei einer naiven Reaktivierung ohne diese Guards würde die KI auf Emotes/Adressierungen reagieren und sich wiederholen — Qualitätsregression.
  - Fix: Beide Filter 1:1 mitportieren (sie sind billig und ohne Modell-Call).

### community

Die Einheit "community" zerfällt in drei sehr unterschiedlich portierte Blöcke. (1) Leaderboard-Lese-Aggregation: Der HTTP-Endpoint GET /twitch/api/v2/category-leaderboard ist nativ in tb-dashboard-api (category_leaderboard.rs) und der interne /stats-Pfad in tb-internal-api (stats_native.rs) ist großteils faithful nachgebaut — die früher gemeldeten SQLX-Decode-Bugs (int4/numeric→f64) sind durch explizite ::float8/CAST(...AS BIGINT/DOUBLE PRECISION) gefixt, das Extended-Plan-Gate existiert jetzt, Partner-/Discord-Enrichment, Subs und Shared-Audience sind 1:1 übernommen. Zwei reale Divergenzen bleiben: yourTier wird aus den gefilterten Leaderboard-Rows statt aus dem ungefilterten Kategorie-Avg bestimmt, und der gesamte zweite Aggregationsblock von _compute_stats (retention/chat/discovery/content_performance) fehlt im nativen /stats. (2) Der !twl-Discord-Command samt UI-View bleibt bewusst in Python (Frontend/Discord über Bridge) — keine Lücke. (3) Partner-Recruitment und das komplette Voice-Reaction-Subsystem sind GAR NICHT portiert und es gibt keinen Proxy. Kritisch und über den bestehenden Audit hinaus: Da der Rust-Monitoring-Takeover live ist und der Python-Poll-Loop dann nicht startet (runtime_bootstrap.py:962), während der Rust-Poll-Hook after_tick/category_streams nicht implementiert ist, läuft Partner-Recruitment in Produktion NIRGENDS mehr — es ist nicht nur "default-off aufgeschoben", sondern ein echter funktionaler Ausfall gegenüber dem Python-Stand. Voice-Reaction ist auch in Python default-off, daher dort low.

- **[HOCH · missing · bestätigt]** Partner-Recruitment läuft in Produktion NIRGENDS mehr (Rust-Hook fehlt, Python-Tick unter Takeover aus)
  - Python: `bot/community/partner_recruit.py:50 (_run_partner_recruit), aufgerufen aus bot/monitoring/monitoring.py:1315`
  - Rust: `rust/bin/tb-bot/src/main.rs:92-99 (SubscriptionPollHooks impl ohne after_tick); rust/crates/tb-monitoring/src/poller/hooks.rs:126 (after_tick Default-Noop)`
  - Wirkung: Die automatische Erkennung+Ansprache frequenter Deadlock-Streamer ist komplett tot. Es werden keine neuen Outreach-Nachrichten mehr gesendet und keine twitch_partner_outreach-Zeilen mehr geschrieben — der Recruiting-Funnel ist seit dem Monitoring-Cutover stumm. Folgeschaden: der bereits portierte Outreach-Boost in tb-raid liest eine Tabelle, die nichts mehr füllt.
  - Verifikation: Selbst am Code verifiziert, Kette hält adversariell stand. PYTHON: _run_partner_recruit (partner_recruit.py:50) wird ausschliesslich an EINER Stelle gerufen — monitoring.py:1315 in _tick (grep über bot/ bestätigt: keine weitere Call-Site). _tick läuft nur via poll_streams (monitoring.py:1086), und poll_streams.start() ist in runtime_bootstrap.py:959 hinter 'if not rust_takeover' gegated. Bei TWITCH_RUST_MONITORING_TAKEOVER=1 (laut Memory live) startet der Python-Poll also nie → Recruitment tot. 
  - Fix: after_tick in SubscriptionPollHooks implementieren und einen nativen Recruit-Pfad bauen (Kandidaten-Query aus _detect_recruit_candidates inkl. RECRUIT_LOOKBACK_DAYS=28/MIN_DAYS=4/MIN_AVG_SAMPLES_PER_DAY=480/MAX_AVG_VIEWERS=40, Tageslimit 8, 3/Tick, 60s-Throttle, 30-Tage-Cooldown, INSERT...ON CONFLICT in twitch_partner_outreach). Alternativ kurzfristig den Python-Recruit-Tick unter Takeover wieder 
- **[MITTEL · divergence · unverif.]** category-leaderboard: yourTier aus gefilterten Leaderboard-Rows statt ungefiltertem Kategorie-Avg
  - Python: `bot/analytics/api_performance.py:1236-1240 (your_tier = _get_peer_group_stats(...).tier) + 210-256 (_get_peer_group_stats, ungefilterte AVG-Query + Session-Fallback)`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/category_leaderboard.rs:138-198 (your_avg_opt aus filtered rows, sonst Session-Fallback)`
  - Wirkung: Bei exclude_external=1 mit eigenem Avg>100 oder bei aktivem tier-Filter, der den Streamer ausschließt, zeigt yourTier ein falsches Tier (aus Session-Avg statt Kategorie-Avg). yourRank/yourEntry bleiben korrekt, nur das Tier-Badge driftet.
  - Fix: your_tier wie Python aus einer eigenen ungefilterten Kategorie-Avg-Query (SELECT AVG(viewer_count) ... WHERE ts_utc>=since AND LOWER(streamer)=$login GROUP BY streamer) ableiten, nur bei NULL auf den twitch_stream_sessions-Fallback gehen; your_avg_opt nicht mehr für die Tier-Bestimmung verwenden.
- **[MITTEL · divergence · unverif.]** Nativer /stats-Pfad lässt vier _compute_stats-Sektionen fallen (retention/chat/discovery/content_performance)
  - Python: `bot/community/leaderboard.py:1024-1256 (zweiter Aggregationsblock, setzt out[retention]/[chat]/[discovery]/[content_performance])`
  - Rust: `rust/crates/tb-internal-api/src/handlers/stats_native.rs:1255-1301 (compute_stats; baut nur tracked/category/avg_*/streamer/monetization/eventsub)`
  - Wirkung: Konsumenten des nativen /stats (dashboard_metrics_mixin._dashboard_stats → runtime_bootstrap.py:236) erhalten keine Retention-/Chat-Health-/Discovery-/Content-Performance-Daten mehr; entsprechende Dashboard-Sektionen bleiben leer, ohne Fehler.
  - Fix: Den zweiten Block aus leaderboard.py:1024-1256 in stats_native.rs portieren (Session-/Chat-Peak-/Rollup-Queries, vier Sektionen exception-safe anhängen) — Severity nur medium, weil !twl selbst diese Keys nicht braucht; vorab prüfen, ob die Dashboard-Sektionen noch live sind, sonst als bewusste Auslassung dokumentieren.
- **[MITTEL · regression · unverif.]** Outreach-Boost-Konsument liest twitch_partner_outreach, das nativ kein Writer mehr füllt
  - Python: `bot/community/partner_recruit.py:290-321 (_record_outreach: INSERT/UPSERT in twitch_partner_outreach)`
  - Rust: `rust/crates/tb-raid/src/auto_raid_pipeline.rs:174-176 (load_boost_logins liest twitch_partner_outreach)`
  - Wirkung: Der portierte Outreach-Boost-Pfad im Auto-Raid bekommt keine frischen Outreach-Einträge mehr (außer evtl. Altbestände), womit der Boost faktisch leerläuft — ein Folgeschaden des fehlenden Recruit-Ports, der den Wert des bereits portierten Boost-Features aushöhlt.
  - Fix: Mit Finding 1 zusammen lösen: sobald der native Recruit-Writer steht, füllt sich die Tabelle wieder und der Boost-Konsument funktioniert wie vorgesehen.
- **[NIEDRIG · missing · unverif.]** Voice-Reaction-Subsystem komplett unportiert (Scheduler/Brain/Audio/Sales-Webhook), Pipeline-Schritt ist Noop
  - Python: `bot/community/voice_reaction/scheduler.py:113 (VoiceReactionScheduler), mixin.py:85/112 (_open_conversation/_voice_reaction_dispatch_message), conversation_brain.py:124 (ConversationBrain.respond), discord_notifier.py:33 (notify_human)`
  - Rust: `—`
  - Wirkung: Voice-Reaction (Audio-Capture, Whisper-Transkription, Conversation-Brain, Sales-Lead-Discord-Webhook) existiert in Rust nicht. Praktisch geringe Auswirkung, da das Feature auch in Python default-OFF ist (scheduler.py:80 enabled=False, dry_run=True).
  - Fix: Bewusst aufgeschoben lassen und in den Cleanup-Decisions dokumentieren; bei späterem Aktivieren des Features nativ in tb-chat/tb-monitoring nachziehen (Scheduler+Brain+Audio+Sales-Webhook). Aktuell kein Handlungsdruck.
- **[NIEDRIG · missing · unverif.]** _voice_reaction_dispatch_message (Chat-Event-Hook) fehlt in der Rust-Chat-Pipeline
  - Python: `bot/community/voice_reaction/mixin.py:112 (_voice_reaction_dispatch_message), aufgerufen aus event_message; chat_listener.py:maybe_dispatch_chat_message`
  - Rust: `rust/crates/tb-chat/src/pipeline.rs:404 (Schritt 1 VoiceReaction — No-op)`
  - Wirkung: Chat-getriggerte Voice-Reaction-Konversationen werden nicht ausgelöst. Wirkung gering, da Gesamtfeature default-OFF; relevant erst bei Aktivierung.
  - Fix: Mit dem Voice-Reaction-Port zusammen verdrahten; bis dahin als bewusster No-op belassen (bereits im Code kommentiert).
- **[NIEDRIG · divergence · unverif.]** category-leaderboard: nicht-numerisches days/limit liefert serde-422 statt Python-400 mit Fehlermeldung
  - Python: `bot/analytics/api_performance.py:1281-1290 (try int(...) → web.json_response({'error':'days must be an integer'}, status=400))`
  - Rust: `rust/crates/tb-dashboard-api/src/handlers/category_leaderboard.rs:40-48,63-64 (days/limit als Option<i32> via serde Query)`
  - Wirkung: Geringfügige API-Kontrakt-Abweichung beim Fehlerformat für ungültige Query-Parameter. Happy-Path identisch (gleiches Clamping 1..365 bzw. 5..100).
  - Fix: Falls Vertragstreue gewünscht: days/limit als Option<String> annehmen, manuell parsen und bei Fehler 400 mit der Python-Meldung zurückgeben; sonst als akzeptierte Abweichung dokumentieren.
- **[NIEDRIG · divergence · unverif.]** Extended-Plan-Gate prüft Session-Login statt streamer-Query-Param (kein 'kein-streamer→skip')
  - Python: `bot/analytics/api_performance.py:1276 (_require_extended_plan) + api_v2.py:638-668 (streamer-Param primär, Session-Fallback, leerer Streamer→skip)`
  - Rust: `rust/crates/tb-dashboard-api/src/auth/mod.rs:122-145 (extended_gate gated nur auf DashboardAuthLevel::Partner.twitch_login)`
  - Wirkung: Ein Partner, der das Leaderboard für einen FREMDEN Streamer aufruft, wird auf seine eigene Entitlement geprüft (Rust) statt auf die des angefragten Streamers (Python); das 'kein-Streamer→skip'-Verhalten fehlt. Im Normalfall (eigenes Dashboard) identisch, daher low.
  - Fix: extended_gate bzw. den Handler so erweitern, dass die Login-Ableitung dem Python-Muster folgt (streamer-Param zuerst, Session-Fallback, leerer Streamer überspringt das Gate).
- **[NIEDRIG · missing · unverif.]** Admin-Mixin (admin.py) Discord-/Helper-Schicht nicht nativ — HTTP-Mutationen aber separat nativ
  - Python: `bot/community/admin.py:19-296 (_cmd_set_channel/_cmd_add/_cmd_remove/_cmd_list_streamers/_cmd_forcecheck/_cmd_invites/_cmd_refresh_invites/_get_valid_invite_codes)`
  - Rust: `— (Discord-Cog-Schicht); HTTP-add/remove/list nativ laut Vor-Audit (tb-dashboard-api streamers)`
  - Wirkung: Die Discord-Command-Convenience-Wrapper (z.B. Invite-Refresh, forcecheck) laufen weiter in Python über die Bridge — bewusst, da Discord-Frontend im Migrationsscope ausgespart bleibt. Keine User-sichtbare Lücke, solange die HTTP-Mutationen nativ funktionieren.
  - Fix: Kein Handlungsbedarf — Discord-Cog-Schicht ist bewusst Python. Nur dokumentieren, dass die admin.py-Helfer kein Rust-Pendant brauchen (HTTP-Pfad ist abgedeckt).

### clipper-title-coaching

Einheit im Rust-Port praktisch nicht nativ. Highlight-Clipper null Rust-Bausteine und kein Proxy. Title-Generator nur via Strangler-Proxy nach Python 8765, Chat title gibt in Rust false zurueck. Coaching-Audit reines Admin-CLI, nie im Runtime. Unter aktivem Chat-Takeover startet Python den Chat-Bot nicht mehr, cmd_title registriert sich nie, Rust antwortet nicht, title produktiv tot. Clipper und Title-Jobs laufen weiter weil ungated; bei Cog-Abschaltung weg.

- **[MITTEL · regression · umgestuft]** Chat title Command produktiv tot
  - Python: `bot/chat/commands.py:770`
  - Rust: `tb-chat/src/commands.rs:353`
  - Wirkung: Mod oder Broadcaster mit title bekommt keine Reaktion, vorher Titel plus Alternativen.
  - Verifikation: Code-Beleg bestätigt: Rust commands.rs:353 gibt für "!title"/"!titel" false zurück (Pipeline fährt fort), und der Rust-Code dokumentiert das selbst als bewusste LÜCKE (commands.rs:1228-1238). Python cmd_title existiert (bot/chat/commands.py:770-862, in RaidCommandsMixin via @twitchio_commands.command). Es wird nur über _init_twitch_chat_bot() registriert, das in runtime_bootstrap.py:910/917 unter aktivem Chat-Takeover NICHT gestartet wird (Zeile 917 ist im elif-Zweig, der bei _rust_chat_takeover
  - Fix: TitlePort in tb-chat andocken, generate_title aus title_ai.py portieren.
- **[MITTEL · proxied · unverif.]** Title Routen nur via Proxy
  - Python: `bot/dashboard/routes_title.py:188`
  - Rust: `tb-dashboard-api/src/proxy.rs:123`
  - Wirkung: Title Tab funktioniert nur solange Python 8765 laeuft.
  - Fix: generate_title plus title_db-Queries portieren, Routen nativ registrieren.
- **[MITTEL · missing · unverif.]** Title Jobs knowledge und insight nur Python
  - Python: `bot/title_generator/knowledge_job.py:75`
  - Rust: `kein Pendant`
  - Wirkung: Bei Cog-Abschaltung trocknet Knowledge-DB aus.
  - Fix: Beide Jobs als Rust-Tokio-Tasks portieren, Scoring eins zu eins.
- **[MITTEL · missing · unverif.]** Sanitizer und Rate-Limiter ohne Rust-Pendant
  - Python: `bot/title_generator/title_ai.py:48`
  - Rust: `kein Pendant`
  - Wirkung: Naive Portierung halluziniert Raenge und ignoriert Limits.
  - Fix: Sanitizer und Sliding-Window-Limiter mitportieren.
- **[NIEDRIG · missing · umgestuft]** Highlight-Clipper unportiert ungated
  - Python: `bot/highlight_clipper/worker.py:47`
  - Rust: `kein Pendant`
  - Wirkung: Bei Cog-Abschaltung Auto-Highlight-Clips fuer alle Partner weg.
  - Verifikation: Faktenlage bestätigt: kein Rust-Port des Highlight-Clippers. Die grep-Treffer auf "clip" in Rust betreffen zwei ANDERE Subsysteme — tb-social-media (Helix-Clip-Fetcher, clip/service.rs) und das !clip-Chatkommando (CLIP_TITLE in commands.rs:45ff). Der Highlight-Clipper (bot/highlight_clipper/worker.py:47 HighlightClipperWorker: Match-Demo-Analyse, KillMoment-Detektion, VOD-Clip-Download, DM-Versand) hat kein Pendant. _hc_start läuft ungated: in runtime_bootstrap.py:986 steht der Aufruf bei indent
  - Fix: Subsystem neu in Rust bauen, bis dahin Python-only markieren.
- **[NIEDRIG · missing · unverif.]** Coaching-Audit null Prozent portiert
  - Python: `scripts/audit_stream_tos.py:709`
  - Rust: `kein Pendant`
  - Wirkung: Kein User-Impact, Admin-Tool laeuft als CLI weiter.
  - Fix: Als Admin-CLI fuehren, keine Portierung.
- **[NIEDRIG · missing · unverif.]** Rust clip ist Manuell-Clip nicht Auto-Clipper
  - Python: `commands.py:284-408`
  - Rust: `tb-chat/src/commands.rs:301`
  - Wirkung: Mapping-Falle, grep clip suggeriert Teilportierung.
  - Fix: ClipPort gegen highlight_clipper in Doku trennen.
- **[NIEDRIG · infra · unverif.]** Clipper bindet boon und ffmpeg
  - Python: `bot/highlight_clipper/demo_analyzer.py:415`
  - Rust: `kein Pendant`
  - Wirkung: Erhoeht Portierungsaufwand durch Demo-Binaerparsing und Toolchain.
  - Fix: boon als Subprozess behalten, Scoring-Konstanten eins zu eins.
