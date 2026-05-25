"""Legal pages mixin: Impressum, Datenschutz and AGB."""

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
LEGAL_GATE_ALLOWED_PATHS = frozenset(("/twitch/impressum", "/twitch/datenschutz", "/twitch/agb"))
LEGAL_GATE_COOKIE_NAME = "twitch_legal_gate"
LEGAL_GATE_COOKIE_TTL_SECONDS = 600
LEGAL_GATE_TURNSTILE_ACTION = "legal_access"
LEGAL_PAGE_SLUGS = frozenset(("impressum", "datenschutz", "agb"))
LEGAL_PAGE_TITLES = {
    "impressum": "Impressum",
    "datenschutz": "Datenschutzerklärung",
    "agb": "Allgemeine Geschäftsbedingungen",
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
        "<p class='sub'>Angaben gemäß § 5 DDG</p>"
        "<h2>Betreiber</h2>"
        "<address>Nathanael Golla<br>Binger Straße 5<br>55263 Wackernheim</address>"
        "<h2>Kontakt</h2>"
        "<p><a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>"
        "<h2>Verantwortlich für den Inhalt</h2>"
        "<p>Verantwortlich für den Inhalt nach § 18 Abs. 2 MStV:<br>"
        "Nathanael Golla, Anschrift wie oben.</p>"
    ),
    "agb": (
        "<p class='sub'>Stand: Mai 2026</p>"
        "<h2>§ 1 Geltungsbereich</h2>"
        "<p>Diese Allgemeinen Geschäftsbedingungen (AGB) gelten für Verträge über die "
        "digitalen Dienste der Deutschen Deadlock Community zwischen Nathanael Golla, "
        "Binger Straße 5, 55263 Wackernheim (nachfolgend <em>Anbieter</em>) und den "
        "Nutzerinnen und Nutzern des Dienstes (nachfolgend <em>Kundschaft</em>). "
        "Abweichende Bedingungen werden nur Vertragsbestandteil, wenn der Anbieter ihnen "
        "ausdrücklich zustimmt.</p>"
        "<h2>§ 2 Vertragsgegenstand</h2>"
        "<p>Der Anbieter stellt digitale Dienste für Twitch-Streamer bereit. Das Angebot kann "
        "insbesondere folgende Bestandteile umfassen:</p>"
        "<ul>"
        "<li><strong>Raid Boost:</strong> bevorzugte Platzierung des Kanals im Raid-Netzwerk.</li>"
        "<li><strong>Analyse-Dashboard:</strong> Zugang zu Statistiken, Viewer-Verläufen und "
        "Wachstumsanalysen.</li>"
        "<li><strong>Bundle:</strong> Kombination aus Analyse-Dashboard und Raid Boost.</li>"
        "</ul>"
        "<p>Der konkrete Leistungsumfang ergibt sich aus der im Checkout ausgewählten Option.</p>"
        "<h2>§ 3 Vertragsschluss</h2>"
        "<p>Die Darstellung der Dienste ist eine unverbindliche Aufforderung zur Bestellung. "
        "Durch Absenden des Checkout-Formulars über Stripe gibt die Kundschaft ein verbindliches "
        "Angebot ab. Der Vertrag kommt zustande, sobald die Zahlung durch Stripe bestätigt wurde "
        "oder der Anbieter den Zugang freischaltet.</p>"
        "<h2>§ 4 Preise und Zahlung</h2>"
        "<p>Die im Checkout angegebenen Preise gelten zum Zeitpunkt der Bestellung. Soweit nicht "
        "anders angegeben, verstehen sich Preise zuzüglich der gesetzlichen Umsatzsteuer. Die "
        "Abrechnung erfolgt über den Zahlungsdienstleister Stripe. Der Rechnungsbetrag wird zu "
        "Beginn des gebuchten Abrechnungszeitraums fällig.</p>"
        "<p>Bei Buchung eines Jahresabonnements wird der Jahresbetrag sofort berechnet. Sofern "
        "im Angebot ausgewiesen, können zusätzliche Bonusmonate gewährt werden. Bonusmonate sind "
        "nicht bar auszahlbar und nicht übertragbar.</p>"
        "<h2>§ 5 Laufzeit und Kündigung</h2>"
        "<p>Abonnements laufen für den gewählten Zeitraum und verlängern sich automatisch um den "
        "gleichen Zeitraum, sofern sie nicht zum Ende der laufenden Periode gekündigt werden. "
        "Die Kündigung ist über die Abo-Verwaltung unter "
        "<a href='/twitch/dashboard'>/twitch/dashboard</a> oder per E-Mail an "
        "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a> möglich.</p>"
        "<h2 id='widerruf'>§ 6 Widerrufsrecht und sofortige Leistungserbringung</h2>"
        "<p>Bei den angebotenen Diensten handelt es sich um digitale Leistungen, die unmittelbar "
        "nach Vertragsschluss bereitgestellt werden können. Das Widerrufsrecht kann nach "
        "<strong>§ 356 Abs. 5 BGB</strong> erlöschen, wenn Verbraucherinnen und Verbraucher "
        "ausdrücklich zustimmen, dass der Anbieter vor Ablauf der Widerrufsfrist mit der "
        "Ausführung beginnt, und bestätigen, dass sie dadurch ihr Widerrufsrecht verlieren.</p>"
        "<p>Diese Zustimmung wird im Bestellprozess gesondert abgefragt, sofern sie für den "
        "jeweiligen Vertrag erforderlich ist. Zwingende gesetzliche Rechte bleiben unberührt.</p>"
        "<h2>§ 7 Verfügbarkeit und Haftung</h2>"
        "<p>Der Anbieter bemüht sich um einen stabilen Betrieb, kann aber keine ununterbrochene "
        "Verfügbarkeit garantieren. Wartung, Störungen bei Drittanbietern wie Twitch, Discord "
        "oder Stripe sowie technische Ausfälle können die Nutzung zeitweise einschränken.</p>"
        "<p>Die Haftung richtet sich nach den gesetzlichen Vorschriften. Für leicht fahrlässige "
        "Pflichtverletzungen haftet der Anbieter nur bei Verletzung wesentlicher Vertragspflichten "
        "und begrenzt auf den vertragstypischen, vorhersehbaren Schaden.</p>"
        "<h2>§ 8 Datenschutz</h2>"
        "<p>Informationen zur Verarbeitung personenbezogener Daten finden sich in der "
        "<a href='/twitch/datenschutz'>Datenschutzerklärung</a>.</p>"
        "<h2>§ 9 Änderungen der AGB</h2>"
        "<p>Der Anbieter kann diese AGB anpassen, wenn sachliche Gründe vorliegen, zum Beispiel "
        "gesetzliche Änderungen, technische Weiterentwicklungen oder Änderungen des "
        "Leistungsumfangs. Wesentliche Änderungen werden rechtzeitig mitgeteilt. "
        "Bestehende gesetzliche Rechte der Kundschaft bleiben unberührt.</p>"
        "<h2>§ 10 Schlussbestimmungen</h2>"
        "<p>Es gilt deutsches Recht unter Ausschluss des UN-Kaufrechts. Für Verbraucherinnen "
        "und Verbraucher gelten zusätzlich die zwingenden Verbraucherschutzvorschriften ihres "
        "gewöhnlichen Aufenthaltsortes. Sollten einzelne Bestimmungen dieser AGB unwirksam sein, "
        "bleibt die Wirksamkeit der übrigen Bestimmungen unberührt.</p>"
    ),
    "datenschutz": (
        "<p class='sub'>Stand: Mai 2026</p>"
        "<h2>Verantwortlicher</h2>"
        "<p>Nathanael Golla<br>Binger Straße 5, 55263 Wackernheim<br>"
        "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>"
        "<h2>Zwecke und Rechtsgrundlagen</h2>"
        "<p>Wir verarbeiten personenbezogene Daten, um Login, Abo-Verwaltung, Zahlungsabwicklung, "
        "Dashboard-Funktionen, Support und den sicheren Betrieb des Dienstes bereitzustellen. "
        "Rechtsgrundlagen sind insbesondere Art. 6 Abs. 1 lit. b DSGVO (Vertragserfüllung), "
        "Art. 6 Abs. 1 lit. c DSGVO (gesetzliche Pflichten) und Art. 6 Abs. 1 lit. f DSGVO "
        "(berechtigte Interessen an Sicherheit, Fehleranalyse und Missbrauchsschutz).</p>"
        "<h2>Verarbeitete Daten</h2>"
        "<p>Je nach Nutzung können insbesondere folgende Daten verarbeitet werden:</p>"
        "<ul>"
        "<li>Twitch-Daten: Twitch-Name, Twitch-ID, OAuth-Status und von Twitch "
        "bereitgestellte Profildaten.</li>"
        "<li>Discord-Daten: Discord-ID, Anzeigename und Rollenstatus, soweit für Community- "
        "oder Admin-Funktionen erforderlich.</li>"
        "<li>Abonnement- und Rechnungsdaten: Plan, Status, Buchungszeitpunkt, "
        "Rechnungsreferenzen und steuerlich relevante Angaben.</li>"
        "<li>Nutzungs- und Analysedaten: Stream-Statistiken, Viewer-Verläufe, Chat- und "
        "Dashboard-Metriken, soweit sie für gebuchte Funktionen benötigt werden.</li>"
        "<li>Technische Daten: IP-Adresse, User-Agent, Zeitstempel, Logdaten, "
        "Sicherheitsereignisse und Session-Cookies.</li>"
        "</ul>"
        "<h2>Empfänger und Dienstleister</h2>"
        "<p>Zahlungen werden über Stripe Payments Europe Ltd. abgewickelt. Stripe verarbeitet "
        "Zahlungsdaten nach eigener Datenschutzrichtlinie: "
        "<a href='https://stripe.com/de/privacy' target='_blank' "
        "rel='noopener noreferrer'>stripe.com/de/privacy</a>.</p>"
        "<p>Für Login- und Plattformfunktionen werden Daten mit Twitch, Discord und den jeweils "
        "angebundenen Plattformen ausgetauscht, soweit dies technisch oder vertraglich "
        "notwendig ist. Für den Schutz der Legal-Seiten kann Cloudflare Turnstile eingesetzt "
        "werden, um automatisierte Zugriffe zu erkennen.</p>"
        "<h2>Cookies</h2>"
        "<p>Diese Website verwendet technisch notwendige Cookies, insbesondere für Login-Sessions, "
        "Abo-Verwaltung und das Legal-Access-Gate. Es werden keine Marketing-Cookies eingesetzt. "
        "Eine Einwilligung ist für unbedingt erforderliche Cookies gemäß § 25 Abs. 2 Nr. 2 TDDDG "
        "nicht erforderlich. Stripe kann während des Bezahlvorgangs Cookies auf eigenen "
        "Domains setzen.</p>"
        "<h2>Speicherdauer</h2>"
        "<p>Daten werden nur so lange gespeichert, wie sie für die genannten Zwecke "
        "erforderlich sind. Abonnement- und Nutzungsdaten werden grundsätzlich für die Dauer "
        "des Vertrags gespeichert. "
        "Rechnungs- und Buchungsdaten können aufgrund gesetzlicher Aufbewahrungspflichten bis zu "
        "10 Jahre gespeichert werden. Sicherheits- und Serverlogs werden regelmäßig gelöscht, "
        "sofern keine längere Aufbewahrung zur Aufklärung von Missbrauch oder Störungen "
        "erforderlich ist.</p>"
        "<h2>Deine Rechte (Art. 15-22 DSGVO)</h2>"
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
        json.dumps(raw_payload, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    return document


class _DashboardLegalMixin:
    """Handlers for /twitch/impressum, /twitch/datenschutz and /twitch/agb."""

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
            else location
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
            "Disallow: /twitch/agb\n"
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
                else next_path
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
            else next_path
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
            "<a class='back' href='/twitch/pricing'>&larr; Zurück zu den Plänen</a>"
            f"<h1>{html.escape(title)}</h1>"
            f"{body}"
            f"<div class='footer'>{footer_html}</div>"
            "</div></body></html>"
        )

    async def abbo_impressum(self, request: web.Request) -> web.StreamResponse:
        """GET /twitch/impressum — §5 DDG behind the legal human gate."""
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
                ("/twitch/abbo", "Pläne"),
                ("/twitch/datenschutz", "Datenschutz"),
                ("/twitch/agb", "AGB"),
            ),
        )
        return web.Response(text=page, content_type="text/html", headers=LEGAL_PAGE_HEADERS)

    async def abbo_agb(self, request: web.Request) -> web.StreamResponse:
        """GET /twitch/agb — AGB for digital subscription services behind the legal human gate."""
        if self._legal_page_request_is_blocked(request):
            return _build_blocked_legal_page_response()
        if not self._legal_gate_is_enabled():
            return self._legal_gate_configuration_error_response()
        if not self._legal_gate_cookie_is_valid(request):
            raise self._legal_gate_redirect(request)
        document = self._load_legal_page_document("agb")
        page = self._render_legal_page(
            title=str(document.get("title") or LEGAL_PAGE_TITLES["agb"]),
            body=str(document.get("body") or ""),
            footer_links=(
                ("/twitch/pricing", "Pläne"),
                ("/twitch/impressum", "Impressum"),
                ("/twitch/datenschutz", "Datenschutz"),
            ),
        )
        return web.Response(text=page, content_type="text/html", headers=LEGAL_PAGE_HEADERS)

    async def abbo_datenschutz(self, request: web.Request) -> web.StreamResponse:
        """GET /twitch/datenschutz — DSGVO Art. 13/14 behind the legal human gate."""
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
                ("/twitch/abbo", "Pläne"),
                ("/twitch/impressum", "Impressum"),
                ("/twitch/agb", "AGB"),
            ),
        )
        return web.Response(text=page, content_type="text/html", headers=LEGAL_PAGE_HEADERS)
