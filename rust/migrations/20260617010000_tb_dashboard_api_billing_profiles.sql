-- B2-P1-billing-profiles (tb-dashboard-api): Tabelle für persistierte
-- Rechnungsempfänger-Profile (Stripe-Customer-Prefill + Abo-Bezahlpfad).
--
-- Python-Orakel `bot/dashboard/billing/billing_mixin.py:_billing_ensure_storage_tables`
-- (Zeilen 316-327) legt diese Tabelle lazy beim ersten Billing-Request an. Der
-- native Pfad (tb-dashboard-api) verlässt sich nicht auf Lazy-DDL, sondern
-- erstellt sie deklarativ als additive Migration — identische Spalten/Typen wie
-- das gewachsene Prod-Schema (alle TEXT, NOT NULL DEFAULT '', PK auf
-- customer_reference).
--
-- Idempotent über IF NOT EXISTS — bestehende Prod-Tabellen bleiben unangetastet.
CREATE TABLE IF NOT EXISTS public.twitch_billing_profiles (
    customer_reference TEXT PRIMARY KEY,
    recipient_name     TEXT NOT NULL DEFAULT '',
    recipient_email    TEXT NOT NULL DEFAULT '',
    company_name       TEXT NOT NULL DEFAULT '',
    street_line1       TEXT NOT NULL DEFAULT '',
    postal_code        TEXT NOT NULL DEFAULT '',
    city               TEXT NOT NULL DEFAULT '',
    country_code       TEXT NOT NULL DEFAULT '',
    vat_id             TEXT NOT NULL DEFAULT '',
    updated_at         TEXT NOT NULL DEFAULT ''
);
