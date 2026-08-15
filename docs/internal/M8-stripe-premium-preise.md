# M8: Stripe-Preise und Timosius (User)

Der Code legt keine Live-Prices an und hängt keine Subscription um.

## Neue Prices in Stripe anlegen

1. Produkt `Premium` anlegen, falls noch keines existiert.
2. Zwei Prices, Endpreis, Steuerverhalten `inclusive`, kein automatischer Steueraufschlag:
   - Monat: 2,99 EUR, Lookup-Key `deadlock_premium_1m_gross_v3`
   - Jahr: 29,90 EUR, Lookup-Key `deadlock_premium_12m_gross_v3`
3. Die erzeugten Price-IDs (`price_…`) in den Code bzw. ins Vault legen:
   - `PRICE_ID_DEFAULTS` für `premium` 1m/12m
   - oder `STRIPE_PRICE_ID_MAP` / `TWITCH_BILLING_STRIPE_PRICE_ID_MAP`
4. Platzhalter `price_pending_premium_1m_gross_v3` und `price_pending_premium_12m_gross_v3` ersetzen.

## Timosius umhängen

- Subscription `sub_1TfbbR0yU8I2yGJ0t0l9uxrT`, customer_reference `123175963`
- Auf den neuen Premium-Preis legen, wirksam zur nächsten Verlängerung
- `proration_behavior=none`, kein rückwirkender Einzug
- Nächstes Rechnungsdatum unverändert lassen
- In Stripe gegenprüfen: Zyklus der DB sagt 12 Monate seit 2026-06-07. Wenn das stimmt, greift die Erhöhung erst im Juni 2027.

## Zweite Subscription klären

`sub_1U1rII0yU8I2yGJ05qW8vjHK` hat keine `customer_reference`. Test oder falsch verknüpfter Kunde.

## Nach dem Anlegen

Ein `customer.subscription.updated` oder Invoice-Event muss `plan_id` und die Perioden in `twitch_billing_subscriptions` füllen. Das Mapping dafür ist M1.

Die Bestandsmigration `20260816121000_streamer_plans_free_premium.sql` und `trials_granted` nicht ohne Freigabe auf Prod anwenden.
