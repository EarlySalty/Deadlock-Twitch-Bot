# Python→Rust Port-Audit — Twitch-Bot (2026-06-13)

Automatisierter, adversarial verifizierter Vergleich aller portierten Subsysteme (26 Einheiten, 150 Agenten, ~9.8M Token). Python = funktionale Referenz; gemeldet sind **semantische Divergenzen Rust≠Python** — keine bewussten Modernisierungen aus 05-cleanup-decisions.md.

**Bestätigt:** 105 (verworfen als False-Positive: 18) — critical 1, high 13, medium 32, low 59.

> **Systemisches Cluster (Top-Priorität):** ~8 Befunde (1 critical + 3 high + 4 medium) teilen EINE Ursache — Postgres-Integer-Typen (`INTEGER`→i32, `SUM(int)`→int8, `AVG(int)`→numeric) werden in den Dashboard-Handlern als `i64`/`f64` dekodiert. sqlx wirft `ColumnDecode`, der per `.ok().flatten().unwrap_or(0)` still verschluckt wird → Felder werden 0. Ein Decode-Vertrag-Fix behebt sie alle. Siehe Marker `[SQLX-DECODE]`.

## Coverage / Funktionslücken ("alles von Python")

| Python-Modul | LOC | Rust-Ziel | Status | Notiz |
|---|---|---|---|---|
| bot/chat | 13122 | tb-chat (14113 LOC) | **ported** | Welle-B-Chat-Flip am 12.6. vollzogen, nativer Chat LIVE. Komplett: spam_filter, scam_pitch, promos, commands, moderation, global_ban_sweep, pipeline (15 Schritte exakt nach bot.py), chatter_tracking, … |
| bot/monitoring | 11397 | tb-monitoring (7432 LOC) | **ported** | Schritt 4 Cutover am 10.6. vollzogen. Poll-Engine, EventSub-Inbox, Session-Lifecycle, Live-State, Live-Embeds, exp_sessions-Write-Hooks alle nativ. Webhook-Eingang seit 12.6. nativ (webhook_receiver 8… |
| bot/raid | 17717 | tb-raid (10877 LOC) | **ported** | Schritt 6 Cutover am 10.6. vollzogen. RaidAuth (DB-only State), Scoring, Auto-Raid-Orchestrator, Arrival-Confirmation, Blacklist-Raid-Guard, OAuth-Flow (auth-url/go-url/oauth-callback/auth-state/block… |
| bot/internal_api | 3788 | tb-internal-api (12591 LOC) | **ported** | 8776 hält Rust. 21 Routen nativ (healthz, eventsub/dispatch, raid/manual, chat/command, globalban x4, raid/blacklist x4, raid-oauth, stats, analytics/streamer, sessions, market-share, self-explainer-l… |
| bot/analytics | 33309 | tb-analytics (6560 LOC) + tb-dashboard-api handlers (13373 LOC) | **partial** | Read-only-GET-Analytics-Routen weitgehend nativ (overview, viewers, audience, raid_analytics, performance, category_*, retention, loyalty, follower_funnel, lurker, session_detail, stats, network, bans… |
| bot/dashboard | 25929 | tb-dashboard-api (13373 LOC), Binary tb-dashboard (8769) | **partial** | Welle D im Aufbau. Nativ: Legal-Komplex (impressum/datenschutz/agb/sicherheit + Turnstile-Gate), Auth-Fundament (Fernet decrypt+encrypt, Session-Lookup, Sliding-Refresh, AuthLevel-Extractor), Strangle… |
| bot/social_media | 13138 | tb-social-media (576 LOC) | **partial** | GROSSE LÜCKE. Rust-Crate enthält NUR den periodischen Clip-Fetcher (clip/repository+helix+service+task) und ist standardmäßig DEAKTIVIERT (TB_CLIP_FETCHER_ENABLED, nicht in tb-bot gestartet). Komplett… |
| bot/engagement | 5093 | (kein Pendant) | **missing** | Engagement-Layer (MiniMax-Chat-AI, threads, soul_store, persona, deadlock_wiki/patches/stats-Grounding, irc_reader, lurker_signal, pipeline, dashboard_api) NICHT portiert. Einziger Rust-Touchpoint: of… |
| bot/community | 4870 | (teilweise/kein Pendant) | **partial** | leaderboard.py (1370) Kern-Aggregation wird von tb-internal-api stats_native.rs gelesen (verweist auf leaderboard.py:490-1258 als Quelle), also Lese-Aggregation nativ nachgebaut. NICHT portiert: voice… |
| bot/highlight_clipper | 1544 | (kein Pendant) | **missing** | Highlight-Clipper (worker.py, demo_analyzer.py, event_detector.py, twitch_vod.py, demo_downloader, dm_sender) NICHT portiert. Einziger Treffer in Rust ist ein Kommentar in commands.rs. Kein Cutover-Sc… |
| bot/title_generator | 949 | (kein Pendant) | **missing** | Title-Generator (title_ai.py LLM-Generierung, title_db, knowledge_job, insight_job, steam_lookup) NICHT portiert. Der !title/!titel-Chat-Command gibt in tb-chat/commands.rs explizit false zurück (bewu… |
| bot/stream_coaching_audit | 1521 | (kein Pendant) | **missing** | Stream-Coaching-Audit (service.py + youtube_archive.py: YouTube-Upload-Pipeline, faster-whisper-Captions, Mitschnitt-Analyse) NICHT portiert. Interne Tooling-Funktion, im Cutover-Plan nicht erwähnt; h… |
| bot/live_announce | 774 | tb-monitoring/announce/template.rs (707 LOC) | **ported** | template.py explizit als Port markiert (announce/template.rs: 'Port des Live-Announcement-Template-Systems bot/live_announce/template.py'). Lief mit dem Monitoring-Cutover (Live-Embeds via Relay). Kon… |
| bot/dashboard/billing + dashboard/affiliate (gutschrift/abbo) | 7000 | (kein Pendant, via Proxy) | **dropped** | Welle C bewusst eingedampft (Nani 12.6.): Stripe macht das meiste. Portiert wird NUR der Stripe-Webhook-Eingang (POST .../billing/stripe/webhook) + Entitlement-Reads — aber selbst der Webhook ist noch… |
| bot/entitlements | 503 | tb-raid score_refresh.rs / tb-analytics plan.rs | **partial** | Score-Boost-Entitlement seit #127 nativ (score_refresh.rs). Allgemeine Entitlement-Repository-Reads laut Welle-C-Plan noch zu portieren (wandert mit Welle D Dashboard). Plan-Gating (tb-analytics/plan.… |
| bot/storage | 7435 | tb-db (99) + tb-crypto (287) + auth/session+fernet | **ported** | pg.py-Godfile bewusst aufgelöst (Cleanup #1/#2): sqlx-Pool statt Eigenbau-LIFO, Migrations als SSOT. Session-Store-Crypto (sessions_db.py Fernet) nativ in tb-dashboard-api/auth/fernet.rs+session.rs mi… |
| bot/core + bot/runtime + bot/base.py | 4212 | bin/tb-bot wiring + tb-http-core + tb-transport-* | **ported** | base.py (2416, DiscordWiring/InviteManager/AlertSender) per Komposition aufgelöst (Cleanup #3). Discord-Sends via Master-Broker-Relay (tb-transport-discord). Runtime-Bootstrap/Token-Loops in tb-bot ch… |
| bot/api | 3179 | tb-internal-api / tb-dashboard-api | **partial** | Öffentliche/interne API-Endpoints weitgehend über tb-internal-api und tb-dashboard-api abgedeckt; einzelne mutierende Admin/Config-Routen (Schritt 9 im Plan) noch via Proxy auf Python. Detailprüfung p… |
| bot/migrations | 1159 | rust/migrations (sqlx::migrate!) | **ported** | Migrations als Single-Source-of-Truth nach Cleanup #1 in ein sqlx-natives migrations/-Verzeichnis konsolidiert, read-only gegen Prod-Schema verifiziert (Schritt 0). |

**Wichtigste Lücken:**
- social_media ist die grösste offene Lücke: die komplette Upload-Pipeline (clip_manager, enrichment, oauth_manager, token_refresh_worker, upload_worker, Uploader für YouTube/TikTok/Instagram, video_processor, Whisper-Transkription, Approval-Service, Social-Dashboard) ist NICHT in Rust — der Rust-Crate (576 LOC) kann nur Clips von Helix fetchen und ist obendrein standardmässig deaktiviert. Schritt 8 des Cutover-Plans wurde noch nicht begonnen.
- Der Engagement-Layer (MiniMax-Chat-AI, Threads, Persona, Soul-Store, Deadlock-Wiki/Patch-Grounding, Lurker-Signal, IRC-Reader, Engagement-Dashboard, ~5100 LOC) ist komplett unportiert; Rust kennt nur das Auto-Off beim Stream-Ende. Im Chat-Pipeline ist der Engagement-Feed bewusst als No-op reserviert.
- Das VoiceReaction-Subsystem unter community/voice_reaction (~3600 LOC: Audio-Capture, Conversation-Brain, Scheduler, Prompts) ist nicht portiert — der VoiceReaction-Dispatch-Schritt in der Rust-Chat-Pipeline ist ein bewusster Platzhalter-No-op.
- Highlight-Clipper (worker, demo_analyzer, event_detector, VOD-Download, DM-Sender, ~1500 LOC) hat keinen Rust-Gegenpart und ist keinem Cutover-Schritt zugeordnet — läuft weiter rein in Python.
- Der KI-Title-Generator (title_ai LLM-Generierung + DB + Knowledge/Insight-Jobs) ist nicht portiert; der !title/!titel-Chat-Command gibt in Rust bewusst false zurück und fällt damit aus.
- Die KI-/Coaching-/Post-Stream-Analytics (api_ai 1154, coaching_engine 1632, api_post_stream 1392 + report_builder 800, api_insights 1030) sind in Rust nicht nativ und laufen nur über den Strangler-Proxy auf Python 8765 weiter — Python bleibt dort autoritativ.
- Der Dashboard-Schreibpfad ist erst zur Hälfte da: Billing-Webhook (Stripe), Live-Announcement-Config-UI, Affiliate-Flächen, abbo-Routen und der mutierende Admin-/Config-Bereich (Schritt 9) sind nicht nativ; laut 11.6.-Audit fehlen noch ~40+ v2-Routen. Vieles ist via Proxy überbrückt, nicht funktional in Rust nachgebaut.
- Billing/Gutschrift-PDF (fpdf2) ist bewusst gedropped (Stripe-hosted, Welle C eingedampft) — aber selbst der als einzig nötig deklarierte Stripe-Webhook-Eingang ist in Rust noch NICHT implementiert (kein Stripe-Signatur-Verify-Handler auffindbar).
- stream_coaching_audit (YouTube-Archiv-Pipeline + faster-whisper-Captions) ist nicht portiert und nicht eingeplant; internes Tool, blockiert ohnehin durch fehlenden separaten YouTube-OAuth-Account.
- Partner-Recruitment (community/partner_recruit + das Rekrutierungs-Anhängsel des Poll-Ticks) ist bewusst pausiert bis zur Outreach-Phase 6g und läuft solange nicht in Rust.

## CRITICAL (1)

### [dash-audience] `[SQLX-DECODE]` category-comparison & leaderboard: int4/numeric-Spalten als f64 dekodiert → viewer_count/peak-Zahlen werden 0
*class:* SQL-/Typ-Drift (Bug-Klasse 4) · *confidence:* 0.83 · *id:* dash-audience-1

- **Python** bot/analytics/api_performance.py:893-927 (your_tracked AVG/MAX), 1011 (peak_sorted), 1227/1252-1253 (leaderboard avg/peak); bot/analytics/api_insights.py:152 (sorted_avgs)
  - Python liest die Aggregate ohne Typzwang: int(your_tracked[1]), float(row[1]) etc. — funktioniert für int4/numeric gleichermaßen und liefert die echten Viewer-Zahlen.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/category_comparison.rs:88-92,141-156,234-245,287-290; category_leaderboard.rs:143-144,192
  - Rust dekodiert AVG(viewer_count) (Postgres numeric) und MAX(viewer_count)/MAX(peak_viewers) (int4) konsequent als try_get::<Option<f64>>. Die Prod-Spalten sind viewer_count INTEGER (belegt: market_share.rs:215/240 liest sie als Option<i32> via i64::from, session_detail.rs:209 als i32) und peak_viewers INTEGER. sqlx ist in Cargo.toml:44 ohne Feature bigdecimal/rust_decimal gebaut, hat also keinen Decode für numeric, und int4≠f64 schlägt ebenfalls fehl. Die Err werden per .ok().flatten()/unwrap_or(0.0) zu 0 verschluckt.
- **Divergenz:** your_tracked_avg/peak=0 (Comparison fällt für avg auf Session-REAL zurück, aber your_peak bleibt 0, da auch MAX(peak_viewers) int4 ist), cat_avg_peak=0, peak_sorted leer → peak_percentile=50 fix, sorted_avgs leer → avg_percentile=50/categoryTotal=0/peerGroup=null. Im Leaderboard werden ALLE avgViewers und peakViewers zu 0 und yourTier=get_tier(0)='starter' für jeden. Python liefert hier die echten Werte. Resultat: die ganze Kategorie-Vergleichs- und Leaderboard-Seite zeigt dauerhaft Nullen/falsche Tiers.
- **Fix:** viewer_count/peak_viewers in den SQLs explizit casten: AVG(viewer_count)::double precision, MAX(viewer_count)::bigint bzw. ::double precision, und Rust-seitig als i64/f64 passend dekodieren (oder das sqlx-Feature rust_decimal/bigdecimal aktivieren). Vorbild: timings nutzt PERCENTILE_CONT (gibt double precision) und COUNT (int8) und ist deshalb korrekt.
- **Verify-Fix:** Integer-Aggregate als Integer, numeric-Aggregate explizit casten — nicht alles als f64. Konkret:

1. MAX(viewer_count)/MAX(peak_viewers) (int4): mit try_get::<Option<i32>, _> lesen (analog market_share.rs:215, session_detail.rs:207) und in i64 hochziehen. Betrifft category_comparison.rs:90-92 (your_tracked_peak), :107 (your_peak/MAX(peak_viewers)), :277/289-291/304 (peer MAX(peak_viewers)), :234-245 (peak_sorted) sowie category_leaderboard.rs:144 (peak_vc).

2. AVG(viewer_count) (numeric): zwei Wege — (a) sauberste/empfohlene Variante: in der SQL explizit casten, z.B. AVG(viewer_count)::float8 AS avg_vc (bzw. AVG(c.viewer_count)::double precision), dann bleibt try_get::<Option<f64>> korrekt; oder (b) sqlx-Feature "bigdecimal"/"rust_decimal" im Workspace aktivieren und Decimal→f64 konvertieren. Variante (a) ist minimal-invasiv und vermeidet eine neue Dependency. Betrifft alle AVG(viewer_count)-Selects in beiden Handlern (category_comparison.rs:83,116,141-143,150-152; category_leaderboard.rs:77,90).

3. Achtung Q4/Q8: AVG(max_vc) über einer int4-MAX-Subquery (category_comparison.rs:141/150) liefert ebenfalls numeric → ::float8 auf das äußere AVG.

4. Verifikation: Integrationstest gegen echte Prod-Spaltentypen (int4/numeric), der für einen Streamer mit bekannten Sessions avgViewers/peakViewers != 0 und yourTier != 'starter' erwartet; ggf. die geschluckten try_get-Errs vorübergehend loggen, um stille Decode-Fehler künftig sichtbar zu machen.

## HIGH (13)

### [ana-crate] partner_access: grace_active prüft nicht mehr ob grace_expires_at in der Zukunft liegt
*class:* Fehlende Guards/Bedingungen (Zeitfenster) · *confidence:* 0.9 · *id:* ana-crate-2

- **Python** bot/analytics/api_v2.py:1043-1047
  - grace_active ist nur true, wenn grace_expires_at in der ZUKUNFT liegt und role_removed false ist. Bei abgelaufenem Grace fällt der Streamer aus dem token_error-Sonderzustand.
- **Rust** rust/crates/tb-analytics/src/partner_access.rs:194-199
  - grace_active = !grace_raw.is_empty() && !role_removed — der `> now()`-Vergleich fehlt komplett. Ein längst abgelaufener Grace gilt weiter als aktiv.
- **Divergenz:** Mit abgelaufenem grace_expires_at setzt Rust partner_status weiterhin auf token_error (Zeile 206-211) wenn technical_pause_reason=='token_error' oder error_count>0. token_error ist in is_analytics_blocked (Zeile 23) → analytics_access_allowed=false. Der Streamer bleibt dauerhaft aus dem Analytics-Dashboard ausgesperrt, obwohl die Gnadenfrist abgelaufen ist und Python ihn freigegeben hätte. Außerdem liefert Rust token_error_grace_expires_at nur bei grace_active, Python immer solange blacklist_row existiert (api_v2.py:1069-1071).
- **Fix:** grace_active um Zukunfts-Check erweitern: grace_raw als chrono-Timestamp parsen, nur aktiv wenn parsed > Utc::now() && !role_removed. token_error_grace_expires_at unabhängig von grace_active setzen.
- **Verify-Fix:** In partner_access.rs den Zukunfts-Vergleich nachziehen, analog zu Python. grace_raw (text, ISO-8601) zu einem UTC-Zeitpunkt parsen (z.B. chrono DateTime<Utc>::parse_from_rfc3339 / fromisoformat-Äquivalent, mit Z→+00:00-Normalisierung und Annahme UTC bei fehlendem Offset, exakt wie _parse_access_state_datetime Z.907-914) und grace_active nur dann true setzen, wenn parsed > Utc::now() && !role_removed. Bei Parse-Fehler grace_active=false (Python gibt None zurück → grace_active=false). Den irreführenden Kommentar (Z.194) belassen oder präzisieren. Zusätzlich Unit-Test mit drei Fällen: (a) Grace in Zukunft → token_error, (b) Grace abgelaufen + error_count>0 ohne pause_reason → active/Zugang erlaubt, (c) unparsbarer/leerer grace → kein token_error. Optional klären, ob token_error_grace_expires_at bei abgelaufenem Grace mit Python (liefert den Wert immer solange row existiert) oder mit der jetzigen Rust-Logik parity haben soll — Python-Parität würde bedeuten, das Feld unabhängig von grace_active zu setzen.

### [chat-pipeline] Scam-Pitch: Pipeline löscht Nachricht + timeoutet schon beim ersten WARNING_STRONG — Python warnt nur
*class:* Vergessene/zusätzliche Seiteneffekte + fehlende Guards · *confidence:* 0.8 · *id:* chat-pipeline-1

- **Python** bot/chat/service_pitch_warning.py:983-1030 (erster WARNING_STRONG: nur _send_chat_message + Cooldowns, kein Timeout, kein Delete) und :928-977 (Timeout nur als Eskalation bei Re-Trigger auf Cooldown, ebenfalls kein Delete)
  - Beim ersten WARNING_STRONG postet _maybe_warn_service_pitch nur eine öffentliche Chat-Warnung, setzt Cooldowns und kehrt zurück. Die auslösende Nachricht bleibt stehen, der User wird NICHT getimeoutet. Ein 600s-Timeout passiert ausschließlich im Eskalationszweig (User schreibt erneut einen Pitch, während er bereits auf user_cd-Cooldown steht). In keinem Zweig wird die Nachricht gelöscht; es gibt auch keinen Discord-Mod-Alert über den Changelog-Cog (nur _record_service_warning in DB/Log).
- **Rust** rust/crates/tb-chat/src/pipeline.rs:444-494 (StrongTimeout → execute_auto_ban ban=false = Delete + timeout_user 600s; PublicWarn → Delete) i.V.m. rust/crates/tb-chat/src/scam_pitch.rs:1053-1084 (erster WARNING_STRONG liefert StrongTimeout)
  - scam_pitch.observe() postet bei WARNING_STRONG/PUBLIC bereits intern die Warnung (send_message) bzw. timeoutet intern im Eskalationszweig und liefert dann StrongTimeout/PublicWarn zurück. Die Pipeline führt darauf zusätzlich aus: StrongTimeout → Nachricht löschen (execute_auto_ban ban=false) UND timeout_user 600s UND Discord-Alert 'scam_pitch_timeout'; PublicWarn → Nachricht löschen UND Discord-Alert 'scam_pitch_warn'. Beim ersten Strong-Treffer wird also gelöscht+getimeoutet, im Eskalationszweig wird zusätzlich zum bereits internen Timeout ein zweiter (redundanter) Timeout abgesetzt und ebenfalls gelöscht.
- **Divergenz:** Für echte Nutzer ist das Ergebnis härter als die Referenz: Python verwarnt beim ersten Strong-Pitch nur, Rust löscht die Nachricht und sperrt 10 Minuten. Bei Falsch-Positiven (Account knapp >Schwelle, harmlose Vorstellung im kleinen Kanal) führt das zu Message-Delete + Timeout statt nur einer Warnung. Im Eskalationszweig kommt ein doppelter Timeout-Call dazu (idempotent, aber unnötig) plus Delete. Zusätzlich gibt es neue Discord-Mod-Alerts, die Python in diesem Pfad nicht sendet. Das Modul-Doc (pipeline.rs Z.14-15) beschreibt dieses Verhalten als gewollt, aber das tatsächliche Moderationsergebnis weicht vom Python-Referenzverhalten ab.
- **Fix:** Wenn Python-Parität gewünscht ist: im StrongTimeout/PublicWarn-Zweig der Pipeline KEIN execute_auto_ban(Delete) aufrufen, und den Timeout nur ausführen, wenn die Decision aus dem Eskalationszweig stammt (scam_pitch.rs müsste First-Strong vs. Eskalation unterscheidbar machen, z.B. separate PitchDecision::EscalatedTimeout). First-Strong sollte zu reiner Public-Warnung ohne Delete/Timeout führen. Falls die härtere Linie bewusst beibehalten wird: in 05-cleanup-decisions.md als beabsichtigte Verhaltensänderung dokumentieren und die doppelte Timeout-Ausführung im Eskalationszweig entfernen.
- **Verify-Fix:** scam_pitch.rs an die Python-Semantik angleichen: Beim ERSTEN WARNING_STRONG (nicht-Cooldown-Pfad, Z.1077-1081) eine warn-only-Decision liefern statt StrongTimeout — d.h. eine eigene Variante (z.B. `StrongWarn`) zurückgeben, auf die die Pipeline KEIN Delete/Timeout/Alert anwendet, nur die interne Chat-Warnung. StrongTimeout (mit Delete+Timeout+Alert in pipeline.rs:445-473) ausschließlich aus dem Eskalationszweig (scam_pitch.rs:1046) liefern, der dem Python-`ESCALATED_TIMEOUT` entspricht — und dort den internen `timeout_user`-Aufruf (Z.1018-1029) entfernen, damit nicht doppelt getimeoutet wird (Pipeline timeoutet dann einmalig). Zusätzlich für die Warn-Stufe: PublicWarn in pipeline.rs:475-491 darf nicht löschen (execute_auto_ban entfernen), da Python im Warn-Pfad weder löscht noch alertet — entweder Delete+Alert streichen oder, falls das Delete-on-warn bewusst gewollt ist, das als ADR/05-cleanup-decisions.md-Eintrag dokumentieren. Wenn das härtere Verhalten produktseitig erwünscht ist, ist das eine Produktentscheidung — dann nicht als Python-Port führen, sondern explizit in 05-cleanup-decisions.md festhalten.

### [chat-scam] Follower-Gate faktisch deaktiviert: pre_warm_follower_cache wird nie aufgerufen, große Kanäle werden nicht mehr ausgenommen
*class:* Vergessene Seiteneffekte / fehlende Guards (2/3) · *confidence:* 0.9 · *id:* chat-scam-1

- **Python** bot/chat/service_pitch_warning.py:700-746 (_get_streamer_followers_hint + _is_low_follower_target), Aufruf 847-849
  - _maybe_warn_service_pitch ruft _is_low_follower_target → _get_streamer_followers_hint, das bei Cache-Miss SYNCHRON die DB liest (twitch_stream_sessions). Hat der Kanal >400 Follower, ist is_low_target=False und die Funktion bricht ab (Z. 848-849: 'if not is_low_target: return False') — KEINE Warnung/Timeout. Nur kleine (≤400) oder Kanäle ohne Session-Daten (None→assume small) durchlaufen weiter.
- **Rust** rust/crates/tb-chat/src/scam_pitch.rs:1162-1189 (get_follower_hint Cache-Miss → (true, None)); pre_warm_follower_cache 1200-1226 nirgends aufgerufen (bin/tb-bot/src/chat_wiring.rs:142-261 ruft es nicht; pipeline.rs:443 ruft nur observe())
  - get_follower_hint liefert bei Cache-Miss immer (is_low_target=true, follower_count=None) ('assume small'). Da pre_warm_follower_cache in keiner Wiring-/Pipeline-Stelle aufgerufen wird, ist der Cache permanent leer → jeder Kanal wird als low-target behandelt. Der Follower-Gate filtert nie einen großen Kanal aus.
- **Divergenz:** Etablierte/große Partnerkanäle (>400 Follower in twitch_stream_sessions), die Python bewusst von Service-Pitch-Warnungen und -Timeouts ausnimmt, erhalten im Rust-Port doch Warnungen bzw. 10-Minuten-Timeouts, sobald ein Chatter genug Score sammelt (crew_threat, growth_pitch usw.). Zusätzlich feuern die Reason 'target:followers_X' und der +1-Boost 'target:very_small_channel' (fc ≤ 200) nie. Folge: unerwünschte Timeouts/Warnungen auf legitimen großen Kanälen.
- **Fix:** get_follower_hint async machen und bei Cache-Miss direkt aus dem Pool lesen (gleiche Query wie pre_warm_follower_cache), Ergebnis cachen — statt synchron 'assume small' zurückzugeben. Alternativ den Cache in der Pipeline vor observe() per pre_warm_follower_cache befüllen (z. B. on_stream_went_live oder einmal pro Kanal/observe). Wichtig: das Verhalten 'None → assume small' bleibt nur für echte Cache-/DB-Miss, nicht für jeden ungewarmten Kanal.
- **Verify-Fix:** Den Follower-Gate wieder scharf stellen, ohne synchronen DB-Call im sync-Hot-Path. Zwei gangbare Wege: (A) `get_follower_hint` async machen und in `observe` (Z.870-874) bei Cache-Miss direkt einen einmaligen Lazy-DB-Read durchführen (die SQL aus `pre_warm_follower_cache` Z.1207-1221 wiederverwenden, Ergebnis in `follower_cache` schreiben, dann wie Python `None→(true,None)` / `count<=MAX_FOLLOWERS` werten) — das spiegelt Pythons Verhalten am genauesten (synchron-lazy pro Kanal). (B) Falls man den Hot-Path nicht async-DB-belasten will: beim Subscription-Reconcile/Startup (`start_background` bzw. `reconcile_chat_subscriptions`) für jeden Partner-/Monitored-Kanal einmalig `pre_warm_follower_cache` aufrufen und periodisch (z.B. im Reconcile-Tick alle 30 min) auffrischen, damit `follower_cache` vor dem ersten `observe()` befüllt ist. Variante A ist robuster gegen neue/unbekannte Kanäle. Nach dem Fix einen Test ergänzen, der für einen Kanal mit >MAX_FOLLOWERS-Session `PitchDecision::None` erwartet, und den Toten-Code-Marker/UNSICHER-Kommentar (Z.1181-1187, 2318) entfernen.

### [dash-audience] `[SQLX-DECODE]` audience-demographics: SUM(viewer_count) (int8) als f64 dekodiert → viewerMinutes/messagesPer100 werden 0 trotz Real-Samples
*class:* SQL-/Typ-Drift (Bug-Klasse 4) · *confidence:* 0.78 · *id:* dash-audience-3

- **Python** bot/analytics/api_audience.py:1226-1248 (viewer_sample_row SUM(GREATEST(sv.viewer_count,0)); viewer_minutes = samples if count>0 else fallback)
  - Python liest SUM(...) als float(...) und setzt viewer_minutes = echte Summe, wenn viewer_sample_count > 0.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/audience_demographics.rs:304-313
  - vm_real = try_get::<f64>('vm') auf SUM(GREATEST(viewer_count,0)). viewer_count ist int4, SUM(int4) liefert in Postgres bigint (int8); f64-Decode schlägt fehl → unwrap_or(0.0) → vm_real=0. Da viewer_sample_count>0 (COUNT(*)=int8 dekodiert korrekt), wird viewer_minutes=vm_real=0 statt der echten Summe.
- **Divergenz:** Sobald twitch_session_viewers-Sample-Zeilen existieren (vm_has_real=true), liefert Rust viewerMinutes=0 und damit messagesPer100ViewerMinutes auf Basis 0, während Python die reale Viewer-Minuten-Summe nutzt. Engagement-method bleibt evtl. real_samples, aber die ausgegebenen Minuten/Interaktionsraten sind falsch.
- **Fix:** SUM(GREATEST(sv.viewer_count,0))::double precision casten oder Rust-seitig als i64 dekodieren und zu f64 casten.
- **Verify-Fix:** Den int8-Wert korrekt dekodieren statt als f64. Zwei gleichwertige Optionen:

1) Im Rust-Decode (Z.311) als i64 lesen und casten:
   `let vm_real: f64 = vsamp.as_ref().and_then(|r| r.try_get::<i64, _>("vm").ok()).unwrap_or(0) as f64;`

2) ODER im SQL (Z.305) explizit zu float8 casten, damit der bestehende f64-Decode passt:
   `COALESCE(SUM(GREATEST(sv.viewer_count,0)),0)::float8 AS vm`

Variante 2 ist konsistenter mit vm_fallback (das schon float8 ist) und macht den Decode-Vertrag im Handler einheitlich. Zusätzlich: einen Test mit eingefügten twitch_session_viewers-Zeilen ergänzen, der viewerMinutes>0 und messagesPer100ViewerMinutes!=null im Real-Samples-Zweig prüft (deckt die fehlende Regression ab). Audit-Empfehlung: alle try_get::<f64> auf SUM/COUNT/Integer-Spalten im Handler (und analogen Handlern) gegen die tatsächlichen Postgres-Aggregat-Typen prüfen — SUM(int4)=int8, COUNT=int8, AVG(int)=numeric.

### [dash-audience] Extended-Plan-Entitlement-Gate fehlt komplett im Rust-Port (demographics/insights/leaderboard/timings/viewer-profiles/sharing)
*class:* Fehlende Guards/Bedingungen (Bug-Klasse 3) · *confidence:* 0.72 · *id:* dash-audience-4

- **Python** bot/analytics/api_audience.py:1048 (demographics), 859 (insights); api_performance.py:1276 (leaderboard), 1432 (timings); api_overview.py:2115 (viewer-profiles), 2217 (audience-sharing); Gate-Definition api_v2.py:638-668 (_require_extended_plan → 403 plan_required ohne Entitlement, admin/localhost-Bypass)
  - Python ruft vor jedem dieser Endpoints _require_extended_plan auf: Ein nicht-Admin-Streamer ohne extended-analytics-Entitlement bekommt HTTP 403 plan_required. Admin/localhost werden durchgelassen.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/audience_demographics.rs:237-239; audience.rs:36-42,665-672 (insights/profiles); category_comparison.rs:68; category_leaderboard.rs:56; category_timings.rs:85 — alle prüfen nur DashboardAuthLevel::None
  - Die Rust-Handler prüfen ausschließlich, ob überhaupt authentifiziert (auth != None). Es gibt keine Entitlement-/Plan-Prüfung; jeder authentifizierte Nutzer erhält die vollen Daten. follower_funnel.rs:5 dokumentiert die Auslassung explizit als bewusst.
- **Divergenz:** Bezahlpflichtige Extended-Analytics werden im Rust-Pfad an Nutzer ohne Plan ausgeliefert (Entitlement-Bypass = Umsatz-/Berechtigungsthema). Die Entscheidung ist nur in Inline-Kommentaren dokumentiert, NICHT in der gesegneten 05-cleanup-decisions.md.
- **Fix:** Entweder bewusst-anders in 05-cleanup-decisions.md aufnehmen und absegnen, oder einen Plan-Entitlement-Check (tb_analytics::plan::resolve_plan_snapshot + has_entitlement, mit Admin/Localhost-Bypass) als Extraktor/Middleware vor diese Routen hängen.
- **Verify-Fix:** Die bereits existierende, korrekte Funktion `tb_dashboard_api::auth::require_extended_plan(pool, login, auth)` in jeden der betroffenen Handler einbauen, direkt nach dem Auth-Check und bevor Daten geladen werden — analog zu Pythons `self._require_extended_plan(request)`: audience_demographics, audience_insights, viewer_profiles, audience_sharing (audience.rs), category_leaderboard, category_comparison, category_timings, plus den bewusst ausgelassenen follower_funnel. Den `login` aus dem `streamer`-Query-Param bzw. der Partner-Session ableiten (Python nutzt streamer-Param, dann Session-Login als Fallback; bei leerem Streamer-Kontext überspringt Python das Gate — dieses 'kein-streamer→skip'-Verhalten spiegeln). Bei `Err(ApiError::plan_required())` 403 zurückgeben. Den irreführenden Kommentar in follower_funnel.rs:5-6 entfernen. Die Aufnahme dieses Gates als Paritäts-Entscheid in 05-cleanup-decisions.md dokumentieren (es ist eben KEINE bewusste Auslassung). Verifikation: integration-test mit Partner-Session ohne Entitlement → erwartet 403 plan_required; mit analytics_pro/analytics_extended bzw. Admin/Localhost → 200.

### [dash-auth-legal] Admin-streamers list planId precedence inverted
*class:* SQL-Drift · *confidence:* 0.93 · *id:* dash-auth-legal-2

- **Python** admin_streamer_queries.py:397-402
  - manual_plan_id then billing
- **Rust** admin_streamers.rs:221
  - billing then manual
- **Divergenz:** manual admin override hidden behind billing
- **Fix:** manual_plan_id.or(billing_plan_id)
- **Verify-Fix:** In rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:221 die Präzedenz umkehren, damit der manuelle Override Vorrang hat wie in Python: `plan_id: r.manual_plan_id.or(r.billing_plan_id)`. Um auch das sekundäre Empty-String-Verhalten anzugleichen (Python behandelt `""` als leer), zusätzlich leere Strings normalisieren, z.B. `plan_id: r.manual_plan_id.filter(|s| !s.trim().is_empty()).or(r.billing_plan_id.filter(|s| !s.trim().is_empty()))`. Anschliessend prüfen, ob der Detail-Handler (gleiche Datei, ab Zeile ~319) dieselbe planId-Ableitung benötigt/konsistent ist, und einen Test mit Streamer = (manual_plan_id=Some, billing_plan_id=Some) ergänzen, der erwartet, dass der manuelle Wert zurückkommt.

### [dash-perf] `[SQLX-DECODE]` viewer_count_sent (INTEGER) als i64 dekodiert → viewers_sent immer 0 → Retention/Conversion-Prozente 0
*class:* sqlx Typ-Mismatch / SQL-Default-Drift · *confidence:* 0.9 · *id:* dash-perf-1

- **Python** bot/analytics/api_overview.py:2004; bot/analytics/api_raids.py:122
  - viewer_count_sent wird als Integer gelesen; ret_pct=chatters_30m/viewers_sent*100, conv_pct=new_chatters/viewers_sent*100 ergeben reale Werte.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/raid_analytics.rs:259,399
  - try_get::<Option<i64>>(viewer_count_sent) auf INTEGER-Spalte scheitert im sqlx-Decode, .ok().flatten().unwrap_or(0)=0; Guard if viewers_sent>0 setzt retention30mPct und chatterConversionPct fuer jeden Raid auf 0.
- **Divergenz:** twitch_raid_retention.viewer_count_sent ist INTEGER (i32); sqlx-Postgres dekodiert INT4 nicht in i64 (ColumnDecode-Error). tb-analytics/raids.rs dokumentiert genau diesen Vertrag (Option<i32>). Alle Raid-Retention/Per-Source-Prozente sind null statt der echten Werte.
- **Fix:** als Option<i32> lesen oder im SQL viewer_count_sent::BIGINT casten.
- **Verify-Fix:** In raid_analytics.rs an beiden Stellen (Zeile 259 und 399) sowie bei den uebrigen INTEGER-Spalten chatters_at_plus5m/15m/30m, new_chatters, known_from_raider (Zeilen 262-266) den Decode-Typ von Option<i64> auf Option<i32> umstellen und das Ergebnis fuer die nachfolgende Arithmetik per `as i64`/`as f64` weiten — z. B. `row.try_get::<Option<i32>, _>("viewer_count_sent").ok().flatten().unwrap_or(0) as i64`. Den Vertrag aus tb-analytics/src/raids.rs (Option<i32> fuer INTEGER) konsistent uebernehmen. Wichtig: das `.ok()` verschluckt den ColumnDecode-Fehler still — kurzfristig sollte der Decode-Fehler mindestens geloggt werden (statt nur .ok()), damit kuenftige Typ-Drift nicht erneut unbemerkt zu Null-Metriken fuehrt. Zusaetzlich einen Integrationstest gegen eine echte INT4-Spalte ergaenzen, der retention30mPct/chatterConversionPct > 0 bei nicht-leeren Daten prueft, da aktuell keinerlei Test diese Pfade abdeckt.

### [dash-perf] `[SQLX-DECODE]` Incoming-Raids: session_viewers.viewer_count (INTEGER) als i64 → Timeline leer → Boost/Retention null
*class:* sqlx Typ-Mismatch · *confidence:* 0.9 · *id:* dash-perf-3

- **Python** bot/analytics/api_raids.py:356
  - Timeline-Dict aus viewer-rows; daraus viewers_before, peak_after, boost_pct, retention_5m/15m/30m.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/raid_analytics.rs:607-609,576
  - filter_map liest viewer_count als i64; Decode auf INTEGER scheitert je Zeile, das ? verwirft sie → timeline-HashMap leer → before/after leer → impact.boost_pct/retention bleiben null. viewers_sent (arrival viewer_count, Z.576) als i64 → 0.
- **Divergenz:** twitch_session_viewers.viewer_count und arrival.viewer_count sind INTEGER; als i64 scheitert sqlx still. incoming_summary.avg_boost_pct, best_raider, avg_retention_15m und alle impact-Felder sind null.
- **Fix:** viewer_count als i32 lesen (session_detail.rs:220 korrekt) oder ::BIGINT casten.
- **Verify-Fix:** In raid_analytics.rs zwei Stellen auf i32 für INTEGER-Spalten umstellen, analog zu session_detail.rs:

- Z.607-609: `viewer_count` als i32 lesen und in den HashMap-Wert übernehmen, z.B. `let timeline: HashMap<i32, i64> = tl_rows.iter().filter_map(|r| Some((r.try_get::<i32,_>("minutes_from_start").ok()?, r.try_get::<i32,_>("viewer_count").ok()? as i64))).collect();` (Cast auf i64 erst nach erfolgreichem Decode, damit die nachgelagerte i64-Arithmetik in before/after/sum unverändert bleibt).
- Z.576: `viewer_count` aus twitch_raid_arrival_tracking als i32 lesen und casten: `let viewers_sent: i64 = rr.try_get::<Option<i32>,_>("viewer_count").ok().flatten().unwrap_or(0) as i64;`

Danach einen Integrationstest für /raids ergänzen, der die Tabellen mit INTEGER-Spalten (wie das Prod-Schema, nicht BIGINT) anlegt, eine Session-Timeline + arrival-Zeile seedet und prüft, dass incoming_summary.avg_boost_pct/avg_retention_15m/best_raider und impact.boost_pct nicht null sind — sonst maskiert ein BIGINT-Test-Schema den Fehler weiter. Optional repo-weit nach `try_get::<i64` auf bekannten INTEGER-Spalten greppen, da sqlx-Postgres diese Verwechslung generell still macht.

### [dash-perf] session/{id}/events: raids und follows hardcodiert leer + Response-Shape/Keys abweichend
*class:* Toter Code / fehlende Portierung · *confidence:* 0.85 · *id:* dash-perf-5

- **Python** bot/analytics/api_v2.py:2388-2457
  - channel_updates[{at,title,game,language}], raids[{at,channel,viewers,direction}] (incoming aus arrival_tracking + outgoing aus raid_history, UNION ALL, ORDER BY 1), follows_per_minute[{minute,count}] (DATE_TRUNC minute).
- **Rust** rust/crates/tb-dashboard-api/src/handlers/session_detail.rs:312-318
  - Gibt {sessionId,streamerLogin,channelUpdates[{recordedAt,title,gameName,language}],raids:[],follows:[]} zurueck. raids/follows feste leere Arrays, Raid-UNION- und Follow-per-Minute-Queries fehlen, Keys umbenannt.
- **Divergenz:** Frontend das die Python-Form erwartet bekommt andere Struktur und nie Raid-/Follow-Daten. Events-Tab der Session-Detailansicht effektiv tot.
- **Fix:** Raid-UNION (arrival incoming + history outgoing) und follows-per-minute portieren; Response-Shape und Keys exakt auf Python bringen.
- **Verify-Fix:** In session_events_handler (session_detail.rs) die fehlenden Queries portieren und die Response-Keys an die Python-/Frontend-Form angleichen: (1) channel_updates statt channelUpdates, Felder at/title/game/language (recordedAt→at, gameName→game). (2) raids über UNION ALL: SELECT detected_at AS at, from_broadcaster_login AS channel, viewer_count, 'incoming' FROM twitch_raid_arrival_tracking WHERE LOWER(to_broadcaster_login)=streamer_login AND detected_at BETWEEN started_at AND COALESCE(ended_at,NOW()) UNION ALL SELECT executed_at, to_broadcaster_login, viewer_count, 'outgoing' FROM twitch_raid_history WHERE LOWER(from_broadcaster_login)=streamer_login AND executed_at BETWEEN ... ORDER BY 1; mappen zu [{at,channel,viewers,direction}]. (3) follows_per_minute über DATE_TRUNC('minute', followed_at::timestamptz) AS minute, COUNT(*) FROM twitch_follow_events WHERE LOWER(streamer_login)=streamer_login AND followed_at BETWEEN ... GROUP BY 1 ORDER BY 1; mappen zu [{minute,count}]. Top-Level-Wrapper-Felder sessionId/streamerLogin sind harmlos (Frontend ignoriert Extra-Keys), können bleiben. Danach gegen die Python-Antwort an einer realen Session mit Raids+Follows diffen und im Events-Tab des SessionDetail rendern lassen.

### [dash-viewers] Bot-Exclusion != ANY filtert in viewers.rs nie etwas heraus
*class:* SQL-/Default-Drift · *confidence:* 0.93 · *id:* dash-viewers-1

- **Python** api_viewers.py:60-65,148
  - NOT IN mit voller Bot-Liste entfernt Bots zuverlaessig.
- **Rust** viewers.rs:307,417,444,629,883,932,933,961,962,985,986
  - != ANY($n): x<>ANY(array) ist bei 10 Bots praktisch immer TRUE, nichts ausgeschlossen.
- **Divergenz:** Bots zaehlen als Viewer; Directory/Cross-Channel/Top-Channels/detail/Churn/Segment/Direction falsch. lurker_analysis.rs/viewer_timeline.rs korrekt.
- **Fix:** != ANY durch != ALL bzw NOT (=ANY) ersetzen.
- **Verify-Fix:** In viewers.rs alle Bot-Exklusionen von `!= ANY($n)` auf die korrekte Negation umstellen — passend zur restlichen Crate-Konvention `AND NOT (LOWER(sc.chatter_login) = ANY($n::text[]))` (alternativ `LOWER(sc.chatter_login) != ALL($n)`). Betroffene Stellen: Zeilen 307, 417, 444, 629, 883, 932, 933, 961, 962, 985, 986. Bei den streamer_login-Vergleichen gegen `&bots` (z.B. 933, 962, 986, 883) prüfen, ob dort die Bot-Exklusion auf streamer_login überhaupt gewollt ist (Python filtert nur chatter_login); falls nicht, diese Bedingung entfernen statt nur den Operator zu korrigieren. Zusätzlich Python-Parität wahren: NULL/Leer-Login-Schutz (`IS NULL OR = ''`) ist in Rust bereits durch das nachgelagerte `.filter(|v| !v.login.is_empty())` bzw. GROUP BY abgedeckt, aber gegenprüfen. Idealerweise einen geteilten Helfer (analog viewer_timeline::bot_not_in_sql) verwenden, um die Drift künftig auszuschließen, und einen Regressionstest mit einem Bot-Login im Fixture ergänzen, der nach Aggregation NICHT auftauchen darf.

### [mon-poll] Kategorie-Sampling läuft ohne Sprachfilter statt deutsch — Python erzwingt hartkodierte DE-Varianten
*class:* SQL-/Default-Drift · *confidence:* 0.78 · *id:* mon-poll-1

- **Python** bot/core/constants.py:18 (TWITCH_LANGUAGE="de de-de de-at de-ch", hartkodierte Konstante) + bot/monitoring/monitoring.py:1062-1072 (_language_filter_values) + 1245-1265 (Kategorie-Loop nutzt diese Filter)
  - Der Sprachfilter fürs Kategorie-Sampling (Discovery) ist eine hartkodierte Konstante TWITCH_LANGUAGE="de de-de de-at de-ch" — KEINE Env-Variable. _language_filter_values() liefert daraus stets [de, de-de, de-at, de-ch], und get_streams_by_category wird pro Variante gefiltert aufgerufen. Das Sample enthält damit ausschließlich deutschsprachige Deadlock-Streams. Das fließt in twitch_stats_category, in die Session-Samples der kategorie-entdeckten Streams und (perspektivisch) in den Rekrutierungs-Pool.
- **Rust** rust/crates/tb-monitoring/src/poller/engine.rs:53 (language_filters: Vec::new() Default) + 99-111 (leer → languages=[None]) + 196-228 (Kategorie-Loop) ; gespeist aus rust/bin/tb-bot/src/main.rs:557-559 via env TWITCH_LANGUAGE_FILTERS (im Run-Skript NICHT gesetzt)
  - PollConfig.language_filters wird aus der Env-Variable TWITCH_LANGUAGE_FILTERS gefüllt (anderer Name als Pythons Konstante). Diese Variable ist weder im Run-Skript run_tb_bot_service.sh gesetzt noch als Infisical-Secret vorhanden. Folge: language_filters ist leer → languages=[None] → streams_by_category(category_id, None, ...) holt das Sample OHNE Sprachfilter, also alle Sprachen. twitch_stats_category und die Kategorie-Session-Samples enthalten dadurch nicht-deutsche Streamer, die Python ausschließt.
- **Divergenz:** Pythons Filter ist eine im Code festgelegte Konstante und greift immer; der Rust-Filter hängt an einer separat benannten, unbesetzten Env-Variable und greift real gar nicht. Damit erfasst der Rust-Monitoring-Poll dauerhaft ein anderes (breiteres, sprachgemischtes) Kategorie-Sample als Python — falsche Daten in den kategorieweiten Stats und in den Viewer-Timelines kategorie-entdeckter Kanäle. Das Monitoring ist laut Cutover live, also produktiv wirksam. (Der Filter für die getrackten Logins ist auf beiden Seiten bewusst aus — das stimmt überein und ist kein Bug.)
- **Fix:** Default für language_filters auf die deutschen Varianten setzen, die Pythons Konstante vorgibt (z. B. PollConfig::default() mit ["de","de-de","de-at","de-ch"]), ODER in main.rs/Run-Skript TWITCH_LANGUAGE_FILTERS="de,de-de,de-at,de-ch" verdrahten. Den Env-Namen an die Python-Semantik angleichen oder TWITCH_LANGUAGE als Quelle lesen. Wichtig: ein leerer Filter darf nicht 'alle Sprachen' bedeuten, wenn Python deutsch erzwingt.
- **Verify-Fix:** TWITCH_LANGUAGE_FILTERS für den Rust-Poller auf "de,de-de,de-at,de-ch" setzen — am saubersten als explizite Export-Zeile in rust/scripts/run_tb_bot_service.sh neben TWITCH_TARGET_GAME_NAME (`export TWITCH_LANGUAGE_FILTERS="${TWITCH_LANGUAGE_FILTERS:-de,de-de,de-at,de-ch}"`), damit es auch den Scout-Pfad (main.rs:607) mitversorgt. Alternativ/zusätzlich den PollConfig-Default in engine.rs:53 auf die DE-Varianten setzen, damit die Parität nicht allein an einer extern gesetzten Env-Var hängt (robuster gegen erneutes Vergessen). Bevorzugt: Default im Code spiegeln + Env-Override behalten, da Python die DE-Liste hartkodiert garantiert.

### [raid-arrival] External-recruitment and partner/recruitment-message follow-up branch not wired; five should-flags dead
*class:* Forgotten side-effects / Dead code · *confidence:* 0.82 · *id:* raid-arrival-1

- **Python** bot/raid/raid_arrival_runtime.py:265-268,337-363,403-416
  - confirm_pending_raid_arrival runs follow-up effects: on target_is_partner delete_external_recruitment_blacklist_pending(to_id); on follow_up_kind external record_confirmed_external_recruitment_raid (abort if None) plus maybe_schedule_external_recruitment_blacklist_pending plus send_recruitment_message; on ours_to_partner send_partner_raid_message.
- **Rust** rust/bin/tb-bot/src/raid_arrival_wiring.rs:256-368; rust/crates/tb-raid/src/arrival_confirmation.rs:428,432,434,435,436
  - confirm_pending_raid only writes the arrival row (when target_is_partner) and calls score_tracking.track_confirmed on ours_to_partner. The five flags should_delete_external_recruitment_blacklist_pending, should_persist_confirmed_external_recruitment_raid, should_schedule_external_recruitment_blacklist_pending, should_send_partner_raid_message, should_send_recruitment_message are set but read nowhere in tb-bot.
- **Divergenz:** Confirmed external raids on a partner neither populate twitch_confirmed_external_recruitment_raids nor schedule the blacklist bot-ban check nor send a recruitment message; partner raids send no welcome message; for every partner target the external_recruitment_blacklist_pending entry is never deleted. Cutover is LIVE so this is in production and silently inert. Messaging may be deferred to slice 6g; the data side-effects (delete/persist/schedule) are 6e scope per the plan.
- **Fix:** Wire the follow-up effects in the confirm adapter: delete blacklist-pending when target_is_partner; write the confirmed-external store (abort if None) plus the scheduler when external; messages behind the 6g port. If 6g is open, close the non-messaging effects now.
- **Verify-Fix:** In rust/bin/tb-bot/src/raid_arrival_wiring.rs::confirm_pending_raid die fehlenden Decision-Flags konsumieren, analog zur Python-Reihenfolge: (1) bei decision.should_delete_external_recruitment_blacklist_pending einen DB-Delete auf twitch_external_recruitment_blacklist_pending für to_broadcaster_id ausführen; (2) bei should_persist_confirmed_external_recruitment_raid einen Insert in twitch_confirmed_external_recruitment_raids (Rückgabe = confirmed_external_raid_count) und bei None/Fehler die externe Follow-up-Kette abbrechen (frühes return wie Python Z.354); (3) anschließend bei should_schedule_external_recruitment_blacklist_pending den Bot-Ban-Check in _external_bot_ban_check_pending / blacklist_pending mit confirmed_raid_count + raid_flow_id einplanen. Diese drei Daten-Seiteneffekte (6e-Scope) zuerst portieren, da silent in Prod inert. Die zwei Messaging-Effekte (should_send_partner_raid_message, should_send_recruitment_message) können in Slice 6g folgen, sollten dann aber explizit in 05-cleanup-decisions.md als bewusst zurückgestellt dokumentiert werden, damit der tote Flag nachvollziehbar bleibt. Zur Absicherung Integrationstest, der eine bestätigte external-Recruitment-Ankunft auf Partner durchspielt und Tabellen-Insert + Schedule prüft.

### [raid-auth] Blacklisten setzt needs_reauth/raid_enabled nicht — Token wird nach invalid_grant nicht sofort gesperrt
*class:* Vergessene Seiteneffekte · *confidence:* 0.85 · *id:* raid-auth-1

- **Python** bot/api/token_error_handler.py:641-762 (add_to_blacklist → _mark_reauth_required @161-201, _disable_raid_bot @767-811)
  - add_to_blacklist schreibt nicht nur den Fehler-Counter in twitch_token_blacklist, sondern ruft danach UNBEDINGT _mark_reauth_required() auf: das setzt auf der twitch_raid_auth-Zeile raid_enabled=FALSE und needs_reauth=TRUE (plus Partner-technical_pause_reason='token_error' und set_partner_raid_bot_enabled(False)). Schon beim ERSTEN invalid_grant (count=1) ist der Token damit gesperrt, weil get_valid_token/get_tokens_for_user/Chat-Pfad auf needs_reauth bzw. raid_enabled gaten. Ab count>=3 zusätzlich _disable_raid_bot + Discord-Notify.
- **Rust** rust/crates/tb-raid/src/token_blacklist.rs:89-188 (add_to_blacklist / add_to_blacklist_inner)
  - add_to_blacklist_inner schreibt ausschließlich error_count/last_error_at/grace_expires_at in twitch_token_blacklist. Es gibt im gesamten tb-raid-Crate keinen Pfad, der nach dem Blacklisten needs_reauth=TRUE oder raid_enabled=FALSE auf twitch_raid_auth setzt (grep bestätigt: nur token_store liest needs_reauth, niemand schreibt es im Blacklist-Pfad). Lockout greift erst, wenn is_blacklisted (error_count>=3) anschlägt.
- **Divergenz:** Bei einem revoked/invalid Refresh-Token bleibt in Rust für die ersten beiden Fehlversuche raid_enabled=TRUE und needs_reauth=FALSE. token_provider.get_valid_token und token_store.load_decrypted (WHERE raid_enabled IS TRUE, Gate auf needs_reauth) lassen den Token also weiter zu und versuchen erneut zu refreshen, statt ihn — wie Python — sofort beim ersten invalid_grant aus dem Betrieb zu nehmen. Der needs_reauth-Lockout, der Discord-Token-Error-Hinweis und der Partner-Pause-Status entfallen komplett.
- **Fix:** Im Blacklist-Pfad (oder direkt im Refresher nach dem InvalidGrant-add_to_blacklist) ein UPDATE twitch_raid_auth SET raid_enabled=FALSE, needs_reauth=TRUE, reauth_notified_at=COALESCE(...) WHERE twitch_user_id=$1 ergänzen — das _mark_reauth_required-Äquivalent. Partner-Mirroring kann per Doku auf 6b+ deferred bleiben, aber das Setzen von needs_reauth/raid_enabled auf der eigenen Auth-Zeile gehört zwingend hierher, sonst kein Sofort-Lockout.
- **Verify-Fix:** Im invalid_grant-Pfad (token_refresher.rs:213-219) bzw. innerhalb von add_to_blacklist den fehlenden Reauth-Seiteneffekt nachziehen — analog zu Python `_mark_reauth_required`, das unbedingt (count=1) läuft:

1. Nach dem Blacklist-Insert auf twitch_raid_auth `SET raid_enabled = FALSE, needs_reauth = TRUE` für die betroffene twitch_user_id schreiben (in derselben/einer Folge-Transaktion).
2. Den Partner-Spiegel setzen: `set_partner_raid_bot_enabled(false)` plus auf twitch_partners `technical_pause_reason = 'token_error'` (mit den Python-Guards: manual_partner_opt_out und vorhandenes 'bot_banned' NICHT überschreiben) und `raid_bot_enabled = 0`.
3. Den Discord-Token-Error-Hinweis (DmNotifier-Äquivalent) auslösen — in Python erst ab count>=3 via _disable_raid_bot, das den Reauth-Flag erneut setzt und benachrichtigt; den count>=3-Branch entsprechend abbilden.

Da add_to_blacklist im TokenBlacklist-Port keinen Zugriff auf den Cipher/Partner-Writer hat, am sichersten den twitch_raid_auth-Reauth-Write direkt in den invalid_grant-Zweig von token_refresher.rs legen (dort ist bereits eine Transaktion offen) oder add_to_blacklist um den Reauth-Seiteneffekt erweitern. Verifizieren mit einem Integrationstest: einmaliger InvalidGrant → twitch_raid_auth.needs_reauth=TRUE und raid_enabled=FALSE; get_valid_token liefert danach None wegen needs_reauth (nicht erst wegen is_blacklisted).

## MEDIUM (32)

### [ana-crate] Entitlement-Katalog weicht für 5 Pläne ab: analytics.ai_full fehlt komplett, raid.priority/chat.promos.disable falsch verteilt
*class:* SQL-/Default-Drift (statischer Katalog) + Entitlement-Logik · *confidence:* 0.97 · *id:* ana-crate-1

- **Python** bot/entitlements/catalog.py:66-132 (PLAN_ENTITLEMENTS_MAP)
  - analysis_dashboard={basic, ai_full, extended, lurker_tax}; bundle_analysis_raid_boost={basic, ai_full, extended, lurker_tax, promos.disable, raid.priority}; bundle_werbefrei_analyse={basic, ai_full, extended, lurker_tax, promos.disable}; bundle_komplett enthält BEIDE ai_mini UND ai_full; analytics_trial={ai_mini, basic, extended, lurker_tax} OHNE raid.priority.
- **Rust** rust/crates/tb-analytics/src/plan.rs:51-95 (plan_entitlements)
  - analytics.ai_full kommt im ganzen Rust-Katalog NICHT vor. analysis_dashboard/bundle_analysis_raid_boost/analytics_trial sind in einen Match-Arm zusammengefasst und bekommen alle {ai_mini, basic, extended, lurker_tax, raid.priority}; bundle_analysis_raid_boost verliert dadurch chat.promos.disable; bundle_werbefrei_analyse bekommt zusätzlich raid.priority; bundle_komplett verliert ai_full; analytics_trial bekommt fälschlich raid.priority.
- **Divergenz:** Die Entitlement-Sets sind live: auth_status.rs:206 reicht p.entitlements ans Dashboard, _plan_ai_model (api_ai.py:148-155) wählt anhand ai_full→Opus vs ai_mini→MiniMax das AI-Modell. Effekte: (1) Jeder Extended-Plan verliert ai_full → zahlende Erweitert/Analyse/Komplett-Kunden werden von Claude-Opus auf MiniMax heruntergestuft. (2) bundle_analysis_raid_boost verliert chat.promos.disable → Werbung wird bei diesen zahlenden Kunden NICHT mehr abgeschaltet (bezahltes Feature kaputt). (3) analysis_dashboard, bundle_werbefrei_analyse, analytics_trial bekommen raid.priority ohne Berechtigung → Raid-Score-Boost (partner_scores.py:620-623).
- **Fix:** plan_entitlements in plan.rs 1:1 an PLAN_ENTITLEMENTS_MAP angleichen: ai_full als eigenes Entitlement einführen, die zusammengefassten Match-Arme entkoppeln (analysis_dashboard ≠ bundle_analysis_raid_boost ≠ analytics_trial), raid.priority/chat.promos.disable je Plan exakt setzen, bundle_komplett ai_mini UND ai_full geben.
- **Verify-Fix:** plan.rs::plan_entitlements an catalog.py:PLAN_ENTITLEMENTS_MAP angleichen: (a) den zusammengefassten Arm analysis_dashboard|bundle_analysis_raid_boost|analytics_trial aufsplitten; (b) `analytics.ai_full` zu analysis_dashboard, bundle_analysis_raid_boost, bundle_werbefrei_analyse, bundle_komplett hinzufügen; (c) `analytics.ai_mini` aus analysis_dashboard/bundle_analysis_raid_boost/bundle_werbefrei_analyse entfernen (Python hat dort kein ai_mini); (d) `chat.promos.disable` zu bundle_analysis_raid_boost ergänzen; (e) `raid.priority` aus analysis_dashboard, bundle_werbefrei_analyse und analytics_trial entfernen. Danach einen Paritäts-Test ergänzen, der für jede Plan-ID die Rust-Menge gegen die in catalog.py definierte Menge vergleicht (Single-Source-of-Truth-Vertrag), damit die drei separaten Rust-Stellen (plan.rs, promos.rs, score_refresh.rs) nicht weiter auseinanderdriften. Trotz fehlender unmittelbarer Geld-Folge fixen, da auth-status-Entitlements UI-Feature-Gating steuern und mittelfristig weitere Rust-Consumer direkt auf plan_entitlements zugreifen könnten.

### [ana-crate] Manueller raid_free-Override wird in Rust von Stripe-Abo überschrieben statt honoriert
*class:* Fehlende Guards/Bedingungen (Prioritäts-Kette) · *confidence:* 0.85 · *id:* ana-crate-3

- **Python** bot/entitlements/repository.py:84-95 + 206-228
  - Ein nicht abgelaufener Manual-Override mit plan_id='raid_free' (Admin entzieht gecomptes Paket) wird angewendet: Snapshot=raid_free, source=manual_override; das Stripe-Abo wird NICHT herangezogen.
- **Rust** rust/crates/tb-analytics/src/plan.rs:185-225 (resolve_plan_snapshot)
  - Bei pid=='raid_free' ist `!expired && pid != raid_free` (Zeile 195) false → kein return; fällt auf Billing-Query durch und liefert das aktive Stripe-Abo.
- **Divergenz:** Ein expliziter Admin-Downgrade auf raid_free wird in Rust ignoriert, sobald der Streamer noch ein aktives/trialing/past_due-Stripe-Abo hat — er behält sein bezahltes Paket inkl. Entitlements, obwohl der Admin herabstufen wollte. Python sperrt den Billing-Fallthrough für jeden aktiven Manual-Override (auch raid_free).
- **Fix:** An build_plan_snapshot angleichen: jeder nicht abgelaufene Manual-Override (egal ob raid_free) anwenden und Billing-Zweig überspringen. Den pid!=raid_free-Sonderfall in plan.rs:188/195 entfernen, stattdessen auf Override-Existenz + nicht-abgelaufen prüfen.
- **Verify-Fix:** In resolve_plan_snapshot den raid_free-Override genauso als terminales Ergebnis behandeln wie jeden anderen aktiven Manual-Override. Konkret: den inneren Guard so ändern, dass bei nicht abgelaufenem Override JEDE plan_id (inkl. 'raid_free') mit source='manual_override' zurückgegeben wird, bevor die Billing-Query läuft. Z.195 von `if !expired && pid != "raid_free"` auf `if !expired` reduzieren und den dann redundanten äußeren Guard Z.188 entfernen, sodass ein aktiver, nicht abgelaufener Manual-Override (egal welcher Plan) immer den Billing-Fallthrough sperrt — exakt wie Pythons `elif`. Anschließend einen Regressionstest ergänzen, der manual_plan_id='raid_free' + aktives Stripe-Abo prüft und raid_free/manual_override erwartet.

### [chat-commands] !clip ist ein reiner Fehler-Stub — erstellt nie einen Clip
*class:* Toter Code / fehlendes Feature · *confidence:* 0.6 · *id:* chat-commands-7

- **Python** bot/chat/commands.py:284-408
  - cmd_clip holt Broadcaster-/Bot-Token, ruft api.create_clip(...) auf, baut die Clip-URL und antwortet '@user 🎬 Clip erstellt – "{titel}" (ca. letzte 60s): {clip_url}'. Nur im echten Fehlerfall kommt 'Clip konnte nicht erstellt werden...'.
- **Rust** rust/crates/tb-chat/src/commands.rs:730-770
  - cmd_clip bereitet nur den Titel auf (korrekt portiert) und sendet IMMER 'Clip konnte nicht erstellt werden. Bitte in 10 Sekunden nochmal versuchen.' — der eigentliche Clip-Pfad (ClipPort) ist nicht implementiert; handle() konsumiert den Command dennoch (true).
- **Divergenz:** Solange tb-chat die Chat-Pipeline für !clip beansprucht, ist der Command komplett funktionslos: jeder !clip-Aufruf scheitert mit Fehlermeldung statt einen Clip zu erzeugen. Bewusst als UNSICHER markiert, aber user-sichtbar kaputt, falls bereits live geschaltet.
- **Fix:** Entweder einen ClipPort-Trait einführen und create_clip nativ anbinden, oder !clip in handle() auf false setzen (Pipeline fährt fort / Python-Fallback übernimmt), bis der Port existiert — damit nicht fälschlich eine Fehlermeldung gesendet wird.
- **Verify-Fix:** Entweder (a) ClipPort/create_clip nativ portieren: ein Trait im ChatApi (z.B. create_clip(broadcaster_id, user_token, title, duration) -> Result<ClipInfo>) ergänzen, in tb-twitch-api den Helix-Endpoint POST helix/clips implementieren (Broadcaster-Token bevorzugt, Bot-Token-Fallback, OAuth-Hinweis bei fehlendem Token wie in Python Zeile 339-344) und in cmd_clip die Erfolgsmeldung "@user 🎬 Clip erstellt …: {clip_url}" zurückgeben. Oder (b) als Übergangslösung den !clip-Pfad in tb-chat NICHT konsumieren (false zurückgeben) und im Python-Worker selektiv den Clip-Command weiterlaufen lassen — aber Achtung: bei TAKEOVER=1 startet der Python-Chat-Bot gar nicht, daher ist (a) der saubere Weg. Bis zum Port mindestens die Fehlermeldung ehrlich machen ("Clip-Feature wird gerade migriert / vorübergehend nicht verfügbar"), damit User nicht wiederholt vergeblich "in 10 Sekunden nochmal" versuchen.

### [chat-moderation] Gelernte Spam-/Safe-Muster werden nur beim Start geladen, nie neu eingelesen — Self-Learning-Loop tot bis Neustart
*class:* Vergessene Seiteneffekte / Default-Drift (fehlender periodischer Reload + Cache-Invalidierung) · *confidence:* 0.92 · *id:* chat-moderation-1

- **Python** bot/chat/spam_ai_review.py:100-141 (load_learned_patterns/load_safe_patterns mit 120s-TTL) + 88-97/223/276 (_invalidate_pattern_cache/_invalidate_safe_cache nach _save_pattern/_save_safe_pattern); aufgerufen pro Nachricht in bot/chat/moderation.py:543-545 + 572-574
  - Python ruft load_learned_patterns()/load_safe_patterns() bei JEDER verdächtigen Nachricht auf; die Funktionen cachen mit 120s-TTL und der AI-Review invalidiert den Cache sofort nach dem Speichern eines neuen Musters. Ein vom AI-Review bestätigtes Spam-/Safe-Muster wird also binnen ≤120s aktiv und beeinflusst Score (Learned-Phrase +2 / Learned-Fragment +1 = hartes Signal) bzw. Negativ-Scoring (Safe(AI) −2).
- **Rust** rust/crates/tb-chat/src/spam_filter.rs:378-432 (LearnedPatterns::load) + 443-451 (SpamFilter hält unveränderliches Snapshot, kein Setter); rust/bin/tb-bot/src/chat_wiring.rs:157 + 203 (einmaliges Laden, Arc<SpamFilter>, kein Reload-Loop); Schreiber rust/crates/tb-chat/src/scam_pitch.rs:1699/1738
  - SpamFilter lädt die Muster genau einmal in chat_wiring.rs (LearnedPatterns::load) und hält sie in einem privaten, unveränderlichen Feld hinter Arc. Es gibt keinen periodischen Reload-Task und keinen Invalidierungs-Hook. Der Rust-AI-Reviewer schreibt neue Muster zwar in dieselben DB-Tabellen, aber der laufende Filter liest sie nie wieder — erst ein Prozess-Neustart übernimmt sie.
- **Divergenz:** Die zentrale auto-improving-Eigenschaft des Spam-Filters ist im Rust-Port faktisch deaktiviert: neu gelernte Spam-Marken/-Phrasen führen nicht zu Bans/Deletes und neu gelernte Safe-Muster korrigieren keine False-Positives, solange der Bot nicht neugestartet wird. Über Tage akkumulieren so divergente Ergebnisse (Python bannt/entschärft, Rust nicht). Der Doc-Kommentar in spam_filter.rs:20-21 behauptet sogar, der Cache könne 'jederzeit neu gebaut werden' — passiert aber nirgends.
- **Fix:** SpamFilter so umbauen, dass learned hinter ArcSwap/RwLock liegt, plus einen periodischen Reload-Task (z.B. tokio::time::interval ~120s ruft LearnedPatterns::load und tauscht das Snapshot) in chat_wiring.rs spawnen — analog zu spawn_periodic_loop der PromoEngine. Optional Invalidierung direkt nach dem AI-Review-Insert triggern.
- **Verify-Fix:** Periodischen Reload-Task analog zum Python-TTL-Verhalten ergänzen. Konkret: (1) SpamFilter.learned hinter ArcSwap<LearnedPatterns> (oder RwLock<Arc<LearnedPatterns>>) statt unveränderlichem Feld halten und einen `reload(&self, pool)`-Hook anbieten; evaluate() liest dann den aktuellen Snapshot. (2) In ChatRuntime::start_background (chat_wiring.rs:238) einen tokio-Loop spawnen, der alle ~120s LearnedPatterns::load(&pool) ausführt und den Snapshot atomar tauscht — entspricht der Python-120s-TTL. Optional zusätzlich Invalidierung direkt nach save_spam_pattern/save_safe_pattern (scam_pitch.rs) auslösen, um die Python-Cache-Invalidierung 1:1 zu treffen. (3) Den irreführenden Doc-Kommentar spam_filter.rs:20-21 korrigieren bzw. nach dem Fix korrekt machen. Mit Test absichern (z.B. Erweiterung von learned_patterns_werden_im_filter_genutzt): Pattern speichern → Reload triggern → evaluate erkennt es ohne Neukonstruktion des Filters.

### [chat-moderation] AlreadyBanned-Pfad schreibt zusätzlichen twitch_ban_events-Eintrag und sendet Chat-Notice (Python unterdrückt beides)
*class:* Fehlende Guards/Bedingungen (Banned- und AlreadyBanned-Zweig zusammengelegt statt getrennt) · *confidence:* 0.85 · *id:* chat-moderation-2

- **Python** bot/chat/moderation.py:1734-1801 (Erfolg 200/201/202: _record_autoban_db_event + Chat-Notice) vs. 1812-1843 (400 'already banned': KEIN _record_autoban_db_event, KEINE Chat-Notice)
  - Bei tatsächlichem Erfolg (HTTP 200/201/202) schreibt Python einen 'ban'-Eintrag in twitch_ban_events (öffentlicher recent-bans-Feed) und sendet die Chat-Notice. Bei HTTP 400 'already banned' setzt Python nur _last_autoban + Review-Log + Mod-Alert (reason='already_banned'), schreibt aber bewusst KEIN twitch_ban_events und sendet KEINE Chat-Notice.
- **Rust** rust/crates/tb-chat/src/moderation.rs:453-471 (BanOutcome::Banned | BanOutcome::AlreadyBanned teilen denselben Zweig → record_ban_event_db Z.460 + if !silent send_message Z.465-469 feuern für BEIDE)
  - Rust matcht Banned und AlreadyBanned im selben Arm und ruft für beide record_ban_event_db (INSERT in twitch_ban_events) und — falls nicht silent — die Chat-Notice send_message auf. Für einen bereits gebannten Chatter entsteht so pro Re-Detection ein neuer öffentlicher Ban-Feed-Eintrag und eine erneute '🛡️ Auto-Mod: … gebannt'-Chatnachricht.
- **Divergenz:** Re-Detektionen bereits gebannter Spammer blähen die öffentliche recent-bans-Statistik mit doppelten/spurious 'ban'-Events auf (twitch_ban_events hat hier keine Idempotenz/ON CONFLICT) und können wiederholte Auto-Mod-Notices im Chat auslösen — beides ist im Python-Original ausdrücklich unterdrückt.
- **Fix:** In moderation.rs den AlreadyBanned-Fall vom Banned-Fall trennen: für AlreadyBanned weiterhin persist_autoban_record (in-memory) ausführen, aber record_ban_event_db UND die Chat-Notice überspringen (entspricht Python-400-Zweig). Nur BanOutcome::Banned schreibt twitch_ban_events und sendet die Notice.

### [chat-moderation] UTF-8-Panic-Risiko: Byte-Slice &content[..200] im sus_invite-Warn-Log
*class:* Arithmetik/Grenzwerte (Byte- statt Zeichen-Index auf UTF-8-String) · *confidence:* 0.8 · *id:* chat-moderation-3

- **Python** bot/chat/moderation.py:876-881 (log.warning(..., content[:200]) — zeichenbasiertes Slicing, panic-frei)
  - content[:200] schneidet in Python nach 200 ZEICHEN ab und kann nie crashen, auch bei Multibyte-Zeichen.
- **Rust** rust/crates/tb-chat/src/sus_invite.rs:122-127 (&content[..content.len().min(200)])
  - &content[..content.len().min(200)] indiziert nach BYTES. Liegt bei Inhalten > 200 Bytes die Byte-Position 200 mitten in einem Multibyte-UTF-8-Codepoint (z.B. Emoji/Umlaut), paniced das Slicing ('byte index is not a char boundary') und reißt den Nachrichten-Handler-Task ab — der sus_invite-Hit (Review-Log + Discord-Alert) geht verloren.
- **Divergenz:** Eine Discord-Invite-Nachricht mit >200 Bytes und ungünstig platziertem Multibyte-Zeichen lässt den Verdachts-Pfad panischen statt zu loggen; Python verarbeitet sie problemlos.
- **Fix:** Char-sicher kürzen, z.B. content.chars().take(200).collect::<String>() oder eine truncate-on-char-boundary-Hilfsfunktion verwenden (wie an anderen Stellen content.chars().take(n)).
- **Verify-Fix:** Zeichenbasiert kürzen statt Byte-Slice. Z.B. `content.chars().take(200).collect::<String>()` in das warn!-Makro geben (paritätisch zu Python `content[:200]`, das ebenfalls nach Code-Points kappt). Alternativ char-boundary-sicher mit `let end = content.char_indices().map(|(i,_)| i).nth(200).unwrap_or(content.len()); &content[..end]`. Erste Variante ist klarer und vermeidet jede Boundary-Arithmetik. Optional zusätzlich die Pipeline.handle-Dispatch defensiv gegen Panics absichern (catch_unwind/JoinHandle-Logging), damit eine künftige Panic nicht stillschweigend Moderations-Events verschluckt.

### [chat-promos] Targeted-Promo-Presets im Rust-Port haben andere Texte/IDs/Tags als promo_presets.py
*class:* Default-Drift (Inhaltsdaten) · *confidence:* 0.95 · *id:* chat-promos-1

- **Python** bot/chat/promo_presets.py:25-92
  - 10 Presets: g_competitive/g_community/g_new_to_deadlock/g_meta/g_chill + u_welcome/u_mates/u_lurker_viewer/u_ranked_grind/u_new_player mit konkreten deutschen Texten inkl. Emojis und reichen Tag-Tupeln fuer die MiniMax-Auswahl.
- **Rust** rust/crates/tb-chat/src/promos.rs:270-339
  - global_presets()/user_presets() liefern andere IDs (global_community, global_competitive, ... / user_duo, user_ranked, ...) und voellig andere Texte ohne Emojis sowie andere Tags.
- **Divergenz:** Targeted-Promo laeuft im aktiven Pfad (_PROMO_ACTIVITY_ENABLED=True). Viewer sehen andere Werbetexte als die Quelle definiert; MiniMax-Auswahl operiert ueber andere Tags. Per grep verifiziert: korrekte Texte existieren in keiner anderen Rust-Datei.
- **Fix:** global_presets()/user_presets() 1:1 aus promo_presets.py uebernehmen (IDs, Texte inkl. Emojis, Tags); PRESET_MAP-Lookup fuer MiniMax-Matching beibehalten.
- **Verify-Fix:** global_presets()/user_presets() in rust/crates/tb-chat/src/promos.rs:270-339 1:1 an bot/chat/promo_presets.py angleichen: dieselben 10 stabilen IDs (g_competitive/g_community/g_new_to_deadlock/g_meta/g_chill, u_welcome/u_mates/u_lurker_viewer/u_ranked_grind/u_new_player), die identischen deutschen Texte inkl. Emojis und die vollständigen Tag-Tupel übernehmen. Da der Rust-PromoPreset.tags ein &'static str ist, entweder auf &'static [&'static str] (Tupel/Slice) umstellen, damit die reichen Tags 1:1 erhalten bleiben und die MiniMax-Preset-Auswahl dieselben Schlagwörter sieht, oder die Tag-Liste mindestens als kommaseparierten String mit denselben Werten abbilden. Anschließend einen Test ergänzen, der ID-Set, Texte und Tags zwischen Python-Quelle und Rust-Port gegenprüft (Default-Drift-Regression), und tb-chat neu bauen.

### [chat-promos] Lurker-Tax: Per-Session-Mention-Dedup fehlt, gleiche Lurker werden wiederholt gepingt
*class:* Vergessener Seiteneffekt / Anti-Repeat · *confidence:* 0.9 · *id:* chat-promos-2

- **Python** bot/chat/promos.py:564-584,1435
  - _select_lurker_tax_mentions ueberspringt Chatter, die schon in _lurker_tax_mentions_for_session(session_id) stehen; nach dem Senden werden die Logins ergaenzt. Jeder Lurker max 1x pro Session erwaehnt.
- **Rust** rust/crates/tb-chat/src/promos.rs:1328-1340
  - get_lurker_tax_candidates liefert jedes Mal die Top-Lurker (ORDER BY ... LIMIT 2); kein Per-Session-Mention-Tracking und kein update nach Senden. Dieselben Lurker werden bei jeder faelligen Gelegenheit (90 min) erneut gepingt.
- **Divergenz:** Derselbe ruhige Zuschauer bekommt mehrfach pro Session eine @name-Lurker-Steuer-Erwaehnung statt einmal, widerspricht der Python-Semantik und wirkt wie Spam.
- **Fix:** HashSet pro (channel, session_id) einfuehren, Kandidaten filtern, nach Senden erwaehnte Logins ergaenzen.
- **Verify-Fix:** Per-Session-Mention-Dedup in Rust nachbauen, analog zu Pythons `_lurker_tax_mentions`-Set: (1) Pro `active_session_id` ein `HashSet<String>` bereits erwähnter Logins im ChannelState (oder einem session-keyed Store) halten. (2) In `get_lurker_tax_candidates` mehr als LURKER_TAX_MAX_MENTIONS Zeilen holen bzw. nach dem Fetch alle Logins aus dem Session-Set herausfiltern und erst dann auf 2 kappen (so rücken die nächstrangigen Lurker nach). (3) Nach erfolgreichem `send_message` (nur bei Erfolg, vgl. Python `if ok`) die tatsächlich gesendeten Logins ins Session-Set `update`-en. Session-Wechsel/Reset muss das Set räumen. Damit wird jeder Lurker wie in Python max. einmal pro Session gepingt.

### [chat-promos] Lurker-Tax: Bot-Token-Scope-Fallback fehlt, Feature feuert nie ohne streamer-eigenen Scope
*class:* Fehlende Guard (OR-Zweig) · *confidence:* 0.85 · *id:* chat-promos-3

- **Python** bot/chat/promos.py:327-349
  - has_moderator_read_chatters True wenn Scope im streamer-eigenen twitch_raid_auth.scopes ODER im zentralen Bot-Token-Manager (bot_scopes). Kommentar: bot-zentriert ueber zentralen Bot-Token ermoeglicht.
- **Rust** rust/crates/tb-chat/src/promos.rs:1308-1326
  - Nur twitch_raid_auth.scopes des Streamers wird geprueft; zentraler Bot-Token-Scope-Fallback fehlt. PromoEngine hat keinen Zugriff auf TokenManager (token.rs::scopes() in promos.rs ungenutzt, per grep bestaetigt).
- **Divergenz:** Streamer, die auf den zentralen Bot-Token angewiesen sind (intendierter Migrationspfad), bekommen NIE eine Lurker-Tax-Erinnerung; das Feature ist fuer diese dauerhaft tot.
- **Fix:** TokenManager-Scopes in PromoEngine injizieren und OR-Zweig nachbilden, sobald TokenManager validiert hat.

### [chat-promos] Lurker-Tax-SQL: negative Lurk-Minuten durch fehlendes CASE-Guarding + Identity-Key-Drift
*class:* SQL-Drift · *confidence:* 0.65 · *id:* chat-promos-7

- **Python** bot/chat/promos.py:467-524
  - SUM(CASE WHEN NULL->0; WHEN last_seen_at<=first_message_at->0; ELSE EXTRACT(EPOCH)/60). Gruppierung/Join ueber chatter_identity_key (id:||id sonst login:||lower(login)).
- **Rust** rust/crates/tb-chat/src/promos.rs:1346-1374
  - SUM(EXTRACT(EPOCH FROM (last_seen_at - first_message_at))/60.0) ohne CASE-Guard -> negative Beitraege moeglich wenn last_seen_at < first_message_at. Gruppierung/Join nur ueber sc.chatter_login.
- **Divergenz:** Negative Differenzen druecken die Summe und koennen das Gate >=240 faelschlich reissen (Lurker faellt raus). Login-only-Gruppierung weicht von id-priorisierter Identitaet ab.
- **Fix:** CASE-Guard uebernehmen und Gruppierung/Join auf chatter_identity_key (id:/login:) umstellen.
- **Verify-Fix:** In rust/crates/tb-chat/src/promos.rs:1346-1374 die SUM-Aggregation an Python angleichen: `COALESCE(SUM(CASE WHEN sc.first_message_at IS NULL OR sc.last_seen_at IS NULL THEN 0 WHEN sc.last_seen_at <= sc.first_message_at THEN 0 ELSE EXTRACT(EPOCH FROM (sc.last_seen_at - sc.first_message_at)) / 60.0 END), 0) AS estimated_lurk_minutes`. Zusätzlich die Gruppierung/Join auf den id-priorisierten Identity-Key umstellen (CASE WHEN TRIM(COALESCE(chatter_id,'')) <> '' THEN 'id:'||TRIM(chatter_id) ELSE 'login:'||LOWER(chatter_login) END) in beiden CTEs und im JOIN, statt nur chatter_login. Test um eine Session mit NULL first_message_at und eine mit last_seen_at < first_message_at erweitern, um die 0-Behandlung abzusichern.

### [dash-audience] category-comparison: category_rank nutzt andere Formel als Python → falscher Rang bei Zwischenwerten
*class:* Arithmetik/Off-by-one (Bug-Klasse 6) · *confidence:* 0.9 · *id:* dash-audience-2

- **Python** bot/analytics/api_performance.py:994,1016 (avg_percentile=int(_percentile_of*100); category_rank=category_total-int(avg_percentile/100*category_total))
  - rank = category_total - int((avg_percentile/100) * category_total), wobei avg_percentile = int(((below+0.5*equal)/n)*100). Für sorted_avgs=[10,20,30,40,50], your_avg=25: below=2,equal=0 → pct=0.4 → avg_percentile=40 → rank = 5 - int(2.0) = 3.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/category_comparison.rs:253-256
  - let below_equal = partition_point(v <= your_avg); rank = category_total - below_equal + 1. Für dasselbe Beispiel: below_equal=2 → rank = 5 - 2 + 1 = 4.
- **Divergenz:** Die beiden Formeln stimmen nur überein, wenn your_avg exakt einem Listenwert entspricht; sobald der eigene Schnitt strikt zwischen zwei Peer-Werten liegt (der Normalfall), weichen sie ab (im Beispiel 3 vs 4). Der angezeigte Kategorie-Rang ist damit systematisch falsch. (Hinweis: real verstärkt durch Finding 1, da sorted_avgs ohnehin leer dekodiert.)
- **Fix:** Rust an die Python-Formel angleichen: avg_percentile (0..100) aus percentile_of berechnen, dann category_rank = category_total - ((avg_percentile as i64 * category_total as i64) / 100).
- **Verify-Fix:** Make the Rust rank formula reproduce the Python pipeline exactly rather than using a separate partition_point heuristic. Reuse the already-correct `percentile_of` helper (which mirrors `_percentile_of`) and apply the same two-stage integer truncation:

```rust
let avg_percentile = percentile_of(&sorted_avgs, your_avg); // already an i32 == Python int(_percentile_of*100)
let category_rank = if category_total > 0 {
    (category_total as i64) - ((avg_percentile as f64 / 100.0 * category_total as f64) as i64)
} else { 0 };
```

This matches Python's `category_total - int(avg_percentile/100*category_total)` including the truncation quirks, so the displayed rank is identical. Remove the misleading inline comment on line 252 ("Anzahl Streamer mit strikt höherem avg... + 1"), since the actual semantics are the truncated-percentile mapping, not a simple count. Add a parity unit test over the claim's example ([10,20,30,40,50], your_avg=25 → 3) plus the edge cases (exact match, top value, value above all, value below all) to lock the behavior.

### [dash-auth-legal] Admin-streamers list status badge wrong precedence
*class:* Guards · *confidence:* 0.92 · *id:* dash-auth-legal-3

- **Python** admin_streamer_queries.py:358-375
  - lifecycle (blocked/departnered/archived/token_error) before live/verified
- **Rust** admin_streamers.rs:194-200
  - is_live then verified then partner_status
- **Divergenz:** blocked/departnered streamer that is live shows live
- **Fix:** lifecycle first like Python

### [dash-auth-legal] Admin-streamers list notes shows promo_message not manual_plan_notes
*class:* SQL-Drift · *confidence:* 0.88 · *id:* dash-auth-legal-6

- **Python** admin_streamer_queries.py:411
  - notes = manual_plan_notes
- **Rust** admin_streamers.rs:230
  - notes = promo_message
- **Divergenz:** admin note hidden; promo shown
- **Fix:** notes = manual_plan_notes
- **Verify-Fix:** Im List-Handler rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:230 das Mapping auf `notes: r.manual_plan_notes,` ändern, passend zum Python-Original (manual_plan_notes als notes). Voraussetzung: das AdminStreamerRow-Struct/Query in tb-analytics liefert manual_plan_notes bereits (Feld vorhanden, Zeile 229 `pub manual_plan_notes`), also genügt der Feldtausch im Handler. Falls die Promo-Nachricht zusätzlich in der Liste gebraucht wird, ein separates promoMessage-Feld ergänzen statt notes umzudeuten. Nach dem Fix den irreführenden Kommentar `// list zeigt promo_message als notes` entfernen und mit dem Detail-Endpoint (der notes/manual_plan_notes korrekt trennt) abgleichen.

### [dash-perf] `[SQLX-DECODE]` Stored-Fallback chatters_at_plus*/new_chatters/known_from_raider (INTEGER) als i64 → nur Nullen
*class:* sqlx Typ-Mismatch · *confidence:* 0.88 · *id:* dash-perf-2

- **Python** bot/analytics/api_overview.py:2006-2010
  - Wenn recalculate_raid_chat_metrics keinen Treffer hat, fallen die gespeicherten Spalten als Integer in die Antwort.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/raid_analytics.rs:262-266
  - INTEGER-Spalten als Option<i64> gelesen → Decode-Fehler → unwrap_or(0); stored/mixed-Pfad liefert konstant 0 fuer chattersAt5m/15m/30m, newChatters, knownFromRaider.
- **Divergenz:** Spalten INTEGER, als i64 gelesen. Fuer Raids ohne target_session_id (recalc uebersprungen) ist der Fallback der einzige Datenpfad und damit wertlos.
- **Fix:** als Option<i32> lesen oder casten.
- **Verify-Fix:** Die fünf stored-Spalten als i32 statt i64 lesen oder serverseitig casten. Variante A (minimal): in raid_analytics.rs:262-266 `row.try_get::<Option<i32>, _>("chatters_at_plus5m").ok().flatten().map(i64::from).unwrap_or(0)` usw. — i32 ist der korrekte sqlx-Typ für Postgres INT4. Variante B (konsistent zum bestehenden Stil): die Spalten in der Basis-Query (Z.222-223) mit `CAST(chatters_at_plus5m AS BIGINT)` etc. selektieren, dann bleibt das i64-Read korrekt. Variante A ist chirurgischer. Zusätzlich sollte das `.ok()`-Swallowing hier durch ein `tracing::warn!` bei Decode-Fehler ergänzt werden, damit künftige Typ-Drifts nicht erneut stumm zu 0 werden. Nach dem Fix: ein Integrationstest gegen eine Postgres-Instanz mit einem Raid OHNE target_session_id, der prüft, dass chattersAt30m/newChatters/knownFromRaider die gespeicherten Werte (≠0) zurückgeben.

### [dash-perf] `[SQLX-DECODE]` rankings metric=growth: SUM(follower_delta) (BIGINT) als f64 gelesen → Werte immer 0
*class:* sqlx Typ-Mismatch · *confidence:* 0.85 · *id:* dash-perf-4

- **Python** bot/analytics/api_performance.py:814
  - value=float(SUM(...)) liefert echten Follower-Zuwachs; sortiert danach.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/rankings.rs:148-149
  - value wird unbedingt als f64 gelesen; bei growth ist value SUM(CASE follower_delta) = BIGINT, f64-Decode eines INT8 scheitert → unwrap_or(0.0); jeder Eintrag value 0.
- **Divergenz:** follower_delta INTEGER, SUM=BIGINT; f64 kann INT8 nicht dekodieren. viewers/retention=AVG(double) → f64 OK, nur growth betroffen.
- **Fix:** value bei growth als i64 lesen oder im SQL value::FLOAT8 casten.
- **Verify-Fix:** In rankings.rs die beiden growth-SQL-Varianten so casten, dass die value-Spalte mit dem f64-Read kompatibel ist, ODER den Read typabhängig machen. Einfachste, konsistente Lösung: in beiden growth-Branches `... ELSE 0 END)::FLOAT8 AS value` (oder ::DOUBLE PRECISION) anhängen, dann bleibt der vorhandene `try_get::<f64, _>` korrekt und alle drei Metriken teilen denselben f64-Read. Alternativ growth separat mit `try_get::<i64, _>("value")` lesen und in f64 konvertieren (analog overview.rs, das ::BIGINT + i64 nutzt). Erste Variante ist minimal-invasiv. Danach mit echten Daten verifizieren, dass das growth-Ranking nicht-null Werte liefert (z.B. Integrationstest, der eine Session mit follower_delge einfügt und value > 0 im JSON prüft) und dass die `.unwrap_or(0.0)`-Maskierung kein weiterer stiller Fehler bleibt.

### [dash-perf] `[SQLX-DECODE]` follower-funnel: total_duration (BIGINT-SUM) als f64 gelesen → avgTimeToFollow konstant 5
*class:* sqlx Typ-Mismatch · *confidence:* 0.82 · *id:* dash-perf-9

- **Python** bot/analytics/api_audience.py:612,687-690
  - total_duration=SUM(duration_seconds); avg_session_mins=total_duration/session_count/60; avg_time_to_follow=max(5,min(45,mins*0.4)) session-abhaengig.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/follower_funnel.rs:117,206-207
  - try_get::<f64>(total_duration) auf COALESCE(SUM(duration_seconds),0) (BIGINT) scheitert → unwrap_or(0.0); total_duration=0 → avg_session_mins=0 → avg_time_to_follow=5 immer.
- **Divergenz:** duration_seconds INTEGER, SUM=BIGINT; f64-Decode eines INT8 scheitert. avgTimeToFollow verliert jede Session-Abhaengigkeit.
- **Fix:** total_duration als i64 lesen und casten oder SUM(duration_seconds)::FLOAT8.
- **Verify-Fix:** In rust/crates/tb-dashboard-api/src/handlers/follower_funnel.rs den SUM-Ausdruck float-kompatibel machen statt als f64 zu decodieren. Zwei gleichwertige Optionen: (a) Query Z.76 casten: `COALESCE(SUM(s.duration_seconds), 0)::float8 AS total_duration` — dann passt try_get::<f64> Z.117. Oder (b) als Integer lesen und konvertieren: Z.117 `let total_duration: f64 = stats.try_get::<i64, _>("total_duration").unwrap_or(0) as f64;` (SUM(INT4) ist INT8 → i64). Variante (b) ist konsistent mit der bestehenden BIGINT+i64-Idiom in streamer_analytics_native.rs. Danach verifizieren, dass avgTimeToFollow für eine reale Session-Reihe wieder >5 liefert (z.B. Sessions ~2h → erwartet 45 nach Cap), z.B. per kurzem Integrationstest gegen eine Test-Session mit duration_seconds=7200.

### [dash-perf] `[SQLX-DECODE]` monthly-stats: MAX(peak_viewers) (INTEGER) als i64 gelesen → peakViewers immer 0
*class:* sqlx Typ-Mismatch · *confidence:* 0.85 · *id:* dash-perf-10

- **Python** bot/analytics/api_performance.py:91
  - peakViewers=MAX(peak_viewers) als Integer.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/performance.rs:91,127
  - try_get::<i64>(peak_viewers) auf MAX(INTEGER) (in Postgres bleibt MAX vom INT4 ein INT4) → Decode-Fehler → unwrap_or(0); peakViewers konstant 0.
- **Divergenz:** MAX(integer)=INT4. weekly/hourly lesen peak als AVG(double) korrekt; nur monthly als i64. title-performance liest MAX korrekt als i32.
- **Fix:** als i32 lesen oder MAX(s.peak_viewers)::BIGINT casten.
- **Verify-Fix:** In rust/crates/tb-dashboard-api/src/handlers/performance.rs entweder (a) SQL Zeile 91 zu `MAX(s.peak_viewers)::BIGINT AS peak_viewers` casten (konsistent mit dem etablierten tb-analytics-Muster) und i64-Read beibehalten, ODER (b) den Read Zeile 127 auf `r.try_get::<i32,_>("peak_viewers").unwrap_or(0)` ändern (konsistent mit title_performance.rs:121 und session_detail.rs:207). Variante (a) bevorzugen, da der JSON-Wert ohnehin als Zahl serialisiert wird und ein expliziter ::BIGINT-Cast die Absicht dokumentiert und Future-Overflow ausschließt. Zusätzlich prüfen: category_comparison.rs:107 liest dasselbe uncastete `MAX(peak_viewers)` als Option<f64> mit stillem .ok().flatten() — wahrscheinlich derselbe Decode-Fehlschlag, separat verifizieren.

### [dash-viewers] viewer-detail: personality-Block fehlt komplett
*class:* Fehlende Guards/Bedingungen · *confidence:* 0.9 · *id:* dash-viewers-2

- **Python** api_viewers.py:545-573,615-616
  - Klassifiziert bis 2000 Nachrichten, setzt payload['personality'] bei Nachrichten.
- **Rust** viewers.rs:763-790
  - Berechnet/liefert personality nie.
- **Divergenz:** Frontend-Panel (Viewers.tsx:376) verschwindet fuer alle Detail-Ansichten.
- **Fix:** Message-Query+Klassifikation portieren.
- **Verify-Fix:** In viewer_detail_handler (viewers.rs) den Personality-Block portieren: vor dem finalen json! eine Abfrage `SELECT m.content FROM twitch_chat_messages m JOIN twitch_stream_sessions s ON s.id = m.session_id WHERE LOWER(s.streamer_login)=$1 AND LOWER(m.chatter_login)=$2 AND m.message_ts>=$3 AND <known-bot-NOT-IN-Klausel> LIMIT 2000` ausfuehren, die Rust-Entsprechung von _classify_message pro Zeile anwenden, type_counts aufsummieren, primary = argmax bestimmen und nur bei vorhandenen Zeilen `"personality": {"primary": ..., "distribution": ...}` in den Payload aufnehmen (analog zu Pythons `if personality`). Die known-chat-bot-Ausschlussklausel (build_known_chat_bot_not_in_clause) muss aequivalent abgebildet werden, damit die Verteilung identisch ist. Danach einen Live-Diff Python- vs. Rust-Antwort auf demselben Viewer fahren, um Paritaet der distribution zu bestaetigen.

### [dash-viewers] viewer-detail: peakHours/mostActiveDay Scheinwerte ohne Roh-Chat
*class:* Toter Code · *confidence:* 0.85 · *id:* dash-viewers-4

- **Python** api_viewers.py:541-543
  - peak_hours nur Stunden mit Nachrichten; day=N/A wenn leer.
- **Rust** viewers.rs:736-744
  - peak_hours immer Top-3 (Nullen->[0,1,2]); day ueber [0;7] -> Sonntag, N/A nie.
- **Divergenz:** Ohne Roh-Chat: Rust [0,1,2]+Sonntag, Python []+N/A.
- **Fix:** peak_hours nur count>0; day N/A wenn alle 0.
- **Verify-Fix:** In viewers.rs vor dem Berechnen von peak_hours/most_active_day pruefen, ob ueberhaupt Roh-Chat vorliegt, und sonst Python-Verhalten spiegeln: `let has_chat = hour_counts.iter().any(|&c| c > 0);` Dann `peak_hours` nur aus Stunden mit count>0 bilden (z.B. `(0..24).filter(|&h| hour_counts[h as usize] > 0).collect()` vor dem Sort), sodass bei leerem Chat `[]` herauskommt. Fuer most_active_day: `if dow_counts.iter().all(|&c| c == 0) { "N/A" } else { ... }`. Zusaetzlich Tie-Break angleichen, falls 1:1-Paritaet gewuenscht: Python `max` liefert bei Gleichstand das ERSTE Maximum, Rust `max_by_key` das LETZTE -- fuer deterministische Gleichheit `dow_counts.iter().enumerate().rev().max_by_key(...)` oder explizit den kleinsten Index waehlen. Mit Test fuer den Leer-Chat-Fall (Lurker) absichern.

### [iapi-rest] verify promotet keine Nicht-Partner-Streamer (kein promote_streamer_to_partner) und fehlender Stats-Backfill
*class:* Vergessene Seiteneffekte / Fehlende Guards · *confidence:* 0.83 · *id:* iapi-rest-1

- **Python** bot/dashboard/streamer_admin_mixin.py:291-352 (_dashboard_verify_storage_step: promote_streamer_to_partner + backfill_tracked_stats_from_category)
  - Bei mode=permanent/temp liest Python twitch_user_id aus twitch_streamers bzw. raid_auth und ruft promote_streamer_to_partner() — legt also einen Streamer, der noch KEIN aktiver Partner ist, als Partner an und verifiziert ihn. Anschließend backfill_tracked_stats_from_category() (kopiert historische Datenpunkte). 'ist nicht gespeichert' kommt nur, wenn KEINE twitch_user_id auffindbar ist.
- **Rust** rust/crates/tb-analytics/src/streamers_crud.rs:300-343 (verify_streamer); rust/crates/tb-internal-api/src/handlers/streamers.rs:385-401
  - verify_streamer macht ausschließlich ein UPDATE auf twitch_partners WHERE status='active'. Für einen in twitch_streamers existierenden, aber noch nicht aktiv-partnerten Streamer trifft das UPDATE 0 Zeilen → Rückgabe NotAPartner → Handler antwortet 200 mit 'ist nicht gespeichert'. Kein Promote, kein Insert, kein Stats-Backfill.
- **Divergenz:** Das Verifizieren eines neu hinzugefügten/nur in twitch_streamers stehenden Streamers schlägt im Rust-Port still fehl (Partner wird nicht angelegt, keine Verifizierung, keine Stats-Übernahme), während Python ihn erfolgreich zum verifizierten Partner promotet. Admin-Aktion liefert irreführend 'ist nicht gespeichert' statt zu verifizieren.
- **Fix:** verify_streamer um den Promote-Pfad erweitern: wenn kein aktiver Partner getroffen wird, twitch_user_id aus twitch_streamers/twitch_raid_auth auflösen und promote_streamer_to_partner-Äquivalent (Insert in twitch_partners mit verification_payload) ausführen; backfill_tracked_stats_from_category nachziehen und die kopierte Anzahl in die Message aufnehmen. Andernfalls Route bewusst proxien statt nativ zu verdrahten.
- **Verify-Fix:** In tb-analytics verify_streamer für mode=permanent/temp die promote-Semantik nachbauen statt nur aktive Partner zu updaten: (1) twitch_user_id ermitteln — zuerst aus twitch_streamers, sonst aus dem aktiven Partner/raid_auth (analog Python source_row/partner_row); ohne id → NotAPartner/"ist nicht gespeichert" (Python-Parität). (2) Mit vorhandener id einen UPSERT auf twitch_partners ausführen: wenn aktive Partnerzeile existiert → UPDATE (bestehende Settings via COALESCE bewahren, wie es promote_streamer_to_partner tut), sonst INSERT INTO twitch_partners (twitch_user_id, twitch_login, status='active', manual_verified_*) — gegen Race idealerweise als ON CONFLICT-Upsert. (3) Identity-Upsert (upsert_streamer_identity-Äquivalent) und backfill_tracked_stats_from_category() portieren bzw. aufrufen, damit die historischen Datenpunkte übernommen werden. Alles in EINER Transaktion. Danach den stale Doc-Kommentar in streamers_crud.rs:297-299 (Legacy-Proxy-Behauptung) korrigieren und einen Live-Diff-Test gegen Python mit einem nur in twitch_streamers stehenden Login fahren. Alternativ kurzfristig: die verify-Route bewusst per Proxy an Python zurückgeben, falls der Lifecycle-Port noch nicht ansteht — dann aber Kommentar UND Routing (explizite Proxy-Weiterleitung statt nur Fallback) konsistent machen.

### [iapi-rest] discord-flag und discord-profile überspringen den Discord-Action-Scope-Guard
*class:* Fehlende Guards/Bedingungen · *confidence:* 0.7 · *id:* iapi-rest-2

- **Python** bot/internal_api/routes/streamers.py:274 (streamer_discord_flag) und :343 (streamer_discord_profile) rufen server._enforce_discord_action_scope(body); Impl bot/internal_api/app.py:817-845
  - Beide Handler erzwingen vor der Mutation _enforce_discord_action_scope(body): wenn TWITCH_INTERNAL_API_ALLOWED_{GUILD,CHANNEL,ROLE}_IDS gesetzt ist, muss der jeweilige Body-Wert (oder None ∉ Allowlist) passen, sonst PermissionError → 403 'action outside configured scope'.
- **Rust** rust/crates/tb-internal-api/src/handlers/streamers.rs:463-496 (discord_flag_handler) und :503-568 (discord_profile_handler) — kein enforce_scope-Aufruf
  - Die Rust-Handler validieren nur Login/Felder und schreiben direkt; es gibt keinen Scope-Guard. Bei gesetzter Role-/Guild-/Channel-Allowlist würden sie schreiben, wo Python mit 403 abbräche.
- **Divergenz:** Mit konfigurierter Allowlist (z.B. TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS) erlaubt der Rust-Port Discord-Flag/Profil-Mutationen, die Python als außerhalb des erlaubten Scopes mit 403 ablehnt. link-click implementiert diesen Guard in Rust korrekt — die beiden Streamer-Mutationen nicht.
- **Fix:** In discord_flag_handler/discord_profile_handler dieselbe Scope-Prüfung wie in telemetry_routes::process_link_click anwenden (parse_allowlist_ids + enforce_scope_allowlist über guild_id/channel_id/role_id aus dem Body), Fehler → 403.
- **Verify-Fix:** In discord_flag_handler und discord_profile_handler vor dem DB-Write die bestehende Rust-Scope-Prüfung aufrufen, analog zu telemetry_routes.rs/raid_oauth.rs. Konkret: `parse_allowlist_ids(ENV_ALLOWED_{GUILD,CHANNEL,ROLE}_IDS)` einlesen und `enforce_scope_allowlist(guild_id_opt, &allowed_guilds, "guild_id")` etc. ausführen, wobei für Felder, die der Body nicht trägt (alle drei in diesen beiden Handlern), `None` übergeben wird — damit schlägt die Prüfung bei gesetzter Allowlist wie in Python via None ∉ Allowlist → 403 'action outside configured scope' durch. Am saubersten die in telemetry_routes.rs liegenden Helfer (parse_allowlist_ids/enforce_scope_allowlist) in ein gemeinsames Modul (z.B. tb-internal-api/src/scope.rs) ziehen und in beiden Streamer-Handlern wie beim link-click-Pfad anwenden. Anschließend Parität-Tests ergänzen (gesetzte ROLE-Allowlist → 403 für discord-flag/profile), analog zu link_click_guild_id_ausserhalb_allowlist_403.

### [iapi-stats] /stats lässt vier Top-Level-Sektionen weg (retention, chat, discovery, content_performance)
*class:* Vergessene Seiteneffekte / fehlende Response-Felder · *confidence:* 0.88 · *id:* iapi-stats-1

- **Python** bot/community/leaderboard.py:1253-1256 (out["retention"]/["chat"]/["discovery"]/["content_performance"]), gesetzt im else-Block ab :1084; Verdrahtung runtime_bootstrap.py:236 (stats=_dashboard_stats → _compute_stats)
  - _compute_stats führt nach den top/hourly/weekday-Aggregaten einen zweiten DB-Block aus (Sessions LIMIT 400, Chat-Peaks, Rollup-Counts) und hängt vier zusätzliche Top-Level-Sektionen an: retention (sessions, ret5/10/20, avg_drop, examples[Top5 dropoff]), chat (unique_per_100, first/returning_share, peaks, total_unique), discovery (unique_viewers_estimate, followers_total_delta, followers_per_session/hour, returning_7d/30d), content_performance (Top20 nach peak_viewers mit engagement_ratio). Diese Felder sind Teil des /stats-Response.
- **Rust** rust/crates/tb-internal-api/src/handlers/stats_native.rs:1240-1285 (compute_stats baut nur tracked/category/avg_*/streamer?/monetization/eventsub); Test :1876-1878 verbietet diese Keys sogar
  - compute_stats erzeugt diese vier Sektionen überhaupt nicht — kein Session-Aggregat, keine Chat-Peaks, keine Rollup-Returning-Counts, keine content_performance. Der Test stats_top_level_keys_exakt_wie_python (:1876) behauptet faktisch falsch, Python liefere diese Keys nicht, und asserted ihre Abwesenheit.
- **Divergenz:** Der /stats-Response des nativen Handlers ist strukturell unvollständig: vier ganze Datenblöcke mit Retention-/Chat-/Discovery-/Content-Performance-Analytik fehlen, die in Python vorhanden sind. Jeder Konsument, der out.retention/out.chat/out.discovery/out.content_performance liest, bekommt nun nichts.
- **Fix:** Den zweiten Aggregations-Block aus leaderboard.py:1024-1256 portieren (Sessions, Chat-Peaks, 7d/30d-Rollup-Counts) und die vier Sektionen an das compute_stats-Ergebnis anhängen; den irreführenden Test korrigieren (Python liefert die Keys, also nicht verbieten).
- **Verify-Fix:** compute_stats in stats_native.rs um den zweiten DB-Block erweitern, der die Python-Logik spiegelt: (1) session_rows aus twitch_stream_sessions (started_at >= NOW()-30d, ended_at NOT NULL, ORDER BY started_at DESC, LIMIT 400) mit denselben Spalten (retention_5m/10m/20m, dropoff_pct/label, unique_chatters, first_time/returning_chatters, follower_delta, followers_start/end, stream_title, notification_text, peak/avg/start_viewers, duration_seconds); (2) chat_peak_rows (Top5 Minute-Buckets aus twitch_chat_messages JOIN twitch_stream_sessions); (3) vier Rollup-Counts aus twitch_chatter_rollup (active/returning 7d/30d). Daraus die vier Sektionen retention/chat/discovery/content_performance exakt wie in leaderboard.py:1135-1256 berechnen (avg-Helper, Top5-dropoff-examples nach dropoff_pct sortiert, content_performance Top20 nach peak_viewers mit engagement_ratio = peak/followers_start*100) und als Top-Level-Keys in out einfügen. Den ganzen Block exception-safe halten (wie Python try/else: bei DB-Fehler einfach weglassen statt fehlschlagen). Den Test stats_top_level_keys_exakt_wie_python (:1876-1878) umdrehen — die vier Keys von 'verboten' auf 'bei vorhandenen Session-Daten erwartet' ändern und den falschen Kommentar 'Python liefert diese nicht' entfernen. Vorab kurz mit Konsumenten abgleichen, ob die /stats-Sektionen tatsächlich noch gebraucht werden; falls bewusst entfernt, stattdessen 05-cleanup-decisions.md dokumentieren und den irreführenden Test-Kommentar korrigieren.

### [iapi-stats] Ad-Viewer-Drop (avg_viewer_drop_pct + worst_ads) mit invertiertem Vorzeichen und falscher Fenster-/Sortierlogik
*class:* Arithmetik / SQL-Default-Drift · *confidence:* 0.92 · *id:* iapi-stats-2

- **Python** bot/dashboard/dashboard_metrics_mixin.py:124-147
  - pre_vals = Mittelwert der Viewer im 5-Minuten-Fenster VOR Ad-Start ([minutes_into-5, minutes_into)); post_vals = Mittelwert im 5-Minuten-Fenster NACH Ad-Ende ([minutes_into+duration_min, +5)); drop_pct = (post_avg - pre_avg)/pre_avg*100 → SIGNIERT, negativ bei Viewer-Verlust; round(.,1); worst_ads.sort(key=drop_pct) AUFSTEIGEND (stärkster Drop = negativster Wert zuerst), dann [:5]; avg_viewer_drop_pct = round(mean(drop_pcts),1) (signiert).
- **Rust** rust/crates/tb-internal-api/src/handlers/stats_native.rs:855-882
  - before = EINZELwert am größten Minute < ad_minute (letzter Sample vor Ad, kein 5-Min-Mittel); after = MINIMUM über [ad_minute, ad_minute+dur_mins] (WÄHREND der Ad, nicht danach, kein Mittel); drop = (bv - av)/bv*100 → INVERTIERTES Vorzeichen (positiv = Drop); sort DESCENDING nach drop (b.2.partial_cmp(a.2)); avg_viewer_drop_pct positiv.
- **Divergenz:** Resultierende Zahlen für avg_viewer_drop_pct und worst_ads[].drop_pct unterscheiden sich in Vorzeichen UND Betrag: Python nutzt 5-Min-Mittel vor/nach der Ad und liefert negative Drops; Rust nutzt Einzel-Punkt vor / Minimum während der Ad und liefert positive Werte. Selbst die Auswahl der 'schlimmsten' Ads kann andere Zeilen treffen, weil Fensterdefinition und Sortierrichtung anders sind.
- **Fix:** 5-Minuten-Mittelwerte vor Ad-Start und nach Ad-Ende (post_start = ad_minute + duration_min) bilden, drop_pct = (post_avg - pre_avg)/pre_avg*100 (signiert), aufsteigend sortieren, [:5]; round auf 1 Dezimale.
- **Verify-Fix:** Rust-Block (stats_native.rs:855-882) auf die Python-Semantik angleichen: (1) before als 5-Min-Mittel über [ad_minute-5, ad_minute) statt Einzel-max-Sample; (2) after als 5-Min-Mittel über das Fenster NACH Ad-Ende [ad_minute+dur_mins, ad_minute+dur_mins+5) statt Minimum WÄHREND der Ad; (3) Formel auf (after_avg - before_avg)/before_avg*100 umstellen, damit das Vorzeichen wie in Python signiert/negativ bei Viewer-Verlust ist; (4) Sortierung auf aufsteigend nach drop_pct (sort_by a.2.partial_cmp(&b.2)). Anschließend mit echten Session-Timeline-Daten gegen die Python-Ausgabe diffen (gleiche avg_viewer_drop_pct und worst_ads-Reihenfolge). Falls die Rust-Variante (Point-before / Min-during, positiv) bewusst als verbesserte Metrik gewollt ist, dann stattdessen in 05-cleanup-decisions.md als bewusste Abweichung dokumentieren und den Frontend-Konsumenten (Label/Vorzeichen-Erwartung) anpassen — aktuell ist es undokumentiert und damit ein stiller Port-Bug.

### [iapi-stats] Insights-Trend nutzt retention_5m statt retention_10m
*class:* SQL-/Feld-Drift (falsche Spalte) · *confidence:* 0.9 · *id:* iapi-stats-3

- **Python** bot/analytics/backend_extended.py:738,741 (sum(t["retention_10m"] ...) für recent/older)
  - Der Positiv-/Negativ-Trend-Insight vergleicht den Mittelwert der letzten 7 Tage gegen die älteren Tage auf Basis von retention_10m aus der Retention-Timeline.
- **Rust** rust/crates/tb-internal-api/src/handlers/streamer_analytics_native.rs:656,660 (.map(|e| e.retention_5m))
  - generate_insights berechnet recent/older-Mittel über e.retention_5m statt retention_10m.
- **Divergenz:** Die Trend-Entscheidung (recent > older*1.10 → 'Positiver Trend', < older*0.90 → 'Negativer Trend') wird auf einer anderen Retention-Kurve getroffen. Da 5m- und 10m-Retention pro Tag unterschiedlich verlaufen, kann der Insight bei identischen Daten anders oder gar nicht feuern.
- **Fix:** In generate_insights recent/older aus RetentionTimelineEntry.retention_10m statt retention_5m bilden.
- **Verify-Fix:** In rust/crates/tb-internal-api/src/handlers/streamer_analytics_native.rs die beiden `.map(|e| e.retention_5m)` in Zeile 656 und 660 auf `.map(|e| e.retention_10m)` ändern, damit die Trend-Entscheidung wie in Python auf der 10-Minuten-Retention basiert. Beide Felder liegen im RetentionTimelineEntry bereits vor (Z. 78), kein Query-/Struct-Eingriff nötig. Danach idealerweise einen Unit-Test ergänzen, der die Timeline mit divergierenden 5m/10m-Werten füllt und prüft, dass der Trend dem 10m-Verlauf folgt.

### [mon-announce] Tracking-Token zufaellig statt deterministisch, Cross-Restart-Idempotenz weg, Doppel-Postings moeglich
*class:* Vergessene Seiteneffekte / Idempotenz-Drift · *confidence:* 0.82 · *id:* mon-announce-1

- **Python** bot/monitoring/monitoring.py:327-338 und :382-469, aufgerufen :450 / :1522
  - Tracking-Token deterministisch: sha256(login|stream_id|started_at|title)[:16]. Derselbe Stream erzeugt prozessuebergreifend (auch nach Neustart) denselben Token. Der Cache-Buster der Bild-URL wird aus diesem Token abgeleitet, also ist die gesamte Embed-Payload byte-identisch ueber Versuche/Neustarts, der Broker dedupliziert ueber den aus der Payload abgeleiteten Idempotency-Key, kein Doppel-Post.
- **Rust** rust/crates/tb-monitoring/src/announce/sink.rs:153-156 (fresh_token), :172-180
  - fresh_token() = uuid::Uuid::new_v4()[..16], ein Zufallswert. Stabilitaet nur in der prozesslokalen retry-Map; bei erfolgreichem Send geloescht, bei Neustart verloren. Send-Timeout + Neustart vor dem Retry erzeugt neuen Token, neuen cb-Wert, andere Payload, anderen Idempotency-Key (relay.rs hasht die volle Payload). Kommentar sink.rs:154 verweist faelschlich auf secrets.token_hex(8), das ist nur der Mixin-Fallback, nicht der Produktionspfad.
- **Divergenz:** Python sichert Doppel-Postings doppelt: primaer DB-State-Guard (message_id_previous), sekundaer payload-stabile Idempotenz. Rust verliert die sekundaere Absicherung im Fenster Send-Timeout/Neustart/Retry, der Broker postet erneut. Zudem ist der DB-gespeicherte last_tracking_token nicht reproduzierbar an die Stream-Identitaet gebunden.
- **Fix:** Token deterministisch wie Python aus login|stream_id|started_at|title (sha256 hex[..16]) berechnen, aus request.stream_id/request.started_at_iso. Alternativ request.previous_tracking_token wiederverwenden.
- **Verify-Fix:** `fresh_token()` deterministisch aus der Stream-Identität ableiten, analog Python: `sha256(format!("{login}|{stream_id}|{started_at}|{title}"))[..16]`. Dazu in `announce_live` die bereits vorhandenen Felder von `AnnounceLiveRequest` nutzen (login, stream_id, started_at_iso, stream.title) statt `Uuid::new_v4()`. Die prozesslokale retry-Map kann als Mikro-Optimierung bleiben, ist dann aber nicht mehr für die Idempotenz nötig — der Token (und damit cb + view_spec + Idempotency-Key) ist über Neustarts hinweg stabil. Den irreführenden Kommentar in sink.rs:154 korrigieren. Ein Regressionstest sollte prüfen, dass zwei Aufrufe mit identischer Stream-Identität (kalte retry-Map) denselben Token und damit denselben `idempotency_key("send", payload)` erzeugen.

### [mon-announce] Sink ignoriert previous_tracking_token / stream_id / started_at_iso aus dem Request (toter Input)
*class:* Toter Code · *confidence:* 0.85 · *id:* mon-announce-2

- **Python** bot/monitoring/monitoring.py:450-455, :1452-1455, :1545-1546
  - Produktionspfad leitet den Token aus stream_id/started_at/title ab und nutzt zusaetzlich den DB-Vorgaenger-Token sowie den gecachten render_now fuer konsistente Payloads ueber Retries/Neustarts.
- **Rust** rust/crates/tb-monitoring/src/poller/hooks.rs:20-24; rust/crates/tb-monitoring/src/announce/sink.rs:172-213
  - Engine befuellt AnnounceLiveRequest mit previous_message_id, previous_tracking_token, stream_id, started_at_iso, active_session_id (engine.rs:482-488). announce_live nutzt nur stream/login/entry; previous_tracking_token, stream_id und started_at_iso werden nie gelesen, wirkungslos.
- **Divergenz:** Die Daten fuer die deterministische/stabile Token- und Render-Zeit-Wahl werden transportiert, aber verworfen. Direkte Ursache von mon-announce-1; Felder sind toter Input.
- **Fix:** Im Sink request.previous_tracking_token bevorzugen, sonst Token deterministisch aus request.stream_id/request.started_at_iso ableiten; render_now aus stabilem Wert statt Utc::now() pro Versuch.
- **Verify-Fix:** In BrokerAnnouncementSink::announce_live die Token-Wahl deterministisch aus dem Request ableiten, analog zur Python-Logik: (1) zuerst request.previous_tracking_token verwenden, falls vorhanden (DB-Carry-forward); sonst (2) einen aus request.login + request.stream_id + request.started_at_iso + Titel gebildeten Token (SHA256[:16], identisch zu _build_live_announcement_tracking_token) berechnen; nur als allerletzten Fallback fresh_token() behalten. Die In-Memory retry-HashMap bleibt als Render-Zeit-Cache nützlich, darf aber nicht die einzige Token-Stabilitätsquelle sein. Damit wird der Token (und der davon abgeleitete cache_buster für das Embed) über Retries UND Prozess-Neustarts hinweg stabil, was mon-announce-1 mitschließt. Den Quellkommentar (sink.rs:5-9) entsprechend aktualisieren.

### [mon-eventsub] [Toter Code/vergessener Seiteneffekt] channel.points-Redemption-Telemetrie wird still verworfen (Handler nie verdrahtet)
*class:*  · *confidence:* 0.9 · *id:* mon-eventsub-1

- **Python** bot/monitoring/eventsub_mixin.py:2477-2493,2520-2525 + Subscription 1635-1644
  - Python registriert Callbacks fuer channel.channel_points_automatic_reward_redemption.add und channel.channel_points_custom_reward_redemption.add und schreibt jedes Redemption ueber _store_channel_points_event in twitch_channel_points_events; die Subscriptions zeigen auf die geteilte Callback-URL.
- **Rust** rust/crates/tb-monitoring/src/dispatch.rs:271-349 (kein channel_points-Arm); telemetry.rs:167-203 (store_channel_points_event nie aufgerufen)
  - store_telemetry hat keinen match-Arm fuer die beiden channel_points-Typen, sie fallen in den other-Zweig (debug-Log, return false). store_channel_points_event existiert, wird aber nirgends aufgerufen.
- **Divergenz:** Da der native Receiver die geteilte Callback-URL bedient, kommen die von Python angelegten channel.points-Events am Rust-Dispatcher an und werden ohne Insert verworfen - dauerhafter Verlust der Redemption-Telemetrie.
- **Fix:** In store_telemetry zwei Arme fuer die channel_points-Sub-Typen ergaenzen, die store_channel_points_event(user_id, event, now) aufrufen.
- **Verify-Fix:** In store_telemetry (dispatch.rs, im match um Z.271-348) zwei Arme ergaenzen, die store_channel_points_event aufrufen: "channel.channel_points_automatic_reward_redemption.add" | "channel.channel_points_custom_reward_redemption.add" => self.telemetry.store_channel_points_event(user_id, event, now).await. Damit wird die bereits existierende, korrekt implementierte Insert-Funktion (telemetry.rs:167-203) tatsaechlich verdrahtet und der "other"-Drop entfaellt. Anschliessend einen dispatch-Test fuer beide Sub-Typen ergaenzen (Tabelle twitch_channel_points_events existiert bereits im Test-Support), der den Insert verifiziert.

### [mon-eventsub] [Vergessener Seiteneffekt] channel.ban: Bot-Selbst-Timeout-Erkennung (TimeoutGuard) fehlt im Routing
*class:*  · *confidence:* 0.8 · *id:* mon-eventsub-2

- **Python** bot/monitoring/eventsub_mixin.py:2305-2330
  - Beim channel.ban prueft Python, ob ein nicht-permanenter Timeout des eigenen Bot-Accounts vorliegt; falls ja record_timeout(channel_login), was die Stummschaltungs-/Pitch-Logik speist; danach store_ban_event.
- **Rust** rust/crates/tb-monitoring/src/dispatch.rs:315-319; record_timeout in tb-chat/src/moderation.rs:619 produktiv nie aufgerufen
  - Der channel.ban-Arm ruft nur store_ban_event; keine is_permanent/bot_id-Pruefung, kein produktiver record_timeout-Aufruf (nur Tests).
- **Divergenz:** Wird der Bot getimed-outet, registriert Rust das nicht im TimeoutGuard, die Moderations-Backoff-Logik greift nicht. Ausloesender Seiteneffekt sitzt im channel.ban-Routing und ist dort verloren.
- **Fix:** Im channel.ban-Pfad (oder via on_channel_ban-Hook) is_permanent==false und banned user_id == Bot-ID pruefen und TimeoutGuard.record_timeout(login) aufrufen.
- **Verify-Fix:** Im Rust-channel.ban-Arm (dispatch.rs:315) den Python-Seiteneffekt nachbauen: `is_permanent` aus dem Event lesen (Default true); bei non-permanent `user_id` mit der eigenen Bot-ID vergleichen; bei Treffer `record_timeout(broadcaster_login)` auf den TimeoutGuard rufen, dann store_ban_event. Voraussetzung: dispatch muss Zugriff auf Bot-ID und die TimeoutGuard-Instanz haben (ggf. über self/context durchreichen). Zusätzlich sollte der zweite, ebenfalls tote Pfad geschlossen werden: in den produktiven send_message-Callern (promos/commands/scam_pitch/fun_responses) das `SendOutcome::Dropped`-Ergebnis auswerten und bei `code ∈ BOT_TIMEOUT_DROP_CODES` `record_timeout(channel_login)` rufen — das ist der Mechanismus, den der Doc-Kommentar in chat.rs:117 bereits ankündigt, aber nirgends implementiert. Tests ergänzen, die einen Bot-Self-Timeout über beide Pfade bis zum Mute/Pitch durchspielen.

### [mon-poll] Scout entdeckt bei leerem Filter alle Sprachen — Python-Scout ist auf deutsch festverdrahtet
*class:* Fehlende Guards/Bedingungen · *confidence:* 0.7 · *id:* mon-poll-2

- **Python** bot/base.py:988-998 (_scout_deadlock_channels: get_streams_for_game(..., language="de", limit=100), ein einziger DE-Request)
  - Der Python-Scout holt live Deadlock-Streams mit hartkodiertem language="de" und registriert nur deutschsprachige Kanäle als is_monitored_only=1. Nicht-deutsche Deadlock-Streamer werden nie aufgenommen.
- **Rust** rust/crates/tb-monitoring/src/scout.rs:261-281 (language_list = leer→[None], Loop ruft get_streams_by_category mit lang=None) ; gespeist aus rust/bin/tb-bot/src/main.rs:607-614 (TWITCH_LANGUAGE_FILTERS, unbesetzt)
  - Der Rust-Scout iteriert über language_filters; ist die Liste leer (Produktionszustand, da TWITCH_LANGUAGE_FILTERS nicht gesetzt), wird genau ein Request mit lang=None (alle Sprachen) abgesetzt. Damit würde der Scout beliebigsprachige Deadlock-Streamer als monitoring-only eintragen.
- **Divergenz:** Sobald der Scout aktiv ist (TB_SCOUT_ENABLED=1) und kein Sprachfilter gesetzt ist, weicht die entdeckte Kanalmenge ab: Rust nimmt nicht-deutsche Streamer auf, Python nicht. Geringeres Gewicht, weil der Scout per Default deaktiviert ist und die Abweichung verschwindet, sobald TWITCH_LANGUAGE_FILTERS=de gesetzt wird.
- **Fix:** Wie bei Befund 1: leeren Filter im Scout nicht als 'alle Sprachen' interpretieren, sondern auf die deutschen Varianten defaulten — analog zum hartkodierten language="de" des Python-Scouts.
- **Verify-Fix:** Scout-Sprachverhalten bewusst angleichen statt es per leerem Env-Default driften zu lassen. Zwei saubere Optionen: (1) Wenn die Produktionsabsicht „nur deutsche Deadlock-Streamer entdecken" ist (wie Python), in rust/bin/tb-bot/src/main.rs:607 für die Scout-Task einen Default setzen: `.unwrap_or_else(|_| vec!["de".to_string()])` statt `.unwrap_or_default()` — dann iteriert scout.rs über `Some("de")` und repliziert das Python-Verhalten. (2) Wenn All-Languages-Discovery gewollt ist, ist das eine bewusste Verhaltensänderung — dann in rust/docs/05-cleanup-decisions.md als Entscheid dokumentieren und ggf. Python nachziehen, damit beide Quellen konsistent sind. Empfehlung: Option (1) (DE-Default) wählen, da das den Ist-Zustand der Python-Pipeline 1:1 erhält; den Env-Override `TWITCH_LANGUAGE_FILTERS` als bewusstes Opt-in für breitere Discovery belassen.

### [raid-arrival] correlation_status diverges: confirmed instead of matched_pending
*class:* SQL/Default drift · *confidence:* 0.9 · *id:* raid-arrival-2

- **Python** bot/raid/raid_arrival_runtime.py:302
  - store_partner_raid_arrival writes correlation_status=matched_pending into twitch_raid_arrival_tracking on the confirmed pending arrival.
- **Rust** rust/bin/tb-bot/src/raid_arrival_wiring.rs:318
  - The Rust adapter binds correlation_status=confirmed on the same path.
- **Divergenz:** The persisted correlation_status holds a permanently different literal than Python for every pending-confirmed raid. Filters/analytics keyed on matched_pending miss or yield permanently wrong counts. Only the confirm path drifts; the independent path correctly uses independent_channel_raid.
- **Fix:** Set correlation_status to matched_pending at raid_arrival_wiring.rs:318.
- **Verify-Fix:** In `rust/bin/tb-bot/src/raid_arrival_wiring.rs:318` `correlation_status: "confirmed".to_string()` auf `"matched_pending".to_string()` ändern, um Python-Parität herzustellen. Zusätzlich `correlation_detail: decision.suppression_reason.clone()` (Zeile 319) auf `None` setzen, da Python hier `correlation_detail=None` übergibt — sonst bleibt eine zweite, kleinere Drift bestehen. Danach einen Regressionstest ergänzen, der die persistierte `correlation_status`-Spalte für den confirm-pending-Partner-Pfad auf `matched_pending` festnagelt (analog zum bestehenden `independent_channel_raid`-Assert in test_chat_notification_raid_confirmation.py:576). Bestehende historische Zeilen mit `confirmed` ggf. per Migration auf `matched_pending` zurückmappen, falls Analytics-Historie konsistent sein soll.

### [raid-pipeline] Score-Snapshot beim Arrival wird zur Bestätigungszeit statt zur Raid-Sendezeit geladen (PendingRaid verlor target_stream_data/_partner_score)
*class:* Vergessene Seiteneffekte / Datenfeld-Drop · *confidence:* 0.78 · *id:* raid-pipeline-1

- **Python** bot/raid/raid_pipeline.py:342 (register_pending_raid(... target_stream_data=target ...)); bot/raid/raid_arrival_runtime.py:260,387-391 (score_snapshot=target_stream_data.get("_partner_score")); bot/raid/partner_raid_score_tracking.py:390-393 (if score_snapshot: use it; else load fresh)
  - Beim Raid-Start friert Python das vollständige target-Dict inkl. _partner_score (final_score, base_score, today_received_raids, Multiplikatoren, last_computed_at) als target_stream_data im PendingRaid ein. Bei der späteren Arrival-Bestätigung (Sekunden bis Minuten danach) ruft Python erst refresh_partner_score_cache_if_available (schreibt den DB-Score neu) und übergibt dann track_confirmed_partner_raid genau diesen eingefrorenen Raid-Zeit-Snapshot (score_snapshot wird bevorzugt, DB-Fallback nur wenn None). Getrackt wird also der Score, wie er zum Sendezeitpunkt war.
- **Rust** rust/crates/tb-raid/src/pending_raids.rs:59-85 (PendingRaid ohne target_stream_data/_partner_score-Feld); rust/crates/tb-raid/src/auto_raid_pipeline.rs:490-509 (register_pending speichert keinen Score-Snapshot); rust/bin/tb-bot/src/confirm_resolver.rs:94,113-122 (snapshot = score_store.load(...) frisch zur Confirm-Zeit); rust/bin/tb-bot/src/raid_arrival_wiring.rs:337-348 (resolve(&ctx, Utc::now()))
  - Rust hat target_stream_data/_partner_score komplett aus PendingRaid entfernt; register_pending speichert keinen Snapshot. Bei der Arrival-Bestätigung lädt confirm_resolver.resolve den Score-Snapshot frisch aus twitch_partner_raid_scores zum Confirm-Zeitpunkt (score_store.load) und schreibt diesen in das Tracking. Getrackt wird der Score zur Bestätigungszeit, nicht zur Sendezeit.
- **Divergenz:** Der in twitch_partner_raid_score_tracking persistierte Score-Snapshot (final_score/base_score/today_received_raids/Multiplikatoren/score_last_computed_at) weicht ab, sobald sich der Cache zwischen Senden und Bestätigung ändert. Das ist real: Python recomputed den Cache direkt vor dem Tracking (should_refresh_partner_score_cache), nutzt aber bewusst den vor-Refresh-Snapshot; Rust nimmt den frisch geladenen (ggf. post-refresh, anderer Tag → today_received_raids verschoben) Wert. Diese Tracking-Daten fließen zurück ins Scoring künftiger Raids, also dauerhaft falsche/abweichende Analytik.
- **Fix:** PendingRaid um ein optionales score_snapshot-Feld (z. B. den PartnerRaidScoreRow oder die relevanten Felder) erweitern, in register_pending aus dem gewählten ResolvedTarget/den geladenen scores befüllen (nur bei is_partner_raid), und confirm_resolver/ConfirmContext den Raid-Zeit-Snapshot bevorzugen lassen — DB-Load nur als Fallback, exakt wie Python (score_snapshot or _load_cached_score_snapshot).
- **Verify-Fix:** Sendezeit-Snapshot in Rust mitführen, statt zur Confirm-Zeit frisch zu laden: PendingRaid um ein optionales Feld erweitern (z. B. score_snapshot: Option<PartnerRaidScoreRow> oder die zum Senden genutzte _partner_score-Zeile), in register_pending (auto_raid_pipeline.rs) den beim Candidate-Selection bereits geladenen Score-Row einfrieren. In confirm_resolver.resolve dann den eingefrorenen Snapshot bevorzugen und score_store.load nur als Fallback nutzen (None) — exakt analog zur Python-Logik `if score_snapshot: use it; else load fresh` (partner_raid_score_tracking.py:390-393). Damit wird der Score zur Sendezeit persistiert, identisch zu Python, und die zurückfließende Analytik bleibt konsistent. Alternativ, falls bewusst gewünscht ist, künftig immer den Confirm-Zeit-Score zu tracken: Entscheidung in rust/docs/05-cleanup-decisions.md dokumentieren und Python angleichen (score_snapshot-Übergabe entfernen), damit beide Implementierungen dasselbe Verhalten zeigen.

### [raid-scoring] track_confirmed setzt deadlock_continued_*/resolved_at/resolution_reason nie — Nicht-Deadlock-Raids bleiben dauerhaft unaufgelöst (Python löst sie beim Insert auf)
*class:* Vergessene Seiteneffekte / SQL-Default-Drift · *confidence:* 0.85 · *id:* raid-scoring-1

- **Python** bot/raid/partner_raid_score_tracking.py:421-424,449-453,478-481
  - track_confirmed_partner_raid berechnet bei was_deadlock_at_raid=False sofort beim INSERT: deadlock_continued_until=confirmed_at_iso, deadlock_continued_sec=0, resolved_at=confirmed_at_iso, resolution_reason='not_deadlock_at_raid'. Die Zeile ist damit ab dem Insert vollständig aufgelöst (resolved_at gesetzt). Der spätere Session-Resolver (resolve_partner_raid_tracking_for_session) filtert genau auf 'resolved_at IS NULL' und überspringt sie korrekt.
- **Rust** rust/crates/tb-raid/src/score_tracking_store.rs:67-83,99-111
  - Der INSERT in score_tracking_store.rs listet nur 21 Spalten und schreibt nie deadlock_continued_until, deadlock_continued_sec, resolved_at oder resolution_reason — sie bleiben für JEDE Zeile NULL, unabhängig von was_deadlock_at_raid. Der Doc-Kommentar (Z. 58-59) behauptet sogar bewusst, diese Felder 'starten NULL (werden später beim Auflösen gesetzt)' — das gilt in Python aber nur für Deadlock-Raids, nicht für Nicht-Deadlock-Raids.
- **Divergenz:** Für einen bestätigten Partner-Raid, dessen Ziel zum Raid-Zeitpunkt NICHT Deadlock spielte (was_deadlock_at_raid=false, in confirm_resolver.rs:83-90 erreichbar), schreibt Python die Tracking-Zeile als bereits aufgelöst mit Dauer 0 und Grund 'not_deadlock_at_raid'. Rust schreibt sie als unaufgelöst (resolved_at NULL). Sobald der Session-Resolver portiert wird (laut docs/plans/2026-06-09-schritt-4-monitoring.md:107 geplanter Cutover-Schritt), würde er diese Rust-Zeilen fälschlich als 'session_ended' mit echter Zeitspanne (resolution_dt - confirmed_at) statt als 'not_deadlock_at_raid' mit 0s auflösen — dauerhaft verfälschte Post-Raid-Deadlock-Dauer-Statistik.
- **Fix:** Im track_confirmed-INSERT die 4 Spalten ergänzen und wie Python befüllen: bei was_deadlock_at_raid=false → deadlock_continued_until=confirmed_at, deadlock_continued_sec=0, resolved_at=confirmed_at, resolution_reason='not_deadlock_at_raid'; bei true → alle vier NULL. TrackConfirmedInput trägt was_deadlock_at_raid und confirmed_at bereits, es braucht keine neuen Eingaben.
- **Verify-Fix:** In score_tracking_store.rs track_confirmed das Python-Verhalten 1:1 nachbauen: vor dem INSERT die vier Felder ableiten — bei was_deadlock_at_raid=false: deadlock_continued_until=confirmed_at, deadlock_continued_sec=0, resolved_at=confirmed_at, resolution_reason="not_deadlock_at_raid"; bei true: alle vier NULL. INSERT auf 25 Spalten erweitern ($22..$25) und die vier Werte binden. Den irreführenden Doc-Kommentar Z.58-59 korrigieren (gilt nur für Deadlock-Raids). Test track_schreibt_zeile... um einen zweiten Fall mit was_deadlock_at_raid=false ergänzen, der resolved_at != NULL und resolution_reason="not_deadlock_at_raid" assertet (der bestehende Test deckt nur den true-Fall ab und übersieht die Lücke). Falls bereits Rust-Zeilen mit was_deadlock_at_raid=0 und resolved_at=NULL in der DB liegen: einmaliges Backfill-UPDATE, das diese Zeilen analog auflöst, bevor ein Rust-Resolver scharfgeschaltet wird.

## LOW (59)

### [ana-crate] recent-bans: Rust filtert event_type='ban', Python zählt/listet auch unban-Events
*class:* SQL-Drift (WHERE-Filter) · *confidence:* 0.6 · *id:* ana-crate-4

- **Python** bot/analytics/api_public.py:84-117 (kein event_type-Filter; total_30d=COUNT(*); writer mixin.py:2176 schreibt 'unban'|'ban')
  - Liste und Aggregate today/total_30d enthalten ALLE Events aus twitch_ban_events inkl. unban-Events, da kein event_type-Filter gesetzt ist.
- **Rust** rust/crates/tb-analytics/src/bans.rs:45-72 (alle drei Queries mit WHERE event_type = 'ban')
  - Liste und Aggregate sind auf event_type='ban' gefiltert; unban-Events erscheinen nicht und werden nicht mitgezählt.
- **Divergenz:** Öffentlicher Feed und der 'geblockte Bans'-Zähler (total_30d) weichen ab: Rust liefert weniger Einträge/kleinere Zahlen, weil Unbans rausfallen. Zusätzlich nutzt Rust für today CURRENT_DATE (Session-TZ) statt Pythons UTC-Mitternacht — leichte TZ-Drift an Tagesgrenzen. Der Rust-Filter entspricht der internal_home.py-Konvention und ist fachlich plausibler, ist aber eine echte Verhaltensänderung gegenüber dem Python-Public-Endpoint.
- **Fix:** Entscheiden ob der Public-Endpoint Unbans zählen soll. Für Parität: event_type-Filter entfernen. Falls beabsichtigt: in 05-cleanup-decisions dokumentieren. today auf UTC-Mitternacht statt CURRENT_DATE umstellen.

### [chat-commands] !raid_status: Erfolgs-Icon des letzten Raids nutzt Gesamt-Erfolgszahl statt Last-Raid-success
*class:* Toter Code / falsche Datenherkunft · *confidence:* 0.8 · *id:* chat-commands-2

- **Python** bot/chat/commands.py:193-197
  - Das Icon für 'Letzter Raid' kommt aus dem success-Feld GENAU dieser letzten Raid-Zeile: 'icon = "✅" if success else "❌"' (success = last_raid[3]).
- **Rust** rust/crates/tb-chat/src/commands.rs:514-520
  - RaidStatusInfo trägt kein last_raid_success-Feld; das Icon wird über 'if info.successful_raids > 0' bestimmt — also über die Gesamtzahl erfolgreicher Raids.
- **Divergenz:** Wenn der letzte Raid fehlschlug, es aber irgendwann früher mind. einen erfolgreichen Raid gab, zeigt Python ❌ (letzter Raid fehlgeschlagen), Rust dagegen ✅. Das Icon vermittelt falsche Info über den jüngsten Raid.
- **Fix:** RaidStatusInfo um last_raid_success: Option<bool> erweitern (aus derselben ORDER BY executed_at DESC LIMIT 1-Zeile wie Python) und das Icon daraus ableiten statt aus successful_raids.

### [chat-commands] !invite setzt Cooldown vor dem Call statt nach erfolgreichem Send
*class:* Vergessene Seiteneffekte / Reihenfolge · *confidence:* 0.82 · *id:* chat-commands-3

- **Python** bot/chat/bot.py:797-848
  - _handle_invite_command prüft den 1h-Cooldown, ruft dann den Endpoint, und setzt self._invite_cmd_cd[cd_key]=now NUR wenn der Chat-Send erfolgreich war (if ok: ... Zeile 845-846). Schlägt der Call fehl oder kommt keine Antwort, bleibt der Cooldown ungesetzt und der User kann sofort erneut !invite tippen.
- **Rust** rust/crates/tb-chat/src/commands.rs:886-914
  - cmd_invite fügt den Cooldown unbedingt VOR dem invite_line-Call ein (cooldowns.insert(key, Instant::now()) Zeile 896), egal ob danach ein Reply gesendet wird oder ob invite_line Err/None liefert.
- **Divergenz:** Bei einem fehlgeschlagenen oder leeren !invite (z.B. Kanal nicht Deadlock-live, technischer Fehler) verbrennt Rust dem Chatter trotzdem 1 Stunde Cooldown, Python nicht. Der Nutzer wird in Rust unnötig blockiert.
- **Fix:** Cooldown erst NACH erfolgreichem reply_plain setzen — also den insert in den 'Ok(Some(reply)) if !reply.is_empty()'-Zweig nach dem Send verschieben.

### [chat-commands] Berechtigungsgesperrte Commands antworten still statt mit 'Nur Mods'-Hinweis
*class:* Vergessene Seiteneffekte / fehlende Antwort · *confidence:* 0.78 · *id:* chat-commands-4

- **Python** bot/chat/commands.py:60-63
  - cmd_raid_enable (60-63), cmd_uban (208-209), cmd_raid (624-627), cmd_silentban (430-433), cmd_silentraid (486-489) senden bei fehlender Mod/Broadcaster-Berechtigung jeweils eine Ablehnung wie '@user Nur der Broadcaster oder Mods können den Twitch-Bot steuern.' und returnen dann.
- **Rust** rust/crates/tb-chat/src/commands.rs:245-278
  - In handle() wird die Berechtigung im Dispatch geprüft (if event.is_mod_or_broadcaster()). Ist sie nicht erfüllt, wird der Command-Body schlicht nicht aufgerufen und true zurückgegeben — es wird KEINE Ablehnungsnachricht gesendet.
- **Divergenz:** Ein Nicht-Mod, der !raid/!uban/!raid_enable/!silentban/!silentraid tippt, bekommt in Python eine sichtbare Ablehnung, in Rust gar keine Reaktion. Nutzererfahrung weicht durchgängig ab.
- **Fix:** Bei fehlender Berechtigung die jeweilige Python-Ablehnungsnachricht über self.reply(...) senden, bevor true zurückgegeben wird (entweder im Dispatch oder am Anfang der jeweiligen cmd_*-Methode).

### [chat-commands] !engagement_on/off antworten still statt mit 'Nur Broadcaster/Mods/Super-Mod'-Hinweis
*class:* Vergessene Seiteneffekte / fehlende Antwort · *confidence:* 0.78 · *id:* chat-commands-5

- **Python** bot/chat/engagement_commands.py:104-108
  - cmd_engagement_on/off senden bei fehlender Berechtigung '@user Nur Broadcaster, Mods oder Super-Mod dürfen das.' und returnen.
- **Rust** rust/crates/tb-chat/src/commands.rs:934-937
  - cmd_engagement_on/off returnen bei !is_engagement_admin still, ohne Nachricht.
- **Divergenz:** Ein unberechtigter Nutzer bekommt in Python eine Ablehnung, in Rust keine Reaktion.
- **Fix:** Vor dem return die Python-Ablehnungsnachricht via self.reply senden.

### [chat-globalban] Rust schreibt resolvte chatter_id in die DB zurück — Python-Sweep tut das nicht
*class:* Vergessene/zusätzliche Seiteneffekte · *confidence:* 0.85 · *id:* chat-globalban-1

- **Python** bot/chat/global_ban_sweep.py:231-235
  - Wenn entry.chatter_id leer ist, löst Python via _resolve_user_id() die ID nur transient für den Ban auf (target_id = await _resolve_user_id(...) or ''). Es gibt KEINEN Write-Back der aufgelösten ID in twitch_chatter_global_ban. Eine Suche in pg.py und global_ban_sweep.py bestätigt: kein UPDATE ... SET chatter_id im Sweep-Pfad (der einzige COALESCE-Write in pg.py:4185 sitzt im Add-Pfad, nicht im Sweep).
- **Rust** rust/crates/tb-chat/src/global_ban_sweep.rs:237-241,416-433
  - Nach erfolgreichem resolve_user_id ruft Rust write_back_chatter_id() auf und führt UPDATE twitch_chatter_global_ban SET chatter_id = $2 WHERE chatter_login = $1 AND chatter_id IS NULL aus — ein zusätzlicher DB-Schreibvorgang, den es in Python nicht gibt. Der Doc-Kommentar zitiert dafür 'global_ban_sweep.py:224-226', aber dort steht in Python der Empty-Login-Skip, kein Write-Back.
- **Divergenz:** Rust persistiert einen Seiteneffekt (chatter_id-Cache in der Listen-Tabelle), den die Python-Referenz nicht hat. Effekt für Nutzer ist benigne (spätere Sweeps sparen den Helix-Lookup, und der Reaktiv-Pfad is_chatter_globally_banned profitiert sogar vom ID-Match), aber es ist eine Verhaltens-/Daten-Abweichung gegenüber der Referenz und die zitierte Quellzeile ist falsch.
- **Fix:** Entweder als bewusste Verbesserung dokumentieren (Kommentar-Referenz korrigieren, nicht 'Port von :224-226' behaupten) — oder den Write-Back entfernen, um exakt der Python-Semantik zu entsprechen. Kein funktionaler Schaden, nur Konsistenz/Doku.
- **Verify-Fix:** Zwei getrennte, kleine Maßnahmen. (1) Falscher Doc-Kommentar: Den Verweis `global_ban_sweep.py:224–226` an write_back_chatter_id (rust/.../global_ban_sweep.rs:415) korrigieren — Python hat hier KEINE Referenzzeile, da der Write-Back eine bewusste Rust-Ergänzung ist; Kommentar entsprechend umformulieren (z.B. "Rust-Optimierung: cacht aufgelöste chatter_id, kein Python-Pendant") statt eine nicht existierende Quellzeile zu zitieren. (2) Verhaltensdivergenz: Da der Effekt benigne und nützlich ist (idempotenter Cache, spart Helix-Lookups), als bewussten Cleanup in rust/docs/05-cleanup-decisions.md festhalten, damit es nicht als unbeabsichtigte Drift gilt. Optional zur 1:1-Parität alternativ den Write-Back entfernen — nicht empfohlen, da der Cache real Wert bringt. Kein dringender Code-Fix nötig.

### [chat-pipeline] is_deadlock_live ohne Session-game_name-Fallback — gateet Fun-Responses/Promos/!invite stummer als Python
*class:* Fehlende Guards/Bedingungen (fehlender Fallback-Zweig) · *confidence:* 0.85 · *id:* chat-pipeline-2

- **Python** bot/chat/bot.py:755-761 (_is_deadlock_live ruft _is_target_game_live_for_chat) und bot/chat/moderation.py:2051-2073 (elif session_id is not None: → game_name der offenen Session als Fallback wenn keine twitch_live_state-Zeile existiert)
  - _is_target_game_live_for_chat prüft zuerst twitch_live_state. Existiert dort KEINE Zeile, fällt es auf die offene twitch_stream_sessions-Zeile zurück und vergleicht deren game_name mit 'deadlock'. Damit gilt ein frisch live gegangener Kanal (oder einer in der Monitoring-Schreiblücke) bereits als Deadlock-live, sobald die Session game_name=Deadlock trägt.
- **Rust** rust/crates/tb-chat/src/channel_classifier.rs:135-146 (is_deadlock_live nur aus twitch_live_state; Ok(None)/last_game=NULL → _ => false, kein Session-Fallback)
  - Der Classifier liest ausschließlich twitch_live_state. Fehlt die Zeile (fetch_optional → None) oder ist last_game NULL, liefert er is_deadlock_live=false ohne den Session-Fallback. Der Tracker (chatter_tracking.rs:207-219) implementiert den Session-Fallback dagegen korrekt — beide Pfade können denselben Kanal im selben Moment unterschiedlich bewerten.
- **Divergenz:** In dem Zeitfenster, in dem twitch_live_state noch keine Zeile hat, aber eine offene Deadlock-Session existiert, liefert Rust is_deadlock_live=false. Dadurch werden Schritt 9 (Fun-Responses), Schritt 12/13 (Activity-Promo) und der !invite-Pfad stumm übersprungen, obwohl Python sie auslösen würde. Zusätzlich entsteht eine interne Inkonsistenz: der Tracker hält den Kanal für Deadlock-live (schreibt Chatter), der Classifier nicht.
- **Fix:** In channel_classifier.rs den Session-game_name-Fallback nachziehen, analog zu chatter_tracking.rs::is_target_game_live: wenn keine twitch_live_state-Zeile existiert, die offene twitch_stream_sessions-Zeile abfragen und game_name.trim().to_lowercase()=='deadlock' werten. Idealerweise dieselbe Hilfsfunktion für Tracker und Classifier verwenden, damit beide nicht auseinanderlaufen.
- **Verify-Fix:** Den Session-Fallback im Classifier nachziehen, damit er paritätisch zum Python `_is_target_game_live_for_chat` und zum Rust-Tracker `is_target_game_live` (chatter_tracking.rs:202-219) ist. Konkret in channel_classifier.rs:135-146: bei `Ok(None)` (keine twitch_live_state-Zeile) die offene Session des Kanals auflösen (`SELECT id FROM twitch_stream_sessions WHERE LOWER(twitch_login)=$1 AND ended_at IS NULL` bzw. via active_session_id) und dann `SELECT game_name FROM twitch_stream_sessions WHERE id=$1 AND ended_at IS NULL` mit `game_name.trim().to_lowercase()=="deadlock"` vergleichen. Idealerweise die bereits existierende Logik aus chatter_tracking.rs in eine gemeinsame Helper-Funktion ziehen, damit Classifier und Tracker nicht erneut auseinanderlaufen. Zusätzlich den !invite-Handler (chat_command.rs:57-80) auf denselben Helper umstellen, statt twitch_live_state isoliert abzufragen — sonst bleibt dort die Lücke bestehen, auch wenn der Classifier gefixt ist. Hinweis: chat_command.rs nutzt `contains("deadlock")` statt exaktem `== "deadlock"` (kleine zusätzliche Drift gegenüber Classifier/Python); beim Vereinheitlichen mitangleichen.

### [chat-pipeline] Token: expires_in=0 setzt expires_at=now statt None → sofortiger Refresh, abweichend von Python
*class:* None/Option-Semantik + Off-by/Grenzwert · *confidence:* 0.5 · *id:* chat-pipeline-3

- **Python** bot/api/token_manager.py:174-176 (expires_at wird NUR gesetzt wenn expires_in truthy) und :123-124 (get_valid_token refresht lazy nur wenn self.expires_at gesetzt UND now >= expires_at - 5min)
  - Liefert validate ein expires_in von 0/fehlt, bleibt expires_at=None. Der Lazy-Refresh in get_valid_token wird dann nie ausgelöst (Guard 'if not should_refresh and self.expires_at'), der vorhandene Access-Token wird unverändert weiterverwendet bis ein 401 einen Force-Refresh erzwingt.
- **Rust** rust/crates/tb-chat/src/token.rs:147-152 (initialize setzt expires_at = now + seconds(v.expires_in) bedingungslos) und :168 (access_token refresht wenn expires_at - now <= 1h)
  - initialize setzt bei expires_in=0 expires_at = now (now + 0s). Beim nächsten access_token()-Aufruf ist expires_at - now <= REFRESH_THRESHOLD(1h) sofort wahr, also wird unmittelbar ein force_refresh ausgelöst, obwohl der Token gerade als gültig validiert wurde.
- **Divergenz:** Edge-Case: bei einem expires_in=0 aus dem validate-Endpoint refresht Rust den frisch validierten Token sofort weg, Python nicht. Real selten (Twitch liefert i.d.R. ein positives expires_in), daher low. Anmerkung: die generelle Lazy-Schwelle (Rust 1h vs Python 5min) ist KEIN Bug — Rust refresht nur früher und bleibt sicher.
- **Fix:** In token.rs::initialize (und ggf. refresh_with, das bereits .max(60) nutzt) expires_at nur setzen wenn v.expires_in > 0, sonst den State ohne harte Ablauf-Frist führen bzw. eine sinnvolle Mindest-Laufzeit ansetzen, damit ein 0/fehlendes expires_in keinen Sofort-Refresh triggert.
- **Verify-Fix:** In `initialize` (rust/crates/tb-chat/src/token.rs:150) dieselbe Untergrenze wie in `refresh_with` anwenden: `expires_at: Utc::now() + chrono::Duration::seconds(v.expires_in.max(60))`. Damit löst ein expires_in=0 aus dem validate-Endpoint keinen sofortigen Force-Refresh des frisch validierten Tokens aus. Alternativ — falls strikte Python-Parität gewünscht ist — bei expires_in==0 das `expires_at` analog zu Python ungesetzt lassen (z.B. einen weit in der Zukunft liegenden Wert verwenden), sodass kein Lazy-Refresh greift, bis ein 401 einen Force-Refresh erzwingt. Die `.max(60)`-Variante ist die kleinere, konsistentere Änderung und reicht aus.

### [chat-promos] Viewer-Spike has_new_raw-Gate weicht im Erst-Promo-Fall ab
*class:* None/Option-Semantik & Grenzwert · *confidence:* 0.7 · *id:* chat-promos-5

- **Python** bot/chat/promos.py:610-623,1353
  - _has_new_raw_chat_since_last_promo: last_sent is None -> True (Erst-Promo auch bei stillem Chat); sonst True nur wenn last_raw > last_sent.
- **Rust** rust/crates/tb-chat/src/promos.rs:1139,1153
  - has_new_raw = raw_msg_count_since_promo > 0. Im Betrieb meist aequivalent (mark_promo_sent resettet auf 0), aber beim Erst-Promo (last_promo_sent=None) mit nie-aktivem Chat (count=0): Python True, Rust False.
- **Divergenz:** Viewer-Spike zielt auf stille Chats mit Zuschauer-Anstieg. Auf einem Kanal ohne bisherige Promo und komplett stillem Chat blockiert Rust den ersten Spike-Promo, Python laesst ihn zu.
- **Fix:** Nachbilden: last_promo_sent.is_none() -> true; sonst last_raw_chat_message_ts.map_or(false, |r| r > last_promo_sent).
- **Verify-Fix:** In rust/crates/tb-chat/src/promos.rs (maybe_send_viewer_spike_promo, ~Z.1139) den Erst-Promo-Zweig wiederherstellen, statt nur auf den Zähler zu prüfen. Z.B.: `let has_raw = state.last_promo_sent.is_none() || state.last_raw_chat_message_ts.map_or(false, |t| state.last_promo_sent.map_or(true, |p| t > p));` — d.h. wenn noch keine Promo gesendet wurde → true (Python Z.612-613), sonst true nur wenn die letzte Roh-Chat-Nachricht jünger als die letzte Promo ist. So wird die `_has_new_raw_chat_since_last_promo`-Delta-Semantik exakt nachgebildet, statt sie mit der Zähl-Schwelle des Chat-Activity-Pfads zu verwechseln. Voraussetzung: `last_raw_chat_message_ts` muss im State geführt werden (existiert bereits, Z.1142). Unit-Test ergänzen: `last_promo_sent=None`, `raw_msg_count_since_promo=0`, `last_raw_chat_message_ts=None` → has_new_raw == true.

### [chat-promos] seen-chatters ignoriert API-getrackte Session-Viewer und 2h-Reset
*class:* Datenquelle weggelassen · *confidence:* 0.6 · *id:* chat-promos-6

- **Python** bot/chat/promos.py:707-764,736-743
  - _get_current_viewers_combined vereint Chat-Bucket (8-min) MIT API-Session-Viewern (twitch_session_chatters). _update_seen_chatters resettet das seen-Set pro Login wenn >2h. _get_new_chatters = current_combined - seen.
- **Rust** rust/crates/tb-chat/src/promos.rs:928-937,1006-1026
  - update_seen_chatters_inner nutzt nur in-memory-activity-Chatter (keine API-Viewer); get_new_chatters_in_window_inner zaehlt nur Bucket-Chatter mit per-Chatter-Alter (>2h=neu). API-Viewer fehlen in seen und current.
- **Divergenz:** Die Neue-Chatter>=2-Schwelle (PROMO_NEW_CHATTERS_MIN) basiert auf anderer Datengrundlage; API-getrackte stille Viewer fehlen, der Count kann niedriger ausfallen und Promos seltener freischalten.
- **Fix:** _get_current_session_viewers (twitch_session_chatters der aktiven Session) zusaetzlich in seen/current einbeziehen.
- **Verify-Fix:** In der Rust-Version eine async Variante analog zu Python bauen: vor dem Gate-Check `twitch_session_chatters JOIN twitch_live_state (is_live=1)` für den Login abfragen (wie schon in get_lurker_tax_candidates Z.1351/targeted Z.1515 vorhanden), die Login-Menge mit den In-Memory-Bucket-Chattern vereinen und sowohl in `seen_chatters` (beim mark_promo_sent) als auch in `get_new_chatters_in_window` einfliessen lassen. Da `promo_activity_ready_inner`/`get_new_chatters_in_window_inner` aktuell synchron und nur state-basiert sind: entweder die API-Viewer-Menge vorab async laden und in den ChannelState/Parameter reinreichen, oder die Gate-Funktion async machen. Wichtig: gleiche Lowercase-Normalisierung und denselben >2h-Reset (PROMO_SEEN_CHATTER_MAX_AGE_SEC) wie Python beibehalten. Falls die API-Viewer-Quelle bewusst weggelassen werden soll, stattdessen einen Eintrag in 05-cleanup-decisions.md ergänzen.

### [chat-promos] Toter Code: leere for-Schleife in update_seen_chatters_inner
*class:* Toter Code · *confidence:* 0.9 · *id:* chat-promos-10

- **Python** bot/chat/promos.py:745-764
  - Kein Aequivalent; Python iteriert nicht ergebnislos.
- **Rust** rust/crates/tb-chat/src/promos.rs:929-932
  - for (_, ts) in &state.activity { let _ = ts; } - Schleife ohne Effekt vor der eigentlichen Logik.
- **Divergenz:** Kein Verhaltensunterschied, aber toter Code.
- **Fix:** Leere Schleife entfernen.
- **Verify-Fix:** Die leere Schleife `for (_, ts) in &state.activity { let _ = ts; }` (promos.rs:929-932) ersatzlos entfernen samt Kommentar. Die nachfolgenden Zeilen 933-936 (Sammeln der Chatter + `seen_chatters.insert`) bleiben unverändert und erbringen bereits die gesamte Funktionalität. Kein Test betroffen, kein Verhalten ändert sich. Optional bei der Gelegenheit prüfen (eigenes Ticket), ob `update_seen_chatters_inner` analog zu Python auch API-Session-Viewer und Max-Age-Reset abbilden müsste.

### [chat-scam] account_age_cache ist totes State + fehlendes 6h-Caching des Account-Alters (jeder Score-Treffer ruft Helix)
*class:* Toter Code (5) + vergessene Seiteneffekte (2) · *confidence:* 0.82 · *id:* chat-scam-2

- **Python** bot/chat/service_pitch_warning.py:653-698 (_get_account_age_days mit 6h-Cache, Z. 659-663 read / 676,681,687 write)
  - _get_account_age_days cached das Account-Alter pro user 6h (ACCOUNT_CACHE_TTL_SEC) — wiederholte Nachrichten desselben Chatters lösen keinen erneuten fetch_users-Call aus.
- **Rust** rust/crates/tb-chat/src/scam_pitch.rs:392 (Feld account_age_cache deklariert), 1300 (nur pruned), nie .insert geschrieben; HelixAccountAge in bin/tb-bot/src/chat_wiring.rs:386-394 cached ebenfalls nicht
  - Das State-Feld account_age_cache wird angelegt und in prune_state aufgeräumt, aber nie befüllt. Die Caching-Verantwortung wurde an AccountAgePort delegiert, doch der Produktions-Adapter HelixAccountAge ruft bei JEDEM gescorten Event user_created_at über Helix auf, ohne Cache.
- **Divergenz:** Kein falscher Entscheid (die Alter-Logik ist identisch), aber deutlich mehr Helix-API-Calls als Python (kein 6h-Cache) → Rate-Limit-/Latenz-Druck bei aktiven Chattern; das account_age_cache-Feld ist reines totes State.
- **Fix:** Entweder den TTL-Cache im AccountAgePort/HelixAccountAge implementieren (6h, key=id|login) und das account_age_cache-Feld nutzen — oder das tote Feld entfernen, falls Caching bewusst entfällt. Konsistent mit cleanup-decisions halten (DB-only war hier nicht gemeint).
- **Verify-Fix:** Caching in den Adapter `HelixAccountAge` (chat_wiring.rs) ziehen ODER das vorhandene `account_age_cache`-State im Fetch-Pfad nutzen. Konkret die saubere Variante: In `score_and_decide` (scam_pitch.rs) das Account-Alter zuerst aus `st.account_age_cache` lesen (Key = `chatter_id` bzw. `login`, Treffer wenn `now - ts <= ACCOUNT_CACHE_TTL_SEC`) und nur bei Miss `AccountAgePort.user_created_at_days` aufrufen, danach das Ergebnis (inkl. `None`) als `(now, age)` zurückschreiben — analog zur Python-Logik (Z. 659-663 Read, 676/681/687 Write). Das aktiviert das bereits angelegte und geprunte Feld, eliminiert das tote State und stellt die 6h-Cache-Parität wieder her. Alternativ (kleinerer Eingriff, aber dann State-Feld endgültig entfernen): einen `Mutex<HashMap<String,(Instant,Option<i64>)>>` direkt in `HelixAccountAge` halten und dort memoizen — dann aber das ungenutzte `account_age_cache`-Feld + Prune-Call in scam_pitch.rs streichen, damit kein totes State zurückbleibt.

### [chat-scam] review_worthwhile-Fallback ohne spam_reasons weicht von Python ab (Python: False, Rust: spam_domain-Match) — nur über toten maybe_review-Pfad erreichbar
*class:* Fehlende Guards / Default-Drift (3/4) · *confidence:* 0.7 · *id:* chat-scam-3

- **Python** bot/chat/spam_ai_review.py:286-305 (_review_worthwhile: leere reasons → return False)
  - Ist spam_reasons leer (kein Phrase/Fragment/Learned, kein mention, kein 'Muster: viewer + name'), gibt _review_worthwhile False zurück → kein AI-Review.
- **Rust** rust/crates/tb-chat/src/scam_pitch.rs:1594-1611 (review_worthwhile: spam_reasons.is_empty() → spam_domain.is_match(content)); aufgerufen nur von maybe_review (1398), das die Pipeline nicht nutzt — pipeline.rs:651-652 ruft maybe_review_with_reasons
  - Bei leerem spam_reasons gibt review_worthwhile zusätzlich True zurück, wenn spam_domain im Content matcht → würde ein Review auslösen. Dieser Pfad ist nur über das parameterlose maybe_review erreichbar, das in der Produktions-Pipeline nicht verwendet wird (dort läuft maybe_review_with_reasons mit echten Reasons, dessen Logik 1:1 zu Python passt).
- **Divergenz:** Abweichendes Verhalten nur im nicht verdrahteten maybe_review (Test-/Komfort-API). Im Produktionspfad identisch. Daher praktisch wirkungslos, aber latente Falle, falls maybe_review je gewired wird.
- **Fix:** Im leeren-reasons-Zweig False zurückgeben (wie Python), oder maybe_review entfernen, da die Pipeline nur maybe_review_with_reasons nutzt.
- **Verify-Fix:** Den `if spam_reasons.is_empty() { ... }`-Branch in `review_worthwhile` (scam_pitch.rs:1607-1608) entfernen, sodass die Funktion bei leerem spam_reasons wie Python `false` zurückgibt. Da der einzige Aufrufer mit leerem Slice die nicht verdrahtete `maybe_review`-API ist, am besten zugleich `maybe_review` (Z. 1393-1590) löschen (oder mit `#[cfg(test)]` markieren), damit die latente Falle nicht versehentlich produktiv verdrahtet wird. Alternativ `maybe_review` so umbauen, dass es ohne echte Reasons gar kein Review startet. Kein Eingriff am Produktionspfad nötig.

### [chat-scam] Warntext-Whitespace weicht kosmetisch ab (Strong: ein vs. zwei Leerzeichen nach Mention; Public: Leerzeichen vor age_hint)
*class:* Arithmetik/Format-Detail (6, kosmetisch) · *confidence:* 0.9 · *id:* chat-scam-4

- **Python** bot/chat/service_pitch_warning.py:790-798 (Strong: f'🛡️ {mention} wurde…'; Public: f'…Angebote {age_hint} ')
  - Strong: nach '🛡️ ' folgt mention (endet selbst auf Leerzeichen) PLUS weiteres Leerzeichen → 'wurde' bekommt doppeltes Leerzeichen davor. Public: ' {age_hint} ' fügt vor age_hint ein zusätzliches Leerzeichen ein (age_hint beginnt bereits mit Leerzeichen → Doppelspace, wenn gesetzt).
- **Rust** rust/crates/tb-chat/src/scam_pitch.rs:690-698 (Strong: '🛡️ {mention}wurde…'; Public: '…Angebote{age_hint} ')
  - Strong: '{mention}wurde' → genau ein Leerzeichen vor 'wurde'. Public: '{age_hint} ' ohne führendes Extra-Leerzeichen → kein Doppelspace vor age_hint.
- **Divergenz:** Reine Darstellung im Chat (überzählige Leerzeichen), keine logische/Score-/Entscheidungs-Auswirkung. Twitch kollabiert Leerzeichen optisch ohnehin teils. Nicht funktional.
- **Fix:** Falls Byte-Parität gewünscht: Rust an Python-Spacing angleichen ('{mention} wurde' bzw. ' {age_hint} '). Andernfalls Rust-Variante (sauberere Leerzeichen) belassen und als bewusste Bereinigung dokumentieren.
- **Verify-Fix:** Falls Byte-Parität gewünscht ist, die Rust-Variante an Python angleichen (oder umgekehrt — sauberer wäre, die Python-Doppelspaces zu entfernen, da das die korrekte Darstellung ist). Konkret entweder in Python in Z. 792 das Extra-Leerzeichen vor "wurde" entfernen (`{mention}wurde`) und in Z. 797 das Leerzeichen vor `{age_hint}` streichen (`Angebote{age_hint}`), womit Python = Rust; oder umgekehrt das Leerzeichen in Rust ergänzen. Da Rust die kosmetisch sauberere Form (einfache Leerzeichen) liefert, ist die Empfehlung: Python an Rust angleichen, kein Rust-Eingriff. Niedrige Priorität — keine funktionale Auswirkung.

### [dash-audience] category-comparison: avg_percentile-Default bei leerer Liste 50 statt 0
*class:* None/Default-Drift (Bug-Klasse 1/4) · *confidence:* 0.8 · *id:* dash-audience-5

- **Python** bot/analytics/api_performance.py:994 (avg_percentile = int(_percentile_of(...)*100) if sorted_avgs else 0)
  - Wenn sorted_avgs leer ist, setzt Python avg_percentile = 0 (nur peak/ret/chat defaulten auf 50).
- **Rust** rust/crates/tb-dashboard-api/src/handlers/category_comparison.rs:30-36,248 (percentile_of gibt bei leerem Slice 50 zurück; avg_percentile = percentile_of(&sorted_avgs, your_avg))
  - Rust nutzt für avg dasselbe percentile_of, das bei leerem Slice 50 liefert. Damit ist avg_percentile=50 statt 0, wenn keine Kategorie-Daten vorliegen.
- **Divergenz:** Im Datenleer-Fall (keine Kategorie-Avgs, real durch Finding 1 häufig getriggert) zeigt Rust 50. Percentile, Python 0.0. Reiner Edge-/Default-Unterschied auf einem Feld.
- **Fix:** Für avg_percentile den leeren Fall separat behandeln: if sorted_avgs.is_empty() { 0 } else { percentile_of(...) }.

### [dash-audience] category-leaderboard: yourTier aus gefiltertem Leaderboard-Avg statt aus ungefiltertem peer_group-Avg
*class:* SQL-/Default-Drift (Bug-Klasse 4) · *confidence:* 0.6 · *id:* dash-audience-6

- **Python** bot/analytics/api_performance.py:1237-1240 (your_tier = _get_peer_group_stats(...)['tier']) i.V.m. 210-273 (Tier aus ungefilterten twitch_stats_category-Avgs, Session-Fallback)
  - yourTier wird unabhängig von exclude_external/tier-Filter über _get_peer_group_stats aus dem ungefilterten Kategorie-Avg (bzw. Session-Avg-Fallback) bestimmt; Streamer mit Avg>100 werden korrekt z.B. als 'top' eingestuft.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/category_leaderboard.rs:138-151,174-198 (your_tier aus your_avg_opt = Leaderboard-Row-Avg, sonst Session-Fallback)
  - yourTier nimmt das avg_vc des Streamers AUS den bereits per exclude_external-HAVING und tier-Filter reduzierten Leaderboard-Rows; fehlt der Streamer dort, greift ein Session-Avg-Fallback. Bei exclude_external=1 mit eigenem Avg>100 ist der Streamer aus den Rows gefiltert → Tier kommt aus dem Session-Avg (andere Metrik) statt aus dem Kategorie-Avg.
- **Divergenz:** Bei aktivem External-Filter oder Tier-Filter weicht der angezeigte yourTier ab, weil die Tier-Quelle in Rust an den gefilterten Datensatz gekoppelt ist statt an die unabhängige, ungefilterte peer_group-Berechnung. (Verschärft durch Finding 1, das avg_vc ohnehin auf 0 dekodiert → in der Praxis 'starter'.)
- **Fix:** yourTier wie Python aus einer separaten, ungefilterten Kategorie-Avg-Abfrage (Äquivalent zu _get_peer_group_stats, ohne threshold/tier-Filter) mit Session-Fallback bestimmen, nicht aus den gefilterten Leaderboard-Rows.
- **Verify-Fix:** your_tier in Rust von den gefilterten Leaderboard-Rows entkoppeln und wie Python aus einer separaten, ungefilterten Kategorie-Avg-Query bestimmen. Konkret: eine eigene Abfrage `SELECT AVG(c.viewer_count) AS avg_vc FROM twitch_stats_category c WHERE c.ts_utc >= $1 AND LOWER(c.streamer) = $2 GROUP BY c.streamer` (ohne HAVING, ohne tier_filter) ausführen; deren Ergebnis als primäre Tier-Quelle nehmen, nur bei NULL auf den twitch_stream_sessions-Fallback (Z.180-193) zurückfallen. `your_avg_opt` aus den Rows nicht mehr für die Tier-Bestimmung verwenden (yourRank/yourEntry bleiben davon unberührt). Damit entspricht die Reihenfolge ungefilterter-Kategorie-Avg -> Session-Fallback exakt dem Python-Verhalten.

### [dash-audience] viewer-overlap: streamerA wird kleingeschrieben ausgegeben statt im Originaltext
*class:* Toter/kosmetischer Unterschied (Bug-Klasse 5) · *confidence:* 0.65 · *id:* dash-audience-7

- **Python** bot/analytics/api_audience.py:761,837 (base=streamer.lower() nur intern; im Output 'streamerA': streamer = roher, getrimmter Query-String in Originalschreibweise)
  - Python verwendet die Originalschreibweise des angefragten Streamers im Feld streamerA (z.B. 'CoolStreamer').
- **Rust** rust/crates/tb-dashboard-api/src/handlers/audience.rs:70-73,162 (streamer wird komplett auf .to_lowercase() gesetzt und so als 'streamerA' ausgegeben)
  - Rust gibt streamerA in Kleinbuchstaben aus, weil der gesamte streamer-Wert vor der Nutzung lowercased wird.
- **Divergenz:** Nur Anzeige-Casing des Feldes streamerA weicht ab; keine Auswirkung auf Zahlen oder Logik.
- **Fix:** Falls Paritätswunsch: Originalschreibweise des Query-Parameters separat behalten und für streamerA verwenden, lowercased nur für SQL-Bindings.
- **Verify-Fix:** Falls Bit-für-Bit-Parität gewünscht: In Rust die Original-Schreibweise für die Ausgabe behalten, indem getrimmter Rohwert und Lowercase-Variante getrennt werden. Konkret in viewer_overlap_handler (Z.70-73) statt `Some(s) => s.to_lowercase()` den getrimmten Originalwert in eine Variable (z.B. `streamer_orig`) und separat `let streamer = streamer_orig.to_lowercase()` für die SQL-Bindings; in der JSON-Ausgabe (Z.162) dann `"streamerA": streamer_orig` verwenden. Da das Feld jedoch von keinem Consumer gerendert wird, ist der Fix optional/kosmetisch und kann auch bewusst zurückgestellt werden.

### [dash-auth-legal] Health raw-chat-lag warns at 120s not 900s, wrong code/level
*class:* Grenzwerte · *confidence:* 0.95 · *id:* dash-auth-legal-1

- **Python** api_admin.py:71,1446-71
  - lag>=900; warning/raw_chat_lag_high; warning/raw_chat_insert_error
- **Rust** system/health.rs:71-92
  - lag>120; warn/RAW_CHAT_LAG; error/RAW_CHAT_ERROR
- **Divergenz:** threshold 120 vs 900; codes+levels differ (frontend reads them)
- **Fix:** use 900 and Python codes/levels
- **Verify-Fix:** In crates/tb-dashboard-api/src/handlers/system/health.rs:73 die Schwelle an Python angleichen: `if lag > 120` ersetzen durch `if lag >= 900` (idealerweise eine benannte Konstante `const RAW_CHAT_LAG_WARNING_SECONDS: i64 = 900;` einführen, analog zu Pythons `_RAW_CHAT_LAG_WARNING_SECONDS`). Falls die niedrigere Schwelle bewusst gewünscht ist, stattdessen die Entscheidung in rust/docs/05-cleanup-decisions.md dokumentieren UND den Python-Wert nachziehen, damit beide Implementierungen konsistent bleiben. Codes/Levels (`RAW_CHAT_LAG`/`warn` vs `raw_chat_lag_high`/`warning`, `RAW_CHAT_ERROR`/`error` vs `raw_chat_insert_error`/`warning`) optional angleichen, da das Frontend sie aktuell ignoriert — niedrige Priorität, aber sinnvoll, falls ein künftiger Consumer auf `code`/`level` filtert.

### [dash-auth-legal] System-errors reads never-written table not log files
*class:* SQL-Drift · *confidence:* 0.8 · *id:* dash-auth-legal-4

- **Python** api_admin.py:1223-45,1628
  - reads admin error log files
- **Rust** system_errors.rs:31-95
  - reads twitch_admin_error_log, no prod writer
- **Divergenz:** Rust panel always empty; Python shows real errors
- **Fix:** wire log-file source or add producer
- **Verify-Fix:** Zwei Optionen. (A) Quelle angleichen: Den Rust-Handler die echten Logdateien lesen lassen (Port von `_admin_error_log_candidates` + Zeilen-Scan + ERROR/CRITICAL/TRACEBACK/EXCEPTION-Parsing nach Rust), damit das Panel wie unter Python die realen Fehler zeigt. (B) Wenn der DB-Tabellen-Ansatz beibehalten werden soll, muss er zuerst funktionsfähig gemacht werden: (1) eine sqlx-Migration anlegen, die `twitch_admin_error_log (id BIGSERIAL PK, created_at TIMESTAMPTZ DEFAULT NOW(), level TEXT, message TEXT, context TEXT)` erstellt, und (2) einen Prod-Writer ergänzen — z. B. einen tracing-Layer/Appender, der ERROR/CRITICAL-Events in die Tabelle schreibt. Ohne (2) bleibt das Panel leer. Variante (A) ist näher am Python-Verhalten und ohne neuen Schreibpfad; Variante (B) ist sauberer, aber mehr Aufwand. Zusätzlich diese bewusste Quellenänderung in 05-cleanup-decisions.md dokumentieren.

### [dash-auth-legal] Admin-streamers displayName never falls back to discord name
*class:* SQL-Drift · *confidence:* 0.85 · *id:* dash-auth-legal-7

- **Python** admin_streamer_queries.py:378-79,460-61
  - discord_display_name or login
- **Rust** admin_streamers.rs:204,297
  - login always
- **Divergenz:** discord name dropped in list+detail
- **Fix:** discord_display_name else login
- **Verify-Fix:** In rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs beide Stellen auf die Python-Präzedenz bringen. List (Z.203-204): `display_name` aus `discord_display_name` mit Login-Fallback ableiten, BEVOR `discord_display_name` ins Struct gemoved wird, z.B.: `let display_name = r.discord_display_name.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| r.twitch_login.clone());` dann `login: r.twitch_login.clone(), display_name, ... discord_display_name: r.discord_display_name,`. Detail (Z.296-297) analog mit `row.discord_display_name`. Beachten: Python trimmt und behandelt leere Strings als nicht gesetzt (`.strip() or login`), das im Rust mit `.trim()`/`filter(!is_empty)` nachbilden. Anschließend Test ergänzen (es existiert detail_returns_200_*-Test), der bei gesetztem discord_display_name `displayName == discord-name` und bei leerem/NULL `displayName == login` prüft.

### [dash-auth-legal] Health last_tick_age and lag not clamped to >=0
*class:* Grenzwerte · *confidence:* 0.72 · *id:* dash-auth-legal-8

- **Python** api_admin.py:1402-05,615-18
  - max(0,...) clamps both
- **Rust** health.rs:58-59;system_health.rs:114-118
  - unclamped, negative on clock skew
- **Divergenz:** negative ages vs 0 in Python
- **Fix:** clamp max(0)/GREATEST(0,...)
- **Verify-Fix:** In health.rs:58-59 den Tick-Alter-Wert klemmen: `last_tick.map(|dt| Utc::now().signed_duration_since(dt).num_seconds().max(0))`. In system_health.rs:116 die SQL-Lag-Berechnung klemmen: `GREATEST(0, EXTRACT(EPOCH FROM (NOW() - newest_signal_at))::BIGINT)`. Damit stimmt das Verhalten 1:1 mit Pythons `max(0, ...)` ueberein und Clock-Skew kann keine negativen Diagnose-Werte erzeugen.

### [dash-perf] retention-curve: sessions_used ist Minuten-Zeilenzahl statt Session-Anzahl
*class:* Off-by-one/Grenzwert · *confidence:* 0.9 · *id:* dash-perf-6

- **Python** bot/analytics/api_performance.py:1744
  - sessions_used = Anzahl der tatsaechlich geladenen Sessions (max 50).
- **Rust** rust/crates/tb-dashboard-api/src/handlers/retention_curve.rs:144
  - rows.len().min(50) mit rows = pro-Minute aggregierte stats-Zeilen (bis 181 Minuten); meldet Minutenanzahl gekappt auf 50, nicht Sessions. 3 Sessions/40 Minuten meldet 40 statt 3.
- **Divergenz:** rows zaehlt Minuten-Buckets (GROUP BY minute), nicht Sessions. Kommentar im Code raeumt es selbst ein. Falsche/aufgeblaehte Stichprobengroesse.
- **Fix:** separates COUNT der recent_sessions zurueckgeben statt rows.len().
- **Verify-Fix:** In retention_curve.rs die Session-Anzahl separat aus dem recent_sessions-CTE bestimmen statt aus den per-Minute aggregierten rows. Option A: in der CTE-Query ein zusätzliches Feld `(SELECT COUNT(*) FROM recent_sessions)` als konstante Spalte mitführen und beim ersten row auslesen (oder per Window-Funktion). Option B: eine kleine separate Query `SELECT COUNT(*) FROM (SELECT id FROM twitch_stream_sessions WHERE LOWER(streamer_login)=$1 AND started_at>=$2 AND ended_at IS NOT NULL ORDER BY started_at DESC LIMIT 50) t` ausführen und deren Wert als sessions_used setzen. Damit entspricht sessions_used wie in Python der echten Session-Zahl (0–50). Den irreführenden Kommentar in Z.139 entfernen.

### [dash-perf] retention-curve: drop_events.type immer unknown — ad_break-Klassifizierung nicht portiert
*class:* Fehlende Portierung · *confidence:* 0.85 · *id:* dash-perf-7

- **Python** bot/analytics/api_performance.py:1696-1730
  - laedt twitch_ad_break_events, markiert Drop-Events deren Minute mit Ad-Break zusammenfaellt als type=ad_break.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/retention_curve.rs:133
  - keine Ad-Break-Query; jedes drop_event fest type=unknown.
- **Divergenz:** Unterscheidung ad_break vs unknown geht durchgaengig verloren.
- **Fix:** twitch_ad_break_events im Session-Fenster laden, Minuten-Offsets bilden, type setzen.
- **Verify-Fix:** In retention_curve.rs vor der Drop-Event-Schleife (vor Z.117) eine Ad-Break-Query analog Python ergänzen: `SELECT a.started_at, s.started_at FROM twitch_ad_break_events a JOIN twitch_stream_sessions s ON s.id = a.session_id` für die 50 recent_sessions (die session_ids stehen bereits in der recent_sessions-CTE — entweder per separater Query mit gleichem streamer/since-Filter oder die CTE um die session_ids erweitern). Pro Row die Minuten-Differenz `(ad.started_at - session.started_at)` in ganze Minuten umrechnen und in ein HashSet<i64> sammeln. In Z.133 dann `"type": if ad_minutes.contains(&(cur_min as i64)) { "ad_break" } else { "unknown" }` setzen. Postgres kann die Minuten-Differenz direkt liefern (z.B. `FLOOR(EXTRACT(EPOCH FROM (a.started_at - s.started_at))/60)::int`), das spart das chrono-Parsing. Da kein Frontend das Feld nutzt, ist der Fix niedrig priorisiert — entweder Parität herstellen oder bewusst in 05-cleanup-decisions.md als weggelassen dokumentieren.

### [dash-perf] retention-curve: avg_watch_duration_min ueberspringt curve[0] (Schleife ab i=1)
*class:* Off-by-one/Grenzwert · *confidence:* 0.8 · *id:* dash-perf-8

- **Python** bot/analytics/api_performance.py:1734-1738
  - iteriert alle Kurvenpunkte inkl Index 0; erster mit median_retention<0.5 liefert avg_watch_duration_min.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/retention_curve.rs:119-126
  - avg_watch_min in Drop-Event-Schleife for i in 1..curve.len() gesetzt; curve[0] nie geprueft. Ist curve[0] der erste unter 0.5, kommt erst ein spaeterer Punkt (oder null).
- **Divergenz:** Kurven die schon Minute 0 unter 50% starten melden zu spaeten oder fehlenden Wert.
- **Fix:** avg_watch in eigener Schleife ueber gesamten curve-Slice ab Index 0.
- **Verify-Fix:** In retention_curve.rs die avg_watch-Berechnung aus der `for i in 1..curve.len()`-Schleife herausziehen und als eigene Schleife über ALLE Punkte ab Index 0 implementieren, exakt wie Python: `let mut avg_watch_min: Option<i32> = None; for p in &curve { if p["median_retention"].as_f64().unwrap_or(0.0) < 0.5 { avg_watch_min = Some(p["minute"].as_i64().unwrap_or(0) as i32); break; } }`. Die Drop-Event-Schleife bleibt unverändert bei i=1. Optional Regressions-Test mit Fixture-Kurve, deren curve[0] bereits < 0.5 ist, gegen Python-Ergebnis.

### [dash-perf] rankings: ORDER BY value DESC NULLS LAST statt Postgres-Default NULLS FIRST
*class:* SQL-Drift · *confidence:* 0.7 · *id:* dash-perf-11

- **Python** bot/analytics/api_performance.py:773,787,800
  - DESC → Default NULLS FIRST: Streamer mit NULL-Wert landen oben (Rank 1, value im Code 0) und koennen echte Top-Streamer aus dem LIMIT verdraengen.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/rankings.rs:74,85,98,111,123,134
  - DESC NULLS LAST → NULL-Wert-Streamer landen hinten und fallen bei LIMIT zuerst raus.
- **Divergenz:** Rangfolge und Ergebnismenge unterscheiden sich bei NULL-Aggregaten (retention/viewers). Rust fachlich plausibler aber abweichend.
- **Fix:** fuer 1:1-Paritaet NULLS LAST entfernen oder als bewusste Korrektur dokumentieren.
- **Verify-Fix:** Python an Rust angleichen, da Rust korrekter ist: in `_load_rankings_payload_sync` bei der retention-Variante `ORDER BY value DESC` zu `ORDER BY value DESC NULLS LAST` ändern (die viewers/growth-Zweige können es kosmetisch mitbekommen, sind aber folgenlos, da deren Aggregate nie NULL werden). Alternativ — wenn Python als SSOT gelten soll — NULLS LAST aus Rust entfernen; das wäre aber schlechter, weil dann NULL-Retention-Streamer als Rank 1 mit value 0 angezeigt würden. Empfehlung: Python fixen (NULLS LAST), damit Streamer ohne berechnete Retention nicht echte Top-Performer verdrängen.

### [dash-perf] follower-funnel: confidence-Schwelle weicht bei kleinem session_count ab
*class:* Off-by-one/Grenzwert · *confidence:* 0.78 · *id:* dash-perf-12

- **Python** bot/analytics/api_audience.py:712
  - high-Schwelle = max(3, floor(session_count*0.6)).
- **Rust** rust/crates/tb-dashboard-api/src/handlers/follower_funnel.rs:215
  - session_count.max(3)*3/5 (Integer-Division); fuer session_count 1..4 ergibt 1,1,1,2 statt 3 → bei wenigen gueltigen Samples zu frueh high statt medium.
- **Divergenz:** Reihenfolge max() vs Multiplikation vertauscht: Python max(3,..) NACH Skalierung, Rust max(.,3) auf session_count VOR Multiplikation. Betrifft nur confidence-Label bei niedrigen Session-Zahlen.
- **Fix:** ((session_count as f64*0.6) as i64).max(3).
- **Verify-Fix:** In follower_funnel.rs Z.215 die Operationsreihenfolge an Python angleichen: `else if follower_valid_samples >= (session_count as f64 * 0.6).floor().max(3.0) as i64` — d.h. erst `session_count * 0.6` (floor), dann `.max(3)`. Damit ergibt sich für session_count 1..4 wieder durchgängig 3 als Schwelle, identisch zu `max(3, int(session_count * 0.6))`. Anschließend den irreführenden Kommentar "Arithmetik (identisch Python ...)" verifizieren bzw. den restlichen Block gegenchecken, da er die Identität behauptet, sie hier aber verletzt war.

### [dash-perf] `[SQLX-DECODE]` session-detail Fallback: unique/first_time/returning_chatters (INTEGER) als i64 → 0
*class:* sqlx Typ-Mismatch · *confidence:* 0.72 · *id:* dash-perf-13

- **Python** bot/analytics/api_v2.py:2256-2258
  - ohne twitch_session_chatters-Daten werden gespeicherte Session-Integer-Spalten zurueckgegeben.
- **Rust** rust/crates/tb-dashboard-api/src/handlers/session_detail.rs:159-162
  - try_get::<i64>(unique_chatters) usw. auf INTEGER scheitert → unwrap_or(0); Fallback-Zweig liefert 0 statt gespeicherter Werte.
- **Divergenz:** Spalten INTEGER, als i64 gelesen. Nur Sessions ohne Chatter-Tracking (Fallback-Pfad).
- **Fix:** als i32 lesen.
- **Verify-Fix:** In rust/crates/tb-dashboard-api/src/handlers/session_detail.rs change the three fallback reads (lines 159-161) from `try_get::<i64, _>` to `try_get::<i32, _>` to match the INTEGER (int4) column type, then cast to the unified type used downstream. Since the chatter_stats branch produces i64 (COUNT→BIGINT), the cleanest fix is to keep the tuple as i64 but read the fallback columns as i32 and widen: e.g. `row.try_get::<i32, _>("unique_chatters").unwrap_or(0) as i64` for all three. Add a regression test inserting a session with non-zero chatter columns and no twitch_session_chatters rows, asserting the API returns the stored values (not 0). Optionally audit other dashboard-api handlers for the same i64-on-INTEGER pattern.

### [dash-viewers] viewer-directory: sort=first_seen degradiert zu Sessions-Sort
*class:* Toter Code · *confidence:* 0.9 · *id:* dash-viewers-3

- **Python** api_viewers.py:375-386
  - sortiert nach firstSeen-ISO (chronologisch).
- **Rust** viewers.rs:363-369 vs 544-551
  - key_of hat keinen firstSeen-Arm -> totalSessions.
- **Divergenz:** sort=first_seen liefert Sessions-Reihenfolge statt Datum.
- **Fix:** firstSeen-Zweig in key_of ergaenzen.
- **Verify-Fix:** In viewer_directory_handler einen firstSeen-Sortierpfad ergänzen, der die RFC3339-Strings (oder besser das DateTime/first_seen_at direkt) vergleicht statt i64. Konkret: key_of darf für "firstSeen" nicht i64 liefern; entweder den Zweig sort=="firstSeen" vor key_of mit einem eigenen String-Vergleich abzweigen (a["firstSeen"].as_str() vs b["firstSeen"].as_str(), None ans Ende) oder die Sortierung generell auf einen Enum-Key mit Datums-Variante umstellen. order_desc/asc-Semantik dabei wie bei den anschließenden i64-Zweigen beibehalten (desc = neueste zuerst). Danach mit einem kleinen Datensatz gegen die Python-Reihenfolge gegenprüfen.

### [dash-viewers] viewer-directory: Streamer-Self+Bot-Identitaeten nicht ausgeschlossen
*class:* Fehlende Guards/Bedingungen · *confidence:* 0.82 · *id:* dash-viewers-5

- **Python** api_viewers.py:32-52,204-209
  - Exclusion enthaelt streamer+bot_login+raid_bot.
- **Rust** viewers.rs:296,409,617
  - Nur statische KNOWN_CHAT_BOTS.
- **Divergenz:** Streamer+eigene Bots zaehlen als Viewer; Totals weichen ab.
- **Fix:** Streamer+Bot-Identitaeten in Exclusion, mit ANY->ALL-Fix.
- **Verify-Fix:** In viewers.rs eine dynamische Exclusion analog zu viewer_timeline.rs einfuehren: streamer-Login (lowercased) zu KNOWN_CHAT_BOTS hinzufuegen und an alle drei Aggregations-Queries binden (fetch_window_viewer_rows Z.297-308, Cross-Channel Z.410-418, viewer-detail Z.620-629). Fuer volle Paritaet zusaetzlich die dynamischen Bot-Logins (bot_login des Bot-Token-Managers + Raid-Bot-Login) verfuegbar machen — diese liegen in Konfiguration/Secrets bzw. koennen einmalig beim Start aufgeloest und als ergaenzbare Liste an die Queries gebunden werden. Minimal-Fix mit hoechstem Nutzen: zumindest den Streamer-Self in der Directory-Aggregation ausschliessen (das ist die haeufigste, garantiert vorhandene Identitaet). Danach Snapshot-Diff Python vs. Rust auf totalViewers/activeViewers fuer einen Streamer mit aktivem Bot zur Verifikation.

### [iapi-rest] Streamer-Mutationen (add/remove/verify/archive/discord-flag/discord-profile) ohne Idempotenz-Layer
*class:* Vergessene Seiteneffekte · *confidence:* 0.72 · *id:* iapi-rest-3

- **Python** bot/internal_api/routes/streamers.py:39-91 (streamer_add), :95-133 (streamer_remove), :147-188 (verify), :202-243 (archive), :257-312 (discord_flag), :324-386 (discord_profile) — alle mit _prepare_idempotency/_release_idempotency_owner
  - Jede dieser Mutationen läuft durch den Idempotenz-Layer: gleicher Idempotency-Key+Fingerprint → gecachte Antwort/Replay statt Zweitausführung; Konflikt → 409; Inflight-Dedup.
- **Rust** rust/crates/tb-internal-api/src/handlers/streamers.rs:193-568 (add/remove/verify/archive/discord_flag/discord_profile_handler) — keiner nimmt IdempotencyState
  - Die Rust-Handler ignorieren den Idempotency-Key-Header komplett (kein Extension<IdempotencyState>), führen jede Anfrage erneut aus.
- **Divergenz:** Ein Client, der mit Idempotency-Key retried (Netz-Retry), bekommt in Rust keine Replay-Antwort und löst die Mutation erneut aus (z.B. doppeltes archive/verify, fehlende 409 bei Key-Reuse-Konflikt). Der Idempotenz-Layer existiert im Crate (idempotency.rs) und wird von link-click/oauth-callback genutzt, nur bei Streamer-CRUD nicht eingehängt.
- **Fix:** Die Streamer-Mutations-Handler analog zu live_link_click_handler/oauth_callback_handler mit Extension<IdempotencyState> + prepare/Owner-complete umbauen (Body bzw. None als Fingerprint, wie Python add/remove).
- **Verify-Fix:** Den vorhandenen geteilten Idempotenz-Layer in die sechs Streamer-Mutations-Handler einhängen, analog zu telemetry_routes.rs (link-click) und raid_oauth.rs: Extension<IdempotencyState> + IDEMPOTENCY_KEY_HEADER in add_handler/remove_handler/verify_handler/archive_handler/discord_flag_handler/discord_profile_handler aufnehmen, am Anfang prepare() aufrufen (Replay/409-Conflict/Inflight-Dedup) und am Ende das Ergebnis cachen (Status+Body), mit gesetztem X-Idempotency-Replayed-Header beim Replay. Scope-Key + Fingerprint-Berechnung 1:1 zur Python-Logik in app.py _request_fingerprint/_idempotency_scope_key halten, damit Key-Reuse mit abweichendem Body denselben 409 liefert. Praktisch zuerst archive (echte Nicht-Idempotenz via toggle) priorisieren. Alternativ, falls bewusst zurückgestellt, in 05-cleanup-decisions.md als geplante Restschuld dokumentieren statt nur im stillen Handler-Docstring.

### [iapi-rest] Längenprüfung der link-click-Textfelder in Bytes statt Unicode-Codepoints
*class:* Arithmetik/Grenzwerte · *confidence:* 0.78 · *id:* iapi-rest-4

- **Python** bot/internal_api/policy.py:296-321 (normalize_tracking_token len(text)>128, normalize_text_field len(text)>max_length — len() zählt Codepoints)
  - Python len() misst Unicode-Codepoints; ein Feld mit 100 Emoji/Umlaut-Zeichen zählt als 100.
- **Rust** rust/crates/tb-internal-api/src/handlers/telemetry_routes.rs:148 (text.len()>max_length) und :186 (text.len()>128)
  - Rust String::len() misst UTF-8-Bytes; dieselben 100 Multibyte-Zeichen zählen als 200–400 Bytes und überschreiten die Grenze → 400, wo Python noch akzeptiert.
- **Divergenz:** discord_username (200), tracking_token (128), source_hint (100): bei nicht-ASCII-Inhalt lehnt der Rust-Port Eingaben mit 400 ab, die Python durchlässt — abweichendes Validierungsverhalten an der Grenze.
- **Fix:** In normalize_text_field/normalize_tracking_token text.chars().count() statt text.len() für die Längenprüfung verwenden (Codepoint-Parität zu Python len()).

### [iapi-rest] Self-Explainer-Relay: float-channel_id wird zu 0 statt zum getrunkten int
*class:* Arithmetik/Grenzwerte · *confidence:* 0.55 · *id:* iapi-rest-5

- **Python** bot/internal_api/routes/discord_log.py:74-91 (not channel_id, dann int(channel_id))
  - Bei channel_id als JSON-Float (z.B. 1.5) ist es truthy; int(1.5) → 1. Der Broker-Payload bekommt channel_id=1.
- **Rust** rust/crates/tb-internal-api/src/handlers/self_explainer_log.rs:158-167 (is_truthy_channel_id), :230-234 (channel_id_int via n.as_i64().unwrap_or(0))
  - is_truthy_channel_id(1.5)=true, danach Value::Number(1.5).as_i64() → None → unwrap_or(0) → channel_id_int=0. Broker bekommt channel_id=0.
- **Divergenz:** Ein Float-channel_id wird in Rust auf 0 gesetzt statt wie in Python abgeschnitten — der Relay würde an Kanal 0 statt an den intendierten gerundeten Kanal senden. Sehr seltener Fall (channel_id ist normalerweise int oder String).
- **Fix:** Für Value::Number den f64-Wert per as_f64().map(|f| f as i64) abschneiden (oder Float-channel_id früher zurückweisen), um Pythons int()-Truncation nachzubilden.
- **Verify-Fix:** In self_explainer_log.rs die channel_id_int-Konvertierung (Zeile 230-234) so anpassen, dass ein Float wie Python `int()` Richtung Null getrunkt wird statt auf 0 zu fallen, z. B. `Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)).unwrap_or(0)`. Damit ergibt 1.5→1, 1.9→1, 2.0→2, parität zu Python. Optional einen Unit-Test mit `{"channel_id":1.5,...}` ergänzen, der channel_id_int==1 erwartet. Da der reale Caller ohnehin Integer sendet, ist der Fix Härtung/Paritäts-Korrektur, keine dringende Maßnahme.

### [iapi-stats] p95-Utilization Index-Methode weicht ab (DB-Pfad)
*class:* Arithmetik / Percentile-Methode · *confidence:* 0.85 · *id:* iapi-stats-4

- **Python** bot/monitoring/eventsub_mixin.py:780-786 (idx = int(round((len(ordered)-1)*0.95)))
  - p95-Index = round((n-1)*0.95), geklemmt auf [0, n-1]. Für n=20: round(18.05)=18 → ordered[18].
- **Rust** rust/crates/tb-internal-api/src/handlers/stats_native.rs:1028-1029 (p95_idx = (len*0.95) as usize).min(len-1))
  - p95_idx = trunc(n*0.95), geklemmt auf min(n-1). Für n=20: trunc(19.0)=19 → sorted[19] (Maximum).
- **Divergenz:** Für viele Sample-Größen (z. B. n=20) wählen die beiden Formeln unterschiedliche Indizes → unterschiedlicher p95_utilization_pct. Rust tendiert höher (näher am Max), Python interpoliert konservativer. Betrifft den nativen DB-only-Pfad, der nach dem Monitoring-Takeover die angezeigten Werte liefert.
- **Fix:** p95_idx = ((sorted.len() as f64 - 1.0) * 0.95).round() as usize, dann auf [0, len-1] klemmen — exakt Pythons _p95.
- **Verify-Fix:** Rust an die Python-Methode angleichen, damit identische Indizes herauskommen: in stats_native.rs:1028 ersetzen durch `let p95_idx = (((sorted_util.len().saturating_sub(1)) as f64 * 0.95).round() as usize).min(sorted_util.len().saturating_sub(1));` (also Basis n-1, `round` statt `as usize`-Truncation, gleiche Klemmung). Damit liefert z. B. n=20 wieder Index 18 statt 19. Alternativ — falls Rust bewusst als kanonische Quelle gelten soll — die Methode in 05-cleanup-decisions.md als bewusste Vereinheitlichung dokumentieren und den Python-Pfad (der ohnehin nach dem Takeover nicht mehr die angezeigten Werte liefert) entsprechend angleichen, damit beide Pfade konsistent bleiben.

### [iapi-stats] EventSub-AVG-Felder in Rust nicht auf 2 Dezimalstellen gerundet
*class:* Arithmetik / Rundungs-Drift · *confidence:* 0.8 · *id:* iapi-stats-5

- **Python** bot/monitoring/eventsub_mixin.py:852,854,856 (round(_avg(...),2) für avg_used_slots/avg_listener_count/avg_ready_listeners)
  - avg_used_slots, avg_listener_count und avg_ready_listeners werden auf 2 Nachkommastellen gerundet (round(.,2)). avg_utilization_pct/p95/max_utilization_pct ebenfalls.
- **Rust** rust/crates/tb-internal-api/src/handlers/stats_native.rs:1099,1101,1103 (avg_i(...) ohne Rundung)
  - avg_utilization_pct/p95/max_utilization_pct werden zwar auf 2 Stellen gerundet (:1096-1098), aber avg_used_slots/avg_listener_count/avg_ready_listeners (avg_i) bleiben ungerundete f64 mit voller Genauigkeit.
- **Divergenz:** Diese drei Felder erscheinen im DB-only-EventSub-Block mit langen Dezimalbrüchen statt der in Python erwarteten 2 Nachkommastellen. Reine Darstellungs-/Wertgenauigkeit, kein Logikbruch.
- **Fix:** avg_i-Ergebnisse für die drei Felder wie die Utilization-Felder via (x*100.0).round()/100.0 auf 2 Dezimalstellen runden.
- **Verify-Fix:** In stats_native.rs die drei avg_i-Aufrufe in den 2-Stellen-Rundungs-Idiom wickeln, analog zu Z.1096-1098: Z.1099 `"avg_used_slots": (avg_i(&used_vals) * 100.0).round() / 100.0`, Z.1101 `"avg_listener_count": (avg_i(&listener_vals) * 100.0).round() / 100.0`, Z.1103 `"avg_ready_listeners": (avg_i(&ready_vals) * 100.0).round() / 100.0`. Alternativ einen Helfer `round2(x: f64) -> f64` einführen und auf alle gerundeten Felder anwenden (DRY), damit die Rundungsregel an einer Stelle steht.

### [mon-announce] s()-Helfer trimmt Konfig-Strings nicht, Mode-Felder mit Whitespace verfehlen die Vergleiche
*class:* Default-/Coercion-Drift · *confidence:* 0.83 · *id:* mon-announce-3

- **Python** bot/live_announce/template.py:23-27 (_coerce_str strippt), :64/:99/:132/:134/:210 (mode = _coerce_str(...).lower())
  - _coerce_str strippt zuerst und gibt den getrimmten Wert zurueck, danach .lower(). Konfigwert ' Twitch ' wird zu 'twitch' und matcht (icon_mode == 'twitch', mode == 'custom').
- **Rust** rust/crates/tb-monitoring/src/announce/template.rs:25-38 (s() gibt ungetrimmtes text.clone() zurueck), :159/:164/:172/:174/:179/:184
  - s() prueft auf trimmed.is_empty(), gibt aber bei Nicht-Leere das ungetrimmte Original text.clone() zurueck. ' Twitch ' wird via s().to_lowercase() zu ' twitch ', Vergleich gegen 'twitch' schlaegt fehl, Fallback-Zweig (kein Icon / kein Custom-Bild / Default-Modus / kein Footer-Timestamp).
- **Divergenz:** Bei Konfigwerten mit fuehrendem/abschliessendem Whitespace in author_icon_mode, footer_icon_mode, footer_timestamp_mode, thumbnail_mode, image_mode oder description_mode rendert Rust den falschen Pfad. Auch Template-Strings behalten den Whitespace, den Python wegtrimmt.
- **Fix:** s() so aendern, dass es den getrimmten Wert (trimmed.to_string()) statt text.clone() zurueckgibt, passend zu Python _coerce_str.
- **Verify-Fix:** In `s()` (template.rs:25-38) bei Nicht-Leere den getrimmten Wert zurückgeben statt des Originals, damit die Python-`_coerce_str`-Semantik (strip-then-return) exakt nachgebildet wird:

```rust
Some(Value::String(text)) => {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()  // statt text.clone()
    }
}
```

Damit matchen Mode-Felder mit umgebendem Whitespace wieder (`' Twitch '` → `'twitch'`) und Template-Strings verhalten sich wie in Python. Für `Some(other)` (Number/Bool als String via `other.to_string()`) ist kein Whitespace zu erwarten, kann aber konsistenzhalber ebenfalls getrimmt werden. Einen Unit-Test mit `" Twitch "`-Konfig ergänzen, der auf das gesetzte Author-Icon prüft.

### [mon-eventsub] [Fehlende Guards] Fehlender Empty-broadcaster_id-Drop: Rust verarbeitet Notifications, die Python verwirft
*class:*  · *confidence:* 0.7 · *id:* mon-eventsub-3

- **Python** bot/monitoring/eventsub_webhook.py:711-717; eventsub_mixin.py:1197-1199/1219-1221
  - Vor jedem Callback bricht Python ab, wenn kein broadcaster_id aufloesbar ist (kein Insert/Enqueue); Enqueue wirft zusaetzlich ValueError bei leerem bid.
- **Rust** rust/crates/tb-monitoring/src/dispatch.rs:217-263
  - Der Dispatcher routet unabhaengig von broadcaster_id: CORE-Enqueue mit leerem id, Telemetrie-Insert mit leerem twitch_user_id, channel.moderate-Hook mit leerem id. Nur handle_stream_offline hat den Empty-Guard.
- **Divergenz:** Notifications ohne broadcaster_id erzeugen in Rust leere/ungueltige DB-Rows bzw. unnoetige Inbox-Arbeit, waehrend Python sie verwirft. Selten, aber dauerhaft abweichend.
- **Fix:** In route() vor CORE-Enqueue/Telemetrie/channel.moderate ein Empty-Check auf context.broadcaster_id ergaenzen, analog zu handle_stream_offline.
- **Verify-Fix:** In EventSubDispatcher::route() (dispatch.rs:217ff) vor dem Routing einen Empty-Guard analog zu Python einziehen: wenn `context.broadcaster_id.trim().is_empty()`, dann debug-loggen und mit `Ok(DispatchOutcome::new(sub_type))` (verworfen, kein queued/processed) zurückkehren — bevor CORE-Enqueue, store_telemetry oder die channel.moderate/raid/chat-Hooks aufgerufen werden. Damit fällt der Drop an einer zentralen Stelle für alle Pfade (inkl. der CORE-Typen stream.online/stream.offline/channel.update) an, statt die in Python vorhandene zweite Enqueue-Validierung (ValueError) einzeln nachzubauen. Test ergänzen: Notification ohne broadcaster_user_id → outcome weder queued noch processed, kein Insert.

### [mon-eventsub] [Fehlende Guards/Sicherheit] Nativer Webhook-Receiver prueft Timestamp-Alter nicht (Replay-Fenster ueber 600s)
*class:*  · *confidence:* 0.75 · *id:* mon-eventsub-4

- **Python** bot/monitoring/eventsub_webhook.py:168-176 + 563-570
  - Python lehnt Notifications ab, deren Twitch-Timestamp aelter als 600s (oder mehr als 600s in der Zukunft) ist - unabhaengig vom Message-ID-Dedup.
- **Rust** rust/crates/tb-monitoring/src/webhook_receiver.rs:111-141; Dedup-TTL 600s in dispatch.rs:23,179-195
  - Der native Receiver verifiziert nur die Signatur und verlaesst sich danach allein auf den Message-ID-Guard (TTL 600s, danach per sweep_expired geloescht). Kein Timestamp-Alters-Check.
- **Divergenz:** Nach Ablauf der 600s-Guard-Row ist ein abgefangener, gueltig signierter Payload wieder einspielbar und wird voll verarbeitet; Python wuerde ihn ueber das Timestamp-Alter weiter ablehnen.
- **Fix:** In handle_callback den Timestamp parsen und bei Abstand ueber 600s mit 403 ablehnen (Port von _is_message_too_old), bevor dispatch() laeuft.
- **Verify-Fix:** In `webhook_receiver.rs::handle_callback` nach der Signaturprüfung (vor dem `serde_json::from_slice`) einen Timestamp-Alterscheck einziehen, der das Python-Verhalten spiegelt: den `twitch-eventsub-message-timestamp`-Header per `DateTime::parse_from_rfc3339` parsen, das Alter gegen `now()` bilden und bei `age.abs() > MESSAGE_DEDUP_TTL_SECONDS` (sowie bei Parse-Fehler) mit `StatusCode::FORBIDDEN` ablehnen. Das schließt die Replay-Lücke nach Guard-Ablauf unabhängig von der TTL und stellt Parität zu `_is_message_too_old` her. Zusätzlich den "Sicherheit"-Doc-Kommentar (Z.12-21) entsprechend ergänzen.

### [mon-poll] Poll-getriebene Partner-Score-Refreshes werden verworfen (after_tick im wired Hook nicht überschrieben)
*class:* Vergessene Seiteneffekte · *confidence:* 0.4 · *id:* mon-poll-3

- **Python** bot/monitoring/monitoring.py:1441-1444 + 1778-1782 (partner_score_refreshes für poll_stream_restarted/online/offline) + 1790 (_schedule_partner_raid_score_refreshes)
  - Am Ende von _process_postings werden die im Tick gesammelten Score-Refreshes (Trigger poll_stream_online/offline/restarted) via _schedule_partner_raid_score_refreshes tatsächlich eingeplant — ein Reconciliation-Backstop für Raid-Scores.
- **Rust** rust/crates/tb-monitoring/src/poller/engine.rs:244,272-277 (refreshes → TickReport.score_refreshes → after_tick) ; rust/bin/tb-bot/src/main.rs:91-98 (SubscriptionPollHooks überschreibt NUR on_stream_went_live) ; rust/crates/tb-monitoring/src/poller/hooks.rs:126 (after_tick Default = Noop)
  - process_entries sammelt die Refreshes korrekt in TickReport.score_refreshes, übergibt sie an after_tick. Der einzige produktiv verdrahtete PollHooks-Impl (SubscriptionPollHooks) überschreibt after_tick aber nicht, also greift die Noop-Default-Methode — die Refreshes verpuffen.
- **Divergenz:** Die poll-getriebenen Score-Refreshes finden in Rust nicht statt. online/offline sind durch native EventSub-Score-Refreshes weitgehend abgedeckt; ohne Entsprechung bleibt nur poll_stream_restarted (Stream-ID-Wechsel ohne offline/online). Niedrige Severity/Confidence, weil after_tick denselben Kanal trägt wie die laut Cutover-Plan bewusst aufgeschobene Partner-Rekrutierung — der Drop könnte mit beabsichtigt sein; im Plan ist aber nur Rekrutierung/Archiv explizit als 'pausiert/no-op' benannt, die Score-Refreshes nicht.
- **Fix:** Klären, ob der poll-Score-Refresh bewusst aufgeschoben ist. Falls nicht: in SubscriptionPollHooks after_tick implementieren und report.score_refreshes über den vorhandenen ScoreRefreshResolver/EventSub-on_score_refresh-Pfad einplanen.
- **Verify-Fix:** `after_tick` im verdrahteten PollHooks-Impl überschreiben, sodass `TickReport.score_refreshes` an den bereits vorhandenen `ScoreRefreshResolver` weitergereicht wird. Konkret: `SubscriptionPollHooks` (main.rs:87-98) um eine `ScoreRefreshResolver`-Referenz erweitern und `after_tick` implementieren, das für jeden `ScoreRefresh` aus `report.score_refreshes` `resolver.refresh_scores(&[(twitch_user_id, login)], Utc::now())` aufruft (analog zu `on_score_refresh` in eventsub_hooks.rs:280). Da online/offline bereits via EventSub gedeckt sind, genügt minimal, mindestens den Trigger `poll_stream_restarted` durchzureichen; sauberer ist, alle drei durchzureichen (die EventSub-Dedup/Guard-Logik verhindert Doppel-Berechnung). Mit einem Test absichern (vgl. tests/poller.rs:187-188, der `score_refreshes` bereits im Report prüft), der zusätzlich verifiziert, dass der Resolver bei einem Restart-Tick aufgerufen wird.

### [mon-sessions] int_field bricht Pythons `a or b or 0`-Fallback-Kette bei führender 0
*class:* None/Option-Semantik (Fallback-Kette) · *confidence:* 0.85 · *id:* mon-sessions-1

- **Python** bot/analytics/mixin.py:1912 (cumulative_months), :1925 (total_gifted default), :2103 (total_progress)
  - Python verkettet mehrere Keys mit `or`, z.B. `int(event.get('cumulative_months') or event.get('months') or 0) or None`. Da `0` in Python falsy ist, wird beim ersten Key mit Wert 0 auf den nächsten Key (months) weitergeschaltet; erst wenn alle 0/leer sind, ist das Ergebnis None.
- **Rust** rust/crates/tb-monitoring/src/telemetry.rs:482-496 (int_field), :69 (cumulative_months), :79 (total_gifted), :220 (total_progress)
  - int_field iteriert die Keys, gibt aber beim ERSTEN vorhandenen Key sofort `Some(value as i32)` zurück — auch wenn der Wert 0 ist (kein Filter im Loop). Damit endet die Kette bei einer führenden 0. Das nachgelagerte `.filter(|v| *v != 0)` macht aus dieser 0 dann None, anstatt den zweiten Key zu probieren.
- **Divergenz:** Beispiel cumulative_months=0, months=5: Python liefert 5, Rust liefert None. Ebenso total=0/gift_total=10 (total_gifted) und total=0/total_progress=7 (Hype-Train). Die Analytics-Zahlen (kumulierte Sub-Monate, Gift-Gesamtmenge, Hype-Train-Fortschritt) werden in diesen Fällen NULL statt des korrekten zweiten Werts.
- **Fix:** In int_field beim Wert 0 NICHT sofort zurückkehren, sondern (wie Pythons `or`) weitersuchen — z.B. den ersten von 0 verschiedenen Wert nehmen und nur dann 0/None liefern, wenn alle Keys 0/leer/fehlend sind. Alternativ pro Aufrufstelle die Key-Reihenfolge mit `.filter(|v|*v!=0).or_else(...)` pro Einzelkey nachbauen.
- **Verify-Fix:** Den `!= 0`-Filter in die Fallback-Iteration ziehen, damit eine führende 0 wie in Python zum nächsten Key durchschaltet. Konkret: entweder einen Helfer `int_field_nonzero(event, keys)` einführen, der im Loop `if v != 0 { return Some(v) }` macht und 0 überspringt, und die drei Call-Sites (:70, :79, :220) sowie ggf. analoge Stellen mit `.filter(|v| *v != 0)` darauf umstellen; oder im bestehenden `int_field` einen Parameter ergänzen, der 0-Werte als Nicht-Treffer behandelt. Wichtig: NUR die Stellen umstellen, deren Python-Pendant tatsächlich `a or b or 0`-Mehrkey-Fallback nutzt (cumulative_months, total_gifted-default/batch_total, total_progress) — Einzelkey-Felder mit nachgelagertem Filter (z.B. streak_months, level, duration_seconds, reward_cost) sind bereits korrekt und dürfen nicht angefasst werden. Anschließend einen Test ergänzen, der `cumulative_months=0,months=5 -> Some(5)` abdeckt (fehlt im aktuellen feld_extraktion_mit_fallbacks-Test).

### [mon-sessions] cumulative_total-Gift: total=0 wird 0 statt Python-Default 1
*class:* Arithmetik/Default-Drift · *confidence:* 0.8 · *id:* mon-sessions-2

- **Python** bot/analytics/mixin.py:1922-1923
  - Im Zweig gift_total_kind=='cumulative_total' macht Python `total_gifted = int(event.get('total') or 1)` — bei total=0 oder fehlend ergibt das 1.
- **Rust** rust/crates/tb-monitoring/src/telemetry.rs:78
  - Rust: `'cumulative_total' => Some(int_field(event, &['total']).unwrap_or(1))`. int_field liefert für ein vorhandenes total=0 `Some(0)`, sodass unwrap_or(1) nicht greift → Ergebnis 0.
- **Divergenz:** Bei einem cumulative_total-Gift-Event mit total=0 schreibt Rust total_gifted=0, Python schreibt 1. Seltener Edge-Case (EventSub liefert hier normalerweise >=1), daher low.
- **Fix:** Pythons `or 1`-Truthiness nachbilden: `int_field(event,&['total']).filter(|v|*v!=0).unwrap_or(1)` und in Some() wickeln.

### [mon-sessions] ad_break is_automatic: nicht-boolescher Wert wird in Rust true statt Python-Truthiness
*class:* None/Option-Semantik (Coercion) · *confidence:* 0.7 · *id:* mon-sessions-3

- **Python** bot/analytics/mixin.py:1986-1988
  - `is_automatic = bool(is_automatic_raw) if is_automatic_raw is not None else False`. Für vorhandene Nicht-Bool-Werte gilt Python-Truthiness: 0, '' , [], {} → False; alles andere True.
- **Rust** rust/crates/tb-monitoring/src/telemetry.rs:116-119
  - `event.get('is_automatic').map(|v| v.as_bool().unwrap_or_else(|| !v.is_null())).unwrap_or(false)`. Für einen vorhandenen Nicht-Bool-Nicht-Null-Wert (z.B. JSON 0 oder "") ist as_bool()=None → !is_null() = true.
- **Divergenz:** Käme `is_automatic` als 0/""/[] (statt echtem Bool), schriebe Rust true, Python false. EventSub liefert hier immer einen echten Bool, daher praktisch irrelevant — low.
- **Fix:** Nur echte JSON-Booleans als Bool werten; für Nicht-Bool die Python-Truthiness abbilden (Zahl 0/leerer String/leeres Array → false). Bei reinem Bool-Feld genügt `as_bool().unwrap_or(false)`.
- **Verify-Fix:** Falls Defensiv-Parität gewünscht: in telemetry.rs die is_automatic-Ableitung an Python-Truthiness angleichen, z.B. `event.get("is_automatic").map(|v| match v { Value::Bool(b) => *b, Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false), Value::String(s) => !s.is_empty(), Value::Array(a) => !a.is_empty(), Value::Object(o) => !o.is_empty(), Value::Null => false }).unwrap_or(false)`. Da EventSub hier garantiert einen echten Bool sendet, ist der Fix optional — alternativ als bekannte, harmlose Divergenz dokumentieren und nicht anfassen (geringerer Aufwand, kein realer Impact).

### [mon-sessions] Hype-Train-End-Update kann bei fehlendem started_at NULL-Rows matchen
*class:* SQL-/Default-Drift · *confidence:* 0.6 · *id:* mon-sessions-4

- **Python** bot/analytics/mixin.py:2132-2133 (WHERE started_at = %s)
  - Beim End-Event matcht Python `WHERE twitch_user_id=%s AND started_at=%s AND ended_at IS NULL`. Ist started_at None, wird `started_at = NULL` gebunden — ein normaler =-Vergleich auf NULL matcht NIE, also fällt Python immer auf den INSERT zurück.
- **Rust** rust/crates/tb-monitoring/src/telemetry.rs:236 (started_at IS NOT DISTINCT FROM $6)
  - Rust nutzt `started_at IS NOT DISTINCT FROM $6` (bewusste NULL-Härtung). Ist das geparste started_at None, matcht `IS NOT DISTINCT FROM NULL` genau die Rows mit started_at IS NULL und ended_at IS NULL — und aktualisiert eine davon statt zu inserten.
- **Divergenz:** Nur relevant, wenn ein Hype-Train-End-Event ohne started_at ankommt UND eine offene begin-Row mit started_at IS NULL existiert. Dann updatet Rust eine fremde/falsche Row statt einen neuen End-Eintrag zu schreiben. Hype-Train-Events tragen praktisch immer ein started_at, daher low.
- **Fix:** Bei None-started_at den UPDATE überspringen und direkt inserten (Python-Verhalten), oder die NULL-Variante explizit ausschließen: `started_at IS NOT DISTINCT FROM $6 AND $6 IS NOT NULL`.
- **Verify-Fix:** Für Python-Parität die NULL-sichere Gleichheit beim End-Update entschärfen, damit Rust bei fehlendem started_at ebenfalls keine fremde Row matcht. Konkret: das `started_at IS NOT DISTINCT FROM $6`-Prädikat nur als NULL-sichere Gleichheit nutzen, wenn ein konkreter started_at-Wert vorliegt — bei `started_at = None` den UPDATE-Versuch ganz überspringen und direkt inserten (mirror Python: `=`-Vergleich gegen NULL matcht nie). Pragmatisch z.B.: vor dem UPDATE prüfen `if started_at.is_none() { /* direkt INSERT */ }` oder das Prädikat zu `(started_at = $6)` ändern (verliert die bewusste NULL-Härtung für den seltenen Fall valider NULL-begin-Rows, stellt aber 1:1-Python-Verhalten her). Da der Befund low ist und die NULL-Härtung sonst nützlich ist, ist die saubere Variante: bei started_at None den UPDATE-Pfad nicht betreten.

### [raid-arrival] correlation_detail filled with suppression_reason instead of None
*class:* SQL/Default drift · *confidence:* 0.85 · *id:* raid-arrival-3

- **Python** bot/raid/raid_arrival_runtime.py:303
  - store_partner_raid_arrival is called with correlation_detail=None on the confirm path, the column stays NULL.
- **Rust** rust/bin/tb-bot/src/raid_arrival_wiring.rs:319
  - The adapter sets correlation_detail = decision.suppression_reason, so for suppressed_external cases partner_target_without_our_raid_confirmation lands in the column instead of NULL.
- **Divergenz:** For arrival rows with a suppression reason the stored correlation_detail diverges (text instead of NULL). Pure data drift in a diagnostic column, no control effect, permanently wrong content.
- **Fix:** Bind correlation_detail to None on the confirm path as Python does.
- **Verify-Fix:** In rust/bin/tb-bot/src/raid_arrival_wiring.rs Z.319 `correlation_detail: decision.suppression_reason.clone()` durch `correlation_detail: None` ersetzen, um den Python-Confirm-Pfad (correlation_detail=None) 1:1 zu spiegeln. Zusätzlich (separater, hier mitgefundener Drift) prüfen, ob Z.318 `correlation_status: "confirmed"` absichtlich von Python `"matched_pending"` abweicht — falls nicht beabsichtigt, ebenfalls auf "matched_pending" angleichen. Danach `cargo test -p tb-raid` und das Wiring-Build laufen lassen; ggf. einen Regressionstest ergänzen, der für ein Partner-Ziel ohne ours_to_partner correlation_detail == NULL erwartet.

### [raid-arrival] viewer_count fallback to registered_viewer_count missing on confirm path
*class:* Arithmetic/boundary · *confidence:* 0.78 · *id:* raid-arrival-4

- **Python** bot/raid/raid_arrival_runtime.py:263,298,315,345,386
  - Python computes effective_viewer_count = int(viewer_count or pending.registered_viewer_count or 0), falling back to the registered count when the signal viewer_count is falsy, used for the arrival row and score tracking.
- **Rust** rust/bin/tb-bot/src/raid_arrival_wiring.rs:314,344
  - The adapter passes the raw viewer_count through (record_arrival and ConfirmContext), no fallback to pending.registered_viewer_count although the popped pending is available. On the channel.chat.notification confirm path viewer_count is legitimately 0.
- **Divergenz:** When confirmation is triggered by a signal with viewer_count=0 (notably channel.chat.notification, which carries the event value) Rust stores 0 instead of the registered viewer count, corrupting arrival analytics and the value flowing into scoring.
- **Fix:** Compute effective_viewer_count = if viewer_count != 0 use viewer_count else pending.registered_viewer_count and use it across the confirm path.
- **Verify-Fix:** In raid_arrival_wiring.rs::confirm_pending_raid den effektiven Count vor Z. 306 berechnen: `let effective_viewer_count = if viewer_count != 0 { viewer_count } else { pending.registered_viewer_count };` (das gepoppte pending aus Z. 270-274 ist verfügbar) und diesen Wert sowohl an RecordArrivalInput.viewer_count (Z. 314) als auch an ConfirmContext.viewer_count (Z. 344) übergeben — exakte Parität mit Python `int(viewer_count or pending.registered_viewer_count or 0)`. Damit ist der Adapter robust, falls der chat.notification-Pfad in einer späteren Welle nativ verdrahtet wird oder ein channel.raid-Event ohne viewers-Feld eintrifft.

### [raid-arrival] raid_history_id and raid_history_executed_at never written into the arrival row
*class:* Forgotten side-effects / SQL default drift · *confidence:* 0.72 · *id:* raid-arrival-5

- **Python** bot/raid/raid_arrival_runtime.py:270-276,305-306
  - On should_load_recent_raid_history_reference (is_partner_raid or ours_to_partner) Python loads the most recent successful raid-history reference and writes raid_history_id plus raid_history_executed_at into the arrival row.
- **Rust** rust/bin/tb-bot/src/raid_arrival_wiring.rs:321-322
  - record_arrival binds raid_history_id and raid_history_executed_at hard to None on the confirm path. The history reference is loaded only inside ConfirmResolver for score tracking, never carried into the arrival row.
- **Divergenz:** The columns stay NULL for all Rust-confirmed partner arrivals although Python fills them. The arrival-to-executed-raid link is lost in the arrival table.
- **Fix:** When should_load_recent_raid_history_reference, load the history reference before record_arrival (reuse ConfirmResolver logic) and pass both fields into RecordArrivalInput.
- **Verify-Fix:** In rust/bin/tb-bot/src/raid_arrival_wiring.rs confirm_pending_raid, when decision.should_load_recent_raid_history_reference is true, load the reference (reuse confirm_resolver's load_raid_history_reference, or factor it into a shared helper on the store) using the from/to broadcaster pair and an upper bound of now()+10min, then bind the resulting (raid_history_id, raid_history_executed_at) into the record_arrival call at lines 321-322 instead of None/None. Mirror Python's gate: only load on is_partner_raid OR ours_to_partner. Note the Python loader uses no +10min upper bound (load_recent_raid_history_reference takes only from_login/to_id) while confirm_resolver's load_raid_history_reference does — verify which window Python's partner_arrival_tracking.py:154 actually applies and match it so the arrival row references the same history id Python would have chosen. Add an integration test asserting the arrival row's raid_history_id is non-NULL after a confirmed partner raid that has a prior successful raid-history entry.

### [raid-arrival] Recent-arrival window anchored to detected_at instead of refreshed confirmed_ts
*class:* Arithmetic/time window · *confidence:* 0.5 · *id:* raid-arrival-6

- **Python** bot/raid/raid_state_store.py:151-155,185
  - Python's cache measures the 600s recent window from confirmed_ts, updated to now on every remember_recent_raid_arrival, so the window slides.
- **Rust** rust/crates/tb-raid/src/arrival_tracking_store.rs:210-233
  - The DB replacement filters on detected_at (insert time, never updated; update_arrival only touches last_signal_at). The window is fixed to the first insert.
- **Divergenz:** For correlations spread over more than 10 minutes Python slides the window, Rust stays anchored. In practice signals fire within minutes so results match almost always; only at the window edge can Rust treat an arrival as independent earlier. Very rare edge case.
- **Fix:** Optionally filter find_recent_arrival on last_signal_at instead of detected_at; verify intended behavior first.
- **Verify-Fix:** find_recent_arrival auf das gleitende Fenster umstellen, sodass es Pythons confirmed_ts-Semantik entspricht. Konkret: das Recent-Fenster soll von der Zeit des letzten Signals (last_signal_at) statt vom Erst-Insert (detected_at) gemessen werden. In der WHERE-Klausel `detected_at > NOW() - ($3 * INTERVAL '1 second')` durch `COALESCE(last_signal_at, detected_at) > NOW() - ($3 * INTERVAL '1 second')` ersetzen (und ORDER BY entsprechend auf COALESCE(last_signal_at, detected_at) DESC). Da update_arrival und mark_unraid last_signal_at bereits auf NOW() setzen, schiebt jedes Sekundär-Signal das Fenster dann genau wie Pythons confirmed_ts-Refresh. Den irreführenden Doc-Kommentar (Z.206-209) zugleich präzisieren: er behauptet Paritaet, die ohne diesen Fix nicht gegeben ist.

### [raid-auth] Refresher prüft needs_reauth/raid_enabled nicht im Re-Read unter dem Lock
*class:* Fehlende Guards/Bedingungen · *confidence:* 0.7 · *id:* raid-auth-2

- **Python** bot/raid/auth.py:1616-1636 (get_valid_token In-Lock-Recheck: raid_enabled/needs_reauth → return None)
  - Innerhalb des pg_advisory_xact_lock liest get_valid_token die Zeile erneut inkl. raid_enabled und needs_reauth und bricht mit return None ab, wenn 'not raid_enabled or needs_reauth' — fängt den Fall ab, dass ein paralleler Writer (z. B. _mark_reauth_required) zwischen Pre-Lock-Read und Lock-Erwerb die Reauth-Pflicht gesetzt hat.
- **Rust** rust/crates/tb-raid/src/token_refresher.rs:160-199 (Re-Read selektiert nur refresh_token_enc, token_expires_at)
  - Der Re-Read unterm Lock holt nur refresh_token_enc und token_expires_at. needs_reauth/raid_enabled werden NICHT erneut geprüft; der Refresh läuft durch, sofern das Token nicht frisch genug ist.
- **Divergenz:** Wird zwischen dem needs_reauth-Check im token_provider (vor dem Lock) und dem Lock-Erwerb die Reauth-Pflicht gesetzt, refresht Rust trotzdem und schreibt einen neuen Token zurück — was den gerade gesetzten needs_reauth-Lockout faktisch unterläuft. Python verhindert das im Lock. Schmales Race-Fenster, daher medium.
- **Fix:** Im Re-Read-SELECT zusätzlich needs_reauth und raid_enabled mitlesen und bei needs_reauth=TRUE oder raid_enabled=FALSE die Transaktion committen und RefreshOutcome::Skipped zurückgeben, bevor der HTTP-Refresh erfolgt.
- **Verify-Fix:** Den Re-Read in token_refresher.rs:165-170 um `raid_enabled, needs_reauth` erweitern und vor dem HTTP-Refresh prüfen (Parität zu Python): `SELECT refresh_token_enc, token_expires_at, raid_enabled, needs_reauth ...`; danach `if !raid_enabled || needs_reauth { tx.commit().await?; return Ok(RefreshOutcome::Skipped); }` direkt nach dem Decrypt/vor der Freshness-Prüfung. Damit schließt Rust das Race vollständig im Lock statt erst beim nächsten Aufruf. Niedrige Dringlichkeit wegen Admin-only-Trigger + Selbstkorrektur (needs_reauth bleibt gesetzt), aber sauberer 1:1-Port und billig umzusetzen.

### [raid-auth] clear_failure_count setzt Partner-technical_pause_reason='token_error' nicht zurück
*class:* Vergessene Seiteneffekte · *confidence:* 0.6 · *id:* raid-auth-3

- **Python** bot/api/token_error_handler.py:846-869 (clear_failure_count: UPDATE twitch_partners ... technical_pause_reason CASE + DELETE blacklist)
  - Nach erfolgreichem Refresh löscht clear_failure_count nicht nur den Blacklist-Eintrag, sondern setzt vorher in twitch_partners technical_pause_reason von 'token_error' zurück auf NULL (CASE WHEN ... ='token_error' THEN NULL).
- **Rust** rust/crates/tb-raid/src/token_blacklist.rs:103-112 (clear_failure_count: nur DELETE FROM twitch_token_blacklist)
  - clear_failure_count führt ausschließlich DELETE FROM twitch_token_blacklist aus. Der Partner-Pause-Grund 'token_error' bleibt stehen.
- **Divergenz:** Ein Partner, der wegen Token-Fehler auf technical_pause_reason='token_error' stand, bleibt in Rust nach erfolgreichem Refresh weiter pausiert, weil der Reset fehlt. Gehört teilweise zur bewusst auf 6b+ deferierten Partner-Verdrahtung (auth_writer-Doku), daher low — aber bei aktivem Partner-Store ein dauerhaft falscher Zustand.
- **Fix:** Beim Verdrahten des Partner-Stores (6b+) den technical_pause_reason='token_error'→NULL-Reset in den clear_failure_count-Pfad mit aufnehmen, analog zum Blacklist-needs_reauth-Fix.
- **Verify-Fix:** In tb-raid/src/token_blacklist.rs clear_failure_count vor dem DELETE den Partner-Pause-Reset ergänzen, 1:1 wie Python und idealerweise in einer Transaktion: `UPDATE twitch_partners SET technical_pause_reason = CASE WHEN LOWER(COALESCE(technical_pause_reason,'')) = 'token_error' THEN NULL ELSE technical_pause_reason END WHERE twitch_user_id = $1`, danach das bestehende DELETE. Beide Statements in eine pool.begin()/commit-Transaktion fassen (wie der Python `transaction()`-Block), damit Blacklist-Löschung und Pause-Reset atomar sind. Test ergänzen: Partner mit technical_pause_reason='token_error' + Blacklist-Eintrag → nach clear_failure_count beide weg; Partner mit technical_pause_reason='blocked' bleibt unverändert. Modul-Doc-Kommentar von 'Blacklist-Teil' / '1:1 zu Python' an die erweiterte Semantik anpassen.

### [raid-candidate] is_recent_deadlock truncation am 360s-Grenzwert
*class:* Arithmetik/Grenzwerte · *confidence:* 0.85 · *id:* raid-candidate-1

- **Python** bot/raid/services/raid_data_sources.py:97
  - total_seconds() Float: 360,7 <= 360 -> False (nicht recent)
- **Rust** rust/crates/tb-raid/src/eligibility.rs:35
  - num_seconds() trunkiert: 360,7 -> 360, also 360 <= 360 -> True (recent)
- **Divergenz:** Sub-Sekunden-Fenster genau am 360s-Limit: Stream in Rust eligible, in Python nicht. Betrifft Quell-Eligibility und Just-Chatting-Partnerklassifizierung.
- **Fix:** Bruchteile beruecksichtigen: (now - dt).num_milliseconds() as f64 / 1000.0 <= cap_seconds as f64
- **Verify-Fix:** Rust an die Float-Semantik von Python angleichen, damit die Grenze identisch ist. In `is_recent_deadlock` (eligibility.rs:35) statt `num_seconds()` die Millisekunden-/Float-Differenz verwenden, z. B. `(now - dt).num_milliseconds() <= cap_seconds * 1000` (Cap in ms ausdrücken) oder `(now - dt).to_std().map(|d| d.as_secs_f64()).unwrap_or(f64::INFINITY) <= cap_seconds as f64`. Damit fällt eine reale Differenz von 360.7 s wie in Python aus dem Fenster. Hinweis: bei negativen Differenzen (Uhr-Drift / zukünftiger Timestamp) ist `to_std()` Err → mit `unwrap_or(f64::INFINITY)`/explizitem Vorzeichen-Check absichern, sonst Verhaltensumkehr. Einen Grenzfall-Test bei 360.0/360.5/361.0 s ergänzen, der vor und nach dem Fix Python-Parität beweist.

### [raid-candidate] select_by_score daily_cap_filtered >0 im All-over-Cap-Fallback (nur Reason)
*class:* Arithmetik-Grenzwert · *confidence:* 0.9 · *id:* raid-candidate-2

- **Python** bot/raid/services/candidate_selection.py:285
  - daily_cap_filtered = len(scored)-len(pool); bei allen ueber Cap faellt pool auf scored zurueck -> 0 -> Reason highest_final_score
- **Rust** rust/crates/tb-raid/src/candidate_selection.rs:173
  - daily_cap_filtered VOR Fallback berechnet; bei leerem under_cap bleibt es = candidates.len() (>0) -> DailyCap-Reason-Varianten
- **Divergenz:** Ausgewaehltes Ziel identisch, nur geloggter selection_reason weicht ab. Reason fliesst nur ins Log (auto_raid_pipeline.rs:349), nicht in DB-History -> kosmetisch
- **Fix:** daily_cap_filtered NACH Fallback als candidates.len()-pool.len() berechnen
- **Verify-Fix:** In Rust `daily_cap_filtered` semantisch an Python angleichen: den Wert NACH dem Fallback bestimmen, damit der All-over-Cap-Fall denselben Reason ("highest_final_score" statt "daily_raid_soft_cap") liefert. Konkret in candidate_selection.rs select_by_score den `pool` zuerst bilden und dann `let daily_cap_filtered = candidates.len() - pool.len();` berechnen — bei leerem under_cap wird pool = alle Kandidaten, also daily_cap_filtered = 0. Da die Auswirkung nur das Log-Label betrifft (kein DB-/Zielunterschied) ist der Fix optional, aber billig und stellt die im Doc-Kommentar zugesicherte Python-Parität wieder her. Alternativ den Doc-Kommentar (Z.147 "exakt wie Python") um die bewusste Abweichung ergänzen, falls der frühe Wert gewünscht ist.

### [raid-partner-setup] Modul-Header behauptet bewusste Python-Abweichung, die im aktuellen Code nicht existiert (Doku-Drift, kein Runtime-Bug)
*class:* toter Code · *confidence:* 0.83 · *id:* raid-partner-setup-1

- **Python** bot/storage/partner_registry.py:824-836,872-898
  - Das aktuelle Python `promote_streamer_to_partner` liest die Partner-Felder (require_discord_link, silent_ban, silent_raid, live_ping_role_id, live_ping_enabled, raid_bot_enabled, manual_partner_opt_out) bei UNSET-Kwargs bereits aus `active_row` (Z. 831-898, Kommentar Z. 824-829 belegt den dortigen Fix), NICHT aus den Quell-Literalen. Eine Re-Auth wipet die Partner-Einstellungen also NICHT.
- **Rust** rust/crates/tb-raid/src/partner_setup.rs:21-32,500-525
  - Der Rust-Port liest dieselben Felder aus `active` (Z. 503-525) und stimmt damit exakt mit dem aktuellen Python überein. Der Modul-Header (Z. 21-32) beschreibt jedoch eine angebliche `bewusste Abweichung von Python (Bugfix)`, derzufolge Python wipe und Rust das `bewusst anders` mache — diese Darstellung ist veraltet, weil Python denselben Fix längst enthält. Es gibt keinen Verhaltensunterschied.
- **Divergenz:** Reine Dokumentations-Drift: Die im Header beschriebene Divergenz existiert im Code nicht (mehr). Das Verhalten beider Implementierungen ist identisch. Risiko nur, dass ein späterer Leser den Header für bare Münze nimmt und das Rust-Verhalten `zurück auf Python` ändert (was tatsächlich identisch ist) oder den vermeintlichen Wipe in Python `fixt` und dabei etwas kaputtmacht.
- **Fix:** Modul-Header Z. 21-32 aktualisieren: Beide Implementierungen fallen bei UNSET auf den aktiven Partner-Wert zurück (Erst-Promotion ohne aktive Zeile → bool_int-Defaults 0/0/0/NULL/1). Der `Bugfix gegenüber Python`-Framing entfernen, da Python denselben Stand hat.
- **Verify-Fix:** Den Modul-Header in rust/crates/tb-raid/src/partner_setup.rs (Z. 21-32) umschreiben: Statt "Bewusste Abweichung von Python (Bugfix, 12.6.)" klarstellen, dass Python denselben Fix seit #151 (5df4388) enthält und beide Implementierungen die UNSET-Felder (require_discord_link/silent_ban/silent_raid/live_ping_role_id/live_ping_enabled) auf den aktiven Partner-Datensatz zurückfallen lassen — also Verhaltensgleichheit, keine Divergenz. Den Inline-Kommentar Z. 500-502, der auf "den im Modul-Header dokumentierten Bugfix" verweist, entsprechend anpassen (auf "active-Fallback bewahrt Einstellungen, identisch zu Python ab #151"). Keine Code-Änderung nötig — nur Doku.

### [raid-pipeline] Boost-Tie-Break: fehlendes started_at sortiert in Rust ans Ende statt an den Anfang (invertierte Tie-Break-Reihenfolge ggü. Python)
*class:* SQL-/Default-Drift (Sentinel statt Leerstring) · *confidence:* 0.6 · *id:* raid-pipeline-2

- **Python** bot/raid/raid_pipeline.py:167-170 (boost_matches.sort(key=lambda s: (int(viewer_count or 0), str(started_at or ""))))
  - Im Outreach-Boost-Pfad wird bei gleichem viewer_count als zweites Sortierkriterium str(started_at or "") genutzt; ein fehlendes/leeres started_at wird zu "" und sortiert damit als Kleinstes an den Anfang (= würde gewählt).
- **Rust** rust/crates/tb-raid/src/target_resolution.rs:215-217 (sort_by (viewer_count, started_at.as_str())); rust/bin/tb-bot/src/raid_adapters.rs:52-54 (started_at → STARTED_AT_SENTINEL '9999-99-99' bei leer)
  - Der Fallback-Adapter ersetzt leeres started_at durch den Sentinel '9999-99-99'; in resolve_boost_target wird nach (viewer_count, started_at) sortiert, der Sentinel ist das Größte und sortiert ans Ende. Bei viewer_count-Gleichstand gewinnt also der gegenteilige Kandidat.
- **Divergenz:** Nur relevant, wenn zwei Boost-Kandidaten exakt gleichen viewer_count haben UND mindestens einer kein started_at trägt. Live-Kategorie-Streams liefern started_at praktisch immer, daher sehr seltener Edge-Case; betrifft nur welcher von zwei gleichwertigen Empfängern den Boost-Raid bekommt.
- **Fix:** Im Boost-Pfad fehlendes started_at wie Python als Leerstring behandeln (z. B. Sentinel vor dem Vergleich auf "" mappen) oder im Adapter für den Boost-Pfad keinen Sentinel setzen. Alternativ den Sentinel-Vergleich in der Boost-Sortierung explizit als 'leer' behandeln.
- **Verify-Fix:** Tie-Break-Richtung an Python angleichen, sodass fehlendes started_at an den ANFANG sortiert (= bevorzugt gewählt wird), oder zumindest die widersprüchliche Doku korrigieren. Zwei saubere Optionen:

Option A (Verhalten angleichen, empfohlen): In resolve_boost_target (und konsistent in resolve_fallback_target) fehlendes/leeres started_at als kleinsten Sortierschlüssel behandeln statt als Sentinel. Z.B. FairnessCandidate.started_at als Option<String> führen und im sort_by Some/None so abbilden, dass None vor Some sortiert; oder direkt einen leeren String als Schlüssel verwenden statt "9999-99-99". Konkret den Adapter `to_fairness_candidate` (raid_adapters.rs:52-54) so ändern, dass leeres started_at als "" durchgereicht wird, und die `cmp`-Logik nutzt "" als kleinsten Wert — exakt wie Pythons `str(started_at or "")`.

Option B (falls Sentinel bewusst beibehalten werden soll): Den irreführenden Kommentar (target_resolution.rs:27 und raid_adapters.rs:13 "sortiert ans Ende, wie Python") korrigieren, da Python das Gegenteil tut, und die Abweichung in 05-cleanup-decisions.md als bewusste Entscheidung dokumentieren.

Zur Absicherung in beiden Fällen einen Unit-Test ergänzen: zwei Boost-Kandidaten mit gleichem viewer_count, einer ohne started_at — und asserten, dass derselbe gewählt wird wie im Python-Pfad.

### [raid-scoring] ConfirmResolver schreibt bei fehlendem Score-Cache NULL-Scores statt der Python-Defaults (0.0/0.5/1.0)
*class:* None/Option-Semantik / Default-Drift · *confidence:* 0.8 · *id:* raid-scoring-2

- **Python** bot/raid/partner_raid_score_tracking.py:122-135,390-393,467-476
  - track_confirmed_partner_raid baut score_payload immer über _score_payload(...). Liegt kein Cache/Snapshot vor (leeres Dict), liefert _score_payload trotzdem konkrete Zahlen: final_score=0.0, base_score=0.0, duration_score=0.5, time_pattern_score=0.5, readiness_score=0.5, fairness_score=0.5, new_partner_multiplier=1.0, raid_boost_multiplier=1.0, today_received_raids=0, score_last_computed_at=None. Diese Werte landen in der Tracking-Zeile.
- **Rust** rust/bin/tb-bot/src/confirm_resolver.rs:94,113-122
  - In confirm_resolver.resolve() ist snapshot = score_store.load(...). Fehlt die Score-Zeile (snapshot=None), werden ALLE Score-Felder per snapshot.as_ref().map(...) zu None und damit als SQL NULL in twitch_partner_raid_score_tracking geschrieben (final_score/base_score/duration_score/time_pattern_score/readiness_score/fairness_score/new_partner_multiplier/raid_boost_multiplier/today_received_raids).
- **Divergenz:** Ein bestätigter Partner-Raid auf einen Partner ohne berechnete Score-Cache-Zeile erzeugt in Rust eine Tracking-Zeile mit NULL-Scores, in Python mit den neutralen Defaults (0.0 für final/base, 0.5 für die Komponenten, 1.0 für die Multiplikatoren, 0 für today_received_raids). Spätere Auswertungen/Aggregationen über diese Spalten (AVG/SUM/Vergleiche) sehen NULL statt definierter Werte — Drift in den gespeicherten Score-Snapshots der Raids.
- **Fix:** Beim Bauen von TrackConfirmedInput im None-Fall die Python-Defaults einsetzen statt None weiterzureichen: final_score/base_score=0.0, duration/time/readiness/fairness_score=0.5, new_partner/raid_boost_multiplier=1.0, today_received_raids=0 (z. B. unwrap_or(default) je Feld). score_last_computed_at darf None bleiben (entspricht Python).
- **Verify-Fix:** In confirm_resolver.rs:113-122 die Python-Defaults nachbilden, statt bei snapshot=None pauschal None zu schreiben. Konkret: die Score-Felder NICHT via `snapshot.as_ref().map(...)` (was None propagiert), sondern mit `unwrap_or`-Defaults belegen, die `_score_payload`-Defaults entsprechen: final_score=0.0, base_score=0.0, duration_score=0.5, time_pattern_score=0.5, readiness_score=0.5, fairness_score=0.5, new_partner_multiplier=1.0, raid_boost_multiplier=1.0, today_received_raids=0; score_last_computed_at bleibt None (entspricht Python). Beispiel: `final_score: Some(snapshot.as_ref().map(|s| s.final_score).unwrap_or(0.0))` bzw. die Felder von Option<f64> auf f64 in TrackConfirmedInput umstellen, falls NULL fachlich nie gewollt ist. Danach einen Integrationstest ergänzen, der resolve() ohne twitch_partner_raid_scores-Zeile aufruft und die neutralen Defaults (0.0/0.5/1.0/0) in der Tracking-Zeile prüft — analog zum bestehenden Test resolve_zieht_session_deadlock_score_und_history.

### [raid-scoring] ConfirmResolver nutzt gespeicherte readiness/fairness-Spalten, Python leitet sie aus duration/time/base neu ab
*class:* SQL-/Berechnungs-Drift · *confidence:* 0.5 · *id:* raid-scoring-3

- **Python** bot/raid/partner_raid_score_tracking.py:183-211,102-115
  - Im Cache-Fallback (_load_cached_score_snapshot) selektiert Python NICHT readiness_score/fairness_score aus der DB. Es liest nur final/base/duration/time_pattern (+ Multiplikatoren) und LEITET readiness_score = clamp(duration*0.6 + time*0.4) sowie fairness_score = clamp((base - readiness*0.65)/0.35) neu ab.
- **Rust** rust/bin/tb-bot/src/confirm_resolver.rs:94,118-119; rust/crates/tb-raid/src/score_store.rs:118-140
  - ConfirmResolver lädt über ScoreStore::load die Zeile inkl. der GESPEICHERTEN Spalten readiness_score und fairness_score und übernimmt sie 1:1 in TrackConfirmedInput (snapshot.readiness_score / snapshot.fairness_score).
- **Divergenz:** Python ignoriert die persistierten readiness/fairness-Werte und rekonstruiert sie algebraisch; Rust vertraut den gespeicherten Spalten. Da base = round(readiness*0.65 + fairness*0.35) aus genau diesen Komponenten gebildet wurde, ist die Rekonstruktion in aller Regel bis auf 6.-Dezimal-Rundung und das zusätzliche Python-clamp identisch — eine messbare Abweichung entsteht nur, wenn die gespeicherten Werte inkonsistent zu base wären oder das clamp greift. Praktischer Wertunterschied < 1e-5, daher niedrige Priorität, aber struktureller Logik-Unterschied.
- **Fix:** Für exakte Python-Parität im ConfirmResolver readiness/fairness analog ableiten (clamp(duration*0.6+time*0.4) bzw. clamp((base-readiness*0.65)/0.35)) statt die gespeicherten Spalten zu übernehmen. Andernfalls bewusst als akzeptierte Modernisierung dokumentieren.
- **Verify-Fix:** Verhalten angleichen, am einfachsten auf Rust-Seite, da der Fix lokal in ConfirmResolver bleibt und keine zweite Codepfad-Quelle berührt: In confirm_resolver.rs:118-119 readiness/fairness NICHT aus den gespeicherten Spalten übernehmen, sondern wie Python aus den geladenen Komponenten neu ableiten — `readiness = clamp01(duration_score*0.6 + time_pattern_score*0.4)`, `fairness = if 0.35>0 { clamp01((base_score - readiness*0.65)/0.35) } else { 0.5 }`. Damit ist tb-bot bit-genau zum Python-Tracking. Alternativ (sauberer, aber breiter): Python so umstellen, dass es die gespeicherten kanonischen Spalten liest — das berührt aber load_prepared_partner_scores UND _load_cached_score_snapshot UND ändert die Audit-Semantik. Da der Wert nur in eine Tracking-Tabelle fließt und die Abweichung ~1e-6 beträgt, ist der minimale Rust-seitige Angleich vorzuziehen; bei nächster Berührung des Codepfads erledigen, nicht dringlich.

### [raid-scoring] avg_duration_sec: Rust .round() (half-away) vs Python round() (banker's) bei exakten .5-Mittelwerten
*class:* Arithmetik/Rundung · *confidence:* 0.55 · *id:* raid-scoring-4

- **Python** bot/raid/partner_scores.py:680-682
  - avg_duration_sec = int(round(sum(recent_durations) / len(recent_durations))). Pythons round() rundet half-to-even (Banker's Rounding): mean 5400.5 → 5400.
- **Rust** rust/bin/tb-bot/src/score_refresh.rs:469
  - avg = (sum as f64 / len as f64).round() as i64. Rusts f64::round() rundet half-away-from-zero: mean 5400.5 → 5401.
- **Divergenz:** Wenn der Dauer-Mittelwert exakt auf einer halben Sekunde landet (z. B. zwei Sessions mit ungerader Summe), weicht avg_duration_sec um 1 Sekunde ab. Effekt auf duration_score = clamp((avg-uptime)/avg) ist ~1/avg (≈ 0.0001) und nach der Score-Pipeline praktisch unsichtbar. Echter, aber marginaler Unterschied; tritt nur bei exakten Halbwert-Mittelwerten auf. Strukturell gilt dasselbe für round_score in scoring.rs an der 6. Dezimalstelle, dort aber faktisch nie auslösbar (daher nicht separat gemeldet).
- **Fix:** Für Bit-Parität banker's rounding auf den Mittelwert nachbilden oder bewusst als vernachlässigbar dokumentieren.
- **Verify-Fix:** Für bitgenaue Python-Parität Banker's Rounding (round-half-to-even) in Rust nachbilden statt f64::round(). Konkret in compute_avg_duration (score_refresh.rs:469) den Mittelwert ganzzahlig runden: avg = (sum + len/2) ergibt half-away — falsch; stattdessen einen explizit round_half_even-Helfer verwenden, z.B. über (x).round_ties_even() (stabil in Rust ab 1.77) statt .round(): `let avg = (sum as f64 / len as f64).round_ties_even() as i64;`. Damit matcht das Verhalten Pythons round(). Alternativ, falls round_ties_even nicht verfügbar: manuell die .5-Grenze prüfen und zur geraden Zahl runden. Da der Effekt marginal ist, ist die Behebung optional/niedrig priorisiert; minimal sollte der irreführende Doc-Kommentar (score_refresh.rs:454) den Rundungsmodus-Unterschied vermerken.

### [transport] send_announcement fällt nicht auf normale Chat-Nachricht zurück (Promo geht bei Scope-/Non-401-Fehler verloren)
*class:* Vergessene Seiteneffekte / fehlender Fallback-Pfad · *confidence:* 0.82 · *id:* transport-1

- **Python** bot/chat/moderation.py:1403-1417 (+1372,1381,1417,1423,1425)
  - _send_announcement liefert bei jedem harten Fehler (Non-401, z. B. fehlendem Scope 'moderator:manage:announcements' oder anderem 4xx/5xx) bzw. nach erschöpftem 401-Retry NICHT False zurück, sondern ruft als Fallback `await self._send_chat_message(channel, text, source=source)` auf. Dadurch wird die Promo trotzdem als normale Chat-Nachricht zugestellt und _send_promo_message (promos.py:1157-1166) bekommt ok=True → _mark_promo_sent läuft, Cooldown wird verbraucht, Promo gilt als gesendet.
- **Rust** rust/crates/tb-transport-twitch/src/chat.rs:183-208 + rust/crates/tb-chat/src/moderation.rs:145-176 + rust/crates/tb-chat/src/promos.rs:818-823
  - HelixChatClient::send_announcement gibt bei Non-true/Non-401 schlicht Ok(false) zurück (chat.rs send_announcement liefert nur bool; tb-chat/moderation.rs hat keinen Fallback auf send_message). In promos.rs:818-823 wird Ok(false) als Hard-Fail behandelt: Promo wird NICHT als normale Nachricht nachgereicht, _mark_promo_sent läuft nicht, return false.
- **Divergenz:** Wenn die Announcement-API aus einem anderen Grund als 401 scheitert (fehlender Scope, vorübergehender 5xx, Channel-Settings), liefert Python die Promo als normale Chat-Message aus und markiert sie als gesendet; Rust verwirft die Promo komplett. Ergebnis für den Nutzer weicht ab (Promo erscheint nicht). Praktisch selten, weil der Bot den Announcement-Scope besitzt.
- **Fix:** In tb-chat/moderation.rs send_announcement bei endgültigem Ok(false) (bzw. nach 401-Retry) auf self.send_message(broadcaster_id, message) zurückfallen und dessen Erfolg als Ergebnis zurückgeben — analog zu Pythons _send_chat_message-Fallback. Cooldown-Markierung in promos.rs erst nach diesem kombinierten Ergebnis.
- **Verify-Fix:** Caller-seitigen Fallback in promos.rs nachziehen, wie ihn der api.rs-Trait-Doc (Z.28-30) ausdrücklich vorsieht. Zwei Optionen:

1) Minimal/verhaltensgetreu: In promos.rs:818-823 bei `!sent` einmal `self.api.send_message(channel_id, &text).await` versuchen; wenn dieser Send erfolgreich (SendOutcome::Sent), die Promo als gesendet markieren (mark_promo_sent + ggf. mark_streamer_invite_sent), sonst false. Das repliziert Pythons Verhalten 1:1. Gilt analog für die anderen Promo-/Scam-Sender (promos.rs:1113, 1471), die das Announcement-Ergebnis aktuell ganz ignorieren (`let _ = ...`).

2) Sauberer: `ChatApi::send_announcement` reicher machen (z.B. Result<AnnounceOutcome, String> mit Sent/HttpError{status,body}/Unauthorized statt blankem bool), damit der Caller 401 vs. echten Hard-Fail unterscheiden und gezielt auf send_message zurückfallen kann — entspricht der Granularität, die der Transport via SendOutcome für send_message schon hat.

Empfehlung: Variante 1 für exakte Paritätswiederherstellung; Variante 2, falls die Announcement-Fehlerbehandlung ohnehin überarbeitet wird. In jedem Fall den irreführenden Trait-Doc-Kommentar (api.rs:28-30) erst entfernen/anpassen, wenn der Caller-Fallback tatsächlich existiert.

### [transport] App-Token: kein 15-Minuten-Cooldown nach invalid-client (Rust re-attemptet jeden Helix-Call)
*class:* Fehlende Guards/Bedingungen (Backoff-Zustand fehlt) · *confidence:* 0.74 · *id:* transport-2

- **Python** bot/api/twitch_api.py:102-128,134,146-157,168-169
  - _ensure_token setzt bei einer 'invalid client'-Antwort via _block_auth einen _auth_blocked_until = now+900s und _auth_block_reason; _raise_if_auth_blocked unterdrückt danach 15 Minuten lang JEDEN weiteren Token-Abruf (sofortiger TwitchClientConfigError ohne Netz-Call). Erst bei erfolgreichem Token wird der Block zurückgesetzt (Z. 168-169).
- **Rust** rust/crates/tb-transport-twitch/src/token.rs:79-115 + rust/crates/tb-transport-twitch/src/client.rs:78-99
  - fetch_app_token gibt bei Non-Success nur TokenError::HttpStatus zurück; client.rs access_token propagiert den Fehler und versucht beim nächsten Helix-Call sofort wieder einen frischen Token-Fetch. Es gibt keinerlei blocked_until/Cooldown-Zustand im HelixClient.
- **Divergenz:** Bei dauerhaft kaputten Client-Credentials hämmert Rust pro Helix-Aufruf erneut den OAuth-Token-Endpoint (mehrere Versuche je Scout-Zyklus), während Python nach dem ersten Fehlschlag 15 Minuten Funkstille hält. Risiko: Rate-Limit/Flagging der Client-IP durch Twitch und Log-Spam. Tritt nur bei Fehlkonfiguration auf, ist aber genau der Fall, für den der Cooldown existiert.
- **Fix:** Im HelixClient ein Arc<Mutex<Option<(Instant, reason)>>> (auth_blocked_until) ergänzen: bei TokenError mit erkanntem invalid-client (is_invalid_client aus user_token.rs wiederverwenden) 900s setzen und in access_token vor dem Fetch prüfen + sofort Fehler liefern; bei Erfolg zurücksetzen.
- **Verify-Fix:** Einen In-Memory-Auth-Block-Zustand in `HelixClient` ergänzen, analog zu Pythons `_auth_blocked_until`/`_auth_block_reason`. Konkret: ein `Arc<Mutex<Option<i64>>> blocked_until` (Unix-Sekunden) im Struct. In `access_token()` zuerst prüfen: ist `blocked_until` in der Zukunft, sofort `HelixError::Token(TokenError::HttpStatus{...})` (oder eine neue Variante `AuthBlocked`) zurückgeben ohne Netz-Call. Wenn `fetch_app_token` mit `TokenError::HttpStatus` zurückkommt und der Body als „invalid client" klassifiziert (die bereits existierende `is_invalid_client`-Logik aus user_token.rs hierfür wiederverwenden/teilen), `blocked_until = unix_now() + 900` setzen. Bei erfolgreichem Token-Fetch `blocked_until = None` zurücksetzen. So entsteht Verhaltensparität mit Python (kein Hämmern des OAuth-Endpunkts, kein Log-Spam bei Fehlkonfiguration). Einen wiremock-Test ergänzen, der bei 400 „invalid client" einen zweiten `access_token()`-Aufruf ohne erneuten OAuth-POST verifiziert (`.expect(1)` auf dem Token-Mock).

### [transport] Helix-GET-Calls retrien nicht bei 5xx/Netzfehlern (Python: 3x mit Backoff)
*class:* Retry-Logik fehlt · *confidence:* 0.6 · *id:* transport-3

- **Python** bot/api/twitch_api.py:366-436 (387-397 5xx-Retry, 420-432 Netz-Retry)
  - _get versucht bis zu 3x: bei HTTP 500/502/503/504 schläft es 0.5*(n+1)s und wiederholt; bei TimeoutError/ClientError/OSError ebenso. Dadurch überstehen Streams/Channels/Search-Calls transiente Upstream-Aussetzer innerhalb desselben Aufrufs.
- **Rust** rust/crates/tb-transport-twitch/src/client.rs:104-112,201-211 + rust/crates/tb-transport-twitch/src/streams.rs:109-110,128-129,154-155
  - Die GET-Pfade (get_streams_by_logins/by_category, get_channel_information, search_category_id, get_users) gehen über check_status_and_json bzw. resp.json und liefern bei Non-2xx bzw. Netzfehler sofort einen Fehler — kein Retry, kein Backoff. Im Transport-Crate existiert keine Retry-Middleware.
- **Divergenz:** Ein einzelner 503/Timeout lässt einen Scout-Zyklus den betroffenen Fetch verlieren statt ihn intern zu wiederholen. Weitgehend kompensiert durch ABSENT_CYCLES_BEFORE_REMOVE (mehrere Fehl-Zyklen nötig, bevor ein Streamer entfernt wird) und das periodische Poll-Intervall, daher nur low.
- **Fix:** Optional reqwest-retry/reqwest-middleware mit RetryTransientMiddleware (Exponential-Backoff, max 3) am Arc<Client> im HelixClient registrieren, oder in check_status_and_json eine kleine Retry-Schleife für 500/502/503/504 + reqwest::Error::is_timeout/is_connect ergänzen.
- **Verify-Fix:** Im tb-transport-twitch-Crate eine schlanke Retry-Schicht für die Helix-GET-Pfade ergänzen, die Pythons `_get` 1:1 spiegelt: bis zu 3 Versuche, Retry nur bei HTTP {500,502,503,504} sowie bei Netz-/Timeout-Fehlern (reqwest `is_timeout()`/`is_connect()`/`is_request()`), Backoff `0.5*(n+1)`s; 4xx und erfolgreiche Antworten NICHT retrien. Saubere Umsetzung: einen privaten Helfer `send_with_retry(builder_fn)` einführen, der den `RequestBuilder` pro Versuch neu baut (RequestBuilder ist nicht clonebar nach `.send()`), und `get_streams_by_logins`/`by_category`, `get_channel_information`, `search_category_id`, `get_users` darüber leiten. Alternativ `reqwest-retry` + `RetryTransientMiddleware` mit `ExponentialBackoff` (max 3) am `ClientBuilder` registrieren — dann gilt der Retry aber auch für POST/DELETE, was vom Python-Verhalten abweicht (Python retriet POST ebenfalls, siehe Zeilen 332-351, daher tolerierbar). Mit Wiremock-Test absichern: erst 503, dann 200 → ein Resultat statt Fehler.

### [transport] get_users chunkt nicht auf 100 Logins (Python batcht, Rust schickt alle auf einmal)
*class:* Off-by-/Grenzwert / fehlendes Batching · *confidence:* 0.7 · *id:* transport-4

- **Python** bot/api/twitch_api.py:462-473 (range(0,len,100))
  - get_users iteriert die Logins in 100er-Chunks und macht je Chunk einen /users-Call (Twitch-Limit: 100 login/id-Parameter pro Request), sammelt alles in eine Map.
- **Rust** rust/crates/tb-transport-twitch/src/client.rs:178-194 + rust/crates/tb-transport-twitch/src/chat.rs:346-370
  - HelixClient::get_users baut die Query aus ALLEN Logins in einem einzigen Request (kein chunks(100)); get_users_created_at ebenso für IDs. Bei >100 Einträgen würde Twitch mit HTTP 400 antworten und der ganze Call schlägt fehl.
- **Divergenz:** Für >100 Logins/IDs bricht Rust ab, wo Python korrekt batcht. Aktuell latent: alle gefundenen Caller (clip/helix.rs, streamers.rs, moderation.rs) übergeben genau 1 Login/ID. Wird erst bei einem Multi-Login-Caller zum echten Bug.
- **Fix:** In get_users und get_users_created_at über logins.chunks(100) iterieren, je Chunk ein Request, Ergebnisse zusammenführen (wie bereits in get_streams_by_logins gemacht).
- **Verify-Fix:** In tb-transport-twitch beide Funktionen auf 100er-Batching umstellen, analog zum bereits vorhandenen streams.rs-Muster. get_users (client.rs:178-194): über `logins.chunks(100)` iterieren, je Chunk einen /users-Call mit den Login-Params, Ergebnisse in die HashMap mergen (leerer Input → leere Map bleibt). get_users_created_at (chat.rs:346-370): analog über `ids.chunks(100)`, je Chunk ein Request, data-Vecs konkatenieren (Eingabe-Reihenfolge bleibt durch sequentielles Anhängen erhalten). Dadurch verschwindet die Inkonsistenz zu get_streams_by_logins, und ein künftiger Multi-Login-Port (z. B. _sync_missing_user_ids aus base.py) kann nicht still in HTTP 400 laufen. Kein Verhaltensunterschied für die heutigen Single-Element-Caller. Optional einen wiremock-Test mit 150 Logins ergänzen, der zwei Requests erwartet.
