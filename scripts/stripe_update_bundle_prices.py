#!/usr/bin/env python3
"""
Create new Stripe prices for updated bundle prices (May 2026).

Changes:
  bundle_chat_quiet_raid_boost: 5,99 → 6,99 EUR/mo (less Raid Boost discount)
  bundle_werbefrei_analyse:    11,49 → 10,49 EUR/mo (more Analyse discount)

Reads STRIPE_KEY from env. Never prints the key.
Run: STRIPE_KEY=<key> python3 scripts/stripe_update_bundle_prices.py
"""
import os
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent.parent))

stripe_key = os.environ.get("STRIPE_KEY", "").strip()
if not stripe_key:
    print("ERROR: STRIPE_KEY env var not set", file=sys.stderr)
    sys.exit(1)

import stripe  # noqa: E402
stripe.api_key = stripe_key

SOURCE_META = "deutsche-deadlock-community.de"

UPDATED_PLANS = [
    {
        "id": "bundle_chat_quiet_raid_boost",
        "name": "Werbefrei + Raid Boost",
        "monthly_net_cents": 699,
    },
    {
        "id": "bundle_werbefrei_analyse",
        "name": "Werbefrei + Analyse",
        "monthly_net_cents": 1049,
    },
]


def get_product_id(plan_id: str) -> str:
    existing = stripe.Product.search(query=f'metadata["plan_id"]:"{plan_id}"')
    for p in existing.auto_paging_iter():
        d = p.to_dict() if hasattr(p, "to_dict") else p
        if not d.get("deleted"):
            print(f"  product found: {d['id']}")
            return d["id"]
    raise RuntimeError(f"No product found for plan_id={plan_id!r}")


def create_price(product_id: str, plan_id: str, cycle: int, amount_cents: int) -> str:
    price = stripe.Price.create(
        currency="eur",
        product=product_id,
        unit_amount=amount_cents,
        recurring={"interval": "month", "interval_count": cycle},
        metadata={
            "plan_id": plan_id,
            "cycle_months": str(cycle),
            "source": SOURCE_META,
            "version": "2026-05",
        },
    )
    d = price.to_dict() if hasattr(price, "to_dict") else price
    print(f"  price created [{cycle}m]: {d['id']}  ({amount_cents} EUR-cents = {amount_cents/100:.2f} EUR)")
    return d["id"]


results: dict[str, dict[int, str]] = {}

for plan in UPDATED_PLANS:
    plan_id = plan["id"]
    monthly = plan["monthly_net_cents"]
    print(f"\n{plan['name']} ({plan_id})")

    product_id = get_product_id(plan_id)
    results[plan_id] = {}

    for cycle in (1, 12):
        amount = monthly * cycle  # no annual discount, bonus months handled in DB
        price_id = create_price(product_id, plan_id, cycle, amount)
        results[plan_id][cycle] = price_id

print("\n\n=== Update STRIPE_PRICE_ID_DEFAULTS in billing_plans.py: ===")
for plan_id, cycles in results.items():
    for cycle, price_id in cycles.items():
        print(f'  "{plan_id}": {{{cycle}: "{price_id}", ...}},')

print("\n=== Full new entries for STRIPE_PRICE_ID_DEFAULTS: ===")
for plan_id, cycles in results.items():
    c1 = cycles.get(1, "")
    c12 = cycles.get(12, "")
    print(f'    "{plan_id}": {{1: "{c1}", 12: "{c12}"}},')
