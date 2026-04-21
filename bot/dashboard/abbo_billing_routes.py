"""Abbo billing routes and HTML pages."""

from __future__ import annotations

import asyncio
import html
from datetime import UTC, datetime
from typing import Any

from aiohttp import web

from .. import storage
from ..core.constants import log
from . import abbo_routes as _abbo_routes
from .billing.billing_plans import (
    build_billing_catalog as _build_billing_catalog,
    normalize_billing_cycle as _normalize_billing_cycle,
)


async def abbo_pay(handler: Any, request: web.Request) -> web.StreamResponse:
    """Create Stripe Checkout session from plan page and redirect to payment."""
    auth_redirect = _abbo_routes._abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect

    plan_id = str(request.query.get("plan_id") or "").strip()
    if not plan_id:
        raise web.HTTPFound("/twitch/abbo")

    cycle_months = _normalize_billing_cycle(request.query.get("cycle"))
    readiness = handler._billing_stripe_readiness_payload()
    catalog = _build_billing_catalog(cycle_months, readiness=readiness)
    selected_plan = next(
        (plan for plan in catalog.get("plans") or [] if str(plan.get("id") or "") == plan_id),
        None,
    )
    if selected_plan is None:
        raise web.HTTPFound("/twitch/abbo")

    try:
        quantity = int(request.query.get("quantity") or "1")
    except (TypeError, ValueError):
        quantity = 1
    quantity = min(max(quantity, 1), 24)

    unit_net_cents = int((selected_plan.get("price") or {}).get("total_net_cents") or 0)
    if unit_net_cents <= 0:
        raise web.HTTPFound("/twitch/abbo")

    if not bool(readiness.get("checkout_ready")):
        raise web.HTTPFound("/twitch/abbo?checkout=unavailable&reason=checkout_not_ready")
    if not bool(readiness.get("price_map_ready")):
        raise web.HTTPFound("/twitch/abbo?checkout=unavailable&reason=stripe_price_id_map_missing")

    stripe_price_id = handler._billing_price_id_for_plan(plan_id, cycle_months)
    if not stripe_price_id:
        raise web.HTTPFound("/twitch/abbo?checkout=unavailable&reason=missing_stripe_price_id")

    stripe_secret_key = str(getattr(handler, "_billing_stripe_secret_key", "") or "").strip()
    if not stripe_secret_key:
        log.warning("billing checkout unavailable: stripe secret key missing")
        raise web.HTTPFound("/twitch/abbo?checkout=unavailable&reason=stripe_secret_key_missing")

    base_url = handler._billing_base_url_for_request(request)
    success_url = f"{base_url}/twitch/abbo?checkout=success&session_id={{CHECKOUT_SESSION_ID}}"
    cancel_url = f"{base_url}/twitch/abbo?checkout=cancelled"

    billing_profile = handler._billing_profile_for_request(request)
    customer_reference = handler._billing_primary_ref_for_request(request)
    customer_email = str(billing_profile.get("recipient_email") or "").strip()
    metadata: dict[str, str] = {
        "plan_id": plan_id,
        "cycle_months": str(cycle_months),
        "quantity": str(quantity),
        "source": "abbo_page_pay_link",
    }
    if customer_reference:
        metadata["customer_reference"] = customer_reference

    session_payload: dict[str, Any] = {
        "mode": "subscription",
        "success_url": success_url,
        "cancel_url": cancel_url,
        "line_items": [{"price": stripe_price_id, "quantity": quantity}],
        "billing_address_collection": "required",
        "tax_id_collection": {"enabled": True},
        "metadata": metadata,
    }
    if plan_id == "analysis_dashboard":
        session_payload["subscription_data"] = {"trial_period_days": 45}
    if customer_reference:
        session_payload["client_reference_id"] = customer_reference
    if customer_email:
        session_payload["customer_email"] = customer_email

    stripe_session, checkout_error = await handler._billing_create_checkout_session_best_effort_async(
        session_payload=session_payload
    )
    if stripe_session is None:
        log.warning("billing checkout redirect failed: %s", str(checkout_error or "unknown"))
        raise web.HTTPFound("/twitch/abbo?checkout=unavailable&reason=checkout_create_failed")

    checkout_url = str(handler._billing_stripe_obj_get(stripe_session, "url", "") or "").strip()
    if not checkout_url:
        raise web.HTTPFound("/twitch/abbo?checkout=unavailable&reason=checkout_missing_url")
    raise web.HTTPFound(checkout_url)


async def abbo_profile_save(handler: Any, request: web.Request) -> web.StreamResponse:
    """Persist invoice recipient profile data for the current account."""
    auth_redirect = _abbo_routes._abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect

    data = await request.post()
    csrf_token = str(data.get("csrf_token") or "").strip()
    if not handler._csrf_verify_token(request, csrf_token):
        return web.json_response({"error": "csrf_token_invalid"}, status=403)
    cycle = _normalize_billing_cycle(data.get("cycle"))
    customer_reference = handler._billing_primary_ref_for_request(request)
    recipient_name = str(data.get("recipient_name") or "").strip()[:180]
    recipient_email = str(data.get("recipient_email") or "").strip()[:180]
    company_name = str(data.get("company_name") or "").strip()[:200]
    street_line1 = str(data.get("street_line1") or "").strip()[:200]
    postal_code = str(data.get("postal_code") or "").strip()[:32]
    city = str(data.get("city") or "").strip()[:120]
    country_code = str(data.get("country_code") or "DE").strip().upper()[:2]
    vat_id = str(data.get("vat_id") or "").strip()[:60]

    if not (
        customer_reference
        and recipient_name
        and recipient_email
        and street_line1
        and postal_code
        and city
        and country_code
    ):
        raise web.HTTPFound(f"/twitch/abbo?cycle={cycle}&profile=invalid")

    try:
        with storage.transaction() as conn:
            handler._billing_ensure_storage_tables(conn)
            handler._billing_upsert_profile(
                conn,
                customer_reference=customer_reference,
                recipient_name=recipient_name,
                recipient_email=recipient_email,
                company_name=company_name,
                street_line1=street_line1,
                postal_code=postal_code,
                city=city,
                country_code=country_code,
                vat_id=vat_id,
            )
    except Exception:
        log.exception("billing profile save failed")
        raise web.HTTPFound(f"/twitch/abbo?cycle={cycle}&profile=error") from None

    raise web.HTTPFound(f"/twitch/abbo?cycle={cycle}&profile=saved")


async def abbo_cancel(handler: Any, request: web.Request) -> web.StreamResponse:
    """Start cancellation via Stripe customer portal, fallback to cancel-at-period-end."""
    auth_redirect = _abbo_routes._abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect

    if request.method != "POST":
        raise web.HTTPFound("/twitch/abbo?cancel=post_required")
    data = await request.post()
    csrf_token = str(data.get("csrf_token") or "").strip()
    if not handler._csrf_verify_token(request, csrf_token):
        raise web.HTTPFound("/twitch/abbo?cancel=csrf_invalid")

    customer_record = handler._billing_customer_record_for_request(request)
    stripe_customer_id = str(customer_record.get("stripe_customer_id") or "").strip()
    stripe_subscription_id = str(customer_record.get("stripe_subscription_id") or "").strip()
    if not stripe_customer_id and not stripe_subscription_id:
        raise web.HTTPFound("/twitch/abbo?cancel=missing")

    stripe, _import_error = handler._billing_import_stripe()
    if stripe is None:
        raise web.HTTPFound("/twitch/abbo?cancel=error")
    stripe_secret_key = str(getattr(handler, "_billing_stripe_secret_key", "") or "").strip()
    if not stripe_secret_key:
        raise web.HTTPFound("/twitch/abbo?cancel=error")
    stripe.api_key = stripe_secret_key

    base_url = handler._billing_base_url_for_request(request)
    portal_url = ""
    if stripe_customer_id:
        try:
            portal_session = await asyncio.to_thread(
                stripe.billing_portal.Session.create,
                customer=stripe_customer_id,
                return_url=f"{base_url}/twitch/abbo?cancel=returned",
            )
            portal_url = str(handler._billing_stripe_obj_get(portal_session, "url", "") or "").strip()
        except Exception:
            log.debug("billing portal unavailable; trying direct cancel fallback", exc_info=True)
    if portal_url:
        raise web.HTTPFound(portal_url)

    if not stripe_subscription_id:
        raise web.HTTPFound("/twitch/abbo?cancel=missing")

    try:
        subscription_obj = await asyncio.to_thread(
            stripe.Subscription.modify,
            stripe_subscription_id,
            cancel_at_period_end=True,
            proration_behavior="none",
        )
        with storage.transaction() as conn:
            handler._billing_ensure_storage_tables(conn)
            payload = handler._billing_subscription_payload_from_object(subscription_obj)
            if payload:
                handler._billing_upsert_subscription_state(conn, **payload)
    except Exception:
        log.exception("billing cancel fallback failed")
        raise web.HTTPFound("/twitch/abbo?cancel=error") from None
    raise web.HTTPFound("/twitch/abbo?cancel=scheduled")


async def abbo_invoices(handler: Any, request: web.Request) -> web.StreamResponse:
    """Render downloadable Stripe invoices for the logged-in customer."""
    auth_redirect = _abbo_routes._abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect

    customer_record = handler._billing_customer_record_for_request(request)
    stripe_customer_id = str(customer_record.get("stripe_customer_id") or "").strip()
    if not stripe_customer_id:
        raise web.HTTPFound("/twitch/abbo?invoice=missing_customer")

    stripe, _import_error = handler._billing_import_stripe()
    if stripe is None:
        raise web.HTTPFound("/twitch/abbo?invoice=error")
    stripe_secret_key = str(getattr(handler, "_billing_stripe_secret_key", "") or "").strip()
    if not stripe_secret_key:
        raise web.HTTPFound("/twitch/abbo?invoice=error")
    stripe.api_key = stripe_secret_key

    try:
        invoice_list = await asyncio.to_thread(
            stripe.Invoice.list,
            customer=stripe_customer_id,
            limit=24,
        )
        invoice_rows = list(handler._billing_stripe_obj_get(invoice_list, "data", []) or [])
    except Exception:
        log.exception("billing invoice list failed")
        raise web.HTTPFound("/twitch/abbo?invoice=error") from None

    invoice_rows.sort(
        key=lambda x: int(handler._billing_stripe_obj_get(x, "created", 0) or 0),
        reverse=True,
    )

    status_badge_class = {"paid": "badge-paid", "open": "badge-open", "void": "badge-void"}
    table_rows: list[str] = []
    for invoice_obj in invoice_rows:
        invoice_id = str(handler._billing_stripe_obj_get(invoice_obj, "id", "") or "").strip()
        invoice_number = str(
            handler._billing_stripe_obj_get(invoice_obj, "number", "")
            or handler._billing_stripe_obj_get(invoice_obj, "id", "")
            or ""
        ).strip()
        status = str(handler._billing_stripe_obj_get(invoice_obj, "status", "open") or "open").strip()
        pdf_url = str(handler._billing_stripe_obj_get(invoice_obj, "invoice_pdf", "") or "").strip()
        currency = str(handler._billing_stripe_obj_get(invoice_obj, "currency", "eur") or "eur").upper()
        total_cents = int(handler._billing_stripe_obj_get(invoice_obj, "total", 0) or 0)
        created_epoch = int(handler._billing_stripe_obj_get(invoice_obj, "created", 0) or 0)
        created_date = (
            datetime.fromtimestamp(created_epoch, tz=UTC).strftime("%d.%m.%Y")
            if created_epoch > 0
            else "-"
        )
        total_label = f"{total_cents / 100:.2f} {currency}"
        badge_class = status_badge_class.get(status, "badge-open")
        pdf_html = (
            f"<a href='{html.escape(pdf_url, quote=True)}' target='_blank' rel='noopener noreferrer'>PDF</a>"
            if pdf_url
            else "<span class='muted'>-</span>"
        )
        table_rows.append(
            "<tr>"
            f"<td>{html.escape(invoice_number or invoice_id or '-')}</td>"
            f"<td>{html.escape(created_date)}</td>"
            f"<td><span class='{badge_class}'>{html.escape(status)}</span></td>"
            f"<td>{html.escape(total_label)}</td>"
            f"<td>{pdf_html}</td>"
            "</tr>"
        )

    if not table_rows:
        table_rows.append(
            "<tr><td colspan='5' class='muted'>Noch keine Stripe-Rechnungen vorhanden.</td></tr>"
        )

    logout_url = (
        handler._discord_admin_logout_url()
        if handler._is_discord_admin_request(request)
        else "/twitch/auth/logout"
    )
    csrf_token = handler._csrf_generate_token(request)
    cancel_form_html = (
        "<form method='post' action='/twitch/abbo/kündigen' style='margin:0;'>"
        f"<input type='hidden' name='csrf_token' value='{html.escape(csrf_token, quote=True)}'>"
        "<button class='btn btn-ghost' type='submit'>Abo kündigen</button>"
        "</form>"
    )
    page_html = (
        "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
        "<meta name='viewport' content='width=device-width,initial-scale=1'>"
        "<title>Stripe Rechnungen</title>"
        "<style>"
        "body{margin:0;background:#0f172a;color:#e2e8f0;font-family:Segoe UI,Arial,sans-serif;}"
        ".wrap{max-width:1040px;margin:0 auto;padding:30px 18px 40px;}"
        ".top{display:flex;justify-content:space-between;align-items:center;gap:12px;flex-wrap:wrap;}"
        "h1{margin:0;font-size:1.7rem;}"
        ".muted{color:#94a3b8;font-size:13px;}"
        ".card{margin-top:16px;background:#111827;border:1px solid #1f2937;border-radius:14px;padding:16px;}"
        "table{width:100%;border-collapse:collapse;}"
        "th,td{padding:11px 10px;border-bottom:1px solid #1f2937;text-align:left;font-size:13px;}"
        "th{color:#94a3b8;font-size:12px;text-transform:uppercase;letter-spacing:.03em;}"
        "a{color:#93c5fd;text-decoration:none;}a:hover{text-decoration:underline;}"
        ".actions{display:flex;gap:10px;flex-wrap:wrap;margin-top:14px;}"
        ".btn{display:inline-block;padding:9px 13px;border-radius:10px;text-decoration:none;font-weight:700;font-size:13px;}"
        ".btn-primary{background:#2563eb;color:#eff6ff;}"
        ".btn-ghost{background:#0b1220;color:#e2e8f0;border:1px solid #334155;}"
        ".badge-paid{background:rgba(22,163,74,0.18);color:#86efac;"
        "border:1px solid rgba(74,222,128,0.38);border-radius:999px;padding:3px 10px;font-size:12px;}"
        ".badge-open{background:rgba(217,119,6,0.18);color:#fde68a;"
        "border:1px solid rgba(251,191,36,0.38);border-radius:999px;padding:3px 10px;font-size:12px;}"
        ".badge-void{background:rgba(220,38,38,0.18);color:#fecaca;"
        "border:1px solid rgba(248,113,113,0.38);border-radius:999px;padding:3px 10px;font-size:12px;}"
        "</style></head><body><main class='wrap'>"
        "<div class='top'>"
        "<div><h1>Rechnungen</h1>"
        "<p class='muted'>PDF-Downloads deiner Stripe-Rechnungen.</p></div>"
        f"<a class='muted' href='{logout_url}'>Logout</a>"
        "</div>"
        "<section class='card'>"
        "<table><thead><tr>"
        "<th>Rechnungsnr</th><th>Datum</th><th>Status</th><th>Betrag</th><th>PDF</th>"
        "</tr></thead><tbody>"
        f"{''.join(table_rows)}"
        "</tbody></table>"
        "<div class='actions'>"
        "<a class='btn btn-primary' href='/twitch/abbo'>Zur Abo Übersicht</a>"
        f"{cancel_form_html}"
        "</div>"
        "</section>"
        "</main></body></html>"
    )
    return web.Response(text=page_html, content_type="text/html")


async def abbo_stripe_settings(handler: Any, request: web.Request) -> web.StreamResponse:
    """Internal Stripe readiness page for billing setup."""
    auth_redirect = _abbo_routes._abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect
    if not handler._check_v2_admin_auth(request):
        raise web.HTTPFound("/twitch/abbo")

    readiness = handler._billing_stripe_readiness_payload()
    checks = list(readiness.get("checks") or [])
    missing_count = len([check for check in checks if not bool(check.get("ready"))])
    logout_url = (
        handler._discord_admin_logout_url()
        if handler._is_discord_admin_request(request)
        else "/twitch/auth/logout"
    )

    if bool(readiness.get("ready_for_live")):
        summary_title = "Live-ready"
        summary_class = "status-live"
        summary_text = "Checkout, Product/Price IDs und Webhook sind fuer Livebetrieb vorbereitet."
    elif bool(readiness.get("checkout_ready")) and bool(readiness.get("price_map_ready")):
        summary_title = "Fast bereit"
        summary_class = "status-partial"
        summary_text = "Checkout und Product/Price IDs sind bereit, Webhook fehlt noch."
    elif bool(readiness.get("checkout_ready")):
        summary_title = "Teilweise bereit"
        summary_class = "status-partial"
        summary_text = "Checkout ist bereit, aber Product/Price IDs oder Webhook fehlen."
    else:
        summary_title = "Nicht bereit"
        summary_class = "status-missing"
        summary_text = "Checkout kann so noch nicht live geschaltet werden."

    rows_html: list[str] = []
    for check in checks:
        ready = bool(check.get("ready"))
        status_label = "OK" if ready else "FEHLT"
        row_class = "ok" if ready else "missing"
        env_keys = ", ".join(str(value) for value in list(check.get("env_keys") or []))
        preview = str(check.get("value_preview") or "").strip() or "nicht gesetzt"
        rows_html.append(
            "<tr>"
            f"<td>{html.escape(str(check.get('label') or ''))}</td>"
            f"<td><code>{html.escape(env_keys)}</code></td>"
            f"<td>{html.escape(preview)}</td>"
            f"<td class='{row_class}'>{status_label}</td>"
            "</tr>"
        )

    missing_list_html = (
        "<ul>"
        + "".join(
            f"<li>{html.escape(str(check.get('label') or str(check.get('id') or '')))}</li>"
            for check in checks
            if not bool(check.get("ready"))
        )
        + "</ul>"
        if missing_count > 0
        else "<p class='ok-note'>Keine fehlenden Keys/URLs.</p>"
    )

    page_html = (
        "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
        "<meta name='viewport' content='width=device-width,initial-scale=1'>"
        "<title>Stripe Settings</title>"
        "<style>"
        "body{margin:0;background:#0b1220;color:#e2e8f0;font-family:Segoe UI,Arial,sans-serif;}"
        ".wrap{max-width:1040px;margin:0 auto;padding:28px 18px 36px;}"
        ".top{display:flex;justify-content:space-between;align-items:center;gap:12px;flex-wrap:wrap;}"
        "h1{margin:0;font-size:1.7rem;}"
        "a{color:#93c5fd;text-decoration:none;}"
        ".panel{margin-top:14px;background:#111a2c;border:1px solid #22314d;border-radius:14px;padding:16px;}"
        ".summary{display:flex;justify-content:space-between;gap:10px;align-items:center;flex-wrap:wrap;}"
        ".badge{display:inline-block;padding:6px 10px;border-radius:999px;font-weight:700;font-size:12px;}"
        ".status-live{background:#064e3b;color:#a7f3d0;}"
        ".status-partial{background:#78350f;color:#fde68a;}"
        ".status-missing{background:#7f1d1d;color:#fecaca;}"
        ".muted{color:#93a4bd;font-size:14px;}"
        ".missing ul{margin:8px 0 0 18px;padding:0;color:#fecaca;}"
        ".ok-note{color:#86efac;margin:8px 0 0;}"
        "table{width:100%;border-collapse:collapse;margin-top:10px;}"
        "th,td{padding:10px;border-bottom:1px solid #24324a;text-align:left;vertical-align:top;}"
        "th{font-size:12px;color:#9fb0c8;text-transform:uppercase;letter-spacing:.02em;}"
        "td code{font-size:12px;color:#cbd5e1;word-break:break-word;}"
        "td.ok{color:#86efac;font-weight:700;}"
        "td.missing{color:#fca5a5;font-weight:700;}"
        ".actions{display:flex;gap:10px;flex-wrap:wrap;margin-top:12px;}"
        ".btn{display:inline-block;padding:9px 13px;border-radius:10px;font-weight:600;text-decoration:none;}"
        ".btn-primary{background:#2563eb;color:#fff;}"
        ".btn-ghost{background:#0b1220;color:#cbd5e1;border:1px solid #334155;}"
        "</style></head><body><main class='wrap'>"
        "<div class='top'>"
        "<h1>Stripe Settings</h1>"
        f"<a href='{logout_url}'>Logout</a>"
        "</div>"
        "<p class='muted'>Readiness fuer Stripe Billing (Windows-Tresor first, kein Secret-Leakage).</p>"
        "<section class='panel summary'>"
        "<div>"
        f"<span class='badge {summary_class}'>{summary_title}</span>"
        f"<p class='muted' style='margin:8px 0 0;'>{html.escape(summary_text)}</p>"
        "</div>"
        "<div class='actions'>"
        "<a class='btn btn-primary' href='/twitch/abbo'>Zur Abo Übersicht</a>"
        "<a class='btn btn-ghost' href='https://docs.stripe.com/billing/quickstart' target='_blank' rel='noopener noreferrer'>Stripe Quickstart</a>"
        "</div>"
        "</section>"
        "<section class='panel missing'>"
        f"<h2 style='margin:0;font-size:1.05rem;'>Fehlende Keys/URLs: {missing_count}</h2>"
        f"{missing_list_html}"
        "</section>"
        "<section class='panel'>"
        "<h2 style='margin:0 0 4px;font-size:1.05rem;'>Konfiguration</h2>"
        "<table>"
        "<thead><tr><th>Check</th><th>Env Keys</th><th>Aktueller Wert</th><th>Status</th></tr></thead>"
        f"<tbody>{''.join(rows_html)}</tbody>"
        "</table>"
        "</section>"
        "</main></body></html>"
    )
    return web.Response(text=page_html, content_type="text/html")


async def abbo_invoice(handler: Any, request: web.Request) -> web.StreamResponse:
    """Render an invoice preview page for the selected plan and cycle."""
    auth_redirect = _abbo_routes._abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect

    cycle_months = _normalize_billing_cycle(request.query.get("cycle"))
    readiness = handler._billing_stripe_readiness_payload()
    catalog = _build_billing_catalog(cycle_months, readiness=readiness)
    plans = list(catalog.get("plans") or [])
    if not plans:
        return web.Response(text="Keine Billing-Plaene verfuegbar.", status=404)

    requested_plan_id = str(request.query.get("plan_id") or "").strip()
    default_plan = next(
        (plan for plan in plans if bool(plan.get("recommended"))),
        plans[0],
    )
    selected_plan = next(
        (plan for plan in plans if str(plan.get("id") or "") == requested_plan_id),
        default_plan,
    )

    try:
        quantity = int(request.query.get("quantity") or "1")
    except (TypeError, ValueError):
        quantity = 1
    quantity = min(max(quantity, 1), 24)

    session = handler._csrf_session(request)
    billing_profile = handler._billing_profile_for_request(request)
    customer_reference = handler._billing_primary_ref_for_request(request)
    customer_name = str(
        request.query.get("customer_name")
        or billing_profile.get("recipient_name")
        or session.get("display_name")
        or session.get("twitch_login")
        or "Streamer Partner"
    ).strip()
    customer_email = str(
        request.query.get("customer_email")
        or billing_profile.get("recipient_email")
        or ""
    ).strip()

    invoice = handler._billing_build_invoice_preview(
        plan=selected_plan,
        cycle_months=cycle_months,
        quantity=quantity,
        customer_reference=customer_reference,
        customer_name=customer_name,
        customer_email=customer_email,
        customer_profile=billing_profile,
    )
    page_html = handler._billing_render_invoice_html(invoice)
    return web.Response(text=page_html, content_type="text/html")
