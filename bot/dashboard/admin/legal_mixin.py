"""Legal pages mixin: Impressum + Datenschutz (§5 TMG / DSGVO)."""

from __future__ import annotations

import hashlib
import hmac
import html
import json
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
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
LEGAL_PAGE_SLUGS = frozenset(("impressum", "datenschutz", "agb"))
LEGAL_PAGE_TITLES = {
    "impressum": "Impressum",
    "datenschutz": "Datenschutzerklaerung",
    "agb": "Allgemeine Geschaeftsbedingungen",
}
_LEGAL_STORAGE_PATH = (
    Path(__file__).resolve().parents[3] / "data" / "admin_dashboard" / "legal_pages.json"
)
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


_DEFAULT_LEGAL_PAGE_BODIES: dict[str, str] = {
    "impressum": (
        "<p class='sub'>Angaben gemaess § 5 TMG</p>"
        "<h2>Betreiber</h2>"
        "<address>Nathanael Golla<br>Binger Strasse 5<br>55263 Wackernheim</address>"
        "<h2>Kontakt</h2>"
        "<p><a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>"
        "<h2>Verantwortlich fuer den Inhalt</h2>"
        "<p>Verantwortlich fuer den Inhalt nach § 18 Abs. 2 MStV:<br>"
        "Nathanael Golla, Anschrift wie oben.</p>"
    ),
    "agb": (
        "<p class='sub'>Stand: Mai 2026</p>"
        "<h2>§ 1 Geltungsbereich</h2>"
        "<p>Diese Allgemeinen Geschaeftsbedingungen (AGB) gelten fuer alle Vertraege zwischen "
        "Nathanael Golla, Binger Strasse 5, 55263 Wackernheim (nachfolgend "
        "<em>Anbieter</em>) und Nutzern des Dienstes Deutsche Deadlock Community "
        "(nachfolgend <em>Kunde</em>). Abweichende Bedingungen des Kunden werden nicht "
        "anerkannt, es sei denn, der Anbieter stimmt ihrer Geltung ausdruecklich schriftlich zu.</p>"
        "<h2>§ 2 Vertragsgegenstand</h2>"
        "<p>Der Anbieter stellt digitale Dienste fuer Twitch-Streamer bereit. Das Angebot umfasst:</p>"
        "<p><strong>Raid Boost</strong> - Bevorzugte Platzierung des Kanals im Raid-Netzwerk des Anbieters.</p>"
        "<p><strong>Analyse Dashboard</strong> - Zugang zu einem Analytics-Dashboard mit Stream-Statistiken, "
        "Viewer-Verlauf und Wachstumsanalysen.</p>"
        "<p><strong>Bundle: Analyse + Raid Boost</strong> - Kombination beider Dienste zu einem "
        "verguenstigten Preis.</p>"
        "<h2>§ 3 Vertragsschluss</h2>"
        "<p>Das Angebot des Anbieters auf der Plattform stellt eine unverbindliche Aufforderung "
        "zur Abgabe eines Angebots dar. Durch das Absenden des Checkout-Formulars (via Stripe) "
        "gibt der Kunde ein verbindliches Angebot ab. Der Vertrag kommt mit der Bestaetigung der "
        "Zahlung durch Stripe zustande.</p>"
        "<h2>§ 4 Preise und Zahlung</h2>"
        "<p>Alle angegebenen Preise verstehen sich als Nettopreise zzgl. der gesetzlichen "
        "Mehrwertsteuer (derzeit 19 % gem. § 12 UStG). Die Abrechnung erfolgt ueber den "
        "Zahlungsdienstleister Stripe. Der Rechnungsbetrag wird zum Beginn des gebuchten "
        "Abrechnungszeitraums faellig.</p>"
        "<p>Bei Buchung eines <strong>Jahresabonnements</strong> (12 Monate) wird der volle "
        "Jahresbetrag sofort bei Vertragsschluss berechnet. Als Dankeschoen fuer die Jahresbindung "
        "gewaehrt der Anbieter zusaetzlich 2 kostenfreie Bonusmonate, sodass der Zugang insgesamt "
        "14 Monate ab Zahlung besteht. Diese Gutschrift ist nicht bar auszahlbar und nicht "
        "uebertragbar.</p>"
        "<h2>§ 5 Laufzeit und Kuendigung</h2>"
        "<p>Abonnements werden fuer den gewaehlten Zeitraum (1 oder 12 Monate) abgeschlossen "
        "und verlaengern sich automatisch um den gleichen Zeitraum, sofern nicht rechtzeitig "
        "gekuendigt wird. Die Kuendigung ist jederzeit zum Ende der laufenden Periode ueber die "
        "Abo-Verwaltung unter <a href='/twitch/dashboard'>/twitch/dashboard</a> moeglich.</p>"
        "<h2 id='widerruf'>§ 6 Widerrufsrecht und sofortige Leistungserbringung</h2>"
        "<p>Bei den angebotenen Diensten handelt es sich um digitale Inhalte, die auf Abruf "
        "bereitgestellt werden (§ 312f Abs. 3 BGB). Der Anbieter beginnt mit der Erbringung "
        "der Leistung unmittelbar nach Vertragsschluss.</p>"
        "<p>Das Widerrufsrecht erlischt gemaess <strong>§ 356 Abs. 5 BGB</strong>, wenn der "
        "Verbraucher vor Beginn der Ausfuehrung ausdruecklich zugestimmt hat, dass der Anbieter "
        "vor Ablauf der Widerrufsfrist mit der Ausfuehrung des Vertrags beginnt, und seine "
        "Kenntnis davon bestaetigt hat, dass er durch seine Zustimmung mit Beginn der Ausfuehrung "
        "sein Widerrufsrecht verliert.</p>"
        "<p>Der Kunde bestaetigt diese Einwilligung im Bestellprozess durch Aktivieren der "
        "entsprechenden Checkbox. Mit Abschluss der Bestellung gilt das Widerrufsrecht als "
        "erloschen. Eine Rueckerstattung bereits erbrachter Leistungen ist daher ausgeschlossen, "
        "sofern nicht zwingende gesetzliche Vorschriften entgegenstehen.</p>"
        "<h2>§ 7 Verfuegbarkeit und Haftung</h2>"
        "<p>Der Anbieter bemueht sich nach besten Kraeften um eine hohe Verfuegbarkeit der "
        "Dienste, uebernimmt jedoch keine Garantie fuer einen unterbrechungsfreien Betrieb. "
        "Die Haftung des Anbieters ist auf Vorsatz und grobe Fahrlaessigkeit beschraenkt, "
        "soweit keine zwingenden gesetzlichen Regelungen entgegenstehen. Eine Haftung fuer "
        "entgangene Gewinne oder mittelbare Schaeden ist ausgeschlossen.</p>"
        "<h2>§ 8 Datenschutz</h2>"
        "<p>Informationen zur Verarbeitung personenbezogener Daten finden sich in der "
        "<a href='/twitch/datenschutz'>Datenschutzerklaerung</a>.</p>"
        "<h2>§ 9 Aenderungen der AGB</h2>"
        "<p>Der Anbieter behaelt sich das Recht vor, diese AGB mit einer Frist von 4 Wochen "
        "zu aendern. Aenderungen werden dem Kunden per E-Mail an die hinterlegte Adresse "
        "mitgeteilt. Widerspricht der Kunde nicht innerhalb von 4 Wochen nach Zugang der "
        "Mitteilung, gelten die geaenderten AGB als angenommen.</p>"
        "<h2>§ 10 Schlussbestimmungen</h2>"
        "<p>Es gilt deutsches Recht unter Ausschluss des UN-Kaufrechts. Gerichtsstand fuer "
        "Kaufleute und juristische Personen des oeffentlichen Rechts ist Wackernheim; "
        "zustaendig ist das Amtsgericht Mainz. Sollten einzelne Bestimmungen dieser AGB "
        "unwirksam sein, bleibt die Wirksamkeit der uebrigen Bestimmungen unberuehrt.</p>"
    ),
    "datenschutz": (
        "<p class='sub'>Stand: Februar 2026</p>"
        "<h2>Verantwortlicher</h2>"
        "<p>Nathanael Golla<br>Binger Strasse 5, 55263 Wackernheim<br>"
        "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>"
        "<h2>Erhobene Daten</h2>"
        "<p>Beim Login und bei der Nutzung des Dienstes werden folgende Daten verarbeitet:</p>"
        "<ul>"
        "<li>Twitch OAuth: Twitch-Name, Twitch-ID, E-Mail-Adresse</li>"
        "<li>Zahlungsdaten: werden ausschliesslich ueber Stripe verarbeitet (s.&nbsp;u.)</li>"
        "</ul>"
        "<h2>Stripe als Zahlungsdienstleister</h2>"
        "<p>Zahlungen werden ueber Stripe Payments Europe Ltd. abgewickelt. "
        "Stripe verarbeitet Zahlungsdaten als eigenverantwortlicher Verantwortlicher "
        "gemaess seiner eigenen Datenschutzrichtlinie: "
        "<a href='https://stripe.com/de/privacy' target='_blank' "
        "rel='noopener noreferrer'>stripe.com/de/privacy</a>.</p>"
        "<h2>Cookies</h2>"
        "<p>Diese Website verwendet ausschliesslich technisch notwendige Cookies (Session-Cookie "
        "fuer die Anmeldung via Twitch OAuth). Es werden keine Tracking-, Analyse- oder "
        "Marketing-Cookies eingesetzt. Eine Einwilligung ist gem. § 25 Abs. 2 TTDSG nicht "
        "erforderlich. Stripe setzt Cookies nur auf der eigenen Domain (stripe.com) "
        "waehrend des Bezahlvorgangs.</p>"
        "<h2>Speicherdauer</h2>"
        "<p>Deine Daten werden gespeichert, solange dein Abonnement aktiv ist oder "
        "gesetzliche Aufbewahrungspflichten bestehen "
        "(z.&nbsp;B. steuerrechtlich 10 Jahre fuer Rechnungsdaten).</p>"
        "<h2>Deine Rechte (Art. 15-22 DSGVO)</h2>"
        "<ul>"
        "<li>Auskunft ueber gespeicherte Daten (Art. 15)</li>"
        "<li>Berichtigung unrichtiger Daten (Art. 16)</li>"
        "<li>Loeschung deiner Daten (Art. 17)</li>"
        "<li>Einschraenkung der Verarbeitung (Art. 18)</li>"
        "<li>Datenuebertragbarkeit (Art. 20)</li>"
        "<li>Widerspruch gegen die Verarbeitung (Art. 21)</li>"
        "</ul>"
        "<p>Zur Wahrnehmung dieser Rechte wende dich an: "
        "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>"
        "<h2>Beschwerderecht</h2>"
        "<p>Du hast das Recht, dich bei der zustaendigen Datenschutz-Aufsichtsbehoerde "
        "zu beschweren. Zustaendig ist der <em>Landesbeauftragte fuer den Datenschutz "
        "und die Informationsfreiheit Rheinland-Pfalz (LfDI)</em>, "
        "Hintere Bleiche 34, 55116 Mainz.</p>"
    ),
}


def normalize_legal_page_slug(raw_value: str | None) -> str | None:
    normalized = str(raw_value or "").strip().lower()
    if normalized not in LEGAL_PAGE_SLUGS:
        return None
    return normalized


def _default_legal_page_document(slug: str) -> dict[str, str | None]:
    normalized_slug = normalize_legal_page_slug(slug)
    if normalized_slug is None:
        raise ValueError("invalid_legal_slug")
    return {
        "slug": normalized_slug,
        "title": LEGAL_PAGE_TITLES[normalized_slug],
        "body": _DEFAULT_LEGAL_PAGE_BODIES[normalized_slug],
        "lastUpdatedAt": None,
        "lastUpdatedBy": None,
    }


def load_legal_page_document(slug: str) -> dict[str, str | None]:
    document = _default_legal_page_document(slug)
    try:
        raw_payload = json.loads(_LEGAL_STORAGE_PATH.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return document
    except Exception:
        return document

    if not isinstance(raw_payload, dict):
        return document

    entry = raw_payload.get(document["slug"])
    if not isinstance(entry, dict):
        return document

    title = entry.get("title")
    body = entry.get("body")
    updated_at = entry.get("lastUpdatedAt")
    updated_by = entry.get("lastUpdatedBy")
    if isinstance(title, str) and title.strip():
        document["title"] = title.strip()
    if isinstance(body, str) and body.strip():
        document["body"] = body
    document["lastUpdatedAt"] = str(updated_at).strip() or None if updated_at is not None else None
    document["lastUpdatedBy"] = str(updated_by).strip() or None if updated_by is not None else None
    return document


def save_legal_page_document(
    slug: str,
    *,
    title: str,
    body: str,
    updated_by: str | None = None,
) -> dict[str, str | None]:
    document = _default_legal_page_document(slug)
    document["title"] = str(title or "").strip() or str(document["title"])
    document["body"] = str(body or "")
    document["lastUpdatedAt"] = datetime.now(UTC).isoformat()
    document["lastUpdatedBy"] = str(updated_by or "").strip() or None

    try:
        raw_payload = json.loads(_LEGAL_STORAGE_PATH.read_text(encoding="utf-8"))
    except FileNotFoundError:
        raw_payload = {}
    except Exception:
        raw_payload = {}

    if not isinstance(raw_payload, dict):
        raw_payload = {}
    raw_payload[str(document["slug"])] = document

    _LEGAL_STORAGE_PATH.parent.mkdir(parents=True, exist_ok=True)
    _LEGAL_STORAGE_PATH.write_text(
        json.dumps(raw_payload, ensure_ascii=True, indent=2),
        encoding="utf-8",
    )
    return document


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
            log.warning(
                "legal_verify: token or secret missing (token_empty=%s, secret_empty=%s)",
                not normalized_token,
                not secret_key,
            )
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
            log.warning("legal_verify: siteverify request failed", exc_info=True)
            return False

        if not bool(result.get("success")):
            log.warning(
                "legal_verify: siteverify success=false, error-codes=%s",
                result.get("error-codes"),
            )
            return False
        action = str(result.get("action") or "").strip()
        if action != LEGAL_GATE_TURNSTILE_ACTION:
            log.warning(
                "legal_verify: action mismatch (got=%r, expected=%r)",
                action,
                LEGAL_GATE_TURNSTILE_ACTION,
            )
            return False
        hostname = str(result.get("hostname") or "").strip().lower()
        expected_host = self._legal_request_host(request)
        if not hostname:
            log.warning("legal_verify: hostname missing in siteverify response")
            return False
        if hostname != expected_host:
            log.warning(
                "legal_verify: hostname mismatch (cf=%r, request=%r)",
                hostname,
                expected_host,
            )
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
            "<title>Einen Moment bitte …</title>"
            "<script src='https://challenges.cloudflare.com/turnstile/v0/api.js' async defer></script>"
            "<style>"
            "*{box-sizing:border-box;margin:0;padding:0}"
            "body{display:flex;align-items:center;justify-content:center;"
            "min-height:100vh;background:#f8fafc;font-family:Segoe UI,Arial,sans-serif;}"
            ".loader{display:flex;flex-direction:column;align-items:center;gap:18px;}"
            ".spin{width:36px;height:36px;border:3px solid #e2e8f0;"
            "border-top-color:#2563eb;border-radius:50%;animation:s .8s linear infinite;}"
            "@keyframes s{to{transform:rotate(360deg)}}"
            "p{font-size:14px;color:#64748b;letter-spacing:.01em;}"
            ".hint{font-size:12px;color:#94a3b8;}"
            "</style></head><body>"
            "<div class='loader'>"
            "<div class='spin'></div>"
            "<p>Einen Moment bitte …</p>"
            "<span class='hint'>Der Server ist gerade etwas langsam.</span>"
            f"<form id='lgf' method='post' action='/twitch/legal/verify' style='display:none'>"
            f"<input type='hidden' name='next' value='{escaped_next}'>"
            f"<div class='cf-turnstile' data-sitekey='{escaped_site_key}'"
            f" data-action='{LEGAL_GATE_TURNSTILE_ACTION}'"
            f" data-appearance='interaction-only'"
            f" data-callback='_tsOk'></div>"
            "</form>"
            "</div>"
            "<script>function _tsOk(){document.getElementById('lgf').submit();}</script>"
            "</body></html>"
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

    @staticmethod
    def _load_legal_page_document(slug: str) -> dict[str, str | None]:
        return load_legal_page_document(slug)

    @staticmethod
    def _save_legal_page_document(
        slug: str,
        *,
        title: str,
        body: str,
        updated_by: str | None = None,
    ) -> dict[str, str | None]:
        return save_legal_page_document(
            slug,
            title=title,
            body=body,
            updated_by=updated_by,
        )

    @staticmethod
    def _render_legal_page(
        *,
        title: str,
        body: str,
        footer_links: tuple[tuple[str, str], ...],
    ) -> str:
        footer_html = " &nbsp;&middot;&nbsp; ".join(
            f"<a href='{html.escape(href, quote=True)}'>{html.escape(label)}</a>"
            for href, label in footer_links
        )
        return (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<meta name='robots' content='noindex, nofollow'>"
            f"<title>{html.escape(title)} · EarlySalty</title>"
            "<style>"
            "body{margin:0;background:#f8fafc;color:#0f172a;"
            "font-family:Segoe UI,Arial,sans-serif;line-height:1.7;}"
            ".wrap{max-width:700px;margin:0 auto;padding:40px 20px 60px;}"
            "h1{font-size:1.7rem;margin:0 0 6px;font-weight:800;}"
            ".back{font-size:13px;color:#64748b;margin-bottom:24px;display:block;"
            "text-decoration:none;}"
            ".back:hover{color:#2563eb;}"
            "h2{font-size:1.05rem;margin:26px 0 6px;color:#0f172a;font-weight:700;}"
            "p,address{font-size:15px;color:#334155;font-style:normal;margin:0 0 10px;}"
            "ul{font-size:15px;color:#334155;margin:0 0 10px;padding-left:22px;}"
            "li{margin-bottom:4px;}"
            "a{color:#2563eb;text-decoration:none;}"
            "a:hover{text-decoration:underline;}"
            ".sub{color:#64748b;font-size:14px;margin:0 0 20px;}"
            ".footer{margin-top:40px;font-size:12px;color:#94a3b8;"
            "border-top:1px solid #e2e8f0;padding-top:14px;}"
            "</style></head><body><div class='wrap'>"
            "<a class='back' href='/twitch/pricing'>&larr; Zurueck zu den Plaenen</a>"
            f"<h1>{html.escape(title)}</h1>"
            f"{body}"
            f"<div class='footer'>{footer_html}</div>"
            "</div></body></html>"
        )

    async def abbo_impressum(self, request: web.Request) -> web.StreamResponse:
        """GET /twitch/impressum — §5 TMG. Must be accessible without login."""
        if self._legal_page_request_is_blocked(request):
            return _build_blocked_legal_page_response()
        if not self._legal_gate_is_enabled():
            return self._legal_gate_configuration_error_response()
        if not self._legal_gate_cookie_is_valid(request):
            raise self._legal_gate_redirect(request)
        document = self._load_legal_page_document("impressum")
        page = self._render_legal_page(
            title=str(document.get("title") or LEGAL_PAGE_TITLES["impressum"]),
            body=str(document.get("body") or ""),
            footer_links=(
                ("/twitch/abbo", "Plaene"),
                ("/twitch/datenschutz", "Datenschutz"),
                ("/twitch/agb", "AGB"),
            ),
        )
        return web.Response(text=page, content_type="text/html", headers=LEGAL_PAGE_HEADERS)

    async def abbo_agb(self, request: web.Request) -> web.StreamResponse:  # noqa: ARG002
        """GET /twitch/agb — AGB für digitale Abo-Dienste. Kein Auth nötig."""
        document = self._load_legal_page_document("agb")
        page = self._render_legal_page(
            title=str(document.get("title") or LEGAL_PAGE_TITLES["agb"]),
            body=str(document.get("body") or ""),
            footer_links=(
                ("/twitch/pricing", "Plaene"),
                ("/twitch/impressum", "Impressum"),
                ("/twitch/datenschutz", "Datenschutz"),
            ),
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
        document = self._load_legal_page_document("datenschutz")
        page = self._render_legal_page(
            title=str(document.get("title") or LEGAL_PAGE_TITLES["datenschutz"]),
            body=str(document.get("body") or ""),
            footer_links=(
                ("/twitch/abbo", "Plaene"),
                ("/twitch/impressum", "Impressum"),
                ("/twitch/agb", "AGB"),
            ),
        )
        return web.Response(text=page, content_type="text/html", headers=LEGAL_PAGE_HEADERS)
