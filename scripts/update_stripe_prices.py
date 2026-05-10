"""
Create Stripe prices for analysis_dashboard and bundle_analysis_raid_boost,
archive the old ones, and persist the new price ID map to the Windows vault.

Run once: python scripts/update_stripe_prices.py
Run dry:  python scripts/update_stripe_prices.py --dry-run
"""

from __future__ import annotations

import json
import sys
from typing import Any

import keyring
import stripe

SERVICE = "DeadlockBot"

# New monthly base prices (net cents)
NEW_MONTHLY_CENTS: dict[str, int] = {
    "analysis_dashboard": 1699,       # €16,99 / Monat
    "bundle_analysis_raid_boost": 2299,  # €22,99 / Monat
    # raid_boost stays at 799 — not touched here
}

CYCLES = [1, 6, 12]
CYCLE_DISCOUNTS = {1: 0, 6: 10, 12: 20}


def _calc_total(monthly_cents: int, cycle: int) -> int:
    subtotal = monthly_cents * cycle
    discount_pct = CYCLE_DISCOUNTS[cycle]
    discount = (subtotal * discount_pct + 50) // 100 if discount_pct else 0
    return subtotal - discount


def _load_stripe_key() -> str:
    key = (
        keyring.get_password(SERVICE, "STRIPE_SECRET_KEY")
        or keyring.get_password(SERVICE, "TWITCH_BILLING_STRIPE_SECRET_KEY")
        or ""
    ).strip()
    if not key:
        raise SystemExit("STRIPE_SECRET_KEY not found in Windows vault")
    return key


def _load_price_map() -> dict[str, dict[str, str]]:
    raw = (
        keyring.get_password(SERVICE, "STRIPE_PRICE_ID_MAP")
        or keyring.get_password(SERVICE, "TWITCH_BILLING_STRIPE_PRICE_ID_MAP")
        or "{}"
    )
    return json.loads(raw)


def _load_product_map() -> dict[str, str]:
    raw = (
        keyring.get_password(SERVICE, "STRIPE_PRODUCT_ID_MAP")
        or keyring.get_password(SERVICE, "TWITCH_BILLING_STRIPE_PRODUCT_ID_MAP")
        or "{}"
    )
    return json.loads(raw)


def _save_price_map(price_map: dict[str, Any]) -> None:
    serialized = json.dumps(price_map, ensure_ascii=True, separators=(",", ":"))
    keyring.set_password(SERVICE, "STRIPE_PRICE_ID_MAP", serialized)
    keyring.set_password(SERVICE, "TWITCH_BILLING_STRIPE_PRICE_ID_MAP", serialized)
    print("  vault updated: STRIPE_PRICE_ID_MAP")


def main(dry_run: bool = False) -> None:
    stripe.api_key = _load_stripe_key()
    price_map = _load_price_map()
    product_map = _load_product_map()

    mode = "DRY RUN — no changes will be made" if dry_run else "LIVE"
    print(f"=== update_stripe_prices.py [{mode}] ===\n")

    for plan_id, monthly_cents in NEW_MONTHLY_CENTS.items():
        product_id = product_map.get(plan_id)
        if not product_id:
            print(f"[SKIP] {plan_id}: no product_id in vault, run sync-products first")
            continue

        print(f"[{plan_id}]  product={product_id}  monthly={monthly_cents} cents (€{monthly_cents/100:.2f})")
        plan_cycle_map: dict[str, str] = price_map.get(plan_id) or {}

        for cycle in CYCLES:
            total_cents = _calc_total(monthly_cents, cycle)
            old_price_id = plan_cycle_map.get(str(cycle), "")

            print(f"  {cycle}m: total={total_cents} cents (€{total_cents/100:.2f})", end="")

            if old_price_id:
                print(f"  [old={old_price_id}]", end="")

            if dry_run:
                print("  -> would create new price + archive old")
                continue

            # Create new price
            new_price = stripe.Price.create(
                unit_amount=total_cents,
                currency="eur",
                recurring={"interval": "month", "interval_count": cycle},
                product=product_id,
                metadata={
                    "plan_id": plan_id,
                    "cycle_months": str(cycle),
                    "source": "update_stripe_prices.py",
                },
            )
            plan_cycle_map[str(cycle)] = new_price.id
            print(f"  -> created {new_price.id}", end="")

            # Archive old price
            if old_price_id and old_price_id != new_price.id:
                try:
                    stripe.Price.modify(old_price_id, active=False)
                    print(f"  archived {old_price_id}", end="")
                except stripe.StripeError as exc:
                    print(f"  [warn: could not archive {old_price_id}: {exc}]", end="")

            print()

        if not dry_run:
            price_map[plan_id] = plan_cycle_map

    if not dry_run:
        _save_price_map(price_map)
        print("\nDone. Restart the bot server to pick up the new price mappings.")
    else:
        print("\nDry run complete — nothing was written.")


if __name__ == "__main__":
    dry_run = "--dry-run" in sys.argv
    main(dry_run=dry_run)
