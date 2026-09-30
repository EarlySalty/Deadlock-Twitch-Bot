# Paket A: Dashboard, Auth und Billing

Arbeitsbranch: `feat/twitch-toml-dashboard-rollout-20260920`, Basis `2252b109`.
Originalcheckout bleibt unberührt. Noch keine Freigabe für Merge oder Deployment.

## Vertrag

Alle Betriebswerte stammen aus dem beim Start validierten Snapshot. Relative Pfade werden relativ zur TOML aufgelöst; bei der Übernahme der bisher zum Arbeitsverzeichnis relativen Pfade müssen daher die bestehenden absoluten Ziele eingetragen werden. Es gibt keine produktiven Testdefaults und keinen zweiten Config-Store. Der vorhandene Admin-Editor bleibt unverändert; sensible Auth-, Rechnungssteller- und Preis-ID-Felder werden dort nicht freigegeben. Preisbeträge, Modelle und Anbieter bleiben unverändert.

`dashboard.options` ist eine explizite Struct mit unbekannte-Felder-Verbot. `knowledge.directory` und `media.{youtube_audit_passed,social_media_public_origin}` sind gemeinsam mit dem Bot genutzte globale Sektionen. `internal_api.client_base_url` bleibt ein unabhängiger Clientoverride, keine Listeneradresse; `probe_allow_non_loopback` betrifft ausschließlich die bestehende Health-Probe. Ihre eigene HTTPS-/Loopback-Normalisierung bleibt erhalten.

## Quellenmatrix (vorher → TOML)

| Bisheriger Betriebsleser / Aliaspriorität | Neues Feld unter dashboard.options | Erhaltener Default / Verhalten |
| --- | --- | --- |
| TWITCH_RUNTIME_ENFORCE → TWITCH_SPLIT_RUNTIME_ENFORCE | runtime_enforce | true |
| TWITCH_RUNTIME_ROLE → TWITCH_SPLIT_RUNTIME_ROLE | runtime_role | leer, bei aktivierter Prüfung Startfehler; Dienstrolle dashboard muss explizit angegeben werden |
| TWITCH_RUNTIME_PID_LOCK_DIR | runtime_lock_dir | data/runtime/locks |
| TB_DASHBOARD_LEGACY_FALLBACK_URL | legacy_fallback_url | fehlt → Proxy aus |
| TB_DASHBOARD_COOKIE_INSECURE | cookie_insecure | false |
| TWITCH_DASHBOARD_NOAUTH | noauth_readiness | false; nur Readinessmeldung, keine neue Authumgehung |
| Pentest-Rate-Limit-Schalter | pentest_disable_rate_limits | false |
| TWITCH_DASHBOARD_AUTH_REDIRECT_URI | oauth_redirect_uri | fehlt → Login nicht eingerichtet; bestehende Callbackvalidierung bleibt |
| TWITCH_AFFILIATE_AUTH_REDIRECT_URI | affiliate_oauth_redirect_uri | fehlt → public_origin + vorhandener Callbackpfad |
| TWITCH_PUBLIC_DASHBOARD_BASE_URL → TWITCH_PUBLIC_URL → PUBLIC_URL | public_dashboard_url | bisheriger Verbraucherrückfall bleibt |
| TWITCH_ADMIN_PUBLIC_URL → MASTER_DASHBOARD_PUBLIC_URL | admin_public_url | bisheriger Admin-Origin bleibt |
| DISCORD_OAUTH_INTERNAL_API_BASE_URL | discord_oauth_broker_url | http://127.0.0.1:8770 |
| TWITCH_ADMIN_OWNER_USER_ID → DISCORD_ADMIN_OWNER_USER_ID | admin_owner_user_id | None; IDs positiv |
| TWITCH_ADMIN_DISCORD_GUILD_IDS → DISCORD_ADMIN_GUILD_IDS | admin_guild_ids | leere Liste; IDs positiv |
| SHARED_ADMIN_COOKIE_DOMAIN | shared_admin_cookie_domain | deutsche-deadlock-community.de |
| TWITCH_DEMO_LOGIN_TWITCH_USER_ID / DISPLAY_NAME | demo_login_twitch_user_id / demo_login_display_name | None / leer |
| TWITCH_DEMO_EMBED_ORIGINS | demo_embed_origins | Community-Origin |
| STRIPE_CHECKOUT_SUCCESS_URL → CANCEL_URL → TWITCH_BILLING_CHECKOUT_SUCCESS_URL → CANCEL_URL → Admin-Origin | billing_public_origin | ersten vorher gültigen Origin übernehmen; fehlt → alter Admin-/Default-Origin |
| STRIPE_PRICE_ID_MAP → TWITCH_BILLING_STRIPE_PRICE_ID_MAP | stripe_price_ids.<plan>.monthly / yearly | keine Overrides; nur ID-Zuordnungen, Monatszyklus 1 / Jahreszyklus 12 |
| STRIPE_PRODUCT_ID_MAP → TWITCH_BILLING_STRIPE_PRODUCT_ID_MAP | stripe_product_ids.<plan> | keine Overrides |
| Discord-Referral-Code | discord_ref_code | DE-Deadlock-Discord |
| STEAM_RANK_URL | steam_rank_url | http://127.0.0.1:8783/rank |
| STEAM_LINK_START_BASE_URL | steam_link_start_base_url | https://deutsche-deadlock-community.de/link |
| TURNIER_INTERNAL_API_BASE_URL | turnier_internal_base_url | http://127.0.0.1:8900 |
| DEADLOCK_ASSETS_BASE_URL | deadlock_assets_base_url | https://assets.deadlock-api.com |
| TWITCH_HELIX_BASE_URL | helix_base_url | fehlt → https://api.twitch.tv/helix |
| ADMIN_DASHBOARD_DIST_PATH / DASHBOARD_V2_DIST_PATH / WEBSITE_DIST_PATH | admin_dist_path / dashboard_dist_path / website_dist_path | getrennte alte Verzeichnisse bleiben |
| Legal-Pfad / öffentlicher Turnstile-Site-Key | legal_pages_path / legal_turnstile_site_key | data/admin_dashboard/legal_pages.json / leer |
| dynamische Bot-Logins / weitere Bot-User-IDs | excluded_bot_logins / additional_bot_user_ids | leer; vorhandene twitch.bot_user_id wird gemeinsam gelesen |
| AFFILIATE_GUTSCHRIFT_SMTP_HOST → SMTP_HOST, PORT, FROM, FROM_NAME, STARTTLS, SSL | affiliate_mail.{host,port,from_email,from_name,starttls,use_ssl} | None,587,None,Deadlock Partner Network,true,false; ohne Host/Absender kein Sender |
| AFFILIATE_GUTSCHRIFT_SELLER_*; EMAIL → FROM_EMAIL, TAX_ID → VAT_ID | affiliate_seller.{name,company,street,postal_code,city,country,email,website,tax_id} | vorhandene Steuerberater-Platzhalter, DE und billing@example.invalid; Website sonst öffentlicher Origin |

SMTP-Benutzername/Passwort, OAuth-/Broker-/Stripe-Tokens, Session-/Turnstile-Geheimnisse und Datenbankzugangsdaten bleiben im bestehenden Secretloader. Der SMTP-Loader bekommt nach der Umstellung ausschließlich die zwei credentialbezogenen Aliaslisten. Keine Zugangsdaten werden aus TOML gelesen.

## Prüfstand

Gemeinsames Schema e937f659 separat getestet und vom Twitch-Integrator übernommen. Erster Produktionscheck der drei betroffenen Pakete grün. Vier zusätzliche Dashboard-Schematests grün, einschließlich echter TOML-Serialisierungsrunde der Stripe-ID-Zuordnung. Die breite Testausführung und die unabhängige Abnahme laufen noch.

Die unveränderte Engagement-Testfixture hatte im aktuellen Lauf beim Sessionabschluss keine Spalte live_test. Die Fixture lädt nun zusätzlich die bereits vorhandene Migration 20260918024500_smalltalk_candidate_state.sql. Keine Produktionsmigration verändert.
