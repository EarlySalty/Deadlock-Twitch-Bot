from __future__ import annotations

import json
from getpass import getpass

import keyring

SERVICE = "DeadlockBot"


def _prompt(name: str, *, secret: bool = False, default: str = "") -> str:
    label = f"{name}"
    if default:
        label = f"{label} [{default}]"
    label = f"{label}: "
    if secret:
        value = getpass(label).strip()
    else:
        value = input(label).strip()
    if not value:
        return default
    return value


def _set(key: str, value: str) -> None:
    keyring.set_password(SERVICE, key, value)
    print(f"stored {key}")


def main() -> None:
    print("Stripe Vault Setup (Windows Credential Manager via keyring)")
    publishable = _prompt("STRIPE_PUBLISHABLE_KEY", secret=True)
    secret = _prompt("STRIPE_SECRET_KEY", secret=True)
    webhook = _prompt("STRIPE_WEBHOOK_SECRET", secret=True)
    success_url = _prompt(
        "STRIPE_CHECKOUT_SUCCESS_URL",
        default="https://deutsche-deadlock-community.de/twitch/abbo?checkout=success&session_id={CHECKOUT_SESSION_ID}",
    )
    cancel_url = _prompt(
        "STRIPE_CHECKOUT_CANCEL_URL",
        default="https://deutsche-deadlock-community.de/twitch/abbo?checkout=cancelled",
    )
    price_map_raw = _prompt(
        "STRIPE_PRICE_ID_MAP JSON (optional)",
        default="",
    )
    product_map_raw = _prompt(
        "STRIPE_PRODUCT_ID_MAP JSON (optional)",
        default="",
    )

    if publishable:
        _set("STRIPE_PUBLISHABLE_KEY", publishable)
        _set("TWITCH_BILLING_STRIPE_PUBLISHABLE_KEY", publishable)
    if secret:
        _set("STRIPE_SECRET_KEY", secret)
        _set("TWITCH_BILLING_STRIPE_SECRET_KEY", secret)
    if webhook:
        _set("STRIPE_WEBHOOK_SECRET", webhook)
        _set("TWITCH_BILLING_STRIPE_WEBHOOK_SECRET", webhook)

    _set("STRIPE_CHECKOUT_SUCCESS_URL", success_url)
    _set("TWITCH_BILLING_CHECKOUT_SUCCESS_URL", success_url)
    _set("STRIPE_CHECKOUT_CANCEL_URL", cancel_url)
    _set("TWITCH_BILLING_CHECKOUT_CANCEL_URL", cancel_url)

    if price_map_raw:
        try:
            json.loads(price_map_raw)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"invalid STRIPE_PRICE_ID_MAP JSON: {exc}") from exc
        _set("STRIPE_PRICE_ID_MAP", price_map_raw)
        _set("TWITCH_BILLING_STRIPE_PRICE_ID_MAP", price_map_raw)

    if product_map_raw:
        try:
            json.loads(product_map_raw)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"invalid STRIPE_PRODUCT_ID_MAP JSON: {exc}") from exc
        _set("STRIPE_PRODUCT_ID_MAP", product_map_raw)
        _set("TWITCH_BILLING_STRIPE_PRODUCT_ID_MAP", product_map_raw)

    print("done")


if __name__ == "__main__":
    main()
