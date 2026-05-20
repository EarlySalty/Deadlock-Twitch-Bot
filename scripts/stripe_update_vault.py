#!/usr/bin/env python3
"""
Merge new Stripe price/product IDs into Infisical vault secrets.
Uses Claude's Infisical service token. Never prints secret values.
"""
from __future__ import annotations
import json
import os
import sys
from pathlib import Path
from urllib import request, error as url_error

CONFIG_DIR = Path(os.getenv("INFISICAL_CONFIG_DIR") or (Path.home() / ".config" / "claude-infisical"))
DEFAULT_API_URL = "http://127.0.0.1:8080"

NEW_PRICE_IDS = {
    "bundle_werbefrei_analyse": {"1": "price_1TZD6U0yU8I2yGJ0fq8MZaqg", "12": "price_1TZD6V0yU8I2yGJ05Kpagdxs"},
    "bundle_komplett": {"1": "price_1TZD6W0yU8I2yGJ0JQzboooa", "12": "price_1TZD6W0yU8I2yGJ09z3wbpbB"},
}
NEW_PRODUCT_IDS = {
    "bundle_werbefrei_analyse": "prod_UYJjXXe90gt8WO",
    "bundle_komplett": "prod_UYJjhWpzqyNqr0",
}

def _load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))

def _api(url: str, token: str, method: str = "GET", body: dict | None = None):
    req = request.Request(url, method=method)
    req.add_header("Authorization", f"Bearer {token}")
    req.add_header("Content-Type", "application/json")
    if body:
        req.data = json.dumps(body).encode()
    with request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read().decode())

def main():
    st_data = _load_json(CONFIG_DIR / "service_token.json")
    bs_data = _load_json(CONFIG_DIR / "bootstrap_result.json")
    api_url = (os.getenv("INFISICAL_API_URL") or DEFAULT_API_URL).rstrip("/")
    project_id = str(bs_data.get("projectId") or "").strip()
    environment = str(st_data.get("serviceTokenData", {}).get("scopes", [{}])[0].get("environment") or "prod").strip()
    service_token = str(st_data.get("serviceToken") or "").strip()
    if not all([project_id, environment, service_token]):
        print("ERROR: incomplete Infisical config", file=sys.stderr)
        sys.exit(1)

    secrets_url = f"{api_url}/api/v3/secrets/raw?workspaceId={project_id}&environment={environment}&secretPath=/"

    # Fetch all secrets
    resp = _api(secrets_url, service_token)
    secrets = {s["secretKey"]: s for s in resp.get("secrets", [])}

    def get_raw(name: str) -> str:
        s = secrets.get(name) or secrets.get(name.replace("TWITCH_BILLING_", "STRIPE_"))
        if s:
            return str(s.get("secretValue") or "").strip()
        return ""

    def update_secret(name: str, value: str):
        s = secrets.get(name)
        if s:
            url = f"{api_url}/api/v3/secrets/raw/{name}"
            body = {"workspaceId": project_id, "environment": environment, "secretPath": "/", "secretValue": value}
            _api(url, service_token, method="PATCH", body=body)
            print(f"  updated: {name}")
        else:
            url = f"{api_url}/api/v3/secrets/raw/{name}"
            body = {"workspaceId": project_id, "environment": environment, "secretPath": "/", "secretValue": value, "type": "shared"}
            _api(url, service_token, method="POST", body=body)
            print(f"  created: {name}")

    # --- Price map ---
    price_name = "STRIPE_PRICE_ID_MAP" if "STRIPE_PRICE_ID_MAP" in secrets else "TWITCH_BILLING_STRIPE_PRICE_ID_MAP"
    raw_price = get_raw(price_name)
    try:
        price_map: dict = json.loads(raw_price) if raw_price else {}
    except Exception:
        price_map = {}

    for plan_id, cycle_prices in NEW_PRICE_IDS.items():
        existing = dict(price_map.get(plan_id) or {})
        for cycle, price_id in cycle_prices.items():
            existing[cycle] = price_id
        price_map[plan_id] = existing

    print(f"\nUpdating {price_name} ...")
    update_secret(price_name, json.dumps(price_map, separators=(",", ":")))

    # --- Product map ---
    product_name = "STRIPE_PRODUCT_ID_MAP" if "STRIPE_PRODUCT_ID_MAP" in secrets else "TWITCH_BILLING_STRIPE_PRODUCT_ID_MAP"
    raw_product = get_raw(product_name)
    try:
        product_map: dict = json.loads(raw_product) if raw_product else {}
    except Exception:
        product_map = {}

    for plan_id, product_id in NEW_PRODUCT_IDS.items():
        product_map[plan_id] = product_id

    print(f"Updating {product_name} ...")
    update_secret(product_name, json.dumps(product_map, separators=(",", ":")))

    print("\nDone. Restart the bot to load the new price maps.")

if __name__ == "__main__":
    main()
