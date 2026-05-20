#!/usr/bin/env python3
"""
Create Stripe products/prices for new bundles and 12-month cycle.
Reads STRIPE_KEY from environment. Never prints the key.
"""
import json
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

# Plans to ensure exist with both cycle=1 and cycle=12
# monthly_net_cents is the base monthly price
NEW_PLANS = [
    {"id": "bundle_werbefrei_analyse", "name": "Werbefrei + Analyse", "monthly_net_cents": 1149,
     "description": "Chat-Werbung dauerhaft aus + volles Analytics-Dashboard"},
    {"id": "bundle_komplett", "name": "Alles drin", "monthly_net_cents": 1399,
     "description": "Werbefrei + Raid Boost + Analytics – das komplette Paket"},
]

CYCLE_DISCOUNTS = {1: 0, 12: 20}

def calc_price(monthly_cents: int, cycle: int) -> int:
    """Total net cents for the full cycle period (with discount)."""
    subtotal = monthly_cents * cycle
    discount = CYCLE_DISCOUNTS.get(cycle, 0)
    discount_cents = (subtotal * discount + 50) // 100 if discount > 0 else 0
    return subtotal - discount_cents

def get_or_create_product(plan_id: str, name: str, description: str) -> str:
    # Search for existing product by metadata
    existing = stripe.Product.search(query=f'metadata["plan_id"]:"{plan_id}"')
    for p in existing.auto_paging_iter():
        if not p.get("deleted"):
            print(f"  product reused: {p['id']}")
            return p["id"]
    # Create new
    product = stripe.Product.create(
        name=name,
        description=description or None,
        metadata={"plan_id": plan_id, "source": SOURCE_META, "billing": "subscriptions"},
    )
    print(f"  product created: {product['id']}")
    return product["id"]

def get_or_create_price(product_id: str, plan_id: str, cycle: int, amount_cents: int) -> str:
    lookup_key = f"{SOURCE_META}/{plan_id}/{cycle}m"
    # Try lookup key first
    try:
        prices = stripe.Price.list(lookup_keys=[lookup_key], active=True)
        for price in prices.auto_paging_iter():
            print(f"  price reused [{cycle}m]: {price['id']}")
            return price["id"]
    except Exception:
        pass
    # Create new
    price = stripe.Price.create(
        currency="eur",
        product=product_id,
        unit_amount=amount_cents,
        recurring={"interval": "month", "interval_count": cycle},
        lookup_key=lookup_key,
        metadata={"plan_id": plan_id, "cycle_months": str(cycle), "source": SOURCE_META},
    )
    print(f"  price created [{cycle}m]: {price['id']} ({amount_cents} EUR-cents)")
    return price["id"]

results: dict[str, dict[str, str]] = {}

for plan in NEW_PLANS:
    plan_id = plan["id"]
    print(f"\n▶ {plan_id}")
    product_id = get_or_create_product(plan_id, plan["name"], plan["description"])
    cycle_map: dict[str, str] = {}
    for cycle in [1, 12]:
        amount = calc_price(plan["monthly_net_cents"], cycle)
        price_id = get_or_create_price(product_id, plan_id, cycle, amount)
        cycle_map[str(cycle)] = price_id
    results[plan_id] = cycle_map

print("\n\n=== Price ID Map (merge into vault) ===")
print(json.dumps(results, indent=2))
print("\nDone.")
