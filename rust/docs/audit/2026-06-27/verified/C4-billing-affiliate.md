# C4 Billing/Affiliate Verifikation

Scope: adversariale Read-only-Verifikation der Befunde `B7-022`, `B5a-001` und `B5a-002`. Keine Secrets, kein Git, keine Runtime-DB. Methode: statische `rg`/`nl`/`sed`-Pruefung der genannten Findings und Rust-/Python-Referenzen.

## B7-022 - Verdict: bestaetigt, aber enger als formuliert

Der reine Paid-Resolver in `plan.rs` ist nicht der Fehler: `resolve_plan_snapshot` sucht Stripe-Abos nach Login **oder** User-ID (`plan.rs:358-380`) und wuerde eine login-referenzierte Subscription als `billing_subscription` zurueckgeben (`plan.rs:383-413`). Die kritische Luecke liegt davor: `resolve_plan_snapshot` ruft den Trial-Auto-Grant vor Manual/Billing-Aufloesung auf (`plan.rs:270-276`), und dieser Trial-Guard prueft aktive Billing-Abos nur gegen `twitch_user_id` (`trial.rs:252-254`, SQL `LOWER(customer_reference)=LOWER($1)` in `trial.rs:203-208`).

Checkout-/Webhook-Trace:

- `billing_page.rs:758-771` baut die `customer_reference` fuer Partner login-first: `twitch_login`, sonst `twitch_user_id`. Der Test `billing_page.rs:926-928` fixiert dieses Verhalten.
- `checkout_start_handler` nutzt genau diese Referenz (`billing_page.rs:180`) und schreibt sie in Stripe als `client_reference_id` sowie `metadata.customer_reference` (`billing_page.rs:217-240`).
- Der Webhook uebernimmt `metadata.customer_reference` bzw. `client_reference_id` (`webhook_apply.rs:562-579`) und upsertet sie nach `twitch_billing_subscriptions.customer_reference` (`webhook_apply.rs:292-315`).
- Der Webhook synchronisiert zusaetzlich nur `streamer_plans.plan_name` (`webhook_apply.rs:349-388`), nicht `manual_plan_id`. Das verhindert den Trial-Grant nicht, weil `trial.rs` nur `manual_plan_id` prueft (`trial.rs:259-295`) und `plan.rs` aktive Manual-Overrides ebenfalls nur aus `manual_plan_id` ableitet (`plan.rs:283-353`).

Konkreter Fehlerpfad:

1. Partner hat `twitch_login='streamer'`, `twitch_user_id='42'`.
2. Rust-Checkout erzeugt ein Abo mit `customer_reference='streamer'`.
3. Webhook legt/aktualisiert `twitch_billing_subscriptions(customer_reference='streamer', plan_id='bundle_analysis_raid_boost', status='active')`.
4. Es existiert eine alte `streamer_plans`-Zeile fuer `42/streamer` mit `first_login_at` aelter als 24h, `trial_ever_granted=0`, und ohne bezahlten `manual_plan_id` bzw. mit `raid_free`.
5. Beim naechsten Plan-Resolve ruft `plan.rs` zuerst `check_and_grant_trial_eligibility(pool, "42", "streamer")` auf.
6. `trial.rs` sucht das aktive Billing-Abo nur mit `customer_reference='42'`; die vorhandene login-basierte Zeile `streamer` wird nicht gefunden.
7. `trial.rs` setzt per UPSERT `manual_plan_id='analytics_trial'` und `manual_plan_expires_at=now+30d` (`trial.rs:301-320`).
8. Zurueck in `plan.rs` gewinnt der nun aktive Manual-Override terminal (`plan.rs:342-353`); die eigentlich vorhandene Stripe-Zeile wird nicht mehr ausgewertet.

Damit kann ein bezahltes Abo tatsaechlich durch einen `analytics_trial` verdeckt werden. Auswirkungen sind je nach Plan unterschiedlich: Bei `analysis_dashboard` bleibt das Analytics-Entitlement gleich, aber Quelle/Status/Ablauf sind falsch; bei Bundles koennen bezahlte Zusatz-Entitlements wie `chat.promos.disable` oder `raid.priority` bis zum Trial-Ablauf verschwinden.

Eintrittswahrscheinlichkeit: realistisch mittel bis hoch fuer Rust-Checkout-Kunden mit normal vorhandenem Twitch-Login und bestehender `streamer_plans.first_login_at`-Zeile aelter als 24h. Kein Treffer, wenn die Subscription noch user_id-basiert ist, kein `streamer_plans`-Eintrag existiert, `first_login_at` fehlt/jung ist, `trial_ever_granted=1` ist oder bereits ein bezahlter `manual_plan_id` gesetzt ist.

## B5a-001 - Verdict: bestaetigt

Rust bewirbt weiterhin nicht registrierte Invoice-Pfade:

- `billing_page.rs:653-654` und `billing_page.rs:744-745` liefern `invoice_preview_path="/twitch/api/billing/invoice-preview"` und `invoice_page_path="/twitch/abbo/rechnung"`.
- Der native Billing-Router registriert nur Abbo-Redirects, Checkout, Kuendigung, `/twitch/abbo/rechnungen`, Rechnungsdaten, Catalog, Readiness, Checkout-Preview und Stripe-Sync (`lib.rs:995-1044`).
- Harte Route-Suche nach `.route("/twitch/api/billing/invoice-preview"...)`, `.route("/twitch/abbo/rechnung"...)` und `.route("/twitch/abbo/stripe-settings"...)` in Rust ergab keinen Treffer.

Folge: beworbene Preview-/Page-Links laufen nativ auf 404 bzw. nur auf einen optionalen Legacy-Fallback, der laut Finding nicht als native Paritaet zaehlt. Python hatte dagegen `POST /twitch/api/billing/invoice-preview` und `GET /twitch/abbo/rechnung` registriert (`routes_billing.py:20-41`, `routes_billing.py:302-372`).

## B5a-002 - Verdict: bestaetigt, mit vorhandener Teilabdeckung

Rust hat eine native Affiliate-Teilflaeche, aber nicht die behauptete Self-Service-/OAuth-Paritaet:

- Vorhanden: `/twitch/affiliate/portal` als SPA-Shell (`lib.rs:1121-1132`) und `/twitch/api/v2/affiliate/portal` als Read-Model (`lib.rs:329-330`, `affiliate_portal.rs:33-157`).
- Vorhanden: Admin-Affiliate-APIs unter `/twitch/api/admin/affiliates...` (`lib.rs:654-679`).
- Nicht vorhanden: native Routen fuer `/twitch/auth/affiliate/login`, `/twitch/auth/affiliate/callback`, `/twitch/affiliate/connect/stripe`, `/twitch/affiliate/connect/stripe/callback`, `/twitch/affiliate/claim` und Legacy `/twitch/api/affiliate/*`.
- Die Treffer in `partner_gate.rs:40-59` sind nur Allowlist-/Gate-Pfade, keine Router-Registrierung.

Python registriert diese Self-Service-Flaeche in `affiliate_mixin.py:1464-1511` und nutzt separate `affiliate_oauth_state`/`affiliate_connect_state`-Stores (`affiliate_mixin.py:186-215`). In Rust fand `rg` keine entsprechenden nativen State-Typen oder Route-Registrierungen. PDF/Payout/Gutschrift-Generierung bleibt wie im Ausgangsbefund bewusst ausserhalb dieses Verdicts.

## Kurzfazit

- B7-022: bestaetigt als Trial-Preemption-Bug. Die breite Aussage "Paid-Resolver prueft nur user_id" ist fuer `plan.rs` falsch, aber der vorauslaufende Trial-Grant kann paid Billing real verdecken.
- B5a-001: bestaetigt, Invoice-Preview/Page werden beworben und nicht nativ geroutet.
- B5a-002: bestaetigt, Affiliate-Portal-Read-Model ist da, OAuth/Connect/Claim/Profile-Legacy-API fehlt nativ.
