"""Abbo dashboard routes and helpers."""

from __future__ import annotations

import html
from typing import Any

from aiohttp import web

from .. import storage
from ..core.constants import log
from ..promo_mode import validate_streamer_promo_message
from .billing.billing_plans import (
    build_billing_catalog as _build_billing_catalog,
    billing_cycle_label as _billing_cycle_label,
    billing_is_paid_plan as _billing_is_paid_plan,
    billing_plan_has_entitlement as _billing_plan_has_entitlement,
)
from .core.abbo_html import render_abbo_page

TWITCH_ABBO_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fabbo"


def _abbo_auth_redirect_or_none(handler: Any, request: web.Request) -> web.StreamResponse | None:
    if handler._check_v2_auth(request):
        return None
    login_url = (
        handler._build_discord_admin_login_url(request, next_path="/twitch/abbo")
        if handler._should_use_discord_admin_login(request)
        else TWITCH_ABBO_LOGIN_URL
    )
    response = handler._dashboard_auth_redirect_or_unavailable(
        request,
        next_path="/twitch/abbo",
        fallback_login_url=login_url,
    )
    if isinstance(response, web.HTTPException):
        raise response
    return response


def _load_abbo_saved_settings(
    handler: Any,
    *,
    twitch_login: str,
    twitch_user_id: str = "",
) -> tuple[bool, str, bool]:
    if not twitch_login and not twitch_user_id:
        return False, "", False

    try:
        with storage.readonly_connection() as conn:
            ensure_cols = getattr(handler, "_billing_ensure_streamer_plan_columns", None)
            if callable(ensure_cols):
                ensure_cols(conn)
            if twitch_user_id:
                row = conn.execute(
                    """
                    SELECT promo_disabled, promo_message, lurker_tax_enabled
                      FROM streamer_plans
                     WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                        OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                     ORDER BY
                        CASE WHEN TRIM(COALESCE(twitch_user_id, '')) = %s THEN 0 ELSE 1 END
                     LIMIT 1
                    """,
                    (twitch_user_id, twitch_login, twitch_user_id),
                ).fetchone()
            else:
                row = conn.execute(
                    """
                    SELECT promo_disabled, promo_message, lurker_tax_enabled
                      FROM streamer_plans
                     WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                     LIMIT 1
                    """,
                    (twitch_login,),
                ).fetchone()
            if row:
                return bool(row[0]), str(row[1] or ""), bool(row[2])
    except Exception:
        log.debug("abbo settings lookup failed for %s", twitch_login or twitch_user_id, exc_info=True)
    return False, "", False


def _abbo_scope_state(
    handler: Any,
    *,
    twitch_login: str,
    twitch_user_id: str = "",
) -> dict[str, Any]:
    login_value = str(twitch_login or "").strip()
    user_id_value = str(twitch_user_id or "").strip()
    if not login_value and not user_id_value:
        return {
            "scopes": set(),
            "has_moderator_read_chatters": False,
        }

    try:
        with storage.readonly_connection() as conn:
            if user_id_value:
                row = conn.execute(
                    """
                    SELECT scopes
                      FROM twitch_raid_auth
                     WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                        OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                     ORDER BY
                        CASE WHEN TRIM(COALESCE(twitch_user_id, '')) = %s THEN 0 ELSE 1 END
                     LIMIT 1
                    """,
                    (user_id_value, login_value, user_id_value),
                ).fetchone()
            else:
                row = conn.execute(
                    """
                    SELECT scopes
                      FROM twitch_raid_auth
                     WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                     LIMIT 1
                    """,
                    (login_value,),
                ).fetchone()
    except Exception:
        log.debug("abbo scope lookup failed for %s", login_value or user_id_value, exc_info=True)
        row = None

    scopes = {
        scope.strip().lower()
        for scope in str((row[0] if row else "") or "").split()
        if scope.strip()
    }
    bot_scopes: set[str] = set()
    bot_scopes_loaded = False
    token_mgr = None
    try:
        resolver = getattr(handler, "_dashboard_token_manager", None)
        if callable(resolver):
            token_mgr = resolver()
        if token_mgr is not None:
            bot_scopes = {
                str(scope).strip().lower()
                for scope in (getattr(token_mgr, "scopes", None) or set())
                if str(scope).strip()
            }
            bot_scopes_loaded = bool(
                getattr(token_mgr, "bot_id", None) or getattr(token_mgr, "expires_at", None)
            )
    except Exception:
        bot_scopes = set()
        bot_scopes_loaded = False

    has_chatters_scope = ("moderator:read:chatters" in scopes) or (
        token_mgr is not None
        and bot_scopes_loaded
        and "moderator:read:chatters" in bot_scopes
    )
    return {
        "scopes": scopes,
        "has_moderator_read_chatters": has_chatters_scope,
    }


def _abbo_upsert_lurker_tax_setting(
    handler: Any,
    *,
    twitch_login: str,
    twitch_user_id: str = "",
    plan_id: str = "",
    enabled: bool,
) -> bool:
    login_value = str(twitch_login or "").strip()
    user_id_value = str(twitch_user_id or "").strip()
    plan_name_resolver = getattr(handler, "_billing_plan_name_from_id", None)
    plan_name = (
        str(plan_name_resolver(plan_id)).strip()
        if callable(plan_name_resolver)
        else "free"
    )

    with storage.transaction() as conn:
        ensure_cols = getattr(handler, "_billing_ensure_streamer_plan_columns", None)
        if callable(ensure_cols):
            ensure_cols(conn)

        if user_id_value:
            conn.execute(
                """
                INSERT INTO streamer_plans (
                    twitch_user_id,
                    twitch_login,
                    plan_name,
                    lurker_tax_enabled
                )
                VALUES (%s, %s, %s, %s)
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    twitch_login = COALESCE(NULLIF(EXCLUDED.twitch_login, ''), streamer_plans.twitch_login),
                    plan_name = COALESCE(NULLIF(EXCLUDED.plan_name, ''), streamer_plans.plan_name),
                    lurker_tax_enabled = EXCLUDED.lurker_tax_enabled
                """,
                (
                    user_id_value,
                    login_value,
                    plan_name,
                    1 if enabled else 0,
                ),
            )
        else:
            existing_row = conn.execute(
                """
                SELECT 1
                  FROM streamer_plans
                 WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                 LIMIT 1
                """,
                (login_value,),
            ).fetchone()
            if not existing_row:
                return False
            conn.execute(
                """
                UPDATE streamer_plans
                   SET lurker_tax_enabled = %s
                 WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                """,
                (1 if enabled else 0, login_value),
            )
    return True


async def abbo_entry(handler: Any, request: web.Request) -> web.StreamResponse:
    """Separated subscription overview dashboard (not linked from main dashboards page)."""
    auth_redirect = _abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect

    csrf_token = handler._csrf_ensure_token(request)
    cycle_raw = (request.query.get("cycle") or "1").strip()
    readiness_loader = getattr(handler, "_billing_stripe_readiness_payload", None)
    readiness = readiness_loader() if callable(readiness_loader) else {}
    catalog = _build_billing_catalog(cycle_raw, readiness=readiness)
    logout_url = (
        handler._discord_admin_logout_url()
        if handler._is_discord_admin_request(request)
        else "/twitch/auth/logout"
    )
    selected_cycle = int(catalog.get("cycle_months") or 1)
    payment = dict(catalog.get("payment") or {})
    checkout_enabled = bool(payment.get("checkout_enabled"))
    price_map_loader = getattr(handler, "_billing_price_id_map", None)
    price_lookup = getattr(handler, "_billing_price_id_for_plan", None)
    price_map = price_map_loader() if callable(price_map_loader) else {}

    customer_record = handler._billing_customer_record_for_request(request)
    billing_profile = handler._billing_profile_for_request(request)
    stripe_imported_fields: list[str] = []
    stripe_customer_id = str(customer_record.get("stripe_customer_id") or "").strip()
    needs_stripe_prefill = any(
        not str(billing_profile.get(key) or "").strip()
        for key in ("recipient_name", "recipient_email", "street_line1", "postal_code", "city")
    )
    if stripe_customer_id and needs_stripe_prefill:
        stripe_profile = handler._billing_profile_from_stripe_customer(stripe_customer_id)
        billing_profile, stripe_imported_fields = handler._billing_prefill_profile_from_stripe(
            billing_profile,
            stripe_profile,
        )

    notices: list[str] = []
    checkout_state = str(request.query.get("checkout") or "").strip().lower()
    if checkout_state == "success":
        notices.append(
            "<div class='notice notice-ok'>Checkout erfolgreich. Dein Abo wird in Stripe aktiviert.</div>"
        )
    elif checkout_state == "cancelled":
        notices.append(
            "<div class='notice notice-warn'>Checkout abgebrochen. Du kannst jederzeit neu starten.</div>"
        )
    elif checkout_state == "unavailable":
        checkout_reason = str(request.query.get("reason") or "").strip().lower()
        if checkout_reason == "stripe_sdk_missing":
            notices.append(
                "<div class='notice notice-error'>Checkout nicht verfügbar: Stripe SDK fehlt auf dem Server.</div>"
            )
        elif checkout_reason == "stripe_secret_key_missing":
            notices.append(
                "<div class='notice notice-error'>Checkout nicht verfügbar: Stripe Secret Key fehlt.</div>"
            )
        elif checkout_reason == "checkout_not_ready":
            notices.append(
                "<div class='notice notice-error'>Checkout nicht verfuegbar: Publishable Key, Secret Key oder Redirect-URLs fehlen.</div>"
            )
        elif checkout_reason == "stripe_price_id_map_missing":
            notices.append(
                "<div class='notice notice-error'>Checkout nicht verfuegbar: Stripe Price-ID-Mapping ist unvollstaendig oder fehlt.</div>"
            )
        elif checkout_reason == "missing_stripe_price_id":
            notices.append(
                "<div class='notice notice-error'>Checkout nicht verfuegbar: fuer diesen Plan/Zyklus ist keine Stripe Price ID hinterlegt.</div>"
            )
        else:
            notices.append(
                "<div class='notice notice-error'>Checkout derzeit nicht verfügbar. Bitte später erneut versuchen.</div>"
            )

    cancel_state = str(request.query.get("cancel") or "").strip().lower()
    if cancel_state == "scheduled":
        notices.append(
            "<div class='notice notice-ok'>Kündigung zum Laufzeitende wurde in Stripe vorgemerkt.</div>"
        )
    elif cancel_state == "missing":
        notices.append(
            "<div class='notice notice-warn'>Keine aktive Stripe-Subscription gefunden.</div>"
        )
    elif cancel_state == "error":
        notices.append(
            "<div class='notice notice-error'>Kündigung konnte nicht ausgeführt werden. Bitte später erneut versuchen.</div>"
        )

    invoice_state = str(request.query.get("invoice") or "").strip().lower()
    if invoice_state == "missing_customer":
        notices.append(
            "<div class='notice notice-warn'>Keine Stripe-Kundennummer gefunden. Bitte zuerst ein Abo abschließen.</div>"
        )
    elif invoice_state == "error":
        notices.append(
            "<div class='notice notice-error'>Stripe-Rechnungen konnten gerade nicht geladen werden.</div>"
        )

    profile_state = str(request.query.get("profile") or "").strip().lower()
    if profile_state == "saved":
        notices.append("<div class='notice notice-ok'>Rechnungsdaten wurden gespeichert.</div>")
    elif profile_state == "invalid":
        notices.append(
            "<div class='notice notice-warn'>Bitte alle Pflichtfelder für Rechnungen ausfüllen.</div>"
        )
    elif profile_state == "error":
        notices.append(
            "<div class='notice notice-error'>Rechnungsdaten konnten nicht gespeichert werden.</div>"
        )
    lurker_tax_state = str(request.query.get("lurker_tax") or "").strip().lower()
    if lurker_tax_state == "saved":
        notices.append(
            "<div class='notice notice-ok'>Lurker Steuer Einstellung gespeichert.</div>"
        )
    elif lurker_tax_state == "error":
        notices.append(
            "<div class='notice notice-error'>Lurker Steuer Einstellung konnte nicht gespeichert werden.</div>"
        )
    if stripe_imported_fields:
        notices.append(
            "<div class='notice notice-warn'>Rechnungsdaten wurden aus Stripe vorbefüllt. Bitte prüfen und speichern.</div>"
        )
    status_notice_html = (
        f"<section class='status-notices'>{''.join(notices)}</section>" if notices else ""
    )

    cycle_switch = []
    for months in (1, 6, 12):
        label = _billing_cycle_label(months)
        css_class = "cycle-btn active" if months == selected_cycle else "cycle-btn"
        cycle_switch.append(
            f"<a class='{css_class}' href='/twitch/abbo?cycle={months}'>{html.escape(label)}</a>"
        )
    cycle_switch_html = "".join(cycle_switch)

    paid_plans = [plan for plan in list(catalog.get("plans") or []) if _billing_is_paid_plan(plan)]
    for plan in paid_plans:
        plan_id = str(plan.get("id") or "").strip()
        stripe_price_id = (
            str(price_lookup(plan_id, selected_cycle, price_map=price_map) or "").strip()
            if callable(price_lookup)
            else ""
        )
        plan["stripe_price_id"] = stripe_price_id or None
        plan["checkout_available"] = bool(checkout_enabled and stripe_price_id)
    current_plan = handler._billing_current_plan_for_request(request)
    current_plan_id = str(current_plan.get("plan_id") or "raid_free").strip() or "raid_free"
    selected_paid_plan = next(
        (
            plan
            for plan in paid_plans
            if str(plan.get("id") or "").strip() == current_plan_id
        ),
        None,
    )
    if selected_paid_plan is None:
        selected_paid_plan = next(
            (plan for plan in paid_plans if bool(plan.get("recommended"))),
            paid_plans[0] if paid_plans else None,
        )

    account_actions: list[str] = []
    if selected_paid_plan is not None:
        pay_plan_id = str(selected_paid_plan.get("id") or "").strip()
        if bool(selected_paid_plan.get("checkout_available")):
            account_actions.append(
                f"<form method='get' action='/twitch/abbo/bezahlen' style='margin:0'>"
                f"<input type='hidden' name='plan_id' value='{html.escape(pay_plan_id, quote=True)}'>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
                f"<input type='hidden' name='cycle' value='{selected_cycle}'>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
                "<input type='hidden' name='quantity' value='1'>"
                "<label class='widerruf-label'>"
                "<input type='checkbox' name='widerruf_ok' required>"
                " Ich stimme zu, dass die Leistung sofort nach Buchung startet und mein "
                "<a href='/twitch/agb#widerruf'>Widerrufsrecht</a> damit erlischt."
                "</label>"
                "<button type='submit' class='action-btn action-primary'>Zu Stripe Checkout</button>"
                "</form>"
            )
        else:
            account_actions.append(
                "<span class='action-btn action-neutral'>Stripe Checkout derzeit nicht bereit</span>"
            )
    account_actions.append(
        "<a class='action-btn action-neutral' href='/twitch/abbo/rechnungen'>Rechnungen herunterladen (PDF)</a>"
    )
    account_actions.append(
        "<form method='post' action='/twitch/abbo/kündigen' style='margin:0;'>"
        f"<input type='hidden' name='csrf_token' value='{html.escape(csrf_token, quote=True)}'>"
        "<button class='action-btn action-danger' type='submit'>Abo kündigen</button>"
        "</form>"
    )
    if handler._is_local_request(request) or handler._is_discord_admin_request(request):
        account_actions.append(
            "<a class='action-btn action-neutral' href='/twitch/abbo/stripe-settings'>Stripe Settings</a>"
        )
    account_actions_html = "".join(account_actions)

    profile_needs_input = any(
        not str(billing_profile.get(key) or "").strip()
        for key in ("recipient_name", "recipient_email", "street_line1", "postal_code", "city")
    )
    details_open_attr = " open" if profile_needs_input else ""
    billing_profile_form_html = (
        f"<details class='profile-details'{details_open_attr}>"
        "<summary class='profile-summary'>"
        "<span>&#9881; Rechnungsdaten</span>"
        "<span class='profile-hint'>Name, Adresse, USt-IdNr</span>"
        "</summary>"
        "<div class='profile-inner'>"
        "<form method='post' action='/twitch/abbo/rechnungsdaten'>"
        f"<input type='hidden' name='cycle' value='{selected_cycle}'>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
        f"<input type='hidden' name='csrf_token' value='{html.escape(csrf_token, quote=True)}'>"
        "<div class='profile-form'>"
        "<div class='profile-field profile-wide'><label for='recipient_name'>Rechnung an (Name)</label>"
        f"<input id='recipient_name' name='recipient_name' required value='{html.escape(str(billing_profile.get('recipient_name') or ''), quote=True)}'></div>"
        "<div class='profile-field'><label for='recipient_email'>E-Mail</label>"
        f"<input id='recipient_email' name='recipient_email' type='email' required value='{html.escape(str(billing_profile.get('recipient_email') or ''), quote=True)}'></div>"
        "<div class='profile-field'><label for='company_name'>Firma (optional)</label>"
        f"<input id='company_name' name='company_name' value='{html.escape(str(billing_profile.get('company_name') or ''), quote=True)}'></div>"
        "<div class='profile-field profile-wide'><label for='street_line1'>Strasse + Hausnummer</label>"
        f"<input id='street_line1' name='street_line1' required value='{html.escape(str(billing_profile.get('street_line1') or ''), quote=True)}'></div>"
        "<div class='profile-field'><label for='postal_code'>PLZ</label>"
        f"<input id='postal_code' name='postal_code' required value='{html.escape(str(billing_profile.get('postal_code') or ''), quote=True)}'></div>"
        "<div class='profile-field'><label for='city'>Stadt</label>"
        f"<input id='city' name='city' required value='{html.escape(str(billing_profile.get('city') or ''), quote=True)}'></div>"
        "<div class='profile-field'><label for='country_code'>Land (ISO, z.B. DE)</label>"
        f"<input id='country_code' name='country_code' required maxlength='2' value='{html.escape(str(billing_profile.get('country_code') or 'DE'), quote=True)}'></div>"
        "<div class='profile-field'><label for='vat_id'>USt-IdNr (optional)</label>"
        f"<input id='vat_id' name='vat_id' value='{html.escape(str(billing_profile.get('vat_id') or ''), quote=True)}'></div>"
        "</div>"
        "<div class='profile-actions'>"
        "<button class='profile-save-btn' type='submit'>Rechnungsdaten speichern</button>"
        "<span class='profile-help'>Pflichtfelder sind Name, E-Mail und Adresse.</span>"
        "</div>"
        "</form>"
        "</div>"
        "</details>"
    )

    plan_cards: list[str] = []
    for plan in catalog.get("plans", []):
        price = dict(plan.get("price") or {})
        discount_percent = int(price.get("discount_percent") or 0)
        plan_id = str(plan.get("id") or "").strip()
        is_current = bool(plan_id and plan_id == current_plan_id)
        badge = html.escape(str(plan.get("badge") or "").replace("_", " ").title())
        current_badge = "<span class='pill pill-active'>Aktiv</span>" if is_current else ""
        recommendation = (
            "<span class='pill pill-rec'>Empfohlen</span>"
            if bool(plan.get("recommended"))
            else ""
        )
        discount_html = (
            f"<p class='discount'>Rabatt im Zyklus: {discount_percent}%</p>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
            if discount_percent > 0
            else ""
        )
        feature_items = "".join(
            f"<li>{html.escape(str(feature))}</li>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
            for feature in list(plan.get("features") or [])
        )
        is_paid_plan = int(plan.get("monthly_net_cents") or 0) > 0
        pay_href = (
            f"/twitch/abbo/bezahlen?plan_id={html.escape(plan_id, quote=True)}"
            f"&cycle={selected_cycle}&quantity=1"
        )
        if is_paid_plan and bool(plan.get("checkout_available")):
            action_html = f"<a class='btn-plan' href='{pay_href}'>Bezahlen</a>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
        elif is_paid_plan:
            action_html = "<span class='pill'>Checkout nicht bereit</span>"
        elif is_current:
            action_html = "<span class='pill pill-active'>Kostenlos aktiv</span>"
        else:
            action_html = "<span class='pill pill-active'>Kostenlos</span>"
        plan_badge_slug = html.escape(str(plan.get("badge") or "default").lower())
        card_class = (
            f"plan-card plan-{plan_badge_slug}"
            + (" recommended" if bool(plan.get("recommended")) else "")
            + (" current" if is_current else "")
        )
        plan_cards.append(
            f"<article class='{card_class}'>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
            "<div class='plan-head'>"
            f"<span class='pill'>{badge}</span>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
            f"{current_badge}"
            f"{recommendation}"
            "</div>"
            f"<h2>{html.escape(str(plan.get('name') or 'Plan'))}</h2>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
            f"<p class='plan-desc'>{html.escape(str(plan.get('description') or ''))}</p>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
            "<div class='price-box'>"
            f"<div class='price'>{html.escape(str(price.get('total_net_label') or '0,00 EUR'))} inkl. MwSt.</div>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
            f"<div class='price-sub'>Effektiv/Monat: {html.escape(str(price.get('effective_monthly_net_label') or '0,00 EUR'))} inkl. MwSt.</div>"  # nosemgrep: python.django.security.injection.raw-html-format.raw-html-format
            "</div>"
            f"{discount_html}"
            f"<ul>{feature_items}</ul>"
            "<div class='plan-actions'>"
            f"{action_html}"
            "</div>"
            "</article>"
        )
    plans_html = "".join(plan_cards)

    session = handler._get_dashboard_auth_session(request)
    twitch_login = str((session or {}).get("twitch_login", "") or "").strip()
    twitch_user_id = str((session or {}).get("twitch_user_id", "") or "").strip()
    has_promo_disable_control = _billing_plan_has_entitlement(
        current_plan_id,
        "chat.promos.disable",
    )
    promo_disabled, promo_message, lurker_tax_enabled = _load_abbo_saved_settings(
        handler,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
    )

    scope_state = handler._abbo_scope_state(
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
    )
    has_chatters_scope = bool(scope_state.get("has_moderator_read_chatters"))
    current_plan_has_lurker_tax = _billing_plan_has_entitlement(
        current_plan_id,
        "chat.lurker_tax",
    )
    lurker_tax_card_html = ""
    if not current_plan_has_lurker_tax:
        lurker_tax_card_html = (
            "<section class='card'>"
            "<strong style='font-size:14px;color:#e2e8f0;'>Lurker Steuer</strong>"
            "<div class='notice notice-warn' style='margin-top:12px;'>"
            "Verfügbar in Raid Boost, Analyse Dashboard und im Bundle."
            "</div>"
            "<p class='muted' style='margin:10px 0 0;'>"
            "Erinnert bekannte aktuell anwesende Lurker höchstens einmal pro Stunde sanft im Chat. "
            "Upgrade im oberen Planbereich, um den Toggle freizuschalten."
            "</p>"
            "</section>"
        )
    else:
        readiness_notice_html = ""
        if not has_chatters_scope:
            readiness_notice_html = (
                "<div class='notice notice-warn' style='margin-bottom:10px;'>"
                "Readiness-Hinweis: Bot-Scope <code>moderator:read:chatters</code> fehlt oder ist noch nicht geladen. "
                "Solange der Zugriff fehlt, feuert die Lurker Steuer nicht."
                "</div>"
            )
        toggle_checked = " checked" if lurker_tax_enabled else ""
        status_label = "Aktiv" if lurker_tax_enabled else "Inaktiv"
        lurker_tax_card_html = (
            "<section class='card'>"
            "<strong style='font-size:14px;color:#e2e8f0;'>Lurker Steuer</strong>"
            f"<p class='muted' style='margin:8px 0 12px;'>Status: {html.escape(status_label)}. "
            "Bekannte aktuell anwesende Lurker werden höchstens einmal pro Stunde weich erinnert. "
            "Im aktiven Stream werden maximal zwei User direkt erwähnt.</p>"
            f"{readiness_notice_html}"
            "<form method='post' action='/twitch/abbo/lurker-tax-settings'>"
            f"<input type='hidden' name='csrf_token' value='{html.escape(csrf_token, quote=True)}'>"
            "<label class='toggle-label'>"
            "<input type='hidden' name='lurker_tax_enabled' value='0'>"
            f"<input type='checkbox' name='lurker_tax_enabled' value='1'{toggle_checked}>"
            "<span>Lurker Steuer aktivieren"
            "<span class='muted'>"
            "Benötigt eine Live-Session, frische Präsenzdaten und den Bot-Scope "
            "<code>moderator:read:chatters</code>."
            "</span>"
            "</span>"
            "</label>"
            "<button type='submit' class='profile-save-btn'>Speichern</button>"
            "</form>"
            "<small class='muted'>"
            "Optional im Chat deaktivierbar, falls du den Reminder spontan abstellen willst."
            "</small>"
            "</section>"
        )

    promo_error = str(request.query.get("promo_error") or "").strip()
    promo_saved = str(request.query.get("promo_saved") or "").strip() == "1"
    page_html = render_abbo_page(
        logout_url=logout_url,
        cycle_switch_html=cycle_switch_html,
        account_actions_html=account_actions_html,
        billing_profile_form_html=billing_profile_form_html,
        status_notice_html=status_notice_html,
        plans_html=plans_html,
        csrf_token=csrf_token,
        lurker_tax_card_html=lurker_tax_card_html,
        has_promo_disable_control=has_promo_disable_control,
        promo_disabled=promo_disabled,
        promo_message=promo_message,
        promo_error=promo_error,
        promo_saved=promo_saved,
        is_authenticated=bool(twitch_login),
    )
    return web.Response(text=page_html, content_type="text/html")


async def abbo_promo_settings(handler: Any, request: web.Request) -> web.StreamResponse:
    """POST /twitch/abbo/promo-settings — toggle promo_disabled for entitled plans."""
    auth_redirect = _abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect

    current_plan = handler._billing_current_plan_for_request(request)
    current_plan_id = str(current_plan.get("plan_id") or "").strip()
    if not _billing_plan_has_entitlement(current_plan_id, "chat.promos.disable"):
        raise web.HTTPFound("/twitch/abbo")

    data = await request.post()
    csrf_token = str(data.get("csrf_token") or "").strip()
    if not handler._csrf_verify_token(request, csrf_token):
        return web.json_response({"error": "csrf_token_invalid"}, status=403)
    promo_disabled = int(handler._form_checkbox_enabled(data, "promo_disabled"))

    session = handler._get_dashboard_auth_session(request)
    twitch_login = str((session or {}).get("twitch_login", "") or "").strip()
    if not twitch_login:
        raise web.HTTPFound("/twitch/abbo")

    try:
        with storage.transaction() as conn:
            conn.execute(
                "UPDATE streamer_plans SET promo_disabled = %s WHERE LOWER(twitch_login) = LOWER(%s)",
                (promo_disabled, twitch_login),
            )
    except Exception:
        log.exception("promo_disabled update failed for %s", twitch_login)

    raise web.HTTPFound("/twitch/abbo?profile=saved")


async def abbo_lurker_tax_settings(handler: Any, request: web.Request) -> web.StreamResponse:
    """POST /twitch/abbo/lurker-tax-settings — toggle Lurker Steuer for paid plans."""
    auth_redirect = _abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect

    current_plan = handler._billing_current_plan_for_request(request)
    current_plan_id = str(current_plan.get("plan_id") or "").strip()
    if not _billing_plan_has_entitlement(current_plan_id, "chat.lurker_tax"):
        raise web.HTTPFound("/twitch/abbo")

    data = await request.post()
    csrf_token = str(data.get("csrf_token") or "").strip()
    if not handler._csrf_verify_token(request, csrf_token):
        return web.json_response({"error": "csrf_token_invalid"}, status=403)
    lurker_tax_enabled = handler._form_checkbox_enabled(data, "lurker_tax_enabled")

    session = handler._get_dashboard_auth_session(request)
    twitch_login = str((session or {}).get("twitch_login", "") or "").strip()
    twitch_user_id = str((session or {}).get("twitch_user_id", "") or "").strip()
    if not twitch_login and not twitch_user_id:
        raise web.HTTPFound("/twitch/abbo")

    try:
        saved = handler._abbo_upsert_lurker_tax_setting(
            twitch_login=twitch_login,
            twitch_user_id=twitch_user_id,
            plan_id=current_plan_id,
            enabled=lurker_tax_enabled,
        )
    except Exception:
        log.exception("lurker_tax_enabled update failed for %s", twitch_login or twitch_user_id)
        raise web.HTTPFound("/twitch/abbo?lurker_tax=error")
    if not saved:
        raise web.HTTPFound("/twitch/abbo?lurker_tax=error")

    raise web.HTTPFound("/twitch/abbo?lurker_tax=saved")


async def abbo_promo_message(handler: Any, request: web.Request) -> web.StreamResponse:
    """POST /twitch/abbo/promo-message — set custom promo message."""
    auth_redirect = _abbo_auth_redirect_or_none(handler, request)
    if auth_redirect is not None:
        return auth_redirect

    session = handler._get_dashboard_auth_session(request)
    twitch_login = str((session or {}).get("twitch_login", "") or "").strip()
    if not twitch_login:
        raise web.HTTPFound("/twitch/abbo")

    data = await request.post()
    csrf_token = str(data.get("csrf_token") or "").strip()
    if not handler._csrf_verify_token(request, csrf_token):
        return web.json_response({"error": "csrf_token_invalid"}, status=403)
    promo_message = str(data.get("promo_message") or "").strip()

    promo_issues = validate_streamer_promo_message(promo_message)
    if promo_issues:
        issue_code = str(promo_issues[0].get("code") or "").strip().lower()
        promo_error = issue_code if issue_code else "invalid_placeholder"
        raise web.HTTPFound(f"/twitch/abbo?promo_error={promo_error}")

    try:
        with storage.transaction() as conn:
            val = promo_message if promo_message else None
            updated = conn.execute(
                "UPDATE streamer_plans SET promo_message = %s WHERE LOWER(twitch_login) = LOWER(%s)",
                (val, twitch_login),
            ).rowcount
            if not updated:
                log.warning("promo_message: no streamer_plans row for %s, skipping", twitch_login)
    except Exception:
        log.exception("promo_message update failed for %s", twitch_login)

    raise web.HTTPFound("/twitch/abbo?promo_saved=1")
