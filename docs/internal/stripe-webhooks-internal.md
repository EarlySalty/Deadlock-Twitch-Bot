# Stripe-Webhooks Internal

## Einstieg

Die Billing-Webhooks laufen ueber `POST /twitch/api/billing/stripe/webhook`. Der Handler validiert zuerst die Signatur gegen das konfigurierte Webhook-Secret. Erst danach wird das Event verarbeitet.

Vor der eigentlichen Fachlogik wird jedes Event in `twitch_billing_events` protokolliert. `stripe_event_id` dient dabei als Dedupe-Schluessel, damit doppelt zugestellte Events nicht mehrfach Seiteneffekte ausloesen.

## Persistente Kerntabellen

- `twitch_billing_events`: roher Event-Eingang, Event-Typ, Objekt-ID, Empfangszeit, Payload
- `twitch_billing_subscriptions`: normalisierte Subscription-Sicht fuer Stripe-Status, Plan, Periode und Cancel-Flags
- `streamer_plans`: abgeleiteter Bot-/Entitlement-State fuer die restliche Anwendung
- `twitch_billing_profiles`: Rechnungsdaten fuer die Billing-Surface

Die zentrale Regel ist: Stripe-State wird in `twitch_billing_subscriptions` gehalten, daraus wird der wirksame Plan in `streamer_plans` synchronisiert.

## Verarbeitete Event-Typen

### `customer.subscription.*`

Alle Subscription-Events mit diesem Prefix laufen ueber denselben Pfad:

1. Subscription-Objekt in internes Payload mappen
2. `twitch_billing_subscriptions` upserten
3. Plan nach `streamer_plans` synchronisieren

Das deckt Aktivierung, Trialing, Statuswechsel und spaetere Updates wie `cancel_at_period_end` ab.

### `checkout.session.completed`

Nur Sessions mit `mode=subscription` sind relevant. Der Handler:

1. liest `plan_id`, `cycle_months`, `quantity` und `customer_reference` aus den Checkout-Metadaten
2. zieht bei Bedarf das echte Stripe-Subscription-Objekt nach
3. schreibt den Subscription-State in `twitch_billing_subscriptions`
4. synchronisiert den effektiven Plan nach `streamer_plans`

Wenn bei Jahreskaeufen `bonus_months` in der Subscription-Metadata stehen, wird zusaetzlich ein manueller Bonus-Zeitraum in `streamer_plans.manual_plan_expires_at` gesetzt.

### `invoice.payment_succeeded`

Dieses Event markiert die Subscription lokal als `active`. Danach wird die Affiliate-Kommission verarbeitet. Das ist wichtig: Affiliate-Provisionen haengen an erfolgreicher Rechnungszahlung, nicht nur an einem gestarteten Checkout.

### `invoice.payment_failed`

Hier wird die Subscription lokal auf `past_due` gesetzt. Weitere Seiteneffekte sind bewusst klein gehalten; das Dashboard kann den Status anzeigen, aber der Handler versucht nicht, selbst kreative Recovery-Logik zu bauen.

## Trial-Lifecycle

Es gibt zwei Trial-Pfade:

- Checkout fuer den monatlichen `analysis_dashboard`-Plan setzt `trial_period_days=30`
- zusaetzlich existiert ein interner/manualer Trial-Start fuer authentifizierte User

Der Schutz dagegen, denselben Trial mehrfach zu vergeben, liegt in `streamer_plans.trial_ever_granted`.

Hinweis: Im Code gibt es aktuell widerspruechliche Kommentare zur Trial-Dauer, waehrend die umgesetzte Logik an mehreren Stellen `30 Tage` verwendet. Fuer Produktentscheidungen die Laufzeit aus der Logik lesen, nicht aus alten Kommentaren.

## Kuendigung

`/twitch/abbo/kuendigen` versucht zuerst das Stripe-Customer-Portal zu oeffnen. Wenn das nicht verfuegbar ist, faellt der Code auf `cancel_at_period_end=True` direkt auf der Subscription zurueck. Danach:

- wird der lokale Subscription-State sofort aktualisiert
- spaetere `customer.subscription.updated`-Events bestaetigen denselben Zustand erneut

Die lokale Fachbedeutung ist: Kuendigung beendet den Zugang nicht sofort, sondern markiert das Ende der laufenden Periode.

## Refunds und Gaps

Es gibt aktuell keinen dedizierten Webhook-Pfad fuer Refund-/Charge-Refund-Events. Das bedeutet:

- Rueckerstattungen fuehren nicht automatisch zu einer eigenen lokalen Billing-Aktion
- es gibt keinen expliziten Commission-Reversal-Pfad nur aufgrund eines Refund-Events
- Plan-Aenderungen passieren nur dann automatisch, wenn Stripe zusaetzlich ein abgedecktes Subscription- oder Invoice-Event liefert

Fuer Support-Faelle mit Refunds ist deshalb heute ein manueller Blick auf Billing- und Affiliate-State noetig.

## Operative Checks

- `twitch_billing_events` auf Dedupe und Event-Typ pruefen
- `twitch_billing_subscriptions` gegen Stripe-Realitaet vergleichen
- `streamer_plans` auf abgeleiteten Effekt kontrollieren
- bei Jahreskaeufen Bonusmonate im manuellen Override mitpruefen
