"""Route group for billing-related dashboard handlers."""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

from aiohttp import web

from . import abbo_billing_routes as _abbo_billing_routes
from .route_deps import BillingRouteDeps


def build_route_defs(server: Any) -> list[web.RouteDef]:
    """Return route definitions for billing and legal routes."""
    return [
        web.get("/robots.txt", server.robots_txt),
        web.get("/twitch/legal/access", server.legal_access_page),
        web.post("/twitch/legal/verify", server.legal_verify),
        web.get("/twitch/abo", server.pricing_redirect),
        web.get("/twitch/abbo", server.pricing_redirect),
        web.get("/twitch/abos", server.pricing_redirect),
        web.get("/twitch/abbo/bezahlen", server.abbo_pay),
        web.post("/twitch/abbo/rechnungsdaten", server.abbo_profile_save),
        web.get("/twitch/abbo/kündigen", server.abbo_cancel),
        web.post("/twitch/abbo/kündigen", server.abbo_cancel),
        web.get("/twitch/abbo/rechnungen", server.abbo_invoices),
        web.get("/twitch/abbo/stripe-settings", server.abbo_stripe_settings),
        web.get("/twitch/abbo/rechnung", server.abbo_invoice),
        web.get("/twitch/impressum", server.abbo_impressum),
        web.get("/twitch/datenschutz", server.abbo_datenschutz),
        web.get("/twitch/agb", server.abbo_agb),
        web.get("/twitch/api/billing/catalog", server.api_billing_catalog),
        web.get("/twitch/api/v2/billing/catalog", server.api_billing_catalog),
        web.get("/twitch/api/billing/readiness", server.api_billing_readiness),
        web.post("/twitch/api/billing/stripe/webhook", server.api_billing_stripe_webhook),
        web.post("/twitch/api/billing/checkout-preview", server.api_billing_checkout_preview),
        web.post("/twitch/api/billing/checkout-session", server.api_billing_checkout_session),
        web.post("/twitch/api/billing/invoice-preview", server.api_billing_invoice_preview),
        web.post("/twitch/api/billing/stripe/sync-products", server.api_billing_stripe_sync_products),
    ]


async def pricing_redirect(_server: Any, _request: web.Request) -> web.StreamResponse:
    raise web.HTTPMovedPermanently("/twitch/pricing")


async def abbo_pay(server: Any, request: web.Request) -> web.StreamResponse:
    return await _abbo_billing_routes.abbo_pay(server, request)


async def abbo_profile_save(server: Any, request: web.Request) -> web.StreamResponse:
    return await _abbo_billing_routes.abbo_profile_save(server, request)


async def abbo_cancel(server: Any, request: web.Request) -> web.StreamResponse:
    return await _abbo_billing_routes.abbo_cancel(server, request)


async def abbo_invoices(server: Any, request: web.Request) -> web.StreamResponse:
    return await _abbo_billing_routes.abbo_invoices(server, request)


async def abbo_stripe_settings(server: Any, request: web.Request) -> web.StreamResponse:
    return await _abbo_billing_routes.abbo_stripe_settings(server, request)


async def abbo_invoice(server: Any, request: web.Request) -> web.StreamResponse:
    return await _abbo_billing_routes.abbo_invoice(server, request)


async def api_billing_catalog(
    server: Any,
    request: web.Request,
    *,
    deps: BillingRouteDeps,
) -> web.Response:
    """Expose prepared subscription plans and cycle pricing for dashboard UI."""
    billing_is_paid_plan = deps.billing_is_paid_plan
    build_billing_catalog = deps.build_billing_catalog
    json_module = deps.json

    if not server._check_v2_auth(request):
        return web.json_response({"error": "auth_required"}, status=401)

    cycle_raw = (request.query.get("cycle") or "1").strip()
    readiness = server._billing_stripe_readiness_payload()
    payload = build_billing_catalog(cycle_raw, readiness=readiness)
    cycle = int(payload.get("cycle_months") or 1)
    price_map = server._billing_price_id_map()
    current_plan = server._billing_current_plan_for_request(request)
    current_plan_id = str(current_plan.get("plan_id") or "raid_free").strip() or "raid_free"
    for plan in list(payload.get("plans") or []):
        plan_id = str(plan.get("id") or "").strip()
        plan["is_current"] = bool(plan_id and plan_id == current_plan_id)
        if not billing_is_paid_plan(plan):
            plan["checkout_available"] = False
            plan["stripe_price_id"] = None
            continue
        price_id = server._billing_price_id_for_plan(plan_id, cycle, price_map=price_map)
        plan["stripe_price_id"] = price_id or None
        plan["checkout_available"] = bool(price_id and readiness.get("checkout_ready"))

    payment = dict(payload.get("payment") or {})
    payment["invoice_preview_path"] = "/twitch/api/billing/invoice-preview"
    payment["invoice_page_path"] = "/twitch/abbo/rechnung"
    payment["stripe_sync_path"] = "/twitch/api/billing/stripe/sync-products"
    payload["payment"] = payment
    payload["current_subscription"] = current_plan
    return web.json_response(payload, dumps=lambda data: json_module.dumps(data, ensure_ascii=True))


async def api_billing_readiness(
    server: Any,
    request: web.Request,
    *,
    deps: BillingRouteDeps,
) -> web.Response:
    """Expose Stripe setup readiness without leaking any secrets."""
    json_module = deps.json

    if not server._check_v2_auth(request):
        return web.json_response({"error": "auth_required"}, status=401)

    payload = server._billing_stripe_readiness_payload()
    return web.json_response(payload, dumps=lambda data: json_module.dumps(data, ensure_ascii=True))


async def api_billing_stripe_webhook(
    server: Any,
    request: web.Request,
    *,
    deps: BillingRouteDeps,
) -> web.Response:
    """Receive and verify Stripe webhook events for subscription lifecycle updates."""
    json_module = deps.json
    log = deps.log
    storage_module = deps.storage

    server._billing_refresh_runtime_secrets()
    webhook_secret = str(getattr(server, "_billing_stripe_webhook_secret", "") or "").strip()
    if not webhook_secret:
        return web.json_response({"error": "stripe_webhook_secret_missing"}, status=503)

    stripe, _import_error = server._billing_import_stripe()
    if stripe is None:
        return web.json_response({"error": "stripe_sdk_missing"}, status=503)

    payload_bytes = await request.read()
    signature = str(request.headers.get("Stripe-Signature") or "").strip()
    if not signature:
        return web.json_response({"error": "stripe_signature_missing"}, status=400)

    try:
        event = stripe.Webhook.construct_event(
            payload=payload_bytes,
            sig_header=signature,
            secret=webhook_secret,
        )
    except Exception:
        return web.json_response({"error": "invalid_stripe_signature"}, status=400)

    event_id = str(server._billing_stripe_obj_get(event, "id", "") or "").strip()
    event_type = str(server._billing_stripe_obj_get(event, "type", "") or "").strip()
    event_data = server._billing_stripe_obj_get(event, "data", {}) or {}
    event_object = server._billing_stripe_obj_get(event_data, "object", {}) or {}
    object_id = str(server._billing_stripe_obj_get(event_object, "id", "") or "").strip()
    livemode = bool(server._billing_stripe_obj_get(event, "livemode", False))
    payload_text = payload_bytes.decode("utf-8", errors="replace")
    received_at = datetime.now(UTC).isoformat()

    duplicate = False
    action = "ignored"
    try:
        with storage_module.transaction() as conn:
            server._billing_ensure_storage_tables(conn)
            if event_id:
                try:
                    conn.execute(
                        """
                        INSERT INTO twitch_billing_events (
                            stripe_event_id,
                            event_type,
                            object_id,
                            received_at,
                            livemode,
                            payload
                        ) VALUES (%s, %s, %s, %s, %s, %s)
                        """,
                        (
                            event_id,
                            event_type,
                            object_id,
                            received_at,
                            1 if livemode else 0,
                            payload_text,
                        ),
                    )
                except Exception as exc:
                    err_text = str(exc).lower()
                    if "unique" in err_text or "primary key" in err_text:
                        duplicate = True
                    else:
                        raise

            if not duplicate:
                action = server._billing_apply_webhook_event(
                    conn,
                    stripe=stripe,
                    event_id=event_id,
                    event_type=event_type,
                    event_object=event_object,
                )
    except Exception:
        log.exception("stripe webhook processing failed")
        return web.json_response({"error": "stripe_webhook_processing_failed"}, status=500)

    return web.json_response(
        {
            "ok": True,
            "status": "duplicate" if duplicate else "processed",
            "event_id": event_id,
            "event_type": event_type,
            "action": action,
        },
        dumps=lambda data: json_module.dumps(data, ensure_ascii=True),
    )


async def api_billing_checkout_preview(
    server: Any,
    request: web.Request,
    *,
    deps: BillingRouteDeps,
) -> web.Response:
    """Validate plan selection and return Stripe-ready checkout metadata."""
    build_billing_catalog = deps.build_billing_catalog
    quickstart_url = deps.billing_stripe_quickstart_url
    json_module = deps.json

    if not server._check_v2_auth(request):
        return web.json_response({"error": "auth_required"}, status=401)

    body = await server._billing_read_request_body(request)
    selected_plan_id = str(body.get("plan_id") or "").strip()
    catalog = build_billing_catalog(body.get("cycle_months"))
    selected_plan = next(
        (plan for plan in catalog["plans"] if str(plan.get("id")) == selected_plan_id),
        None,
    )
    if not selected_plan:
        return web.json_response(
            {
                "error": "unknown_plan_id",
                "available_plan_ids": [str(plan.get("id")) for plan in catalog["plans"]],
            },
            status=404,
        )

    total_cents = int((selected_plan.get("price") or {}).get("total_net_cents") or 0)
    readiness = server._billing_stripe_readiness_payload()
    cycle = int(catalog.get("cycle_months") or 1)
    price_id = server._billing_price_id_for_plan(selected_plan_id, cycle)
    checkout_possible = bool(
        total_cents > 0 and readiness.get("checkout_ready") and readiness.get("price_map_ready") and price_id
    )
    if total_cents <= 0:
        message = "Dieser Plan bleibt kostenlos und benoetigt keinen Stripe-Checkout."
    elif checkout_possible:
        message = "Stripe Checkout ist bereit und kann direkt gestartet werden."
    elif readiness.get("checkout_ready") and not price_id:
        message = "Checkout Keys sind gesetzt, aber für diesen Plan fehlt noch eine Stripe Price ID."
    else:
        message = "Stripe Checkout ist noch nicht vollstaendig konfiguriert."
    payload = {
        "ready": bool(total_cents <= 0 or checkout_possible),
        "provider": "stripe",
        "integration_state": str(readiness.get("integration_state") or "planned"),
        "currency": catalog["currency"],
        "tax_mode": catalog["tax_mode"],
        "gross_available": catalog["gross_available"],
        "cycle_months": catalog["cycle_months"],
        "cycle_label": catalog["cycle_label"],
        "plan": selected_plan,
        "stripe_price_id": price_id or None,
        "checkout_session_path": "/twitch/api/billing/checkout-session",
        "invoice_preview_path": "/twitch/api/billing/invoice-preview",
        "invoice_page_path": "/twitch/abbo/rechnung",
        "message": message,
        "stripe_docs_url": quickstart_url,
        "next_steps": [
            "stripe_product_price_ids_hinterlegen",
            "checkout_session_endpoint_live_testen",
            "webhook_verarbeitung_fuer_abos_aktivieren",
        ],
    }
    return web.json_response(payload, dumps=lambda data: json_module.dumps(data, ensure_ascii=True))


async def api_billing_checkout_session(
    server: Any,
    request: web.Request,
    *,
    deps: BillingRouteDeps,
) -> web.Response:
    """Create a live Stripe Checkout Session for a paid billing plan."""
    build_billing_catalog = deps.build_billing_catalog
    format_eur_cents = deps.format_eur_cents
    json_module = deps.json

    if not server._check_v2_auth(request):
        return web.json_response({"error": "auth_required"}, status=401)

    body = await server._billing_read_request_body(request)
    selected_plan_id = str(body.get("plan_id") or "").strip()
    if not selected_plan_id:
        return web.json_response(
            {"error": "plan_id_required", "contract_version": "2026-02-27", "required_fields": ["plan_id"]},
            status=400,
        )

    catalog = build_billing_catalog(body.get("cycle_months"))
    selected_plan = next(
        (plan for plan in catalog["plans"] if str(plan.get("id")) == selected_plan_id),
        None,
    )
    if not selected_plan:
        return web.json_response(
            {
                "error": "unknown_plan_id",
                "contract_version": "2026-02-27",
                "available_plan_ids": [str(plan.get("id")) for plan in catalog["plans"]],
            },
            status=404,
        )

    quantity_raw = body.get("quantity", 1)
    try:
        quantity = int(quantity_raw or 1)
    except (TypeError, ValueError):
        quantity = -1
    if quantity < 1 or quantity > 24:
        return web.json_response(
            {"error": "invalid_quantity", "contract_version": "2026-02-27", "allowed_range": [1, 24]},
            status=400,
        )

    unit_net_cents = int((selected_plan.get("price") or {}).get("total_net_cents") or 0)
    if unit_net_cents <= 0:
        return web.json_response(
            {"error": "free_plan_no_checkout_required", "contract_version": "2026-02-27", "plan_id": selected_plan_id},
            status=400,
        )

    default_success_url = str(getattr(server, "_billing_checkout_success_url", "") or "").strip()
    default_cancel_url = str(getattr(server, "_billing_checkout_cancel_url", "") or "").strip()
    success_url = str(body.get("success_url") or default_success_url).strip()
    cancel_url = str(body.get("cancel_url") or default_cancel_url).strip()
    allowed_redirect_hosts = list(server._billing_checkout_allowed_redirect_hosts())
    if success_url and not server._billing_is_http_url(success_url):
        return web.json_response(
            {"error": "invalid_success_url", "contract_version": "2026-02-27", "field": "success_url", "allowed_hosts": allowed_redirect_hosts},
            status=400,
        )
    if cancel_url and not server._billing_is_http_url(cancel_url):
        return web.json_response(
            {"error": "invalid_cancel_url", "contract_version": "2026-02-27", "field": "cancel_url", "allowed_hosts": allowed_redirect_hosts},
            status=400,
        )

    readiness = server._billing_stripe_readiness_payload()
    if not bool(readiness.get("checkout_ready")):
        return web.json_response(
            {
                "error": "checkout_not_ready",
                "contract_version": "2026-02-27",
                "missing": list(readiness.get("missing") or []),
                "readiness": readiness,
            },
            status=409,
        )
    if not bool(readiness.get("price_map_ready")):
        return web.json_response(
            {
                "error": "stripe_price_id_map_missing",
                "contract_version": "2026-02-27",
                "required_price_ids": int(readiness.get("required_price_ids") or 0),
                "mapped_price_ids": int(readiness.get("mapped_price_ids") or 0),
                "missing_price_slots": list(readiness.get("missing_price_slots") or []),
            },
            status=409,
        )
    if not success_url or not cancel_url:
        return web.json_response(
            {"error": "missing_checkout_urls", "contract_version": "2026-02-27", "required_fields": ["success_url", "cancel_url"]},
            status=409,
        )

    cycle_months = int(catalog.get("cycle_months") or 1)
    stripe_price_id = server._billing_price_id_for_plan(selected_plan_id, cycle_months)
    if not stripe_price_id:
        return web.json_response(
            {"error": "missing_stripe_price_id", "contract_version": "2026-02-27", "plan_id": selected_plan_id, "cycle_months": cycle_months},
            status=409,
        )

    customer_reference = server._billing_primary_ref_for_request(request)
    billing_profile = server._billing_profile_for_request(request)
    customer_email = str(body.get("customer_email") or billing_profile.get("recipient_email") or "").strip()
    idempotency_key = str(request.headers.get("Idempotency-Key") or body.get("idempotency_key") or "").strip()

    raw_metadata = body.get("metadata")
    metadata: dict[str, str] = {}
    if isinstance(raw_metadata, dict):
        for raw_key, raw_value in raw_metadata.items():
            key = str(raw_key or "").strip()
            if not key:
                continue
            value = str(raw_value or "").strip()
            if value:
                metadata[key[:40]] = value[:500]

    total_net_cents = unit_net_cents * quantity
    metadata["plan_id"] = selected_plan_id
    metadata["cycle_months"] = str(cycle_months)
    metadata["quantity"] = str(quantity)
    if customer_reference:
        metadata["customer_reference"] = customer_reference

    stripe_secret_key = str(getattr(server, "_billing_stripe_secret_key", "") or "").strip()
    if not stripe_secret_key:
        return web.json_response(
            {"error": "stripe_secret_key_missing", "contract_version": "2026-02-27"},
            status=409,
        )

    session_payload: dict[str, Any] = {
        "mode": "subscription",
        "success_url": success_url,
        "cancel_url": cancel_url,
        "line_items": [{"price": stripe_price_id, "quantity": quantity}],
        "billing_address_collection": "required",
        "tax_id_collection": {"enabled": True},
        "metadata": metadata,
    }
    if customer_reference:
        session_payload["client_reference_id"] = customer_reference
    if customer_email:
        session_payload["customer_email"] = customer_email

    stripe_session, checkout_error = await server._billing_create_checkout_session_best_effort_async(
        session_payload=session_payload,
        idempotency_key=idempotency_key,
    )
    if stripe_session is None:
        return web.json_response(
            {
                "error": "stripe_checkout_create_failed",
                "contract_version": "2026-02-27",
                "message": str(checkout_error or "Stripe checkout create failed"),
            },
            status=502,
        )

    session_id = str(server._billing_stripe_obj_get(stripe_session, "id", "") or "")
    session_url = str(server._billing_stripe_obj_get(stripe_session, "url", "") or "")
    expires_at_epoch = int(server._billing_stripe_obj_get(stripe_session, "expires_at", 0) or 0)
    expires_at_iso = datetime.fromtimestamp(expires_at_epoch, tz=UTC).isoformat() if expires_at_epoch > 0 else None
    payload = {
        "ok": True,
        "provider": "stripe",
        "integration_state": "live",
        "contract_version": "2026-02-27",
        "currency": catalog["currency"],
        "tax_mode": catalog["tax_mode"],
        "request": {
            "plan_id": selected_plan_id,
            "stripe_price_id": stripe_price_id,
            "cycle_months": cycle_months,
            "quantity": quantity,
            "success_url": success_url,
            "cancel_url": cancel_url,
            "customer_reference": customer_reference,
            "customer_email": customer_email,
            "idempotency_key": idempotency_key,
            "metadata": metadata,
        },
        "plan": selected_plan,
        "amount": {
            "unit_net_cents": unit_net_cents,
            "total_net_cents": total_net_cents,
            "unit_net_label": format_eur_cents(unit_net_cents),
            "total_net_label": format_eur_cents(total_net_cents),
        },
        "checkout": {
            "status": "created",
            "mode": "subscription",
            "session_id": session_id,
            "session_url": session_url or None,
            "expires_at": expires_at_iso,
        },
        "invoice_preview_path": "/twitch/api/billing/invoice-preview",
        "invoice_page_path": "/twitch/abbo/rechnung",
        "message": "Stripe Checkout Session wurde erfolgreich erstellt.",
    }
    return web.json_response(payload, status=201, dumps=lambda data: json_module.dumps(data, ensure_ascii=True))


async def api_billing_invoice_preview(
    server: Any,
    request: web.Request,
    *,
    deps: BillingRouteDeps,
) -> web.Response:
    """Return structured and HTML invoice preview for the selected billing plan."""
    build_billing_catalog = deps.build_billing_catalog
    json_module = deps.json
    normalize_billing_cycle = deps.normalize_billing_cycle

    if not server._check_v2_auth(request):
        return web.json_response({"error": "auth_required"}, status=401)

    body = await server._billing_read_request_body(request)
    selected_plan_id = str(body.get("plan_id") or "").strip()
    if not selected_plan_id:
        return web.json_response(
            {"error": "plan_id_required", "required_fields": ["plan_id"]},
            status=400,
        )

    cycle_months = normalize_billing_cycle(body.get("cycle_months"))
    catalog = build_billing_catalog(cycle_months)
    selected_plan = next(
        (plan for plan in catalog.get("plans") or [] if str(plan.get("id") or "") == selected_plan_id),
        None,
    )
    if not selected_plan:
        return web.json_response(
            {
                "error": "unknown_plan_id",
                "available_plan_ids": [str(plan.get("id") or "") for plan in catalog.get("plans") or []],
            },
            status=404,
        )

    try:
        quantity = int(body.get("quantity") or 1)
    except (TypeError, ValueError):
        quantity = 1
    quantity = min(max(quantity, 1), 24)

    session = server._csrf_session(request)
    billing_profile = server._billing_profile_for_request(request)
    customer_reference = server._billing_primary_ref_for_request(request)
    customer_name = str(
        body.get("customer_name")
        or billing_profile.get("recipient_name")
        or session.get("display_name")
        or session.get("twitch_login")
        or "Streamer Partner"
    ).strip()
    customer_email = str(body.get("customer_email") or billing_profile.get("recipient_email") or "").strip()

    invoice = server._billing_build_invoice_preview(
        plan=selected_plan,
        cycle_months=cycle_months,
        quantity=quantity,
        customer_reference=customer_reference,
        customer_name=customer_name,
        customer_email=customer_email,
        customer_profile=billing_profile,
    )
    payload = {
        "ok": True,
        "provider": "stripe",
        "invoice": invoice,
        "html": server._billing_render_invoice_html(invoice),
    }
    return web.json_response(payload, dumps=lambda data: json_module.dumps(data, ensure_ascii=True))


async def api_billing_stripe_sync_products(
    server: Any,
    request: web.Request,
    *,
    deps: BillingRouteDeps,
) -> web.Response:
    """Create or reuse Stripe products and prices and persist IDs into the Windows vault."""
    asyncio_module = deps.asyncio
    billing_cycle_discounts = deps.billing_cycle_discounts
    billing_is_paid_plan = deps.billing_is_paid_plan
    billing_public_error_message = deps.billing_public_error_message
    build_billing_catalog = deps.build_billing_catalog
    json_module = deps.json
    log = deps.log

    admin_error = server._require_v2_admin_api(request)
    if admin_error is not None:
        return admin_error

    body = await server._billing_read_request_body(request)
    dry_run_raw = str(body.get("dry_run") or "").strip().lower()
    dry_run = dry_run_raw in {"1", "true", "yes", "on"}

    stripe, _import_error = server._billing_import_stripe()
    if stripe is None:
        return web.json_response(
            {"error": "stripe_sdk_missing", "message": billing_public_error_message("stripe_sdk_missing")},
            status=503,
        )

    stripe_secret_key = str(getattr(server, "_billing_stripe_secret_key", "") or "").strip()
    if not stripe_secret_key:
        return web.json_response(
            {"error": "stripe_secret_key_missing", "missing": ["stripe_secret_key"]},
            status=409,
        )
    stripe.api_key = stripe_secret_key

    product_map = server._billing_product_id_map()
    price_map = server._billing_price_id_map()
    cycle_catalogs = {
        cycle: build_billing_catalog(cycle) for cycle in sorted(billing_cycle_discounts.keys())
    }
    base_catalog = cycle_catalogs.get(1) or build_billing_catalog(1)
    paid_plans = [plan for plan in list(base_catalog.get("plans") or []) if billing_is_paid_plan(plan)]

    operations: list[dict[str, Any]] = []
    created_products = 0
    reused_products = 0
    created_prices = 0
    reused_prices = 0

    for plan in paid_plans:
        plan_id = str(plan.get("id") or "").strip()
        plan_name = str(plan.get("name") or "").strip() or plan_id
        plan_description = str(plan.get("description") or "").strip()
        product_id = str(product_map.get(plan_id) or "").strip()
        operation: dict[str, Any] = {
            "plan_id": plan_id,
            "name": plan_name,
            "product": {"id": product_id or None, "status": "missing"},
            "prices": [],
        }

        if product_id and not dry_run:
            try:
                product_obj = await asyncio_module.to_thread(stripe.Product.retrieve, product_id)
                if bool(server._billing_stripe_obj_get(product_obj, "deleted", False)):
                    product_id = ""
                else:
                    operation["product"] = {"id": product_id, "status": "reused"}
                    reused_products += 1
            except Exception:
                product_id = ""

        if not product_id:
            if dry_run:
                operation["product"] = {"id": None, "status": "would_create"}
            else:
                try:
                    product_obj = await asyncio_module.to_thread(
                        stripe.Product.create,
                        name=plan_name,
                        description=plan_description or None,
                        metadata={"plan_id": plan_id, "source": "deutsche-deadlock-community.de", "billing": "subscriptions"},
                    )
                except Exception as exc:
                    log.warning("stripe product create failed for %s (%s)", plan_id, type(exc).__name__)
                    return web.json_response(
                        {"error": "stripe_product_create_failed", "plan_id": plan_id, "message": "stripe_product_create_failed"},
                        status=502,
                    )
                product_id = str(server._billing_stripe_obj_get(product_obj, "id", "") or "").strip()
                if not product_id:
                    return web.json_response(
                        {"error": "stripe_product_id_missing", "plan_id": plan_id},
                        status=502,
                    )
                product_map[plan_id] = product_id
                operation["product"] = {"id": product_id, "status": "created"}
                created_products += 1

        for cycle in sorted(billing_cycle_discounts.keys()):
            cycle_catalog = cycle_catalogs.get(cycle) or build_billing_catalog(cycle)
            cycle_plan = next(
                (
                    entry
                    for entry in list(cycle_catalog.get("plans") or [])
                    if str(entry.get("id") or "") == plan_id
                ),
                None,
            )
            if not cycle_plan:
                continue
            amount_cents = int((cycle_plan.get("price") or {}).get("total_net_cents") or 0)
            if amount_cents <= 0:
                continue

            cycle_map = price_map.setdefault(plan_id, {})
            price_id = str(cycle_map.get(cycle) or "").strip()
            lookup_key = f"deadlock_{plan_id}_{cycle}m_net_v2"
            price_status = "missing"

            if price_id and not dry_run:
                try:
                    await asyncio_module.to_thread(stripe.Price.retrieve, price_id)
                    price_status = "reused"
                    reused_prices += 1
                except Exception:
                    price_id = ""

            if not price_id and not dry_run:
                try:
                    price_list = await asyncio_module.to_thread(
                        stripe.Price.list,
                        active=True,
                        lookup_keys=[lookup_key],
                        limit=1,
                    )
                    existing_prices = list(server._billing_stripe_obj_get(price_list, "data", []) or [])
                except Exception:
                    existing_prices = []

                if existing_prices:
                    existing_price = existing_prices[0]
                    price_id = str(server._billing_stripe_obj_get(existing_price, "id", "") or "").strip()
                    if price_id:
                        cycle_map[cycle] = price_id
                        price_status = "reused_lookup"
                        reused_prices += 1

            if not price_id:
                if dry_run:
                    price_status = "would_create"
                else:
                    try:
                        price_obj = await asyncio_module.to_thread(
                            stripe.Price.create,
                            currency="eur",
                            product=product_id,
                            unit_amount=amount_cents,
                            recurring={"interval": "month", "interval_count": cycle},
                            lookup_key=lookup_key,
                            metadata={"plan_id": plan_id, "cycle_months": str(cycle), "source": "deutsche-deadlock-community.de"},
                        )
                    except Exception as exc:
                        log.warning(
                            "stripe price create failed for %s/%sm (%s)",
                            plan_id,
                            cycle,
                            type(exc).__name__,
                        )
                        return web.json_response(
                            {"error": "stripe_price_create_failed", "plan_id": plan_id, "cycle_months": cycle, "message": "stripe_price_create_failed"},
                            status=502,
                        )
                    price_id = str(server._billing_stripe_obj_get(price_obj, "id", "") or "").strip()
                    if not price_id:
                        return web.json_response(
                            {"error": "stripe_price_id_missing", "plan_id": plan_id, "cycle_months": cycle},
                            status=502,
                        )
                    cycle_map[cycle] = price_id
                    price_status = "created"
                    created_prices += 1

            operation["prices"].append(
                {
                    "cycle_months": cycle,
                    "amount_net_cents": amount_cents,
                    "price_id": price_id or None,
                    "lookup_key": lookup_key,
                    "status": price_status,
                }
            )
        operations.append(operation)

    persisted = False
    if not dry_run:
        product_persisted = server._billing_set_product_id_map(product_map)
        price_persisted = server._billing_set_price_id_map(price_map)
        persisted = bool(product_persisted and price_persisted)

    readiness = server._billing_stripe_readiness_payload()
    payload = {
        "ok": True,
        "provider": "stripe",
        "dry_run": dry_run,
        "persisted_to_windows_vault": persisted,
        "created_products": created_products,
        "reused_products": reused_products,
        "created_prices": created_prices,
        "reused_prices": reused_prices,
        "operations": operations,
        "product_id_map": product_map,
        "price_id_map": price_map,
        "readiness": readiness,
    }
    return web.json_response(payload, dumps=lambda data: json_module.dumps(data, ensure_ascii=True))
