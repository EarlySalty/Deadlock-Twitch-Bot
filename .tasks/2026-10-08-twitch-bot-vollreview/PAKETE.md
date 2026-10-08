# Review-Pakete des Twitch-Bot-Vollreviews

Stand: 2026-10-08. Grundlage: `origin/main` `0ecae1370f1a80d1a101249b5c932663d69be8af`, eingefrorener Arbeitscommit `e8801a0202059dbf919863101904d9a152a94367` im Worktree `/home/nathanael/.worktrees/tb-vollreview-artefakte`.

**108 disjunkte Pakete, 1745 primäre Dateien, 597180 Textzeilen, 22788893 Bytes.** Nicht zugeordnete oder mehrfach zugeordnete primäre Dateien: 0. Alle übrigen versionierten Pfade sind nach den vollständigen Regeln in `INVENTAR.md` unterstützende Daten/Dokumente, generierte Referenzen oder ausdrückliche Ausschlüsse.

## Leseregel und Eigentum

- `pakete.json` ist das maschinenlesbare Manifest. Pro ID stehen dort die vollständigen repo-relativen `paths`, `reference_paths`, Größe, Priorität, Angriffsfläche und Bereichshinweise. Die folgenden Tabellen zeigen dieselben Eigentumsmuster mit ausdrücklich genannten gemeinsamen Präfixen, nicht bloß Beispielpfade.
- Ein Verzeichnis steht für seinen gesamten versionierten Unterbaum. `{a,b}` erweitert explizite Alternativen. `*` und `?` passen innerhalb einer Pfadkomponente, nicht über `/` hinweg. Alle Muster werden ausschließlich gegen den eingefrorenen Git-Baum ausgewertet.
- Vor jedem Eigentumsmatch gelten die S/G/X-Regeln aus `INVENTAR.md`. Eine README oder ein Binärasset in einem Paketverzeichnis wird damit nicht versehentlich zum zweiten Produktionscode. Ausnahme: `rust/knowledge/` ist bewusst produktives Review-Eigentum.
- Jede primäre Datei hat genau einen Dateieigentümer: ihre Paket-ID. Eigentum ist in dieser Phase nur die Vollständigkeitsverantwortung des Lesers, keine Schreibfreigabe. `reference_paths` sind beliebig teilbare, ebenfalls nur lesbare Aufrufer, Abwehrschichten oder Verträge. An Referenzdateien entstehen keine zusätzlichen Eigentümer.
- `Dateien`, `Zeilen` und `Bytes` in den Tabellen zählen ausschließlich primäres Eigentum, nicht referenzierte Pfade, Lockdateien, Medienassets oder Begleitdokumente. Textzeilen umfassen auch eingebettete Tests und Vorlagen.
- Die Tabellenreihenfolge ist keine Ausführungsreihenfolge. **P0 zuerst**, insbesondere öffentliche Eingänge und ihre Auth-Schichten. P1 sind indirekte Integrationen, Datenhaltung und Ressourcen. P2 sind gemeinsame Darstellung und Verwaltungs-/Testwerkzeuge. Verteilung: P0 56, P1 49, P2 3.
- Eine genannte Angriffsfläche bezeichnet einen vorhandenen Eingabe- oder Vertrauensbereich. Sie ist kein Befund und kein Beleg, dass ein Port tatsächlich öffentlich erreichbar ist. Aufrufer, Router und installierte Proxy-Konfiguration müssen später gegenprüfen, welcher Pfad live erreichbar ist.

## Blickwinkel je Paket

Für jedes Paket sind die fünf ursprünglich beauftragten Blickwinkel vollständig anzuwenden: Security, Korrektheit, Fehlerbehandlung, Nebenläufigkeit und Daten, Ressourcen. Das ergibt 540 Paket-Blickwinkel-Kombinationen. Der aktuelle Auftragsnachtrag ergänzt Bauqualität als rein bewertenden sechsten Blickwinkel; derselbe Schnitt trägt damit 648 Kombinationen. Struktur-, Duplikat-, Testbarkeits- und Wartbarkeitsempfehlungen sind ausschließlich C, keine Umbauaufträge.

Keine Stichprobenabdeckung. Ein Leser bearbeitet sämtliche Eigentumsdateien seines Pakets. Große Crates und Frontends sind deshalb aufgeteilt. Der größte Lesebereich umfasst 11983 Zeilen; die größten Einzeldateien bleiben ungeteilt und haben bei Bedarf ein eigenes Paket.

## Vollständige kleinere Crates und produktive Daten

Die Pfade in dieser Tabelle sind vollständig repo-relativ.

| ID | Bereich und Eigentum | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| R01 | Modellauswahl: `rust/crates/fireworks-model-selection/` | P1 | 2 | 257 | 9135 | LLM-Anbieter; Konfiguration; Dienstgrenze |
| R02 | Konfiguration: `rust/crates/tb-config/` | P1 | 23 | 5315 | 186010 | Dateien; Secret-Manager; Dienststart; Verwaltungsbinaries |
| R03 | Kryptografie: `rust/crates/tb-crypto/` | P1 | 8 | 650 | 23169 | Verschlüsselte Daten; Fernet; Python-Testorakel nur Legacy |
| R04 | DB-Kern: `rust/crates/tb-db/` | P1 | 21 | 3264 | 112715 | SQL; Rollen; Transaktion; Pool; Retry; Schema |
| R05 | Domänentypen: `rust/crates/tb-domain/` | P1 | 7 | 479 | 15700 | Plattform-ID; Typ- und Identitätsgrenze |
| R06 | Effort/Quests: `rust/crates/tb-effort/` | P1 | 16 | 5245 | 214937 | SQL-Evidenz; Cursor; Kalender; Projektion |
| R07 | Fehler: `rust/crates/tb-error/` | P1 | 2 | 49 | 1575 | Fehlerweitergabe über Dienstgrenzen |
| R08 | Highlight: `rust/crates/tb-highlight/` | P1 | 14 | 4306 | 154565 | Medien; OCR; Dateien; Prozesse |
| R09 | HTTP-Kern: `rust/crates/tb-http-core/` | P0 | 8 | 680 | 21176 | HTTP; Auth; Loopback; Idempotenz |
| R10 | Wissen: `rust/crates/tb-knowledge/` | P1 | 9 | 1120 | 36086 | Wissenssuche; Dateien; LLM-Kontext |
| R11 | LLM-Hub: `rust/crates/tb-llm/` | P1 | 20 | 4724 | 174670 | Anbieter-HTTP; Nutzertext; Budget; Nutzungsledger |
| R12 | Laststeuerung: `rust/crates/tb-load/` | P1 | 4 | 406 | 16082 | Prozesslast; Ressourcen; Warteschlangen |
| R13 | Observability: `rust/crates/tb-observability/` | P1 | 9 | 1609 | 53943 | Logs; SQL; Nutzungsdaten; Aufbewahrung |
| R14 | Plattformkern: `rust/crates/tb-platform-core/` | P1 | 10 | 1546 | 49515 | Plattformidentität; Fähigkeiten; Integrationsverträge |
| R15 | Scout: `rust/crates/tb-scout/` | P1 | 7 | 2061 | 77337 | Community-Daten; SQL-Cursor; Recherche |
| R16 | Stream-Audit: `rust/crates/tb-stream-audit/` | P1 | 10 | 3504 | 130646 | LLM; Streamdaten; SQL; Berichte |
| R17 | Tipps: `rust/crates/tb-tips/` | P1 | 4 | 279 | 7621 | Chat-Ausgabe; Nutzerkontext |
| R18 | Discord: `rust/crates/tb-transport-discord/` | P1 | 6 | 1992 | 74195 | Discord-HTTP; Nachrichten; Dienstzugänge |
| R19 | Twitch: `rust/crates/tb-transport-twitch/` | P0 | 17 | 7429 | 269718 | Helix; EventSub; IRC; OAuth; HTTP |
| R20 | VOD-Worker: `rust/crates/tb-vod-archive/` | P1 | 10 | 4685 | 171735 | Downloads; Pfade; Archivspeicher; Prozesse; SQL |
| R21 | Vendor-Transport: `rust/vendor/uplink-infisical-transport/` | P1 | 2 | 105 | 4188 | Secret-Manager-HTTP; Dienstzugänge |
| K01 | Eingebettetes Wissen: `rust/knowledge/` | P1 | 24 | 761 | 49684 | Produktive Wissens-/Promptdaten; öffentliche Hilfe |

R02 umfasst die beiden zusätzlichen Konfigurationsbinaries. R04 referenziert, besitzt aber nicht die separat unterstützende `tests/fresh_schema_snapshot.txt`. R10 referenziert K01, statt dessen Daten nochmals zu besitzen. R16 und BI03 sind Bestandteile dieses Twitch-Repositories; das externe Projekt `ai-coach` bleibt ausgeschlossen.

## Dashboard-API

**Präfix für sämtliche Eigentumspfade dieser Tabelle: `rust/crates/tb-dashboard-api/`.** Abwehr- und Routerreferenzen stehen je ID ausdrücklich in `pakete.json`. Gemeinsame Referenzen sind `src/lib.rs`, `src/auth/`, `rust/crates/tb-http-core/src/middleware/`, `ops/caddy/` und fachliche Crates. Die gemeinsame Referenz ist niemals ein zweiter Dateieigentümer.

| ID | Bereich und exakte Eigentumsmuster relativ zum Präfix | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| DA01 | Router/Zustand: `Cargo.toml`; `src/{admin_audit,ai_state,ai_store,lib,process_info,proxy,query_int,test_database,uplink_config}.rs`; `src/handlers/mod.rs`; `tests/`; `examples/` | P0 | 20 | 7750 | 304280 | HTTP-Router; Middleware-Reihenfolge; Proxy; SQL-Pools |
| DA02 | Identität: `src/auth/{csrf,fernet,idor_e2e_tests,level,mod,partner_access,partner_gate,security,session,streamer_scope}.rs`; `src/auth/session/` | P0 | 11 | 8622 | 318294 | Sessions; Rollen; Kanal-ID; CSRF; Rate-Limit; SQL |
| DA03 | Login: `src/auth/{discord_admin_login,oauth_login,partner_login,steam_openid}.rs`; `src/handlers/{auth_login,auth_status,demo_login,discord_link,partner_login,player_connect}.rs`; `src/handlers/player_connect/` | P0 | 12 | 9111 | 320360 | Öffentliche OAuth-/OpenID-Callbacks; State; Redirect; Kontoverknüpfung |
| DA04 | Admin-Aktionen: `src/handlers/{admin_actor,admin_announcements,admin_audit_log,admin_chat_action,admin_config,admin_form_aliases,admin_global_ban,admin_legacy_streamers,admin_legal,admin_mode,admin_operating_config,admin_partner_signup_block,admin_partner_signup_tag_block,admin_promo_mode,admin_research,admin_roadmap,admin_scout,admin_streamers,brain_lab,forward_auth,legacy_form}.rs` | P0 | 21 | 10102 | 371720 | Admin-HTTP; Forward-Auth; Formulare; Moderation; interne HTTP-Aktionen |
| DA05 | System/Diagnose: `src/handlers/system/`; `src/handlers/{health_probe,self_explainer}.rs` | P0 | 9 | 4014 | 146302 | Health-HTTP; DB-Query; Betriebsdaten; Fehlerausgabe |
| DA06 | Billing: `src/handlers/{admin_billing,admin_manual_plan,billing,billing_page,billing_profile,billing_stripe_sync,billing_webhook,monetization}.rs` | P0 | 8 | 5365 | 214911 | Öffentlicher Stripe-Webhook; Signatur; Zahlungs-HTTP; SQL; Personendaten |
| DA07 | Affiliate/Profile: `src/handlers/{admin_affiliate,affiliate,affiliate_portal,partner_profiles}.rs`; `src/handlers/partner_profiles/` | P0 | 7 | 7575 | 247304 | Öffentliche Profile/Assets; OAuth; Stripe-Connect; HTML; SQL |
| DA08 | Social-Hauptschnittstelle: `src/handlers/{social_media,social_media_fetch_tests,social_media_status_tests}.rs` | P0 | 3 | 7826 | 289350 | HTTP-Uploads; OAuth; Medienpfade; Kanalzugriff; SQL |
| DA09 | Social-Zusatzhandler: `src/handlers/{social_media_clip_contest,social_media_tiktok_direct,social_media_vod_archive,social_media_vod_archive_tests}.rs` | P0 | 4 | 1201 | 46621 | Posting; Download; VOD; Identität; HTTP |
| DA10 | OBS/Uplink: `src/obs/`; `src/handlers/uplink.rs` | P0 | 4 | 6424 | 245295 | WebSocket `/obs/ws`; HTTP-Steuerung; Dienstzugänge; SQL |
| DA11 | Zuschauer/Chat/Community: `src/handlers/{audience,audience_demographics,chat_analytics,chat_content_analysis,chat_deep_llm,chat_hype_timeline,chat_social_graph,chatter_verlauf,community,engagement_mode,exp_analytics,follower_funnel,loyalty_curve,lurker_analysis,retention_curve,viewer_exclusion,viewer_timeline,viewers,watch_time}.rs`; `src/handlers/community/` | P1 | 22 | 9161 | 353217 | Kanal-HTTP; Chattext; Zuschaueridentität; SQL |
| DA12 | Einstellungen/Moderation: `src/handlers/{ad_manager,ads_schedule,bans,clip_command_settings,command_name_settings,engagement_settings,greeting_settings,lurk_command_settings,lurker_tax_settings,moderation_settings,scam_guard_enforce,scam_guard_queue,scam_guard_settings,silent_settings,stat_command_settings,streamer_disconnect,sub_reminder_settings,tip_settings,title_command_settings}.rs` | P0 | 19 | 7182 | 236944 | Schreibendes HTTP; Auth/CSRF; Moderation; Kanaltrennung; SQL |
| DA13 | Stream/Raid/Netzwerk: `src/handlers/{last_session,leaderboard,network,network_stats,overview,performance,raid_analytics,raid_history,raid_network_analytics,raid_pages,raid_requirements,raids,rankings,session_detail,stream_kennzahlen,stream_report,streamer_comparison,streamers,tag_analysis,title_performance}.rs` | P0 | 20 | 10440 | 387590 | Öffentlicher Vergleich; Raid-OAuth-Seiten; Kanalidentität; SQL |
| DA14 | LLM/Assistent/Titel: `src/handlers/{ai_analysis,ai_chat,ai_history,coaching,dashboard_assistent,title}.rs` | P0 | 6 | 3593 | 126718 | HTTP-Nutzertext; LLM-Prompts; persistierte Dialoge; SQL |
| DA15 | Formulare/Wettbewerb: `src/handlers/{challenges,clip_contest,feedback,onboarding}.rs`; `src/handlers/clip_contest/`; `src/handlers/feedback/` | P0 | 7 | 4183 | 144012 | Öffentliche Einreichung/Voting; Discord-Login; CSRF; SQL-Schreibrollen |
| DA16 | Interne Seiten/Vorlagen: `src/handlers/{internal_home,legal,market,roadmap,roadmap_page}.rs`; `templates/` | P0 | 10 | 7366 | 307688 | HTML; LLM-Changelog; HTTP-Formulare; SQL |
| DA17 | Plattform/Overlay: `src/handlers/{platform_store,platform_token,plattform_connect,plattform_oauth,overlay,caster_overlay}.rs`; `src/handlers/{overlay,caster_camera,caster_overlay}.html` | P0 | 9 | 8820 | 319877 | OAuth-Callbacks; Plattformkonten; Dienstzugänge; Browser-Overlay |
| DA18 | SPA/Hilfe/Player: `src/handlers/{admin_spa,demo,help_page,obsolete_routes,pause_loop,spa,website}.rs`; `src/handlers/pause_loop_player.html` | P0 | 8 | 7013 | 285344 | Öffentliche HTML-/Assetpfade; Redirects; Dateizugriff; Medienplayer |
| DA19 | Kategorie: `src/handlers/{category_activity,category_collector,category_comparison,category_leaderboard,category_timings}.rs` | P0 | 5 | 1559 | 57364 | Kategorie-HTTP; Collector-Status; Parameter; SQL |

## Analytics

**Präfix: `rust/crates/tb-analytics/`.** HTTP-Handler sind Referenzen in DA-Paketen, nicht Eigentum dieser Datengruppen.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| AN01 | Admin/Partner: `Cargo.toml`; `src/{lib,admin_config,admin_streamers,streamers,streamers_crud,streamer_link,partner_access,partner_signup_block,partner_signup_tag_block,bans,global_ban,promo_mode,promo_timers,bekannte_bots}.rs` | P1 | 15 | 8717 | 315178 | SQL-Schreiben; Partner-ID; Sperren; Betrieb |
| AN02 | Billing/Stripe/Plan: `src/{admin_billing,affiliate_claim_window,plan,stufe,trial}.rs`; `src/billing/`; `src/stripe/` | P0 | 11 | 6226 | 243297 | Signatur; Zahlungsanbieter-HTTP; SQL; Idempotenz; Plangrenzen |
| AN03 | Affiliate-Finanzen: `src/{admin_affiliate,affiliate_commission,affiliate_gutschrift,affiliate_pii}.rs` | P1 | 4 | 6646 | 238290 | Geld; Personendaten; SQL; Dokumenterzeugung |
| AN04 | Werbung/Kategorie: `src/{ad_manager,ads_schedule_collector,category,category_activity}.rs`; `src/ad_manager/`; `tests/` | P1 | 13 | 5315 | 211687 | Helix/Steam-HTTP; Zeitplan; SQL |
| AN05 | Chat-Metriken: `src/{chat_analytics,chat_analytics_lexicon,chat_content_analysis,chat_content_lexicon,chat_deep_llm,chat_hype_timeline,chat_social_graph,chat_typen,chatter_verlauf,raw_chat_status}.rs` | P1 | 10 | 4135 | 164418 | Chattext; LLM; SQL-Aggregation |
| AN06 | LLM/Berichte: `src/{ai_analysis,ai_history,coaching,dashboard_assistent_log,post_stream,self_explainer_log}.rs` | P1 | 6 | 8550 | 331503 | Nutzertext; LLM; SQL-Historie; Reports |
| AN07 | Reichweite/Community/Raids: `src/{community_announcements,community_points,community_points_tests,engagement_metrics,exp_analytics,market,monetization,network,network_stats,overview,peer_group,raid_blacklist,raid_history,raids,stream_kennzahlen,subs_snapshot_collector,tag_analysis,watch_time}.rs` | P1 | 18 | 9754 | 370946 | SQL; Punkte; Zuschauer-ID; Helix-Snapshots |
| AN08 | System/Telemetrie: `src/{system_database,system_errors,system_eventsub,system_health,system_oauth_scopes,telemetry_routes}.rs` | P1 | 6 | 1735 | 61330 | SQL-Systemdaten; Logs; Diagnose |

## Chat

**Präfix: `rust/crates/tb-chat/`.** Gemeinsame Referenzen: `src/pipeline.rs`, Twitch-Transport, Bot-Chat-Wiring, fachliche interne Handler und LLM-Hub, jeweils als konkrete `reference_paths` im Manifest.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| CH01 | Pipeline/Kanäle: `Cargo.toml`; `src/{api,channel_classifier,channel_policy,chatter_tracking,lib,lurker_policy,pipeline,types,zuschauer_register}.rs`; `tests/{channel_policy,chatter_tracking_db,zuschauer_register_db}.rs` | P0 | 13 | 8116 | 299682 | Öffentlicher Chat; Kanalpolitik; Identität; SQL |
| CH02 | Befehle/Spieler: `src/{catalog,clip_contest_submit,clip_contest_submit_tests,command_names,command_regression_tests,command_target,command_target_tests,commands,personal_discord_command_tests,player_link_command_tests,player_links,rank_lookup,stat_commands,stats,stats_http_tests,steam_lookup,watchtime_tests}.rs`; `src/rank_lookup/` | P0 | 19 | 11563 | 419017 | Chat-Befehl; Zielkanal; Steam-ID; HTTP; SQL |
| CH03 | Moderation: `src/{crew_guard,global_ban_sweep,global_chatter_ban,moderation,moderation_settings,safe_list,spam_filter,suppression_guard,timeout_tracking}.rs`; `tests/{crew_radar_db,global_chatter_ban_db,moderation_db,suppression_db}.rs` | P0 | 13 | 10719 | 383731 | Chattext; Sperren; Plattformwirkung; SQL-Evidenz |
| CH04 | Scam/Einladungen: `src/{conversation_scam,invite_question,scam_pitch,sus_invite}.rs`; `tests/conversation_scam_db.rs` | P0 | 5 | 11774 | 431079 | Links; Gespräche; LLM-Kontext; Moderationswirkung |
| CH05 | Zugänge: `src/{db_token_store,db_token_store_tests,secret_sink,token}.rs` | P0 | 4 | 2609 | 98170 | Geheime Chat-Eingaben; Verschlüsselung; SQL; Logs |
| CH06 | Promos: `src/promos.rs` | P0 | 1 | 7777 | 293751 | Chat; Werbetimer; SQL; externe Daten |
| CH07 | Pitches/Antworten: `src/{fun_responses,lfg_pitch,mention_scoring,pitch_beispiele,pitch_bewertung,promo_pitch,standard_replies,streamer_voice,style_score,sub_reminder,sub_reminder_tests}.rs`; `tests/lfg_pitch.rs` | P0 | 12 | 7005 | 251040 | LLM-Nutzertext; Antwortstil; Reminder; SQL |
| CH08 | Titeljobs: `src/{title_ai,title_db,title_jobs}.rs`; `tests/title_costream_review.rs` | P0 | 4 | 3188 | 114933 | LLM; Titeländerung; Plattform-HTTP; SQL-Jobs |

## Raid

**Präfix: `rust/crates/tb-raid/`.** OAuth-Wiring, EventSub-Hooks, Effort-Engine und Transport stehen jeweils als Referenzen im Manifest.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| RA01 | OAuth/Zugänge: `src/{auth_writer,bot_oauth,oauth_flow,reauth_admin,scope_fallback_warn,scope_profiles,token_blacklist,token_lifecycle,token_provider,token_refresher,token_store}.rs`; `tests/{ad_manager_scopes,auth_writer,background_token_refresh,oauth_grant_consistency,token_blacklist,token_provider,token_refresher,token_store}.rs`; `tests/support/` | P0 | 20 | 9348 | 357795 | OAuth-State/Callback; Refresh; Rollen; SQL-Zugänge |
| RA02 | Auswahl/Ausführung: `Cargo.toml`; `src/{auto_raid_pipeline,candidate_selection,eligibility,lib,manual_suppression,offline_eligibility,pending_raids,raid_blacklist,raid_executor,raid_history_store,raid_messaging,score_store,score_tracking_store,scoring,state_store,strikes_store,target_generation,target_resolution,util}.rs`; `tests/{auto_raid_pipeline,offline_eligibility,raid_blacklist,raid_executor,score_tracking_store,state_store}.rs` | P0 | 26 | 10242 | 387115 | Raid-HTTP; Kanal-ID; Jobzustand; SQL |
| RA03 | Ankunft/Signale: `src/{arrival_confirmation,arrival_runtime,arrival_tracking_store,partner_raid_delivery,signal_correlation}.rs`; `tests/arrival_runtime.rs` | P0 | 6 | 4166 | 163128 | EventSub; Chat; Deduplizierung; SQL; Zustellung |
| RA04 | Partner/Boost/Pause: `src/{alias_store,courtesy,courtesy_store,deadlock_pause,external_recruitment_store,flip_unraid,monthly_raid_boost,outreach_boost,partner_roster,partner_score_refresh,partner_setup,recruitment_messaging,signup_denylist,streamer_referrals}.rs`; `tests/{monthly_raid_boost,outreach_boost,partner_roster,partner_score_refresh,partner_setup}.rs`; `tests/fixtures/` | P1 | 20 | 10310 | 370493 | Partner-ID; Credits; SQL; Rekrutierung; Plattformaktionen |

## Social Media

**Präfix: `rust/crates/tb-social-media/`.** Referenzen verbinden diese Pakete mit DA08/DA09, LLM, VOD, STT und Highlight. Fonts, Lizenz und Logo sind Unterstützung, nicht neue Codepakete.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| SM01 | OAuth/Upload: `src/{capabilities,credentials,disconnect,http_security,oauth,partner_access,reauth,refresh_worker,tiktok_recovery,upload_download_tests,upload_worker}.rs`; `src/uploaders/` | P0 | 15 | 11983 | 472801 | OAuth; URL-Vertrauen; Upload/Download; externe Plattformen; SQL |
| SM02 | Clips/Planung: `Cargo.toml`; `src/{analytics,approval,approval_worker,batch,clip_analytics,clip_manager,clip_prep_worker,clip_queue,forms,insights_worker,lib,posting_plan,retention,retention_worker,scheduler,settings,test_support,vod_archive}.rs`; `src/clip/`; `templates/` | P1 | 27 | 9523 | 361344 | Clip-HTTP; SQL-Jobs; Freigaben; Retention; VOD |
| SM03 | Medien/Rendering: `src/{clip_templates,layout,preview,render,rendering,subtitles,transcription,video_processor}.rs`; `src/bin/` | P1 | 10 | 3802 | 135892 | Dateipfade; Medienprozesse; STT-HTTP; Nutzermedien |
| SM04 | LLM/Kontext: `src/{clip_context,clip_context_harvest,correction,enrich_pipeline,enrichment,enrichment_worker,llm,llm_dispatch,report_dispatcher,report_writer,seed_vocab,title_gate,vocab}.rs`; `tests/` | P1 | 14 | 6197 | 229642 | LLM-Prompts; Chat-/Clipkontext; SQL-Lernen; Berichte |

## Monitoring und EventSub

**Präfix: `rust/crates/tb-monitoring/`.** Öffentlicher Webhook-Eingang MO01 zuerst; fachliche Wirkungen zusätzlich gegen BO03 und RA03 referenzieren.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| MO01 | EventSub/Inbox: `src/{dispatch,guard,handlers,inbox_runtime,subscriptions,webhook_receiver}.rs`; `src/inbox_store/`; `tests/{eventsub_dispatch,subscriptions}.rs` | P0 | 10 | 9091 | 329526 | Öffentlicher Webhook; Signatur; Replay; Inbox-SQL; Subscription-HTTP |
| MO02 | Polling/Live: `Cargo.toml`; `src/{lib,live_state,scout,stream,streamer_login}.rs`; `src/poller/`; `src/announce/`; `tests/{announce,hermetic,poller,twitch_rename}.rs` | P1 | 19 | 10243 | 370723 | Twitch-Antworten; Polling; Loginwechsel; SQL; Discord-Ausgabe |
| MO03 | Chatters/IRC: `src/{anonymous_chat,chatters_poller,irc_lurker}.rs`; `tests/{chatters_poller,presence_platform_id}.rs` | P0 | 5 | 3582 | 129065 | IRC; Helix; Zuschauer-ID; Präsenz-SQL |
| MO04 | Sessions/Retention: `src/{exp_sessions,observability_retention,raid_retention,stats,telemetry}.rs`; `src/sessions/`; `tests/{observability_retention,raid_retention,session_id_first,write_core}.rs`; `tests/support/` | P1 | 14 | 5624 | 205476 | Sessions; Zuschauerwerte; SQL; Aufbewahrung; Telemetrie |

## Engagement

**Präfix: `rust/crates/tb-engagement/`.** Referenzen sind LLM-Hub, Twitch-Transport, STT und die jeweiligen BO-Wirings.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| EN01 | Chat/Reaktion: `Cargo.toml`; `src/{auto_off,background,channel_background,conversation,gate,global_sentiment,irc_message,irc_reader,learn_irc_reader,lib,llm_chat,lurker_signal,pipeline,reaction_learning,rhythm,sender_auth,stealth_sender,stream_state,threads,types}.rs`; `tests/ledger_side_effects.rs` | P0 | 22 | 10167 | 412542 | Chat; LLM; Senderzugänge; SQL-Ledger; Nachrichten |
| EN02 | Outreach/Smalltalk: `src/{outreach_shadow,outreach_shadow_store,persona,shadow_review,smalltalk_live_store,smalltalk_loop_store,soul_store,style_examples}.rs`; `src/smalltalk_live_candidates.sql`; `tests/{outreach_shadow_store,smalltalk_loop_store}.rs` | P1 | 11 | 6571 | 235051 | SQL-Gespräche; LLM-Kontext; Auswahlzustand; Nutzertext |
| EN03 | Audio/Spielkontext: `src/{audio_capture,deadlock_patches,deadlock_stats,deadlock_wiki,match_context,stream_transcripts,transcribe}.rs` | P1 | 7 | 3729 | 135958 | Audio; STT-HTTP; Spieldaten; Dateien; Prozesse |

## Interne API

**Präfix: `rust/crates/tb-internal-api/`.** `src/security.rs` und `src/lib.rs` sind zwingende Abwehrreferenzen für sämtliche Fachhandler. Diese API verwendet eigene Schutzlayer, nicht bloß die Middleware aus tb-http-core.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| IA01 | Router/Schutz: `Cargo.toml`; `src/{idempotency,lib,security,streamer_lifecycle,streamer_referral_tests}.rs`; `src/handlers/{common,healthz,legacy_proxy,mod,python_stubs}.rs` | P0 | 11 | 5035 | 190605 | Loopback-HTTP; Dienstauth; Proxy; Idempotenz; SQL |
| IA02 | Streamer/Raid/Zugänge: `src/handlers/{discord_invite,global_ban,partner_signup_block,raid,raid_blacklist,raid_oauth,reauth_all,scam_guard,streamer_link,streamers}.rs` | P0 | 10 | 7362 | 268670 | Moderation; OAuth; Kanaltrennung; Dienst-HTTP; SQL |
| IA03 | Eingänge/Community: `src/handlers/{chat_command,clip_contest,clip_contest_tests,community_points,eventsub,patch_announcement,patch_announcement_tests,scout_community,scout_community_privacy_tests,scout_community_tests,spam_learning}.rs` | P0 | 11 | 5662 | 201625 | Weitergeleitete EventSub-/Chatdaten; Lernen; Punkte; Nachrichten |
| IA04 | Diagnose/Statistik: `src/handlers/{diagnose,market_share,self_explainer_log,session_detail,stats_native,streamer_analytics_native,telemetry_routes}.rs` | P1 | 7 | 7968 | 289953 | Interne HTTP-Abfragen; SQL; Nutzer-/Diagnosedaten |

## Bot-Binary und übrige Workspace-Binaries

**Präfix der BO-Eigentumspfade: `rust/bin/tb-bot/`.** Die BI-Pfade sind vollständig repo-relativ. Crate-Implementierungen sind Referenzen und haben ihre eigenen Eigentümer.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| BO01 | Bootstrap/MCP: `Cargo.toml`; `src/{category_followers,main,mcp,task_supervisor,user_id_backfill,wiring}.rs` | P0 | 7 | 5636 | 228235 | Listener; Loopback-MCP; Tasks; Konfiguration; SQL |
| BO02 | Chat/Spam-Wiring: `src/{brain_chat_wiring,chat_typen_wiring,chat_wiring,chatters_wiring,community_points_wiring,crew_archive,irc_lurker_wiring,scam_enforce_impl,scam_notify_impl,scam_revoke_impl,scout_chat,zuschauer_register_backfill}.rs` | P0 | 12 | 8625 | 319903 | Chat; Moderationswirkung; LLM; IDs; SQL-Backfill |
| BO03 | EventSub/OBS: `src/{eventsub_hooks,eventsub_stats_adapter,obs_dock,offline_side_effects}.rs` | P0 | 4 | 3932 | 150872 | EventSub; Chat-OBS-Brücke; SQL; Nebenwirkungen |
| BO04 | OAuth/Reauth-Wiring: `src/{oauth_followups,raid_oauth_impl,reauth_reminder,social_reauth,streamer_link,token_lifecycle_wiring}.rs` | P0 | 6 | 5639 | 214485 | OAuth-Nachwirkungen; Refresh; Identität; SQL; Nachrichten |
| BO05 | Raids/Partner/Werbung: `src/{ad_manager_wiring,auto_raid,confirm_resolver,flip_unraid,monthly_raid_boost,partner_lookup,partner_recruit,raid_adapters,raid_arrival_wiring,raid_greeting,score_refresh}.rs` | P1 | 11 | 9751 | 382686 | Raid-/Anzeigen-HTTP; Scheduler; Partner-ID; SQL |
| BO06 | Smalltalk/Patches: `src/{outreach_shadow_wiring,patch_feed,shadow_review_wiring,smalltalk_loop_wiring}.rs` | P1 | 4 | 5198 | 190595 | LLM; externe Patchdaten; SQL-Loops; Chat-Ausgabe |
| BI01 | `rust/bin/tb-dashboard/` | P0 | 3 | 891 | 32554 | HTTP-Listener; Router; Rollen; Konfiguration |
| BI02 | `rust/bin/tb-category-collector/` | P1 | 5 | 1416 | 55447 | Twitch-HTTP; Collector-Prozess; DB-Rollen |
| BI03 | `rust/bin/tb-stream-audit/` | P1 | 2 | 6678 | 290530 | Stream-/Dateieingaben; LLM; SQL; Prozesse |
| BI04 | `rust/bin/tb-stt-server/` | P0 | 3 | 1879 | 63981 | Audio-HTTP; Dienstzugang; Dateien; Modellressourcen |

## Streamer-Dashboard

**Präfix: `bot/dashboard_v2/`.** Ausnahme FE08: vollständiger Pfad `bot/shared-theme/`. Gemeinsame HTTP- und serverseitige Authreferenzen sind pro ID im Manifest angegeben. Frontendgates ersetzen nie Serverberechtigungen.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| FE01 | Shell/Client: `{.gitignore,eslint.config.js,index.html,package.json,tsconfig.app.json,tsconfig.json,tsconfig.node.json,vite.config.ts}`; `src/{App.css,App.tsx,index.css,main.tsx,runtimeConfig.ts,tabAliases.ts}`; `src/api/{admin,auth,client,core,httpError,index}.ts`; `src/{context,hooks,i18n,motion,preview}/`; `src/components/{layout,onboarding,banners,modals,scopes}/`; `src/utils/{browserErkennung,formatters,zeitraum}.ts` | P0 | 52 | 6498 | 251868 | Cookies; Browser-HTTP; Clientzustand; Identität; Vorschau |
| FE02 | Zuschauer/Streams: `src/api/{analytics,home}.ts`; `src/pages/{Audience,Comparison,Experimental,Growth,Overview,Publikum,SessionDetail,Sessions,Tagesform,ViewerTimeline,Viewers,Wachstum,WasTun}.tsx`; `src/components/cards/`; `src/components/tables/`; `src/components/SessionEventTimeline.tsx`; `src/types/analytics.ts`; `src/utils/{engagementKpi,healthScoreContext,sharingTopN}.ts` | P1 | 31 | 8851 | 298569 | Kanalparameter; API-Antworten; Zuschauerwerte |
| FE03 | Chat/LLM/Community: `src/api/{ai,assistent,challenges,community,feedback,title}.ts`; `src/pages/{AIAnalysis,Challenges,Coaching,Community,InternalHomeLanding,Planung,StreamReports,TitleGenerator}.tsx`; `src/pages/{chatAnalyticsContent,chatAnalyticsDeepSections,chatAnalyticsShared,chatSubPages,coachingSubPages}.tsx`; `src/pages/{chatAnalyticsQueries,chatAnalyticsViewModel,useChatAnalyticsPage}.ts`; `src/pages/feedback/`; `src/components/{assistent,feedback,roadmap}/`; `src/types/community.ts`; `src/utils/{community,titlePreferences}.ts` | P1 | 34 | 9323 | 406912 | Nutzertext; LLM-Dialog; Feedback; Chatdaten; HTTP |
| FE04 | Verwaltung/Profile: `src/api/{adManager,clipCommand,commandNames,disconnectBot,engagement,greeting,lurkCommand,lurkerTax,moderation,onboarding,partnerProfile,scamGuard,silentNotifications,statCommands,subReminder,titleCommand}.ts`; `src/pages/{AuthScopes,OverlayBuilder,PartnerProfile,Schedule,Verwaltung}.tsx`; `src/pages/verwaltungTabs.ts`; `src/components/verwaltung/`; `src/types/scopes.ts`; `src/utils/partnerProfile.ts` | P0 | 42 | 7870 | 341643 | Schreibendes HTTP; Moderation; Trennung; Overlay; Formulare |
| FE05 | Social/VOD: `src/api/socialMedia.ts`; `src/pages/{SocialMedia,SocialMediaManager}.tsx`; `src/components/socialmedia/`; `src/types/socialMedia.ts`; `src/utils/{socialMediaChannel,socialMediaLayout}.ts` | P0 | 18 | 7378 | 289037 | Upload; OAuth-Navigation; Posting; Download; Kanal-ID |
| FE06 | Uplink: `src/api/uplink.ts`; `src/uplink*.ts`; `src/uplinkHelp.css`; `src/pages/Uplink*`; `src/components/uplink/`; `public/uplink/` | P0 | 22 | 6068 | 255339 | OBS; Browser-HTTP; Plattformziele; Hilfeseiten; Dienstzugänge |
| FE07 | Billing: `src/api/billing.ts`; `src/pages/{Monetization,Pricing}.tsx`; `src/components/pricing/`; `src/types/billing.ts`; `src/utils/monetization.ts` | P1 | 10 | 1627 | 66799 | Zahlungsnavigation; Planstatus; HTTP; Personendaten |
| FE08 | Designsystem: `bot/shared-theme/` (kein Dashboard-Präfix) | P2 | 3 | 642 | 23856 | Gemeinsame CSS-/Browserdarstellung |
| FE09 | Testverträge/Legacy-Bilder: `tests/`; `tools/` | P2 | 75 | 9022 | 408968 | Prüfharness; Browser-Fixtures; Bildprozesse; Python-Legacy |
| FE10 | Kategorie/Charts: `src/pages/{Category,CategoryCollector}.tsx`; `src/pages/{categoryCollectorStaleness,categoryCollectorViewModel}.ts`; `src/components/charts/`; `src/components/heatmaps/`; `src/utils/calendarHeatmap.ts` | P1 | 19 | 3862 | 161049 | API-Daten; Zeit/Kalender; Kanalparameter; Browserressourcen |

FE06 besitzt auch `src/pages/Uplink.layout.test.tsx`, weil diese Datei im selben konkreten Seitenmuster liegt. FE09 besitzt die separaten `tests/`-Verträge. Diese Trennung ist absichtlich eindeutig, nicht eine Annahme über Dateiendungen.

## Admin-Dashboard

**Präfix: `bot/admin_dashboard/`.** Konkrete serverseitige Auth-, Forward-Auth- und Fachhandlerreferenzen stehen pro ID im Manifest.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| AD01 | Shell/API/Auth: `{eslint.config.js,index.html,package.json,tsconfig.app.json,tsconfig.json,tsconfig.node.json,vite.config.ts}`; `src/{App.tsx,index.css,main.tsx}`; `src/{api,components,hooks,utils}/`; `src/pages/{Dashboard.tsx,_placeholder}` | P0 | 46 | 6160 | 207874 | Admin-Cookies; Browser-HTTP; Authstatus; Bestätigung; Identität |
| AD02 | Betrieb/Streamer/DB: `src/pages/{streamers,operations,monitoring,config}/` | P0 | 12 | 3247 | 136399 | Admin-Schreibaktionen; DB-Query; Streamer-ID; Betriebssteuerung |
| AD03 | Finanzen/Community: `src/pages/{billing,community,content,money}/` | P0 | 20 | 6105 | 256985 | Finanz-/Inhaltsformulare; Sperren; Chataktionen; HTML |

## Website

**Präfix: `website/`.** Öffentliche HTML-Einstiege werden nicht als generierte Dateien herausgenommen; sie sind versionierter produktiver Bestandteil. Bilder, Fonts und Videoclips sind separat Unterstützung. Aktive Lockdatei ist Lieferkettenreferenz.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| WE01 | Shell/Netzwerk: `{eslint.config.js,index.html,package.json,tsconfig.app.json,tsconfig.json,vite.config.ts}`; `{affiliate-portal,faq,onboarding,v1,v3,vergleich,vertriebler}/`; `scripts/`; `src/{App.tsx,faq-lobby.css,faq.tsx,index.css,main.tsx,streamer-comparison.tsx,streamer-v2.css,streamer-v2.tsx,streamer-v3.css,streamer-v3.tsx,theme-v2.css}`; `src/components/{effects,faq,ui}/`; `src/components/layout/{Footer,Navbar,PublicInfoFooter,PublicInfoHeader}.tsx`; `src/{data,hooks,lib}/`; `src/pages/{FaqDoormanPage,StreamerComparisonPage,StreamerNetworkPage,StreamerNetworkV3Page}.tsx`; `tests/`; `public/` | P0 | 74 | 7204 | 288546 | Öffentliche Seiten; Netzwerk-HTTP; externe Links; Assets; Prerender |
| WE02 | Formulare/Chatbot: `clips/`; `src/{affiliate-portal.tsx,clips.css,clips.tsx,onboarding.tsx,vertriebler.tsx}`; `src/components/layout/{AffiliateNavbar,SiteChatbot}.tsx`; `src/components/onboarding/`; `src/pages/{AffiliatePortal,AffiliateProgramPage,StreamerOnboardingPage}.tsx` | P0 | 15 | 3047 | 129947 | Clip-Voting; Affiliate-Login; Onboarding; Nutzertext; HTTP |
| WE03 | Marketingvarianten: `src/components/{partner-clean,partner-v3,sections}/` | P0 | 32 | 5626 | 215381 | Öffentliche Partner-/Banfeed-Daten; externe Links; Browser-HTTP |

## Datenbank, Ops, Legacy und Build

Die Pfade dieser Tabelle sind vollständig repo-relativ. OP01 besitzt die versionierten Caddy-Snippets ausschließlich als Lesebestand; ihr Zweck für andere Pakete ist Routing- und Abwehrreferenz. Kein Änderungsauftrag an Caddy.

| ID | Bereich und exakte Eigentumsmuster | Prio | Dateien | Zeilen | Bytes | Eingänge und Grenzen |
|---|---|---|---:|---:|---:|---|
| DB01 | `rust/migrations/` | P1 | 181 | 10421 | 466237 | SQL-Constraints; Rollen; Trigger; IDs; Deduplizierung |
| DB02 | `bot/migrations/`; `rust/test-support/` | P1 | 12 | 1615 | 69321 | Legacy-SQL; Schema-/DB-Testverträge; Identität |
| OP01 | `ops/systemd/`; `ops/caddy/`; `ops/deadlock-twitch-pg-dsn-rotation.timer`; `ops/model-selection/`; `scripts/run_twitch_bot_service.sh`; `scripts/run_twitch_dashboard_service.sh`; `scripts/run_with_infisical.sh`; `rust/scripts/run_tb_bot_service.sh`; `rust/scripts/run_tb_dashboard_service.sh`; `rust/scripts/run_stream_audit_service.sh`; `rust/scripts/prepare_uplink_release.sh`; `ops/tests/` | P0 | 55 | 4255 | 190254 | Systemd; Deploy; Dateien/Prozesse; Secret-Manager; Peer-Rollen; Proxy |
| OP02 | `ops/highlight-detector/`; `ops/stt-server/`; `ops/learn-samples.sh` | P1 | 27 | 1791 | 61780 | Legacy-Medienanalyse; Audio; Dateien; DB; Prozesse |
| TO01 | `tools/`; `tests/` | P2 | 23 | 3381 | 227259 | Einmalanalyse; Roadmap; HTTP; Dateipfade; Python-Legacy |
| BU01 | `rust/Cargo.toml`; `rust/rust-toolchain.toml`; `rust/.cargo/`; `rust/osv-scanner.toml`; `rust/.gitignore`; `rust/bin/build_revision.rs`; `rust/deployment/`; `rust/scripts/{20260801_rename_derechtecoolys_to_coolysdl.sql,20260802_backfill_grosse_tabellen.sql,check-brain-consumer.sh,sqlx-prepare.sh,test-fresh-schema.sh,test_db.sh}`; `scripts/{install-git-hooks.sh,security-scan-local.sh,sqlx-prepare.sh}`; `.githooks/`; `.github/`; `.gitguardian.yaml`; `.gitignore`; `.gitleaks.toml`; `.graphifyignore`; `.semgrepignore`; `.trivyignore.yaml`; `osv-scanner.toml`; `rechnung-vorschau.html` | P1 | 28 | 1335 | 52533 | Workspace/Lieferkette; Hooks; Scanner; Wartungs-SQL; Testcontainer; Dateien |

DB01 umfasst 180 SQL-Migrationen und `.gitkeep`. DB02 umfasst acht Legacy-Migrationen und vier Rust-Testhilfen. Bei beiden gilt: keine Migration ausführen und keine angewandte Migration ändern. TO01 schließt `tools/boon` durch die vorgelagerte X-Regel aus. Python in allen Paketen ist nur Legacy-Bestand beziehungsweise bestehendes Verwaltungs-/Build-/Testwerkzeug, kein Python-Fixziel.

## Verbindliche Referenzgrenzen für spätere Gegenprüfung

Die exakten Listen je Paket stehen im Feld `reference_paths` von `pakete.json`. Sie wurden gegen den eingefrorenen Git-Baum geprüft; alle existieren. Hier sind die wichtigsten übergreifenden Vertrauensketten:

1. **HTTP und Kanalidentität:** BI01 → DA01 → DA02/DA03 → Fachhandler → AN/RA/SM/DB. Browserzustand und Login-Namen dürfen nicht ohne serverseitige Plattform-ID als Autorität gelten. Der Schnitt behauptet hierzu noch keinen Defekt.
2. **Reverse-Proxy:** OP01 `ops/caddy/partner-profiles.caddy` und `partner-profile-assets.caddy` → DA07/DA18. DA04 `forward_auth.rs` → Admin-Zugang. Es gibt im versionierten Bestand keine vollständige installierte Caddy-Allowlist. Eine spätere Widerlegung durch Caddy braucht einen konkreten separaten Beleg.
3. **Interne Dienste:** BO01 → IA01 `lib.rs`/`security.rs` → IA-Fachhandler; Dashboard-Proxy DA01 als Aufrufer. R09 liefert gemeinsame Middleware, ist aber nicht automatisch die tatsächlich verwendete Abwehr des internen Routers.
4. **OAuth:** DA03/DA17/DA08/DA07 und RA01 → BO04 → Transport und verschlüsselte Speicherung. Callback-State, Redirect, Rollen, Kontoverknüpfung und Refresh sind jeweils an der Fachgrenze nachzulesen.
5. **Webhook:** MO01 → BO03 → RA03/MO04 und SQL-Inbox. Stripe-Eingang DA06 → AN02 `webhook_sig.rs`/`webhook_apply.rs`. Signaturprüfung, Replay und Wirkung gehören zusammen, trotz disjunkter Dateieigentümer.
6. **Chat und LLM:** R19/MO03 → BO02/EN01 → CH01 → Chatfachpakete; LLM-Hub R11 und produktives Wissen K01 sind Referenzen. Jeder Prompt aus Chat, HTTP oder Transkript ist unvertrauenswürdiger Kontext, auch bei internem Transport.
7. **Medien und Prozesse:** DA08/DA09/FE05 → SM-Pakete → R20/R08/BI04; Dateipfade, Download-URLs, Renderer, STT und externe Prozesse werden an allen Übergängen referenziert.
8. **SQL und Rollen:** R04/DB01/DB02 sowie OP01 `pg_hba-twitch.conf`, `twitch-runtime-roles.sql`, `category-runtime-roles.sql`, `install-twitch-contest-peer`. Constraints und Rollen sind mögliche Abwehrschichten, aber müssen auf den konkret behaupteten Pfad passen.
9. **Lieferkette und Generierung:** BU01 referenziert vier aktive Lockdateien und `rust/.sqlx/`. Historische Task-Locks bleiben Historie. Assets und Dokumente werden bei Bedarf gelesen, ohne neue Primäreigentümer zu erzeugen.

## Ausschlüsse, Grenzen und Übergabe

Die vollständige Ausschluss- und Unterstützungsbilanz steht in `INVENTAR.md`. `ai-coach`, echte Secrets und ENV-Dateien bleiben ungelesen. Das versionierte Binärwerkzeug `tools/boon` hat hier keine lesbare Quelle. Unversionierte Live-Konfiguration und tatsächliche Erreichbarkeit sind keine Aussagen dieses Inventars.

Nur die drei beauftragten Artefakte wurden geschrieben. Keine Defektsuche, Fixes, Reviewer, weiteren Threads, Builds, Tests, Commits, Pushes, Merges, Deploys oder Dienständerungen. Der Schnitt ist für die vollständigen späteren Blickwinkel bereit.
