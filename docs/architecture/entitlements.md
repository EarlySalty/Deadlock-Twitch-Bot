# entitlements/ — Architektur & Funktionsreferenz

> Pfad: `bot/entitlements/` · Stand: 2026-06-08 · 4 Dateien, ~507 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [dashboard.md](dashboard.md) (Billing/Plan-Gating), [storage.md](storage.md) (`ensure_billing_entitlement_schema`).

## 1. Zweck & Abgrenzung

`entitlements/` ist die **gemeinsame Wahrheit über Pläne und Feature-Berechtigungen**. Es beantwortet: „Welchen Plan hat dieser Streamer (manuell gesetzt oder via Stripe-Abo) und welche Features schaltet der frei?“ Beide Seiten — Dashboard und Bot — nutzen dieselbe Logik, damit Plan-Gating konsistent ist.

Abgrenzung: Es **bezahlt/verwaltet** keine Abos (das ist `dashboard/billing`), sondern **interpretiert** den gespeicherten Zustand zu konkreten Entitlements.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `dashboard/billing` + diverse Feature-Gates (Bot + Dashboard). |
| **Nutzt** | `storage/` (Billing-/Entitlement-Tabellen, manuelle Overrides). |
| **DB-Tabellen** | Billing-Subscription + manuelle Plan-Overrides (Schema via `storage.ensure_billing_entitlement_schema`). |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `repository.py` | 265 | Persistenz: Override/Subscription laden, Plan-Snapshot auflösen. |
| `catalog.py` | 175 | Kanonische Plan-Metadaten + abgeleitete Entitlements. |
| `resolver.py` | 37 | Dünne Auflösungs-Fassade. |
| `__init__.py` | 30 | Öffentliche Symbole. |

## 4. Datenfluss / Lebenszyklus

Für eine Streamer-Referenz (Login/User-ID) lädt `repository.resolve_plan_snapshot(conn, refs, *, fallback_ref)`:
1. einen **manuellen Override** (`load_manual_override`) — Admin-gesetzte Pläne haben Vorrang,
2. sonst die **Stripe-Subscription** (`load_billing_subscription`),
3. und baut daraus einen `build_plan_snapshot(...)` (Plan-ID, Tier, Gültigkeit).

`catalog` übersetzt die Plan-ID dann in konkrete Entitlements: `plan_entitlements(plan_id)` / `plan_has_entitlement(plan_id, entitlement)`. Legacy-Plannamen werden über `normalize_plan_id_from_legacy_name` / `legacy_plan_name_has_entitlement` abgebildet.

## 5. Funktionsreferenz pro Datei

### catalog.py
- `normalize_plan_id(raw_plan_id) -> str` / `normalize_plan_id_from_legacy_name(raw_plan_name) -> str` — Plan-ID normalisieren (auch aus alten Namen).
- `plan_tier(plan_id) -> str` / `plan_display_name(plan_id) -> str` / `plan_is_extended(plan_id) -> bool` — abgeleitete Eigenschaften.
- `plan_entitlements(plan_id) -> tuple[str, ...]` — Feature-Set des Plans.
- `plan_has_entitlement(plan_id, entitlement) -> bool` / `legacy_plan_name_has_entitlement(raw_plan_name, entitlement) -> bool` — Einzel-Check.

### repository.py
- `resolve_plan_snapshot(conn, refs, *, fallback_ref="") -> dict` — der zentrale Einstiegspunkt (Override → Subscription → Snapshot).
- `load_manual_override(conn, refs) -> dict | None` / `manual_override_from_row(row)` — Admin-Override.
- `load_billing_subscription(conn, refs) -> dict | None` — Stripe-Abo.
- `build_plan_snapshot(*, manual_override, billing_subscription, fallback_ref) -> dict` — beides zu einem Snapshot kombinieren.
- Robustheit: `is_missing_current_period_end_error`, `is_missing_manual_override_metadata_error`, `parse_datetime_value`, `normalize_candidate_refs`, `row_value`.

### resolver.py
Dünne Fassade, die `repository` + `catalog` zu einem bequemen „ref → Entitlements“-Aufruf zusammenführt.

## 6. Datenbank & externe Schnittstellen

- **DB:** Billing-Subscription + manuelle Plan-Overrides (Schema über `storage.ensure_billing_entitlement_schema`). Keine direkten externen Dienste (Stripe-I/O liegt in `dashboard/billing`).

## 7. Stolperfallen / Besonderheiten

- **Manueller Override schlägt Abo:** Ein Admin-gesetzter Plan hat Vorrang vor der Stripe-Subscription — gewollt (z. B. Trials, Sonderfälle, Jahresabo-Bonusmonate).
- **Legacy-Plannamen:** Alte Namens-basierte Pläne werden auf kanonische IDs gemappt; beim Vergleichen `normalize_plan_id*` benutzen, nie Rohnamen.
- **Schema-Toleranz:** `repository` fängt fehlende Spalten ab (`is_missing_*_error`) — die Entitlement-Tabellen werden idempotent migriert und dürfen partiell „alt“ sein.
- **Geteilte Wahrheit:** Dashboard und Bot müssen dieselbe `catalog`/`repository`-Logik nutzen; doppelte Gating-Logik anderswo würde driften.
