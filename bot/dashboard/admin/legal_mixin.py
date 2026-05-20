"""Legal pages mixin: Impressum + Datenschutz (§5 TMG / DSGVO)."""

from __future__ import annotations

import hashlib
import hmac
import html
import time
from urllib.parse import urlencode, urlsplit

import aiohttp
from aiohttp import web

from ...core.constants import log

LEGAL_PAGE_HEADERS = {
    "X-Robots-Tag": "noindex, nofollow, noarchive, nosnippet, noimageindex"
}
LEGAL_GATE_TURNSTILE_VERIFY_URL = "https://challenges.cloudflare.com/turnstile/v0/siteverify"
LEGAL_GATE_ALLOWED_PATHS = frozenset(("/twitch/impressum", "/twitch/datenschutz"))
LEGAL_GATE_COOKIE_NAME = "twitch_legal_gate"
LEGAL_GATE_COOKIE_TTL_SECONDS = 600
LEGAL_GATE_TURNSTILE_ACTION = "legal_access"
BLOCKED_LEGAL_PAGE_USER_AGENT_TOKENS: tuple[str, ...] = (
    "gptbot",
    "chatgpt-user",
    "oai-searchbot",
    "claudebot",
    "anthropic-ai",
    "perplexitybot",
    "perplexity-user",
    "google-extended",
    "ccbot",
    "bytespider",
    "facebookbot",
    "meta-externalagent",
    "applebot",
    "amazonbot",
    "petalbot",
    "yandexbot",
    "duckassistbot",
    "crawler",
    "spider",
    "slurp",
    "bot/",
)


def _is_blocked_legal_page_user_agent(user_agent: str) -> bool:
    normalized = str(user_agent or "").strip().lower()
    if not normalized:
        return False
    return any(token in normalized for token in BLOCKED_LEGAL_PAGE_USER_AGENT_TOKENS)


def _build_blocked_legal_page_response() -> web.Response:
    return web.Response(
        text="Forbidden",
        status=403,
        content_type="text/plain",
        headers=LEGAL_PAGE_HEADERS,
    )


class _DashboardLegalMixin:
    """Handlers for /twitch/impressum and /twitch/datenschutz — no auth required."""

    @staticmethod
    def _legal_page_request_is_blocked(request: web.Request) -> bool:
        return _is_blocked_legal_page_user_agent(request.headers.get("User-Agent"))

    def _legal_turnstile_site_key(self) -> str:
        cached = getattr(self, "_legal_turnstile_site_key_cache", None)
        if isinstance(cached, str):
            return cached
        loader = getattr(self, "_load_secret_value", None)
        value = ""
        if callable(loader):
            value = str(
                loader("TWITCH_LEGAL_TURNSTILE_SITE_KEY", "TURNSTILE_SITE_KEY") or ""
            ).strip()
        setattr(self, "_legal_turnstile_site_key_cache", value)
        return value

    def _legal_turnstile_secret_key(self) -> str:
        cached = getattr(self, "_legal_turnstile_secret_key_cache", None)
        if isinstance(cached, str):
            return cached
        loader = getattr(self, "_load_secret_value", None)
        value = ""
        if callable(loader):
            value = str(
                loader("TWITCH_LEGAL_TURNSTILE_SECRET_KEY", "TURNSTILE_SECRET_KEY") or ""
            ).strip()
        setattr(self, "_legal_turnstile_secret_key_cache", value)
        return value

    def _legal_gate_cookie_secret(self) -> str:
        cached = getattr(self, "_legal_gate_cookie_secret_cache", None)
        if isinstance(cached, str):
            return cached
        loader = getattr(self, "_load_secret_value", None)
        value = ""
        if callable(loader):
            value = str(
                loader("TWITCH_LEGAL_GATE_COOKIE_SECRET", "LEGAL_GATE_COOKIE_SECRET") or ""
            ).strip()
        setattr(self, "_legal_gate_cookie_secret_cache", value)
        return value

    def _legal_gate_configuration_state(self) -> str:
        parts = (
            self._legal_turnstile_site_key(),
            self._legal_turnstile_secret_key(),
            self._legal_gate_cookie_secret(),
        )
        if all(parts):
            return "enabled"
        if any(parts):
            return "misconfigured"
        return "missing"

    def _legal_gate_is_enabled(self) -> bool:
        return self._legal_gate_configuration_state() == "enabled"

    def _legal_gate_configuration_error_response(self) -> web.Response:
        state = self._legal_gate_configuration_state()
        log.error(
            "Legal human gate is unavailable: configuration state=%s. "
            "Expected keyring secrets TWITCH_LEGAL_TURNSTILE_SITE_KEY, "
            "TWITCH_LEGAL_TURNSTILE_SECRET_KEY, and TWITCH_LEGAL_GATE_COOKIE_SECRET.",
            state,
        )
        return web.Response(
            text="Legal access gate is not configured.",
            status=503,
            content_type="text/plain",
            headers=LEGAL_PAGE_HEADERS,
        )

    @staticmethod
    def _normalize_legal_gate_next_path(raw_path: str | None) -> str:
        candidate = str(raw_path or "").strip()
        if candidate in LEGAL_GATE_ALLOWED_PATHS:
            return candidate
        return "/twitch/impressum"

    def _legal_gate_cookie_value(self, *, expires_at: int) -> str:
        expires_raw = str(int(expires_at))
        signature = hmac.new(
            self._legal_gate_cookie_secret().encode("utf-8"),
            expires_raw.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()
        return f"{expires_raw}.{signature}"

    def _legal_gate_cookie_is_valid(self, request: web.Request) -> bool:
        if not self._legal_gate_is_enabled():
            return False
        raw_cookie = str(request.cookies.get(LEGAL_GATE_COOKIE_NAME) or "").strip()
        if "." not in raw_cookie:
            return False
        expires_raw, provided_signature = raw_cookie.split(".", 1)
        if not expires_raw.isdigit() or not provided_signature:
            return False
        expires_at = int(expires_raw)
        if expires_at <= int(time.time()):
            return False
        expected_cookie = self._legal_gate_cookie_value(expires_at=expires_at)
        return hmac.compare_digest(raw_cookie, expected_cookie)

    def _legal_gate_redirect(self, request: web.Request) -> web.HTTPFound:
        next_path = self._normalize_legal_gate_next_path(request.path)
        location = f"/twitch/legal/access?{urlencode({'next': next_path})}"
        safe_location = (
            self._safe_internal_redirect(location, fallback="/twitch/legal/access")
            if hasattr(self, "_safe_internal_redirect")
            else "/twitch/legal/access"
        )
        return web.HTTPFound(safe_location)

    @staticmethod
    def _legal_request_host(request: web.Request) -> str:
        raw_host = str(request.headers.get("Host") or request.host or "").strip()
        if not raw_host:
            return ""
        candidate = raw_host if "://" in raw_host else f"//{raw_host}"
        try:
            parsed = urlsplit(candidate)
        except Exception:
            return ""
        return str(parsed.hostname or "").strip().lower()

    def _legal_turnstile_remote_ip(self, request: web.Request) -> str | None:
        cf_connecting_ip = str(request.headers.get("CF-Connecting-IP") or "").strip()
        peer_getter = getattr(self, "_peer_host", None)
        trusted_proxy_checker = getattr(self, "_is_trusted_proxy_host", None)
        peer_host = str(peer_getter(request)).strip() if callable(peer_getter) else ""
        if (
            cf_connecting_ip
            and callable(trusted_proxy_checker)
            and trusted_proxy_checker(peer_host)
        ):
            return cf_connecting_ip
        remote = str(request.remote or "").strip()
        return remote or None

    def _legal_gate_set_cookie(
        self,
        response: web.StreamResponse,
        request: web.Request,
    ) -> None:
        secure_checker = getattr(self, "_is_secure_request", None)
        is_secure = bool(secure_checker(request)) if callable(secure_checker) else False
        response.set_cookie(
            LEGAL_GATE_COOKIE_NAME,
            self._legal_gate_cookie_value(
                expires_at=int(time.time()) + LEGAL_GATE_COOKIE_TTL_SECONDS
            ),
            max_age=LEGAL_GATE_COOKIE_TTL_SECONDS,
            httponly=True,
            secure=is_secure,
            samesite="Lax",
            path="/twitch/",
        )

    async def _verify_legal_turnstile_token(
        self,
        request: web.Request,
        token: str,
    ) -> bool:
        normalized_token = str(token or "").strip()
        secret_key = self._legal_turnstile_secret_key()
        if not normalized_token or not secret_key:
            return False

        remote_ip = self._legal_turnstile_remote_ip(request)
        payload = {
            "secret": secret_key,
            "response": normalized_token,
        }
        if remote_ip:
            payload["remoteip"] = remote_ip

        timeout = aiohttp.ClientTimeout(total=10)
        try:
            async with aiohttp.ClientSession(timeout=timeout) as session:
                async with session.post(LEGAL_GATE_TURNSTILE_VERIFY_URL, data=payload) as response:
                    result = await response.json()
        except Exception:
            return False

        if not bool(result.get("success")):
            return False
        action = str(result.get("action") or "").strip()
        if action != LEGAL_GATE_TURNSTILE_ACTION:
            return False
        hostname = str(result.get("hostname") or "").strip().lower()
        if not hostname:
            return False
        if hostname != self._legal_request_host(request):
            return False
        return True

    @staticmethod
    def _render_legal_gate_page(*, next_path: str, site_key: str) -> str:
        escaped_next = html.escape(next_path, quote=True)
        escaped_site_key = html.escape(site_key, quote=True)
        return (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<meta name='robots' content='noindex, nofollow'>"
            "<title>Zugang freischalten · EarlySalty</title>"
            "<script src='https://challenges.cloudflare.com/turnstile/v0/api.js' async defer></script>"
            "<style>"
            "body{margin:0;background:#f8fafc;color:#0f172a;font-family:Segoe UI,Arial,sans-serif;}"
            ".wrap{max-width:680px;margin:0 auto;padding:48px 20px 72px;}"
            ".card{background:#ffffff;border:1px solid #e2e8f0;border-radius:18px;padding:28px;"
            "box-shadow:0 18px 46px rgba(15,23,42,.08);}"
            "h1{margin:0 0 12px;font-size:1.85rem;font-weight:800;}"
            "p{margin:0 0 14px;line-height:1.65;color:#334155;font-size:15px;}"
            ".hint{font-size:13px;color:#64748b;}"
            "button{margin-top:18px;border:0;border-radius:10px;padding:12px 18px;font-size:15px;"
            "font-weight:700;background:#2563eb;color:#fff;cursor:pointer;}"
            "button:hover{background:#1d4ed8;}"
            ".turnstile{margin:20px 0 6px;}"
            "</style></head><body><div class='wrap'><div class='card'>"
            "<h1>Bitte kurz bestätigen</h1>"
            "<p>Diese Seite ist für Menschen weiterhin zugänglich, aber wir schützen sie vor Bots "
            "und KI-Crawlern. Bestätige einmal kurz den Zugriff.</p>"
            f"<form method='post' action='/twitch/legal/verify'><input type='hidden' name='next' value='{escaped_next}'>"
            f"<div class='cf-turnstile turnstile' data-sitekey='{escaped_site_key}' data-action='{LEGAL_GATE_TURNSTILE_ACTION}'></div>"
            "<button type='submit'>Weiter</button></form>"
            "<p class='hint'>Freigabe gilt nur kurzzeitig und nur für die Legal-Seiten.</p>"
            "</div></div></body></html>"
        )

    async def robots_txt(self, request: web.Request) -> web.StreamResponse:  # noqa: ARG002
        robots = (
            "User-agent: *\n"
            "Disallow: /twitch/impressum\n"
            "Disallow: /twitch/datenschutz\n"
        )
        return web.Response(text=robots, content_type="text/plain")

    async def legal_access_page(self, request: web.Request) -> web.StreamResponse:
        if self._legal_page_request_is_blocked(request):
            return _build_blocked_legal_page_response()
        next_path = self._normalize_legal_gate_next_path(request.query.get("next"))
        if not self._legal_gate_is_enabled():
            return self._legal_gate_configuration_error_response()
        if self._legal_gate_cookie_is_valid(request):
            safe_next_path = (
                self._safe_internal_redirect(next_path, fallback="/twitch/impressum")
                if hasattr(self, "_safe_internal_redirect")
                else "/twitch/impressum"
            )
            raise web.HTTPFound(safe_next_path)
        page = self._render_legal_gate_page(
            next_path=next_path,
            site_key=self._legal_turnstile_site_key(),
        )
        return web.Response(text=page, content_type="text/html", headers=LEGAL_PAGE_HEADERS)

    async def legal_verify(self, request: web.Request) -> web.StreamResponse:
        if self._legal_page_request_is_blocked(request):
            return _build_blocked_legal_page_response()
        body = await request.post()
        next_path = self._normalize_legal_gate_next_path(body.get("next"))
        if not self._legal_gate_is_enabled():
            return self._legal_gate_configuration_error_response()
        turnstile_token = str(body.get("cf-turnstile-response") or "").strip()
        if not await self._verify_legal_turnstile_token(request, turnstile_token):
            return web.Response(
                text="Turnstile verification failed.",
                status=403,
                content_type="text/plain",
                headers=LEGAL_PAGE_HEADERS,
            )
        safe_next_path = (
            self._safe_internal_redirect(next_path, fallback="/twitch/impressum")
            if hasattr(self, "_safe_internal_redirect")
            else "/twitch/impressum"
        )
        response = web.HTTPFound(safe_next_path)
        self._legal_gate_set_cookie(response, request)
        raise response

    async def abbo_impressum(self, request: web.Request) -> web.StreamResponse:
        """GET /twitch/impressum — §5 TMG. Must be accessible without login."""
        if self._legal_page_request_is_blocked(request):
            return _build_blocked_legal_page_response()
        if not self._legal_gate_is_enabled():
            return self._legal_gate_configuration_error_response()
        if not self._legal_gate_cookie_is_valid(request):
            raise self._legal_gate_redirect(request)
        page = (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<meta name='robots' content='noindex, nofollow'>"
            "<title>Impressum · EarlySalty</title>"
            "<style>"
            "body{margin:0;background:#f8fafc;color:#0f172a;"
            "font-family:Segoe UI,Arial,sans-serif;line-height:1.7;}"
            ".wrap{max-width:700px;margin:0 auto;padding:40px 20px 60px;}"
            "h1{font-size:1.7rem;margin:0 0 6px;font-weight:800;}"
            ".back{font-size:13px;color:#64748b;margin-bottom:24px;display:block;"
            "text-decoration:none;}"
            ".back:hover{color:#2563eb;}"
            "h2{font-size:1.05rem;margin:26px 0 6px;color:#0f172a;font-weight:700;}"
            "p,address{font-size:15px;color:#334155;font-style:normal;margin:0 0 8px;}"
            "a{color:#2563eb;text-decoration:none;}"
            "a:hover{text-decoration:underline;}"
            ".sub{color:#64748b;font-size:14px;margin:0 0 20px;}"
            ".footer{margin-top:40px;font-size:12px;color:#94a3b8;"
            "border-top:1px solid #e2e8f0;padding-top:14px;}"
            "</style></head><body><div class='wrap'>"
            "<a class='back' href='/twitch/abbo'>&larr; Zurück zu den Plänen</a>"
            "<h1>Impressum</h1>"
            "<p class='sub'>Angaben gemäß § 5 TMG</p>"
            "<h2>Betreiber</h2>"
            "<address>Nathanael Golla<br>Binger Straße 5<br>55263 Wackernheim</address>"
            "<h2>Kontakt</h2>"
            "<p><a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>"
            "<h2>Verantwortlich für den Inhalt</h2>"
            "<p>Verantwortlich für den Inhalt nach § 18 Abs. 2 MStV:<br>"
            "Nathanael Golla, Anschrift wie oben.</p>"
            "<div class='footer'>"
            "<a href='/twitch/abbo'>Pläne</a>"
            " &nbsp;&middot;&nbsp; "
            "<a href='/twitch/datenschutz'>Datenschutz</a>"
            " &nbsp;&middot;&nbsp; "
            "<a href='/twitch/agb'>AGB</a>"
            "</div>"
            "</div></body></html>"
        )
        return web.Response(text=page, content_type="text/html", headers=LEGAL_PAGE_HEADERS)

    async def abbo_agb(self, request: web.Request) -> web.StreamResponse:  # noqa: ARG002
        """GET /twitch/agb — AGB für digitale Abo-Dienste. Kein Auth nötig."""
        page = (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<title>AGB · EarlySalty</title>"
            "<style>"
            "body{margin:0;background:#f8fafc;color:#0f172a;"
            "font-family:Segoe UI,Arial,sans-serif;line-height:1.7;}"
            ".wrap{max-width:700px;margin:0 auto;padding:40px 20px 60px;}"
            "h1{font-size:1.7rem;margin:0 0 6px;font-weight:800;}"
            ".back{font-size:13px;color:#64748b;margin-bottom:24px;display:block;"
            "text-decoration:none;}"
            ".back:hover{color:#2563eb;}"
            "h2{font-size:1.05rem;margin:28px 0 6px;color:#0f172a;font-weight:700;}"
            "p{font-size:15px;color:#334155;margin:0 0 10px;}"
            "a{color:#2563eb;text-decoration:none;}"
            "a:hover{text-decoration:underline;}"
            ".sub{color:#64748b;font-size:14px;margin:0 0 20px;}"
            ".footer{margin-top:40px;font-size:12px;color:#94a3b8;"
            "border-top:1px solid #e2e8f0;padding-top:14px;}"
            "</style></head><body><div class='wrap'>"
            "<a class='back' href='/twitch/abbo'>&larr; Zurück zu den Plänen</a>"
            "<h1>Allgemeine Geschäftsbedingungen</h1>"
            "<p class='sub'>Stand: Mai 2026</p>"

            "<h2>§ 1 Geltungsbereich</h2>"
            "<p>Diese Allgemeinen Geschäftsbedingungen (AGB) gelten für alle Verträge zwischen "
            "Nathanael Golla, Binger Straße 5, 55263 Wackernheim (nachfolgend "
            "<em>Anbieter</em>) und Nutzern des Dienstes EarlySalty / Deadlock Partner Network "
            "(nachfolgend <em>Kunde</em>). Abweichende Bedingungen des Kunden werden nicht "
            "anerkannt, es sei denn, der Anbieter stimmt ihrer Geltung ausdrücklich schriftlich zu.</p>"

            "<h2>§ 2 Vertragsgegenstand</h2>"
            "<p>Der Anbieter stellt digitale Dienste für Twitch-Streamer bereit. Das Angebot umfasst:</p>"
            "<p><strong>Raid Boost</strong> — Bevorzugte Platzierung des Kanals im Raid-Netzwerk des Anbieters.</p>"
            "<p><strong>Analyse Dashboard</strong> — Zugang zu einem Analytics-Dashboard mit Stream-Statistiken, "
            "Viewer-Verlauf und Wachstumsanalysen.</p>"
            "<p><strong>Bundle: Analyse + Raid Boost</strong> — Kombination beider Dienste zu einem "
            "vergünstigten Preis.</p>"

            "<h2>§ 3 Vertragsschluss</h2>"
            "<p>Das Angebot des Anbieters auf der Plattform stellt eine unverbindliche Aufforderung "
            "zur Abgabe eines Angebots dar. Durch das Absenden des Checkout-Formulars (via Stripe) "
            "gibt der Kunde ein verbindliches Angebot ab. Der Vertrag kommt mit der Bestätigung der "
            "Zahlung durch Stripe zustande.</p>"

            "<h2>§ 4 Preise und Zahlung</h2>"
            "<p>Alle angegebenen Preise verstehen sich als Nettopreise zzgl. der gesetzlichen "
            "Mehrwertsteuer (derzeit 19 % gem. § 12 UStG). Die Abrechnung erfolgt über den "
            "Zahlungsdienstleister Stripe. Der Rechnungsbetrag wird zum Beginn des gebuchten "
            "Abrechnungszeitraums fällig.</p>"
            "<p>Bei Buchung eines <strong>Jahresabonnements</strong> (12 Monate) wird der volle "
            "Jahresbetrag sofort bei Vertragsschluss berechnet. Als Dankeschön für die Jahresbindung "
            "gewährt der Anbieter zusätzlich 2 kostenfreie Bonusmonate, sodass der Zugang insgesamt "
            "14 Monate ab Zahlung besteht. Diese Gutschrift ist nicht bar auszahlbar und nicht "
            "übertragbar.</p>"

            "<h2>§ 5 Laufzeit und Kündigung</h2>"
            "<p>Abonnements werden für den gewählten Zeitraum (1 oder 12 Monate) abgeschlossen "
            "und verlängern sich automatisch um den gleichen Zeitraum, sofern nicht rechtzeitig "
            "gekündigt wird. Die Kündigung ist jederzeit zum Ende der laufenden Periode über die "
            "Abo-Verwaltung unter <a href='/twitch/abbo'>/twitch/abbo</a> möglich.</p>"

            "<h2 id='widerruf'>§ 6 Widerrufsrecht und sofortige Leistungserbringung</h2>"
            "<p>Bei den angebotenen Diensten handelt es sich um digitale Inhalte, die auf Abruf "
            "bereitgestellt werden (§ 312f Abs. 3 BGB). Der Anbieter beginnt mit der Erbringung "
            "der Leistung unmittelbar nach Vertragsschluss.</p>"
            "<p>Das Widerrufsrecht erlischt gemäß <strong>§ 356 Abs. 5 BGB</strong>, wenn der "
            "Verbraucher vor Beginn der Ausführung ausdrücklich zugestimmt hat, dass der Anbieter "
            "vor Ablauf der Widerrufsfrist mit der Ausführung des Vertrags beginnt, und seine "
            "Kenntnis davon bestätigt hat, dass er durch seine Zustimmung mit Beginn der Ausführung "
            "sein Widerrufsrecht verliert.</p>"
            "<p>Der Kunde bestätigt diese Einwilligung im Bestellprozess durch Aktivieren der "
            "entsprechenden Checkbox. Mit Abschluss der Bestellung gilt das Widerrufsrecht als "
            "erloschen. Eine Rückerstattung bereits erbrachter Leistungen ist daher ausgeschlossen, "
            "sofern nicht zwingende gesetzliche Vorschriften entgegenstehen.</p>"

            "<h2>§ 7 Verfügbarkeit und Haftung</h2>"
            "<p>Der Anbieter bemüht sich nach besten Kräften um eine hohe Verfügbarkeit der "
            "Dienste, übernimmt jedoch keine Garantie für einen unterbrechungsfreien Betrieb. "
            "Die Haftung des Anbieters ist auf Vorsatz und grobe Fahrlässigkeit beschränkt, "
            "soweit keine zwingenden gesetzlichen Regelungen entgegenstehen. Eine Haftung für "
            "entgangene Gewinne oder mittelbare Schäden ist ausgeschlossen.</p>"

            "<h2>§ 8 Datenschutz</h2>"
            "<p>Informationen zur Verarbeitung personenbezogener Daten finden sich in der "
            "<a href='/twitch/datenschutz'>Datenschutzerklärung</a>.</p>"

            "<h2>§ 9 Änderungen der AGB</h2>"
            "<p>Der Anbieter behält sich das Recht vor, diese AGB mit einer Frist von 4 Wochen "
            "zu ändern. Änderungen werden dem Kunden per E-Mail an die hinterlegte Adresse "
            "mitgeteilt. Widerspricht der Kunde nicht innerhalb von 4 Wochen nach Zugang der "
            "Mitteilung, gelten die geänderten AGB als angenommen.</p>"

            "<h2>§ 10 Schlussbestimmungen</h2>"
            "<p>Es gilt deutsches Recht unter Ausschluss des UN-Kaufrechts. Gerichtsstand für "
            "Kaufleute und juristische Personen des öffentlichen Rechts ist Wackernheim; "
            "zuständig ist das Amtsgericht Mainz. Sollten einzelne Bestimmungen dieser AGB "
            "unwirksam sein, bleibt die Wirksamkeit der übrigen Bestimmungen unberührt.</p>"

            "<div class='footer'>"
            "<a href='/twitch/abbo'>Pläne</a>"
            " &nbsp;&middot;&nbsp; "
            "<a href='/twitch/impressum'>Impressum</a>"
            " &nbsp;&middot;&nbsp; "
            "<a href='/twitch/datenschutz'>Datenschutz</a>"
            "</div>"
            "</div></body></html>"
        )
        return web.Response(text=page, content_type="text/html")

    async def abbo_datenschutz(self, request: web.Request) -> web.StreamResponse:
        """GET /twitch/datenschutz — DSGVO Art. 13/14. Must be accessible without login."""
        if self._legal_page_request_is_blocked(request):
            return _build_blocked_legal_page_response()
        if not self._legal_gate_is_enabled():
            return self._legal_gate_configuration_error_response()
        if not self._legal_gate_cookie_is_valid(request):
            raise self._legal_gate_redirect(request)
        page = (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<meta name='robots' content='noindex, nofollow'>"
            "<title>Datenschutz · EarlySalty</title>"
            "<style>"
            "body{margin:0;background:#f8fafc;color:#0f172a;"
            "font-family:Segoe UI,Arial,sans-serif;line-height:1.7;}"
            ".wrap{max-width:700px;margin:0 auto;padding:40px 20px 60px;}"
            "h1{font-size:1.7rem;margin:0 0 6px;font-weight:800;}"
            ".back{font-size:13px;color:#64748b;margin-bottom:24px;display:block;"
            "text-decoration:none;}"
            ".back:hover{color:#2563eb;}"
            "h2{font-size:1.05rem;margin:26px 0 6px;color:#0f172a;font-weight:700;}"
            "p{font-size:15px;color:#334155;margin:0 0 10px;}"
            "ul{font-size:15px;color:#334155;margin:0 0 10px;padding-left:22px;}"
            "li{margin-bottom:4px;}"
            "a{color:#2563eb;text-decoration:none;}"
            "a:hover{text-decoration:underline;}"
            ".sub{color:#64748b;font-size:14px;margin:0 0 20px;}"
            ".footer{margin-top:40px;font-size:12px;color:#94a3b8;"
            "border-top:1px solid #e2e8f0;padding-top:14px;}"
            "</style></head><body><div class='wrap'>"
            "<a class='back' href='/twitch/abbo'>&larr; Zurück zu den Plänen</a>"
            "<h1>Datenschutzerklärung</h1>"
            "<p class='sub'>Stand: Februar 2026</p>"
            "<h2>Verantwortlicher</h2>"
            "<p>Nathanael Golla<br>Binger Straße 5, 55263 Wackernheim<br>"
            "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>"
            "<h2>Erhobene Daten</h2>"
            "<p>Beim Login und bei der Nutzung des Dienstes werden folgende Daten verarbeitet:</p>"
            "<ul>"
            "<li>Twitch OAuth: Twitch-Name, Twitch-ID, E-Mail-Adresse</li>"
            "<li>Zahlungsdaten: werden ausschließlich über Stripe verarbeitet (s.&nbsp;u.)</li>"
            "</ul>"
            "<h2>Stripe als Zahlungsdienstleister</h2>"
            "<p>Zahlungen werden über Stripe Payments Europe Ltd. abgewickelt. "
            "Stripe verarbeitet Zahlungsdaten als eigenverantwortlicher Verantwortlicher "
            "gemäß seiner eigenen Datenschutzrichtlinie: "
            "<a href='https://stripe.com/de/privacy' target='_blank' "
            "rel='noopener noreferrer'>stripe.com/de/privacy</a>.</p>"
            "<h2>Cookies</h2>"
            "<p>Diese Website verwendet ausschließlich technisch notwendige Cookies (Session-Cookie "
            "für die Anmeldung via Twitch OAuth). Es werden keine Tracking-, Analyse- oder "
            "Marketing-Cookies eingesetzt. Eine Einwilligung ist gem. § 25 Abs. 2 TTDSG nicht "
            "erforderlich. Stripe setzt Cookies nur auf der eigenen Domain (stripe.com) "
            "während des Bezahlvorgangs.</p>"
            "<h2>Speicherdauer</h2>"
            "<p>Deine Daten werden gespeichert, solange dein Abonnement aktiv ist oder "
            "gesetzliche Aufbewahrungspflichten bestehen "
            "(z.&nbsp;B. steuerrechtlich 10 Jahre für Rechnungsdaten).</p>"
            "<h2>Deine Rechte (Art. 15–22 DSGVO)</h2>"
            "<ul>"
            "<li>Auskunft über gespeicherte Daten (Art. 15)</li>"
            "<li>Berichtigung unrichtiger Daten (Art. 16)</li>"
            "<li>Löschung deiner Daten (Art. 17)</li>"
            "<li>Einschränkung der Verarbeitung (Art. 18)</li>"
            "<li>Datenübertragbarkeit (Art. 20)</li>"
            "<li>Widerspruch gegen die Verarbeitung (Art. 21)</li>"
            "</ul>"
            "<p>Zur Wahrnehmung dieser Rechte wende dich an: "
            "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>"
            "<h2>Beschwerderecht</h2>"
            "<p>Du hast das Recht, dich bei der zuständigen Datenschutz-Aufsichtsbehörde "
            "zu beschweren. Zuständig ist der <em>Landesbeauftragte für den Datenschutz "
            "und die Informationsfreiheit Rheinland-Pfalz (LfDI)</em>, "
            "Hintere Bleiche 34, 55116 Mainz.</p>"
            "<div class='footer'>"
            "<a href='/twitch/abbo'>Pläne</a>"
            " &nbsp;&middot;&nbsp; "
            "<a href='/twitch/impressum'>Impressum</a>"
            " &nbsp;&middot;&nbsp; "
            "<a href='/twitch/agb'>AGB</a>"
            "</div>"
            "</div></body></html>"
        )
        return web.Response(text=page, content_type="text/html", headers=LEGAL_PAGE_HEADERS)
