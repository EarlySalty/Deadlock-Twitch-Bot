from __future__ import annotations

import keyring
import stripe

SERVICE = "DeadlockBot"
WEBHOOK_URL = "https://deutsche-deadlock-community.de/twitch/api/billing/stripe/webhook"
EVENTS = [
    "checkout.session.completed",
    "checkout.session.expired",
    "customer.subscription.created",
    "customer.subscription.updated",
    "customer.subscription.deleted",
    "invoice.payment_succeeded",
    "invoice.payment_failed",
]


def _read_secret_key() -> str:
    return (
        keyring.get_password(SERVICE, "STRIPE_SECRET_KEY")
        or keyring.get_password(SERVICE, "TWITCH_BILLING_STRIPE_SECRET_KEY")
        or ""
    ).strip()


def _store_webhook_secret(secret: str) -> None:
    keyring.set_password(SERVICE, "STRIPE_WEBHOOK_SECRET", secret)
    keyring.set_password(SERVICE, "TWITCH_BILLING_STRIPE_WEBHOOK_SECRET", secret)


def main() -> None:
    secret_key = _read_secret_key()
    if not secret_key:
        raise SystemExit("missing STRIPE_SECRET_KEY in Windows vault")

    stripe.api_key = secret_key

    endpoint = stripe.WebhookEndpoint.create(
        url=WEBHOOK_URL,
        enabled_events=EVENTS,
        metadata={
            "source": "deutsche-deadlock-community.de",
            "billing": "subscriptions",
            "managed_by": "codex",
        },
    )

    endpoint_id = str(getattr(endpoint, "id", "") or "").strip()
    endpoint_secret = str(getattr(endpoint, "secret", "") or "").strip()
    if not endpoint_id or not endpoint_secret:
        raise SystemExit("failed to create Stripe webhook endpoint")

    _store_webhook_secret(endpoint_secret)

    existing = stripe.WebhookEndpoint.list(limit=100)
    same_url_count = 0
    for item in list(getattr(existing, "data", []) or []):
        if str(getattr(item, "url", "") or "").strip() == WEBHOOK_URL:
            same_url_count += 1

    print(f"created_webhook_endpoint_id={endpoint_id}")
    print("webhook_credentials_saved=true")
    print(f"webhook_url={WEBHOOK_URL}")
    print(f"same_url_endpoint_count={same_url_count}")
    if same_url_count > 1:
        print("warning=multiple_webhook_endpoints_for_same_url")


if __name__ == "__main__":
    main()
