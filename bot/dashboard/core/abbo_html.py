"""HTML rendering for the Abonnement-Pläne (/twitch/abbo) page."""

from __future__ import annotations

import html as _html


def render_abbo_page(
    *,
    logout_url: str,
    cycle_switch_html: str,
    account_actions_html: str,
    billing_profile_form_html: str,
    status_notice_html: str,
    plans_html: str,
    csrf_token: str = "",
    lurker_tax_card_html: str = "",
    has_promo_disable_control: bool = False,
    promo_disabled: bool = False,
    promo_message: str = "",
    promo_error: str = "",
    promo_saved: bool = False,
    is_authenticated: bool = False,
) -> str:
    """Return the complete HTML for the Abonnement-Pläne page."""
    # --- Promo toggle card ---
    bundle_toggle_html = ""
    if has_promo_disable_control:
        checked = " checked" if promo_disabled else ""
        bundle_toggle_html = (
            "<section class='card'>"
            "<strong style='font-size:14px;color:#e2e8f0;'>Chat-Promos</strong>"
            f"<form method='post' action='/twitch/abbo/promo-settings'>"
            f"<input type='hidden' name='csrf_token' value='{_html.escape(csrf_token, quote=True)}'>"
            "<label class='toggle-label'>"
            "<input type='hidden' name='promo_disabled' value='0'>"
            f"<input type='checkbox' name='promo_disabled' value='1'{checked}>"
            "<span>Chat-Promos deaktivieren"
            "<span class='muted'>Der Bot postet keinen Discord-Invite mehr in deinen Twitch-Chat bei Viewer-Spikes.</span>"
            "</span>"
            "</label>"
            "<button type='submit' class='profile-save-btn'>Speichern</button>"
            "</form>"
            "</section>"
        )

    # --- Discord promo message card ---
    promo_message_html = ""
    if is_authenticated:
        promo_error_html = ""
        if promo_error == "missing_invite":
            promo_error_html = (
                "<div class='notice notice-error' style='margin-bottom:8px;'>"
                "Die Nachricht muss <code>{invite}</code> enthalten.</div>"
            )
        elif promo_error == "too_long":
            promo_error_html = (
                "<div class='notice notice-error' style='margin-bottom:8px;'>"
                "Die Nachricht darf maximal 500 Zeichen lang sein.</div>"
            )
        elif promo_error == "invalid_placeholder":
            promo_error_html = (
                "<div class='notice notice-error' style='margin-bottom:8px;'>"
                "Erlaubt ist in deiner Nachricht nur der Platzhalter <code>{invite}</code>.</div>"
            )
        promo_success_html = ""
        if promo_saved:
            promo_success_html = (
                "<div class='notice notice-ok' style='margin-bottom:8px;'>"
                "Promo-Nachricht gespeichert.</div>"
            )
        escaped_msg = _html.escape(promo_message)
        promo_message_html = (
            "<section class='card'>"
            "<strong style='font-size:14px;color:#e2e8f0;'>Discord-Promo anpassen</strong>"
            "<p class='muted'>Der Bot postet gelegentlich eine Nachricht in deinem Twitch-Chat. "
            "Du kannst den Text frei anpassen, auch mit mehreren Zeilen &mdash; "
            "<code>{invite}</code> muss immer enthalten bleiben und es sind maximal 500 Zeichen erlaubt.</p>"
            f"<form method='post' action='/twitch/abbo/promo-message'>"
            f"<input type='hidden' name='csrf_token' value='{_html.escape(csrf_token, quote=True)}'>"
            f"{promo_success_html}"
            f"{promo_error_html}"
            f"<textarea name='promo_message' placeholder='z.B. Hey! Komm auf unseren Discord: {{invite}}'"
            f" maxlength='500'>{escaped_msg}</textarea>"
            "<button type='submit' class='profile-save-btn'>Speichern</button>"
            "</form>"
            "<small class='muted'>Leer lassen = Standard-Nachrichten des Bots werden verwendet.</small>"
            "</section>"
        )

    return (
        "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
        "<meta name='viewport' content='width=device-width,initial-scale=1'>"
        "<title>Abonnement-Pläne · EarlySalty</title>"
        "<style>"
        ":root{color-scheme:dark;}"

        # === Base ===
        "body{margin:0;"
        "background:linear-gradient(160deg,#111827 0%,#1c1245 40%,#0f3320 75%,#0b3d1e 100%);"
        "min-height:100vh;"
        "color:#f1f5f9;font-family:Segoe UI,system-ui,Arial,sans-serif;line-height:1.55;}"
        ".wrap{width:clamp(320px,70vw,1600px);margin:0 auto;"
        "padding:40px clamp(18px,2vw,30px) 64px;}"

        # === Header ===
        ".top{display:flex;justify-content:space-between;align-items:flex-start;"
        "gap:12px;flex-wrap:wrap;margin-bottom:6px;}"
        ".page-title{margin:0;font-size:2.2rem;font-weight:900;letter-spacing:-0.5px;"
        "background:linear-gradient(135deg,#60a5fa 0%,#a78bfa 100%);"
        "-webkit-background-clip:text;-webkit-text-fill-color:transparent;"
        "background-clip:text;}"
        ".page-subtitle{color:#94a3b8;font-size:1.05rem;margin:0 0 30px;}"
        ".logout-link{font-size:13px;color:#64748b;padding:6px 12px;border-radius:8px;"
        "border:1px solid rgba(255,255,255,0.1);background:rgba(255,255,255,0.05);"
        "text-decoration:none;transition:border-color 0.15s,color 0.15s;}"
        ".logout-link:hover{color:#f87171;border-color:#f87171;text-decoration:none;}"

        # === Links ===
        "a{color:#60a5fa;text-decoration:none;}"
        "a:hover{text-decoration:underline;}"
        ".muted{color:#94a3b8;font-size:14px;}"

        # === Cards ===
        ".card{margin-top:16px;"
        "background:rgba(255,255,255,0.05);backdrop-filter:blur(16px);"
        "border:1px solid rgba(255,255,255,0.1);border-radius:18px;"
        "padding:20px 22px;box-shadow:0 4px 24px rgba(0,0,0,0.4);}"
        ".launch-note{border-left:4px solid #22c55e;}"
        ".launch-title{font-size:14px;color:#e2e8f0;font-weight:700;}"
        ".launch-text{margin-top:7px;font-size:13px;color:#94a3b8;}"

        # === Cycle Buttons ===
        ".cycle-row{display:flex;gap:8px;flex-wrap:wrap;margin-top:12px;}"
        ".cycle-btn{padding:7px 18px;border-radius:999px;"
        "border:1.5px solid rgba(255,255,255,0.12);"
        "background:rgba(255,255,255,0.05);color:#cbd5e1;"
        "text-decoration:none;font-size:13px;font-weight:600;transition:all 0.15s;}"
        ".cycle-btn:hover{border-color:#818cf8;color:#a5b4fc;text-decoration:none;"
        "background:rgba(129,140,248,0.1);}"
        ".cycle-btn.active{background:linear-gradient(135deg,#3b82f6,#8b5cf6);"
        "border-color:transparent;color:#fff;"
        "box-shadow:0 2px 10px rgba(99,102,241,0.5);}"

        # === Plan Grid ===
        ".plans{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));"
        "gap:18px;margin-top:22px;}"

        # === Plan Cards — base ===
        ".plan-card{"
        "--plan-color:#818cf8;--plan-glow:rgba(129,140,248,0.15);"
        "--plan-pill-bg:rgba(129,140,248,0.15);--plan-pill-color:#a5b4fc;"
        "--plan-price-bg:rgba(129,140,248,0.08);--plan-btn-bg:rgba(129,140,248,0.15);"
        "background:rgba(255,255,255,0.04);backdrop-filter:blur(12px);"
        "border:1px solid rgba(255,255,255,0.09);border-radius:18px;padding:20px;"
        "box-shadow:0 4px 20px rgba(0,0,0,0.3);"
        "border-top:3px solid var(--plan-color);"
        "display:flex;flex-direction:column;"
        "transition:transform 0.18s,box-shadow 0.18s;}"
        ".plan-card:hover{transform:translateY(-4px);"
        "box-shadow:0 12px 40px rgba(0,0,0,0.45),0 0 0 1px var(--plan-color) inset;}"

        # === Per-plan colors ===
        ".plan-free{"
        "--plan-color:#10b981;--plan-glow:rgba(16,185,129,0.2);"
        "--plan-pill-bg:rgba(16,185,129,0.15);--plan-pill-color:#34d399;"
        "--plan-price-bg:rgba(16,185,129,0.08);--plan-btn-bg:rgba(16,185,129,0.15);}"
        ".plan-raids{"
        "--plan-color:#8b5cf6;--plan-glow:rgba(139,92,246,0.2);"
        "--plan-pill-bg:rgba(139,92,246,0.15);--plan-pill-color:#c4b5fd;"
        "--plan-price-bg:rgba(139,92,246,0.08);--plan-btn-bg:rgba(139,92,246,0.15);}"
        ".plan-analytics{"
        "--plan-color:#3b82f6;--plan-glow:rgba(59,130,246,0.2);"
        "--plan-pill-bg:rgba(59,130,246,0.15);--plan-pill-color:#93c5fd;"
        "--plan-price-bg:rgba(59,130,246,0.08);--plan-btn-bg:rgba(59,130,246,0.15);}"
        ".plan-bundle{"
        "--plan-color:#f59e0b;--plan-glow:rgba(245,158,11,0.2);"
        "--plan-pill-bg:rgba(245,158,11,0.15);--plan-pill-color:#fcd34d;"
        "--plan-price-bg:rgba(245,158,11,0.08);--plan-btn-bg:rgba(245,158,11,0.15);}"

        # === Recommended ===
        ".plan-card.recommended{"
        "border:1.5px solid var(--plan-color);"
        "background:rgba(255,255,255,0.07);"
        "box-shadow:0 4px 32px var(--plan-glow),0 0 0 1px var(--plan-color);}"
        ".plan-card.current{"
        "border:1.5px solid #22c55e;"
        "background:rgba(22,101,52,0.16);"
        "box-shadow:0 0 0 1px rgba(34,197,94,0.8),0 8px 28px rgba(16,185,129,0.25);}"

        # === Plan internals ===
        ".plan-head{display:flex;justify-content:space-between;align-items:center;"
        "gap:8px;margin-bottom:6px;}"
        ".pill{display:inline-block;padding:4px 11px;border-radius:999px;"
        "background:var(--plan-pill-bg);color:var(--plan-pill-color);"
        "font-size:11px;font-weight:700;text-transform:uppercase;letter-spacing:0.05em;}"
        ".pill-active{background:rgba(34,197,94,0.2);color:#86efac;"
        "box-shadow:0 0 10px rgba(34,197,94,0.22);}"
        ".pill-rec{background:rgba(245,158,11,0.2);color:#fcd34d;"
        "box-shadow:0 0 8px rgba(245,158,11,0.3);}"
        ".plan-card h2{margin:8px 0 6px;font-size:1.05rem;font-weight:800;color:#f1f5f9;}"
        ".plan-desc{color:#94a3b8;font-size:13.5px;margin:0 0 14px;line-height:1.5;}"

        # === Price ===
        ".price-box{background:var(--plan-price-bg);border-radius:10px;"
        "padding:10px 12px;margin:0 0 12px;border:1px solid rgba(255,255,255,0.05);}"
        ".price{font-size:1.3rem;font-weight:900;color:var(--plan-color);}"
        ".price-sub{font-size:11.5px;color:#64748b;margin-top:2px;}"
        ".discount{display:inline-block;font-size:12px;color:#fcd34d;"
        "background:rgba(245,158,11,0.15);border-radius:999px;"
        "padding:2px 9px;margin:0 0 10px;font-weight:600;}"

        # === Feature list ===
        ".plan-card ul{margin:0 0 10px 0;padding:0;list-style:none;flex:1;}"
        ".plan-card li{margin-bottom:7px;padding-left:22px;position:relative;"
        "font-size:13.5px;color:#cbd5e1;}"
        ".plan-card li::before{content:'✓';position:absolute;left:0;"
        "color:var(--plan-color);font-weight:800;font-size:14px;}"

        # === Plan actions ===
        ".plan-actions{display:flex;gap:8px;margin-top:auto;padding-top:12px;"
        "border-top:1px solid rgba(255,255,255,0.07);}"
        ".btn-plan{display:inline-block;padding:8px 14px;border-radius:9px;font-weight:600;"
        "font-size:13px;text-decoration:none;"
        "background:var(--plan-btn-bg);color:var(--plan-pill-color);"
        "border:1px solid rgba(255,255,255,0.07);"
        "transition:opacity 0.15s,transform 0.12s;}"
        ".btn-plan:hover{opacity:0.8;transform:translateY(-1px);text-decoration:none;}"
        # === Legal ===
        ".legal-section{margin-top:30px;"
        "background:rgba(255,255,255,0.04);border:1px solid rgba(255,255,255,0.1);"
        "border-radius:14px;padding:20px 24px;}"
        ".legal-section h2{margin:0 0 14px;font-size:12px;font-weight:700;"
        "color:#64748b;text-transform:uppercase;letter-spacing:0.08em;}"
        ".legal-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));"
        "gap:12px;}"
        ".legal-item{display:flex;align-items:flex-start;gap:10px;"
        "background:rgba(255,255,255,0.03);border-radius:10px;padding:12px 14px;"
        "border:1px solid rgba(255,255,255,0.06);}"
        ".legal-icon{font-size:18px;flex-shrink:0;line-height:1;margin-top:1px;}"
        ".legal-text{font-size:13px;color:#94a3b8;line-height:1.5;}"
        ".legal-text strong{display:block;color:#cbd5e1;font-size:13px;margin-bottom:2px;}"
        ".legal-text a{color:#93c5fd;}"
        ".legal-text a:hover{color:#bfdbfe;text-decoration:underline;}"

        # === Status notices ===
        ".status-notices{display:grid;gap:10px;margin-top:16px;}"
        ".notice{border-radius:12px;padding:11px 13px;font-size:13px;font-weight:600;border:1px solid transparent;}"
        ".notice-ok{background:rgba(22,163,74,0.18);border-color:rgba(74,222,128,0.38);color:#bbf7d0;}"
        ".notice-warn{background:rgba(217,119,6,0.18);border-color:rgba(251,191,36,0.38);color:#fde68a;}"
        ".notice-error{background:rgba(220,38,38,0.18);border-color:rgba(248,113,113,0.38);color:#fecaca;}"

        # === Billing actions ===
        ".account-actions{display:flex;gap:10px;flex-wrap:wrap;margin-top:12px;}"
        ".action-btn{display:inline-flex;align-items:center;justify-content:center;"
        "padding:9px 14px;border-radius:10px;font-weight:700;font-size:13px;"
        "text-decoration:none;border:1px solid rgba(255,255,255,0.16);"
        "transition:transform 0.12s,opacity 0.12s;}"
        ".action-btn:hover{text-decoration:none;opacity:0.92;transform:translateY(-1px);}"
        ".action-primary{background:linear-gradient(135deg,#2563eb,#3b82f6);color:#eff6ff;border-color:transparent;}"
        ".action-neutral{background:rgba(255,255,255,0.08);color:#e2e8f0;}"
        ".action-danger{background:rgba(190,24,93,0.24);color:#fbcfe8;border-color:rgba(244,114,182,0.35);}"
        ".action-note{margin-top:10px;font-size:12px;color:#94a3b8;}"
        ".profile-form{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:10px;margin-top:12px;}"
        ".profile-field{display:flex;flex-direction:column;gap:6px;}"
        ".profile-field label{font-size:11px;color:#94a3b8;font-weight:700;text-transform:uppercase;letter-spacing:0.04em;}"
        ".profile-field input{border-radius:10px;border:1px solid rgba(255,255,255,0.18);"
        "background:rgba(15,23,42,0.5);color:#e2e8f0;padding:9px 10px;font-size:13px;outline:none;}"
        ".profile-field input:focus{border-color:#60a5fa;box-shadow:0 0 0 2px rgba(96,165,250,0.25);}"
        ".profile-field.profile-wide{grid-column:1/-1;}"
        ".profile-actions{display:flex;gap:10px;align-items:center;flex-wrap:wrap;margin-top:12px;}"
        ".profile-save-btn{display:inline-flex;align-items:center;justify-content:center;padding:9px 14px;border-radius:10px;"
        "font-weight:700;font-size:13px;text-decoration:none;border:1px solid transparent;cursor:pointer;"
        "background:linear-gradient(135deg,#2563eb,#3b82f6);color:#eff6ff;}"
        ".profile-help{font-size:12px;color:#94a3b8;}"

        # === Toggle label ===
        ".toggle-label{display:flex;gap:10px;align-items:flex-start;margin:12px 0;"
        "font-size:13px;color:#e2e8f0;cursor:pointer;line-height:1.5;}"
        ".toggle-label input[type=checkbox]{margin-top:3px;flex-shrink:0;}"
        ".toggle-label .muted{display:block;margin-top:2px;}"

        # === Textarea ===
        "textarea{width:100%;min-height:80px;border-radius:10px;"
        "border:1px solid rgba(255,255,255,0.18);background:rgba(15,23,42,0.5);"
        "color:#e2e8f0;padding:9px 10px;font-size:13px;font-family:inherit;"
        "outline:none;resize:vertical;box-sizing:border-box;margin:8px 0;}"
        "textarea:focus{border-color:#60a5fa;box-shadow:0 0 0 2px rgba(96,165,250,0.25);}"

        # === Footer ===
        ".site-footer{margin-top:32px;text-align:center;font-size:12px;color:#475569;"
        "padding-top:16px;border-top:1px solid rgba(255,255,255,0.06);}"
        ".site-footer a{color:#64748b;}"
        ".site-footer a:hover{color:#818cf8;}"
        "@media (max-width:1200px){.wrap{width:78vw;}}"
        "@media (max-width:980px){.wrap{width:88vw;padding:30px 16px 54px;}}"
        "@media (max-width:680px){"
        ".wrap{width:auto;padding:24px 12px 40px;}"
        ".page-title{font-size:1.8rem;}"
        ".plan-actions{padding-top:10px;}"
        "}"

        # === Collapsible Profile ===
        ".profile-details{margin-top:16px;"
        "background:rgba(255,255,255,0.05);backdrop-filter:blur(16px);"
        "border:1px solid rgba(255,255,255,0.1);border-radius:18px;"
        "padding:0;box-shadow:0 4px 24px rgba(0,0,0,0.4);}"
        ".profile-summary{cursor:pointer;display:flex;justify-content:space-between;"
        "align-items:center;padding:16px 22px;color:#e2e8f0;font-weight:700;"
        "font-size:14px;list-style:none;user-select:none;}"
        ".profile-summary::-webkit-details-marker{display:none;}"
        "details[open] .profile-summary{border-bottom:1px solid rgba(255,255,255,0.1);color:#93c5fd;}"
        ".profile-hint{font-size:12px;color:#64748b;font-weight:400;}"
        ".profile-inner{padding:0 22px 20px;}"
        # === Widerruf ===
        ".widerruf-label{font-size:12px;color:#94a3b8;display:flex;gap:8px;"
        "align-items:flex-start;margin-top:8px;line-height:1.5;}"
        ".widerruf-label input{margin-top:2px;flex-shrink:0;}"
        ".widerruf-label a{color:#93c5fd;}"

        "</style></head><body><main class='wrap'>"

        # --- Header ---
        "<div class='top'>"
        "<div>"
        "<h1 class='page-title'>Abonnement-Pläne</h1>"
        "<p class='page-subtitle'>W&auml;hle deinen Plan &middot; Raids, Analytics oder beides</p>"
        "</div>"
        f"<a class='logout-link' href='{_html.escape(logout_url)}'>Logout</a>"
        "</div>"

        # --- Cycle selector ---
        "<section class='card'>"
        "<strong style='font-size:14px;color:#e2e8f0;'>Abrechnungszyklus</strong>"
        "<span class='muted' style='margin-left:8px;'>"
        "6 Monate: &minus;10&nbsp;% &nbsp;&middot;&nbsp; 12 Monate: &minus;20&nbsp;%</span>"
        f"<div class='cycle-row'>{cycle_switch_html}</div>"
        "</section>"

        f"{status_notice_html}"

        # --- Plan cards ---
        "<section class='plans'>"
        f"{plans_html}"
        "</section>"

        f"{billing_profile_form_html}"

        "<section class='card'>"
        "<strong style='font-size:14px;color:#e2e8f0;'>Abo Verwaltung</strong>"
        f"<div class='account-actions'>{account_actions_html}</div>"
        "<div class='action-note'>"
        "Abrechnung l&auml;uft &uuml;ber Stripe. Rechnungsdaten bitte vor dem Checkout pflegen."
        "</div>"
        "</section>"

        f"{lurker_tax_card_html}"
        f"{bundle_toggle_html}"
        f"{promo_message_html}"

        # --- Legal hints ---
        "<section class='legal-section'>"
        "<h2>Rechtliche Hinweise</h2>"
        "<div class='legal-grid'>"
        "<div class='legal-item'>"
        "<span class='legal-icon'>&#x1F4B6;</span>"
        "<div class='legal-text'>"
        "<strong>Preise netto</strong>"
        "Alle Preise verstehen sich zzgl. 19&nbsp;% MwSt. gem&auml;&szlig; &sect;&nbsp;12 UStG."
        "</div></div>"
        "<div class='legal-item'>"
        "<span class='legal-icon'>&#x1F512;</span>"
        "<div class='legal-text'>"
        "<strong>Sichere Zahlungsabwicklung</strong>"
        "Zahlungsdaten werden ausschlie&szlig;lich bei der Abwicklung verarbeitet."
        " Es werden keine Kartendaten auf dieser Plattform gespeichert."
        "</div></div>"
        "<div class='legal-item'>"
        "<span class='legal-icon'>&#x21A9;&#xFE0F;</span>"
        "<div class='legal-text'>"
        "<strong>Widerrufsrecht</strong>"
        "Das Widerrufsrecht erlischt bei digitalen Diensten mit sofortigem Zugang nach Aktivierung."
        "</div></div>"
        "<div class='legal-item'>"
        "<span class='legal-icon'>&#x1F4C4;</span>"
        "<div class='legal-text'>"
        "<strong>Rechtliches</strong>"
        "<a href='/twitch/impressum'>Impressum</a>"
        " &nbsp;&middot;&nbsp; "
        "<a href='/twitch/datenschutz'>Datenschutz</a>"
        " &nbsp;&middot;&nbsp; "
        "<a href='/twitch/agb'>AGB</a>"
        "</div></div>"
        "</div>"
        "</section>"

        # --- Footer ---
        "<footer class='site-footer'>"
        "&copy; 2026 EarlySalty / Nathanael Golla"
        "</footer>"
        "</main></body></html>"
    )
