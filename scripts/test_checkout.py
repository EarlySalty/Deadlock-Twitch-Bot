"""
Create a Stripe Checkout Session and print the URL for manual testing.

Usage:
  python scripts/test_checkout.py                         # analysis_dashboard, 1 month
  python scripts/test_checkout.py --plan raid_boost       # Raid Boost, 1 month
  python scripts/test_checkout.py --plan bundle --cycle 6 # Bundle, 6 months

Plans: raid_free, raid_boost, analysis_dashboard, bundle_analysis_raid_boost
Cycles: 1, 6, 12

NOTE: This uses your LIVE Stripe keys.
      Use Stripe test card 4242 4242 4242 4242 (exp: any future date, CVC: any 3 digits)
      -- but only if your Stripe account is in TEST mode (sk_test_...).
      With LIVE keys (sk_live_...) only real payment methods work.
"""

from __future__ import annotations

import argparse
import json

import keyring
import stripe

SERVICE = "DeadlockBot"

PLAN_ALIASES = {
    "bundle": "bundle_analysis_raid_boost",
    "analysis": "analysis_dashboard",
    "raid": "raid_boost",
}


def _load_vault(key: str) -> str:
    return (keyring.get_password(SERVICE, key) or "").strip()


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Create a test Stripe Checkout Session"
    )
    parser.add_argument(
        "--plan", default="analysis_dashboard", help="Plan ID (or alias)"
    )
    parser.add_argument(
        "--cycle", type=int, default=1, choices=[1, 6, 12], help="Billing cycle months"
    )
    parser.add_argument(
        "--email", default="test@example.com", help="Prefill customer email"
    )
    args = parser.parse_args()

    plan_id = PLAN_ALIASES.get(args.plan, args.plan)
    cycle = args.cycle

    secret_key = _load_vault("STRIPE_SECRET_KEY") or _load_vault(
        "TWITCH_BILLING_STRIPE_SECRET_KEY"
    )
    if not secret_key:
        raise SystemExit("STRIPE_SECRET_KEY not in vault")

    price_map_raw = _load_vault("STRIPE_PRICE_ID_MAP") or _load_vault(
        "TWITCH_BILLING_STRIPE_PRICE_ID_MAP"
    )
    if not price_map_raw:
        raise SystemExit("STRIPE_PRICE_ID_MAP not in vault")

    price_map: dict[str, dict[str, str]] = json.loads(price_map_raw)
    cycle_map = price_map.get(plan_id)
    if not cycle_map:
        raise SystemExit(
            f"Plan '{plan_id}' not found in price map. Available: {list(price_map)}"
        )

    price_id = cycle_map.get(str(cycle))
    if not price_id:
        raise SystemExit(f"No price for plan='{plan_id}' cycle={cycle}m")

    success_url = (
        _load_vault("STRIPE_CHECKOUT_SUCCESS_URL")
        or _load_vault("TWITCH_BILLING_CHECKOUT_SUCCESS_URL")
        or "https://deutsche-deadlock-community.de/twitch/abbo?checkout=success&session_id={CHECKOUT_SESSION_ID}"
    )
    cancel_url = (
        _load_vault("STRIPE_CHECKOUT_CANCEL_URL")
        or _load_vault("TWITCH_BILLING_CHECKOUT_CANCEL_URL")
        or "https://deutsche-deadlock-community.de/twitch/abbo?checkout=cancelled"
    )

    stripe.api_key = secret_key
    mode_label = "LIVE" if secret_key.startswith("sk_live") else "TEST"

    print(f"=== test_checkout.py [{mode_label} MODE] ===")
    print(f"  Plan:   {plan_id}")
    print(f"  Cycle:  {cycle} month(s)")
    print(f"  Price:  {price_id}")
    print()

    if mode_label == "LIVE":
        print("  [!] LIVE MODE: Only real payment methods accepted.")
        print("      For test cards (4242...) you need sk_test_ keys.")
        print()

    # Retrieve price to show amount
    price_obj = stripe.Price.retrieve(price_id)
    amount = int(price_obj.unit_amount or 0)
    print(f"  Amount: {amount / 100:.2f} EUR (net, 19% VAT added at checkout)")
    print()

    session = stripe.checkout.Session.create(
        mode="subscription",
        line_items=[{"price": price_id, "quantity": 1}],
        customer_email=args.email,
        success_url=success_url,
        cancel_url=cancel_url,
        automatic_tax={"enabled": True},
        metadata={
            "plan_id": plan_id,
            "cycle_months": str(cycle),
            "source": "test_checkout.py",
            "customer_reference": "test_user",
        },
    )

    print(f"  Session ID:  {session.id}")
    print(f"  Expires at:  {session.expires_at}")
    print()
    print("  CHECKOUT URL (open in browser):")
    print(f"  {session.url}")
    print()
    print(
        "  After payment: webhook fires -> twitch_billing_events + twitch_billing_subscriptions updated"
    )


if __name__ == "__main__":
    main()
