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
LEGAL_PAGE_SLUGS = frozenset(("impressum", "datenschutz", "agb", "sicherheit"))
LEGAL_PAGE_TITLES = {
    "impressum": "Impressum",
    "datenschutz": "Datenschutzerklärung",
    "agb": "Allgemeine Geschäftsbedingungen",
    "sicherheit": "Sicherheitskonzept",
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
        "<p class='sub'>Stand: Juni 2026</p>"
        "<h2>§ 1 Geltungsbereich</h2>"
        "<p>Diese Allgemeinen Geschäftsbedingungen (AGB) gelten ausschließlich für den "
        "<strong>Twitch-Bot der Deutschen Deadlock Community</strong> samt zugehöriger "
        "Web-Dienste (Streamer-Dashboard, Statistik-Seiten, Abo-Verwaltung), betrieben "
        "von Nathanael Golla, Binger Straße 5, 55263 Wackernheim (nachfolgend "
        "<em>Anbieter</em>). Andere Angebote der Deutschen Deadlock Community — etwa "
        "die Discord-Bots, der Steam-Bot oder der Community-Discord-Server — sind nicht "
        "Gegenstand dieser AGB. Sie gelten sowohl für die unentgeltliche Nutzung des "
        "Twitch-Bots als auch für kostenpflichtige Zusatzleistungen. "
        "Nutzerinnen und Nutzer sind insbesondere "
        "Streamerinnen und Streamer, die am Partnerprogramm teilnehmen (nachfolgend "
        "<em>Partner</em>), sowie bei kostenpflichtigen Leistungen die <em>Kundschaft</em>. "
        "Abweichende Bedingungen werden nur Vertragsbestandteil, wenn der Anbieter ihnen "
        "ausdrücklich zustimmt.</p>"
        "<h2>§ 2 Leistungen des Anbieters</h2>"
        "<p>Der Anbieter betreibt einen Chat-Bot und zugehörige Dienste für Twitch-Streamer "
        "der Deadlock-Community. Die kostenlose Basisnutzung kann insbesondere umfassen:</p>"
        "<ul>"
        "<li><strong>Chat-Bot:</strong> Chat-Befehle, Community-Hinweise und automatische "
        "Nachrichten im Kanal des Partners.</li>"
        "<li><strong>Raid-Netzwerk:</strong> automatische Weiterleitung der Zuschauer (Raid) "
        "an einen anderen Partner zum Stream-Ende. Die gegenseitigen Raids sind fester "
        "Bestandteil des Partnernetzwerks.</li>"
        "<li><strong>Automatische Moderation:</strong> Erkennung und Moderation von Spam-, "
        "Scam- und Bot-Aktivität (siehe § 4).</li>"
        "<li><strong>Statistiken und Dashboard:</strong> Stream- und Community-Auswertungen "
        "für Partner.</li>"
        "<li><strong>Discord-Integration:</strong> z. B. Live-Ankündigungen im "
        "Community-Discord.</li>"
        "</ul>"
        "<p>Kostenpflichtige Zusatzleistungen können insbesondere umfassen:</p>"
        "<ul>"
        "<li><strong>Raid Boost:</strong> bevorzugte Platzierung des Kanals im Raid-Netzwerk.</li>"
        "<li><strong>Analyse-Dashboard:</strong> Zugang zu erweiterten Statistiken, "
        "Viewer-Verläufen und Wachstumsanalysen.</li>"
        "<li><strong>Bundle:</strong> Kombination aus Analyse-Dashboard und Raid Boost.</li>"
        "</ul>"
        "<p>Der konkrete Leistungsumfang kostenpflichtiger Leistungen ergibt sich aus der im "
        "Checkout ausgewählten Option. Auf unentgeltliche Leistungen besteht kein Anspruch; "
        "der Anbieter kann sie weiterentwickeln, einschränken oder einstellen.</p>"
        "<h2>§ 3 Teilnahme am Partnerprogramm</h2>"
        "<p>Die Nutzung setzt ein Twitch-Konto voraus. Die Nutzungsbedingungen von Twitch "
        "bleiben unberührt und sind einzuhalten. Die Aufnahme in das Partnerprogramm erfolgt "
        "nach Freischaltung durch den Anbieter; ein Anspruch auf Aufnahme besteht nicht.</p>"
        "<p>Funktionen, die der Bot im Namen des Partners ausführt (insbesondere Raids), "
        "erfordern eine ausdrückliche Autorisierung über Twitch (OAuth). Der Umfang der "
        "Berechtigungen wird im Twitch-Autorisierungsdialog angezeigt. Die Autorisierung "
        "kann jederzeit in den Twitch-Kontoeinstellungen unter <em>Verbindungen</em> "
        "widerrufen werden; der Bot erkennt den Widerruf und deaktiviert die betroffenen "
        "Funktionen automatisch.</p>"
        "<h2>§ 4 Automatische Moderation und Bannliste</h2>"
        "<p>Der Bot führt in Partner-Kanälen automatische Moderation durch. Dazu gehören "
        "das Löschen von Nachrichten, Timeouts und Banns bei erkannter Spam-, Scam- oder "
        "Bot-Aktivität. Zum Schutz aller Partner führt der Anbieter eine kanalübergreifende "
        "Bannliste; Einträge können in allen Partner-Kanälen vollzogen werden.</p>"
        "<p>Moderationsentscheidungen werden automatisiert getroffen. Wer eine Maßnahme für "
        "fehlerhaft hält, kann sich per E-Mail an "
        "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a> oder über den "
        "Community-Discord melden; der Eintrag wird dann durch einen Menschen geprüft. "
        "Die Verantwortung der Partner für ihren eigenen Kanal bleibt unberührt.</p>"
        "<h2>§ 5 KI-gestützte Funktionen</h2>"
        "<p>Einzelne Funktionen nutzen künstliche Intelligenz, etwa die Bewertung "
        "verdächtiger Chat-Nachrichten, automatische Antworten, der Titel-Generator und "
        "Stream-Analysen. Dabei können einzelne Inhalte (z. B. Chat-Nachrichten) zur "
        "Verarbeitung an KI-Dienstleister übermittelt werden; Einzelheiten regelt die "
        "<a href='/twitch/datenschutz'>Datenschutzerklärung</a>. KI-generierte Inhalte "
        "können Fehler enthalten; der Anbieter übernimmt keine Gewähr für ihre Richtigkeit.</p>"
        "<h2>§ 6 Pflichten der Nutzerinnen und Nutzer</h2>"
        "<p>Es ist untersagt, den Dienst zu manipulieren, zu stören oder zu überlasten, "
        "Moderations- oder Schutzmechanismen (einschließlich der KI-Schutzmechanismen) zu "
        "umgehen oder den Dienst zur Verbreitung rechtswidriger Inhalte zu nutzen. Bei "
        "Verstößen kann der Anbieter Nutzerinnen und Nutzer vom Dienst ausschließen und "
        "Partner aus dem Partnerprogramm entfernen.</p>"
        "<h2>§ 7 Vertragsschluss bei kostenpflichtigen Leistungen</h2>"
        "<p>Die Darstellung der Dienste ist eine unverbindliche Aufforderung zur Bestellung. "
        "Durch Absenden des Checkout-Formulars über Stripe gibt die Kundschaft ein verbindliches "
        "Angebot ab. Der Vertrag kommt zustande, sobald die Zahlung durch Stripe bestätigt wurde "
        "oder der Anbieter den Zugang freischaltet.</p>"
        "<h2>§ 8 Preise und Zahlung</h2>"
        "<p>Die im Checkout angegebenen Preise gelten zum Zeitpunkt der Bestellung. Soweit nicht "
        "anders angegeben, verstehen sich Preise zuzüglich der gesetzlichen Umsatzsteuer. Die "
        "Abrechnung erfolgt über den Zahlungsdienstleister Stripe. Der Rechnungsbetrag wird zu "
        "Beginn des gebuchten Abrechnungszeitraums fällig.</p>"
        "<p>Bei Buchung eines Jahresabonnements wird der Jahresbetrag sofort berechnet. Sofern "
        "im Angebot ausgewiesen, können zusätzliche Bonusmonate gewährt werden. Bonusmonate sind "
        "nicht bar auszahlbar und nicht übertragbar.</p>"
        "<h2>§ 9 Laufzeit und Kündigung</h2>"
        "<p>Abonnements laufen für den gewählten Zeitraum und verlängern sich automatisch um den "
        "gleichen Zeitraum, sofern sie nicht zum Ende der laufenden Periode gekündigt werden. "
        "Die Kündigung ist über die Abo-Verwaltung unter "
        "<a href='/twitch/dashboard'>/twitch/dashboard</a> oder per E-Mail an "
        "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a> möglich.</p>"
        "<p>Die unentgeltliche Nutzung kann von beiden Seiten jederzeit ohne Einhaltung einer "
        "Frist beendet werden — durch Partner insbesondere durch Austritt aus dem "
        "Partnerprogramm oder Widerruf der Twitch-Autorisierung, durch den Anbieter "
        "insbesondere bei Verstößen gegen diese AGB. Bestehende kostenpflichtige Abonnements "
        "und zwingende gesetzliche Rechte bleiben davon unberührt.</p>"
        "<h2 id='widerruf'>§ 10 Widerrufsrecht und sofortige Leistungserbringung</h2>"
        "<p>Bei den angebotenen Diensten handelt es sich um digitale Leistungen, die unmittelbar "
        "nach Vertragsschluss bereitgestellt werden können. Das Widerrufsrecht kann nach "
        "<strong>§ 356 Abs. 5 BGB</strong> erlöschen, wenn Verbraucherinnen und Verbraucher "
        "ausdrücklich zustimmen, dass der Anbieter vor Ablauf der Widerrufsfrist mit der "
        "Ausführung beginnt, und bestätigen, dass sie dadurch ihr Widerrufsrecht verlieren.</p>"
        "<p>Diese Zustimmung wird im Bestellprozess gesondert abgefragt, sofern sie für den "
        "jeweiligen Vertrag erforderlich ist. Zwingende gesetzliche Rechte bleiben unberührt.</p>"
        "<h2>§ 11 Verfügbarkeit und Haftung</h2>"
        "<p>Der Anbieter bemüht sich um einen stabilen Betrieb, kann aber keine ununterbrochene "
        "Verfügbarkeit garantieren. Wartung, Störungen bei Drittanbietern wie Twitch, Discord "
        "oder Stripe sowie technische Ausfälle können die Nutzung zeitweise einschränken.</p>"
        "<p>Die Haftung richtet sich nach den gesetzlichen Vorschriften. Für leicht fahrlässige "
        "Pflichtverletzungen haftet der Anbieter nur bei Verletzung wesentlicher Vertragspflichten "
        "und begrenzt auf den vertragstypischen, vorhersehbaren Schaden. Für unentgeltlich "
        "erbrachte Leistungen haftet der Anbieter nur für Vorsatz und grobe Fahrlässigkeit. "
        "Die Haftung für Schäden aus der Verletzung von Leben, Körper oder Gesundheit sowie "
        "nach dem Produkthaftungsgesetz bleibt jeweils unberührt.</p>"
        "<h2>§ 12 Datenschutz</h2>"
        "<p>Informationen zur Verarbeitung personenbezogener Daten finden sich in der "
        "<a href='/twitch/datenschutz'>Datenschutzerklärung</a>. Wie das Projekt mit "
        "Sicherheit umgeht, beschreibt das öffentliche "
        "<a href='/twitch/sicherheit'>Sicherheitskonzept</a>.</p>"
        "<h2>§ 13 Änderungen der AGB</h2>"
        "<p>Der Anbieter kann diese AGB anpassen, wenn sachliche Gründe vorliegen, zum Beispiel "
        "gesetzliche Änderungen, technische Weiterentwicklungen oder Änderungen des "
        "Leistungsumfangs. Wesentliche Änderungen werden rechtzeitig mitgeteilt. "
        "Bestehende gesetzliche Rechte der Kundschaft bleiben unberührt.</p>"
        "<h2>§ 14 Schlussbestimmungen</h2>"
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
    "sicherheit": (
        "<p class='sub'>Stand: Juni 2026</p>"
        "<p>Der Bot bekommt in Partner-Kanälen Moderator-Rechte und verarbeitet "
        "Twitch-Autorisierungen. Wer so viel Vertrauen bekommt, sollte offenlegen, wie er "
        "damit umgeht. Diese Seite beschreibt deshalb, wie das Projekt das Thema Sicherheit "
        "angeht — zum Nachlesen für alle, die es genauer wissen wollen.</p>"
        "<h2>Grundprinzipien</h2>"
        "<ul>"
        "<li><strong>Minimale Berechtigungen:</strong> Der Bot fordert nur die Rechte an, "
        "die eine Funktion wirklich braucht.</li>"
        "<li><strong>Datensparsamkeit:</strong> Gespeichert wird, was für Statistiken und "
        "Moderation nötig ist — bevorzugt aggregiert statt als Rohdaten.</li>"
        "<li><strong>Verschlüsselung:</strong> Sensible Daten wie Zugriffs-Tokens liegen "
        "nie im Klartext in der Datenbank.</li>"
        "<li><strong>Transparenz:</strong> Jede Berechtigung ist im "
        "Twitch-Autorisierungsdialog sichtbar und jederzeit widerrufbar.</li>"
        "</ul>"
        "<h2>Wie der Bot selbst geschützt wird</h2>"
        "<p>Die wichtigste Frage ist nicht nur, was der Bot darf — sondern was passiert, "
        "wenn etwas schiefgeht. Deshalb ist der Bot so gebaut, dass ein einzelner Fehler "
        "oder ein kompromittierter Baustein möglichst wenig Schaden anrichten kann:</p>"
        "<ul>"
        "<li><strong>Begrenzter Schadensradius:</strong> Der Bot-Account besitzt keine "
        "Broadcaster-Rechte. Selbst wenn jemand den Bot-Account übernehmen würde, könnte "
        "er damit keine Kanäle umkonfigurieren — die mächtigeren Streamer-Autorisierungen "
        "liegen separat und verschlüsselt.</li>"
        "<li><strong>Fail-safe statt fail-open:</strong> Fehlt eine sichere Konfiguration "
        "(z. B. ein Verschlüsselungsschlüssel oder eine Datenbank-Anbindung), startet der "
        "betroffene Dienst gar nicht erst, statt unsicher weiterzulaufen. Bei "
        "Entschlüsselungsfehlern wird die Aktion abgebrochen — niemals mit Klartext-Daten "
        "improvisiert.</li>"
        "<li><strong>Getrennte Dienste:</strong> Chat, Moderation, Raids und Dashboard "
        "laufen als getrennte Prozesse mit klaren Schnittstellen. Ein Absturz oder Fehler "
        "in einem Teil legt nicht das Ganze lahm und öffnet keinen Zugriff auf die anderen "
        "Teile.</li>"
        "<li><strong>Schutz vor Doppel-Aktionen:</strong> Kritische Aktionen (z. B. Raids "
        "oder Banns) sind idempotent abgesichert — ein Netzwerk-Wackler, der eine Anfrage "
        "doppelt sendet, löst die Aktion trotzdem nur einmal aus.</li>"
        "<li><strong>Vorsichtige Rollouts:</strong> Neue automatische Funktionen (etwa "
        "KI-Chat-Antworten) laufen zuerst im Beobachtungsmodus und werden überprüft, "
        "bevor sie live wirken dürfen.</li>"
        "<li><strong>Laufende Selbstkontrolle:</strong> Automatische Log-Prüfungen laufen "
        "stündlich, ein vollständiges Audit täglich. Auffälligkeiten alarmieren die "
        "Betreiber sofort — Probleme sollen auffallen, bevor Nutzer sie bemerken.</li>"
        "</ul>"
        "<h2>Was der Bot darf — und was nicht</h2>"
        "<p>Es gibt zwei getrennte Berechtigungsebenen. Der Bot-eigene Twitch-Account "
        "arbeitet ausschließlich mit Moderator-Rechten: Nachrichten lesen und schreiben, "
        "Spam moderieren, Ankündigungen senden. Er besitzt bewusst keine "
        "Broadcaster-Berechtigungen — er kann also weder Streamtitel noch "
        "Kanaleinstellungen ändern.</p>"
        "<p>Funktionen, die im Namen des Streamers laufen (insbesondere der automatische "
        "Raid zum Stream-Ende), erfordern eine separate, ausdrückliche OAuth-Autorisierung "
        "durch den Streamer. Der angefragte Umfang steht im Twitch-Dialog. Die "
        "Autorisierung lässt sich jederzeit in den Twitch-Kontoeinstellungen unter "
        "<em>Verbindungen</em> widerrufen — der Bot erkennt das und deaktiviert die "
        "betroffenen Funktionen automatisch, statt mit ungültigen Rechten weiterzulaufen.</p>"
        "<h2>Umgang mit Zugangsdaten und Tokens</h2>"
        "<p>Betriebsgeheimnisse (API-Schlüssel, Datenbank-Zugänge) liegen in einem "
        "zentralen Secret-Manager statt in Konfigurationsdateien. Tokens und "
        "Verbindungsdaten werden in Logdateien maskiert, sodass sie auch bei der "
        "Fehlersuche nicht im Klartext auftauchen. Schlüssel-Rotationen werden "
        "protokolliert, und die Code-Historie wird automatisiert auf versehentlich "
        "eingecheckte Geheimnisse gescannt.</p>"
        "<p>Die OAuth-Tokens der Streamer sind das Sensibelste, was wir speichern. "
        "Deshalb liegen sie nicht einfach in einer pauschal verschlüsselten Datenbank, "
        "sondern <strong>jeder Wert wird einzeln</strong> mit AES-256-GCM "
        "verschlüsselt — so funktioniert das im Detail:</p>"
        "<ul>"
        "<li><strong>Frischer Zufallswert pro Verschlüsselung:</strong> Jeder Wert "
        "bekommt eine eigene, zufällig erzeugte Nonce. Selbst zwei identische Tokens "
        "ergeben damit komplett unterschiedliche verschlüsselte Daten — aus der "
        "Datenbank lässt sich kein Muster ablesen.</li>"
        "<li><strong>Manipulation fällt sofort auf:</strong> Jeder Wert trägt ein "
        "kryptographisches Echtheitssiegel. Wird auch nur ein Byte verändert, schlägt "
        "die Entschlüsselung kontrolliert fehl, statt stillschweigend falsche Daten zu "
        "liefern.</li>"
        "<li><strong>Fest an Spalte und Konto gebunden:</strong> Beim Verschlüsseln "
        "wird der Kontext mitversiegelt — Tabelle, Spaltenname und die Twitch-Konto-ID "
        "des Streamers. Ein verschlüsselter Wert lässt sich dadurch nicht an eine "
        "andere Stelle kopieren: Der Token von Streamer A kann technisch nie als Token "
        "von Streamer B entschlüsselt werden, und ein Refresh-Token geht nie als "
        "Access-Token durch.</li>"
        "<li><strong>Schlüssel getrennt vom Datenbestand:</strong> Der "
        "Master-Schlüssel liegt im Secret-Manager, niemals in der Datenbank. Ein "
        "kopierter Datenbank-Dump enthält nur unlesbare Blöcke.</li>"
        "<li><strong>Rotation eingebaut:</strong> Jeder verschlüsselte Wert trägt eine "
        "Schlüssel-Versions-Kennung. Der Schlüssel kann gewechselt werden, ohne alle "
        "Daten auf einen Schlag neu verschlüsseln zu müssen.</li>"
        "</ul>"
        "<p>So sieht ein solcher verschlüsselter Token in der Datenbank tatsächlich aus "
        "— hier ein Beispielpaar (Access- und Refresh-Token) im echten Speicherformat:</p>"
        "<pre class='cryptobox'>"
        "Access&nbsp;&nbsp;: 01027631cc1a318fc855cd096f414d4eb9140a1efaabbe7033619b73"
        "76eb2ade6399c5600e0bde49e5a9a314708da992a6e6c430146335861b9766631c795c9379b40f0"
        "3da76d94ee642ec\n"
        "Refresh : 010276315877678bb2ed322b62caed1893dda920643dafb46c11418b47095a2276fa"
        "41c0b95787b2fba008224c4d32173bfd6631bbca88554ed73a66aea84ff9de382af161cde3f6539"
        "8480e</pre>"
        "<p class='cryptohint'>Aufbau: ein Versions-Byte, die Schlüssel-ID, ein "
        "Zufalls-Nonce und schließlich der Chiffretext mit angehängtem Echtheits-Siegel "
        "— alles als Hex. Ohne den Master-Schlüssel ist daraus nichts zu gewinnen: Es "
        "gibt keine Struktur, kein Muster, keinen Angriffspunkt. (Das gezeigte Paar ist "
        "ein Format-Beispiel mit Wegwerf-Schlüssel, kein echter Zugang.)</p>"
        "<h2>Netzwerk und Infrastruktur</h2>"
        "<p>Interne Dienste des Bots sind nur auf der lokalen Maschine erreichbar "
        "(Loopback-Bindung) und zusätzlich per Firewall von außen blockiert. Öffentlich "
        "erreichbar ist nur, was über den Reverse-Proxy mit einer expliziten "
        "Pfad-Freigabeliste läuft. Interne Schnittstellen verlangen einen eigenen "
        "Zugriffs-Token, dessen Prüfung in konstanter Zeit erfolgt (Schutz vor "
        "Timing-Angriffen), sowie Idempotenz-Schlüssel, damit wiederholte Anfragen keine "
        "doppelten Aktionen auslösen.</p>"
        "<h2>Wie der Server geschützt wird</h2>"
        "<p>Der Bot läuft auf einem eigenen Server, der in mehreren Schichten "
        "abgesichert ist:</p>"
        "<ul>"
        "<li><strong>Mehrschichtige Firewall:</strong> Vor dem Server filtert bereits "
        "die Firewall des Rechenzentrums (IONOS) den Verkehr; auf dem Server selbst "
        "läuft eine zweite Firewall. Offen ist nur das Minimum, das Website und Bot "
        "brauchen — alles andere wird verworfen, die internen Dienste sind zusätzlich "
        "nur lokal erreichbar.</li>"
        "<li><strong>Fernzugriff nur privat:</strong> Die Verwaltung des Servers läuft "
        "über ein privates VPN-Netz und ist nicht offen aus dem Internet erreichbar.</li>"
        "<li><strong>Schutz vor SQL-Injection:</strong> Alle Datenbank-Zugriffe laufen "
        "über parametrisierte Abfragen — Nutzereingaben (z. B. Chat-Nachrichten oder "
        "Formularfelder) werden der Datenbank immer als Daten übergeben, nie als Teil "
        "des SQL-Befehls. Eingeschleuste Befehle laufen damit ins Leere.</li>"
        "<li><strong>Schutz vor eingeschleustem Code im Browser (XSS):</strong> "
        "Ausgaben werden konsequent maskiert, und der Webserver setzt eine "
        "Content-Security-Policy, die fremde Skripte blockiert.</li>"
        "<li><strong>Aktuelle Software:</strong> Sicherheitsupdates des Betriebssystems "
        "werden automatisch eingespielt — inklusive kontrolliertem Neustart, wenn ein "
        "Update ihn erfordert.</li>"
        "</ul>"
        "<h2>Logins und Sessions</h2>"
        "<p>Streamer melden sich am Dashboard per Twitch-OAuth an; die Anmelde-Abläufe "
        "sind mit CSRF-Schutz abgesichert, und die Zwischen-Tokens des OAuth-Flows sind "
        "einmalig verwendbar und verfallen nach zehn Minuten von selbst. Sessions werden "
        "verschlüsselt gespeichert (auch hier liegt der Schlüssel außerhalb der "
        "Datenbank) und laufen automatisch ab. Der Admin-Zugang ist nicht an ein "
        "statisches Passwort, sondern an die Mitgliedschaft im Community-Discord-Team "
        "gebunden. Und weil jeder Login über Twitch bzw. Discord läuft, gilt "
        "grundsätzlich: <strong>Wir haben nie ein Passwort von dir</strong> — es "
        "existiert bei uns schlicht keines, das gestohlen werden könnte.</p>"
        "<h2>Zahlungsdaten</h2>"
        "<p>Bezahlt wird ausschließlich über Stripe Checkout. Kreditkarten- oder "
        "Kontodaten erreichen unseren Server zu keinem Zeitpunkt — sie werden direkt "
        "bei Stripe eingegeben und verarbeitet. Bei uns gespeichert werden nur Plan, "
        "Status und Rechnungsreferenzen. Eingehende Zahlungs-Ereignisse akzeptiert der "
        "Server nur mit gültiger Stripe-Signatur, jedes Ereignis wird genau einmal "
        "verarbeitet (Schutz vor eingespielten Wiederholungen), und die "
        "Weiterleitungs-Adressen rund um den Checkout sind auf eine feste Liste "
        "erlaubter Domains begrenzt. Die Zustimmung zum sofortigen Leistungsbeginn "
        "(Widerruf) hält Stripe mit Zeitstempel pro Bestellung fest.</p>"
        "<h2>Ereignisse von außen nur mit Echtheitsnachweis</h2>"
        "<p>Von Stream-Starts und anderen Twitch-Ereignissen erfährt der Bot über "
        "signierte Benachrichtigungen (EventSub). Jede eingehende Nachricht wird "
        "kryptographisch geprüft — die Signatur wird über Nachrichten-ID, Zeitstempel "
        "und Inhalt gebildet, was die Prüfung nicht besteht, wird verworfen. Niemand "
        "kann dem Bot also gefälschte Ereignisse der Art "
        "<em>Streamer XY ist live</em> unterschieben.</p>"
        "<h2>Sicherheit der KI-Funktionen</h2>"
        "<p>Die Frage-Box auf der Website hat einen mehrschichtigen Schutz gegen "
        "Prompt-Injection: bekannte Manipulationsmuster werden erkannt, das Modell darf "
        "nur mit einem festen Fakten-Steckbrief antworten (kein erfundenes Wissen), und "
        "die Ausgabe wird geprüft, bevor sie angezeigt wird. Automatische Chat-Antworten "
        "durchlaufen einen Review-Prozess, bevor sie live gehen. Der lernende Spam-Filter "
        "hat einen eingebauten Fehlbann-Schutz: Er lernt nicht nur Spam-Muster, sondern "
        "auch ausdrücklich unbedenkliche Muster, und normalisiert Zeichen-Tricks "
        "(z. B. kyrillische Doppelgänger-Buchstaben), bevor er bewertet.</p>"
        "<h2>Moderation mit Augenmaß</h2>"
        "<p>Automatische Maßnahmen sind bewusst konservativ gebaut: Raid-Ereignisse werden "
        "über mehrere unabhängige Signale korreliert, bevor reagiert wird; auffällige "
        "externe Kanäle bekommen erst eine Karenzzeit statt eines Sofort-Banns; und wo der "
        "Bot keine Rechte hat oder nicht willkommen ist, stellt er das Senden ein, statt "
        "es immer wieder zu versuchen.</p>"
        "<h2>Überwachung und Reaktion</h2>"
        "<p>Der Betrieb wird laufend überwacht: strukturierte, rotierende Logs (mit "
        "maskierten Geheimnissen), automatische Alarme an die Betreiber bei kritischen "
        "Ereignissen, eine stündliche automatische Log-Prüfung und ein tägliches "
        "Gesamt-Audit. Der Code wird regelmäßig mit statischer Analyse, "
        "Abhängigkeits-Audits und Container-Scans geprüft.</p>"
        "<h2>Sicherheitslücke gefunden?</h2>"
        "<p>Hinweise auf Schwachstellen sind ausdrücklich willkommen — bitte vertraulich "
        "an <a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a> oder direkt an "
        "das Team im Community-Discord. Bitte keine Details öffentlich posten, bevor das "
        "Problem behoben ist (Responsible Disclosure).</p>"
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
        noindex: bool = True,
    ) -> str:
        footer_html = " &nbsp;&middot;&nbsp; ".join(
            f"<a href='{html.escape(href, quote=True)}'>{html.escape(label)}</a>"
            for href, label in footer_links
        )
        robots_meta = "<meta name='robots' content='noindex, nofollow'>" if noindex else ""
        return (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            f"{robots_meta}"
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
            ".cryptobox{background:#0f172a;color:#7dd3fc;font-family:"
            "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12px;"
            "line-height:1.5;padding:14px 16px;border-radius:8px;overflow-x:auto;"
            "white-space:pre-wrap;word-break:break-all;margin:0 0 8px;}"
            ".cryptohint{font-size:13px;color:#64748b;margin:0 0 10px;}"
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
                ("/twitch/sicherheit", "Sicherheit"),
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
                ("/twitch/sicherheit", "Sicherheit"),
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
                ("/twitch/sicherheit", "Sicherheit"),
            ),
        )
        return web.Response(text=page, content_type="text/html", headers=LEGAL_PAGE_HEADERS)

    async def abbo_sicherheit(self, request: web.Request) -> web.StreamResponse:  # noqa: ARG002
        """GET /twitch/sicherheit — public security concept, intentionally ungated."""
        document = self._load_legal_page_document("sicherheit")
        page = self._render_legal_page(
            title=str(document.get("title") or LEGAL_PAGE_TITLES["sicherheit"]),
            body=str(document.get("body") or ""),
            footer_links=(
                ("/twitch/impressum", "Impressum"),
                ("/twitch/datenschutz", "Datenschutz"),
                ("/twitch/agb", "AGB"),
            ),
            noindex=False,
        )
        return web.Response(text=page, content_type="text/html")
