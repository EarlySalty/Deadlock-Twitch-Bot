# V3 Billing Recheck

Scope: Re-Verifikation P1 `B7-022` mit Git-Archaeologie. Read-only fuer Code und Git; geschrieben wurde nur dieses Audit-Dokument. Keine Secrets, keine Runtime-DB, kein Checkout/Add/Commit/Push.

Verdict: **FIX-CLEAR**.

Der breite Originalsatz "Paid-Resolver prueft nur user_id" ist zu weit: `resolve_plan_snapshot` matcht Stripe-Abos nach Login **oder** User-ID. Der Bug bleibt aber real im vorgelagerten Trial-Auto-Grant: Checkout schreibt `customer_reference` login-first, der Trial-Paid-Guard prueft Billing nur gegen `twitch_user_id` und kann danach per `analytics_trial`-Manual-Override das Stripe-Abo verdecken.

## Ausgangsbefunde

- `findings/B7-engagement-tips.md` nennt B7-022 als P1: Rust-Checkout bevorzugt Login, Trial prueft nur User-ID, Auto-Grant laeuft vor Resolver, Manual-Override gewinnt danach.
- `verified/C4-billing-affiliate.md` schraenkt korrekt ein: Der reine Paid-Resolver in `plan.rs` ist nicht defekt, weil er Login oder User-ID prueft. Kritisch ist der davor laufende Trial-Auto-Grant.

## Git-Archaeologie

Ausgefuehrt:

- `git log --oneline -- rust/crates/tb-dashboard-api/src/handlers/billing_page.rs rust/crates/tb-dashboard-api/src/handlers/trial.rs rust/crates/tb-dashboard-api/src/handlers/plan.rs rust/crates/tb-analytics/src/trial.rs rust/crates/tb-analytics/src/plan.rs`
- `git log --oneline -S"customer_reference" --all -- ...`
- `git log --oneline -S"customer_ref" --all -- ...`
- `git log --oneline -S"client_reference_id" --all -- ...`
- `git blame` auf Checkout-Reference, Checkout-Payload, Trial-Paid-Check, Trial-Auto-Grant und Billing-Resolver.
- `git show` fuer `2f408d3`, `420fccc`, `e3146d4`, `4cc858a`, `7a1d31f`, `8f1a887`, `c2ad150`, `854bf4a`, `8a3fa3a`.

Relevante Timeline:

- `2f408d3 feat(dashboard): Extended-Analytics-Paywall + einmaliger 30-Tage-Trial` fuehrt `trial.rs` ein. `has_active_paid_billing_sub(pool, twitch_user_id)` sucht seitdem nur `LOWER(customer_reference)=LOWER($1)` und bindet `twitch_user_id`.
- `420fccc fix(entitlements): Plan-Resolver +twitch_user_id & extended_gate Stripe-aware` erweitert den **Plan-Resolver** bewusst auf Login oder User-ID. Commit-Message: ein per `twitch_user_id` referenziertes Stripe-Abo wurde sonst nicht gefunden; beide Queries matchen jetzt login oder nicht-leere user_id.
- `e3146d4 feat(tb-analytics): 24h-Grace-Auto-Grant des Analytics-Trials` verdrahtet `check_and_grant_trial_eligibility` vor der Plan-Aufloesung. Dabei wird die existierende Billing-Pruefung nur parametrisiert, aber weiter nur mit `twitch_user_id` aufgerufen.
- `4cc858a tb-dashboard: nativer Abo-/Billing-Bezahlpfad (Block 2A, Stripe-hosted)` fuehrt `billing_page.rs` ein. `customer_reference_for` ist ab Entstehung login-first und der Test `customer_reference_prefers_login` fixiert das. Checkout schreibt diese Reference in `client_reference_id` und `metadata.customer_reference`.
- Spaetere Billing-Commits (`8f1a887`, `c2ad150`, `854bf4a`, `8a3fa3a`) erweitern Profil, Cancel, Preview, Rechnungs-Redirects und Auth, ziehen den Trial-Paid-Guard aber nicht nach.

Blame-Kernstellen:

- `billing_page.rs:756-771` stammt aus `4cc858a`: Kommentar und Code "Login bevorzugt, sonst User-ID".
- `billing_page.rs:217-240` stammt aus `4cc858a`: `client_reference_id` und `metadata.customer_reference` bekommen genau diese Reference.
- `trial.rs:119-121` und `trial.rs:202-209` stammen aus `2f408d3`: Billing-Sub-Pruefung bindet nur `twitch_user_id`.
- `trial.rs:247-255` stammt aus `e3146d4`: Auto-Grant nutzt dieselbe user_id-only-Pruefung.
- `plan.rs:358-380` wurde in `420fccc` bewusst auf Login oder User-ID erweitert.

## Intent / Grillme

Grillme Block 2 belegt die Produktentscheidung "Abo/Billing bauen, aber Stripe-hosted wo moeglich": Webhook -> Entitlements, Checkout-Start, `/twitch/abbo`, Kuendigung, Katalog/Readiness/Product-Price-Sync; eigene Invoice-/Download-/Rechnungsengine wird gedroppt (`rust/docs/audit/2026-06-15-grillme-entscheidungen.md:49-52`).

Das 2026-06-27-Intent-Ledger sagt ebenfalls: Billing/Entitlements minimal und Stripe-hosted; Trial/Entitlement-Lesen bleiben (`rust/docs/audit/2026-06-27/00-baseline.md:62`). Es legt aber **keine explizite Produktentscheidung** "login statt user_id" fest.

Die Referenzstrategie ist trotzdem im implementierten Billing-Pfad belegt:

- `billing_page.rs:756-771`: login-first Helper.
- `stripe/webhook_apply.rs:349-356`: `customer_reference` ist ein Twitch-Login und wird ueber `twitch_streamers_partner_state` auf `twitch_user_id` aufgeloest.
- `affiliate_commission.rs:110-116`: Streamer-Identifier ist `customer_reference` als Twitch-Login.
- `docs/internal/stripe-webhooks-internal.md:16` beschreibt `twitch_billing_subscriptions` als Stripe-State, aus dem `streamer_plans` synchronisiert wird.

Damit sieht die login-Drehung nach bewusster Implementierungsentscheidung fuer den nativen Stripe-Pfad aus. Dass der Trial-Paid-Guard nicht mitgezogen wurde, wirkt nicht bewusst: `420fccc` hatte exakt die Resolver-Luecke "login oder user_id" schon adressiert, `e3146d4` und `4cc858a` beruehren aber unterschiedliche Teile und hinterlassen den alten user_id-only-Guard.

## Repro / Fehlerpfad

Statischer Repro ohne DB-Zugriff:

1. Partner-Session: `twitch_login='streamer'`, `twitch_user_id='42'`.
2. Checkout nutzt `customer_reference_for` und schreibt `customer_reference='streamer'`.
3. Webhook upsertet `twitch_billing_subscriptions(customer_reference='streamer', plan_id='bundle_analysis_raid_boost', status='active')`.
4. `resolve_plan_snapshot(pool, "streamer", "42")` ruft vor der eigentlichen Aufloesung `check_and_grant_trial_eligibility(pool, "42", "streamer")` auf.
5. `trial.rs` sucht Paid-Billing nur mit `customer_reference='42'`; die login-basierte Abo-Zeile wird nicht gefunden.
6. Bei `first_login_at >= 24h`, `trial_ever_granted=0` und keinem bezahlten `manual_plan_id` setzt der Auto-Grant `manual_plan_id='analytics_trial'`.
7. Danach gewinnt der aktive Manual-Override terminal in `plan.rs:342-353`; das Stripe-Abo wird nicht mehr ausgewertet.

Auswirkung: Bei reinen Analyseplaenen bleiben manche Analytics-Rechte gleich, aber Quelle/Status/Ablauf sind falsch. Bei Bundles koennen bezahlte Zusatz-Entitlements wie `chat.promos.disable` oder `raid.priority` bis Trial-Ablauf fehlen. Das ist geld-relevant.

## Entwarnung / Nicht-Fehler

`plan.rs` ist kein user_id-only-Paid-Resolver. Die Query matcht `LOWER(customer_reference)=LOWER(login)` oder, falls vorhanden, `LOWER(customer_reference)=LOWER(user_id)`. Damit ist der breite Teil des Ausgangsbefunds falsch/enger zu formulieren.

Auch `billing_page.rs::active_customer_record` fuer Cancel/Portal matcht Login, User-ID oder primaere Reference. Die Luecke sitzt spezifisch in `tb-analytics/src/trial.rs`.

## Fix-Spec

Minimaler Fix: Trial-Paid-Checks an dieselbe Referenzmenge angleichen wie Checkout/Resolver, ohne Checkout wieder auf User-ID zurueckzudrehen.

Konkrete Stellen:

- `rust/crates/tb-analytics/src/trial.rs:119-121`: `grant_trial_inner` soll `has_active_paid_billing_sub(pool, twitch_user_id, twitch_login)` aufrufen.
- `rust/crates/tb-analytics/src/trial.rs:191-208`: Signatur von `has_active_paid_billing_sub`/`has_active_paid_billing_sub_in` um `twitch_login` erweitern.
- `rust/crates/tb-analytics/src/trial.rs:202-208`: SQL auf `LOWER(customer_reference)=LOWER($login) OR ($user_id <> '' AND LOWER(customer_reference)=LOWER($user_id))` erweitern. Status- und Plan-Allowlist zunaechst unveraendert lassen, ausser Produkt will explizit auch `past_due` wie der Resolver als paid blockierend werten.
- `rust/crates/tb-analytics/src/trial.rs:252-254`: Auto-Grant ebenfalls mit `twitch_login` pruefen.

Tests:

- Neuer DB-Test in `trial.rs`: vorhandene `streamer_plans`-Zeile mit `twitch_user_id='42'`, `twitch_login='streamer'`, `first_login_at` > 24h, `trial_ever_granted=0`; dazu `twitch_billing_subscriptions(customer_reference='streamer', plan_id='bundle_analysis_raid_boost', status='active')`. Erwartung: `check_and_grant_trial_eligibility(&pool, "42", "streamer") == false`, `manual_plan_id` bleibt nicht `analytics_trial`, `trial_ever_granted` bleibt `0`.
- Zweiter Test fuer Self-Claim: login-basierte `twitch_billing_subscriptions(customer_reference='streamer', plan_id='bundle_komplett', status='active')` blockt `start_trial_for_user(&pool, "42", "streamer")` mit `TrialOutcome::HasPaidPlan`.
- Regressionstest optional: user_id-basierte Subscription blockt weiterhin.

Verifikation in dieser Runde: statische Source-Inspection und Git-Archaeologie; keine Tests/Builds, keine DB, keine Secrets.
