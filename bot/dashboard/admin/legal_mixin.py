"""Legal pages mixin: Impressum + Datenschutz (§5 TMG / DSGVO)."""

from __future__ import annotations

from aiohttp import web

LEGAL_PAGE_HEADERS = {
    "X-Robots-Tag": "noindex, nofollow, noarchive, nosnippet, noimageindex"
}
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

    async def robots_txt(self, request: web.Request) -> web.StreamResponse:  # noqa: ARG002
        robots = (
            "User-agent: *\n"
            "Disallow: /twitch/impressum\n"
            "Disallow: /twitch/datenschutz\n"
        )
        return web.Response(text=robots, content_type="text/plain")

    async def abbo_impressum(self, request: web.Request) -> web.StreamResponse:
        """GET /twitch/impressum — §5 TMG. Must be accessible without login."""
        if self._legal_page_request_is_blocked(request):
            return _build_blocked_legal_page_response()
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
            "<p class='sub'>Stand: März 2026</p>"

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

            "<h2>§ 5 Laufzeit und Kündigung</h2>"
            "<p>Abonnements werden für den gewählten Zeitraum (1, 6 oder 12 Monate) abgeschlossen "
            "und verlängern sich automatisch um den gleichen Zeitraum, sofern nicht rechtzeitig "
            "gekündigt wird. Die Kündigung ist jederzeit zum Ende der laufenden Periode über die "
            "Abo-Verwaltung unter <a href='/twitch/abbo'>/twitch/abbo</a> möglich.</p>"

            "<h2 id='widerruf'>§ 6 Widerrufsrecht</h2>"
            "<p>Verbrauchern steht grundsätzlich ein Widerrufsrecht zu. Dieses erlischt jedoch "
            "gemäß § 356 Abs. 5 BGB bei Verträgen über die Lieferung von digitalen Inhalten, "
            "wenn der Anbieter mit der Ausführung des Vertrags begonnen hat und der Verbraucher "
            "ausdrücklich zugestimmt hat, dass der Anbieter vor Ablauf der Widerrufsfrist mit der "
            "Ausführung beginnt, und seine Kenntnis davon bestätigt hat, dass er durch seine "
            "Zustimmung mit Beginn der Ausführung sein Widerrufsrecht verliert.</p>"
            "<p>Der Kunde stimmt im Bestellprozess ausdrücklich zu, dass die Leistungserbringung "
            "sofort nach Vertragsschluss beginnt und er sein Widerrufsrecht damit verliert.</p>"

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
