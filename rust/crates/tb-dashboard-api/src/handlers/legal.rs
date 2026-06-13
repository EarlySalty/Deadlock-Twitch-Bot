//! Legal-Seiten: Impressum, Datenschutz, AGB und Sicherheitskonzept.
//!
//! 1:1-Port von `bot/dashboard/admin/legal_mixin.py` + den Legal-Routen aus
//! `bot/dashboard/routes_billing.py`. Ziel: byte-identische HTML-Antworten
//! für den Live-Diff gegen den Python-Dashboard-Prozess (8765).
//!
//! Bestandteile:
//! - Human-Gate via Cloudflare Turnstile vor Impressum/Datenschutz/AGB
//!   (HMAC-signiertes Cookie `twitch_legal_gate`, TTL 600 s)
//! - User-Agent-Blockliste gegen AI-/Suchmaschinen-Crawler auf den
//!   gegateten Seiten
//! - `/twitch/sicherheit` ist bewusst UNgegated und indexierbar
//!   (öffentliches Sicherheitskonzept)
//! - Default-Inhalte im Code; Overrides aus `legal_pages.json`
//!   (Pfad via `TB_LEGAL_PAGES_PATH`, Default wie Python:
//!   `data/admin_dashboard/legal_pages.json` relativ zum Repo-Root/CWD)
//!
//! Secrets (via Infisical-Env wie alle tb-* Settings):
//! `TWITCH_LEGAL_TURNSTILE_SITE_KEY`, `TWITCH_LEGAL_TURNSTILE_SECRET_KEY`,
//! `TWITCH_LEGAL_GATE_COOKIE_SECRET` (Fallback-Namen ohne `TWITCH_`-Präfix
//! wie im Python-Loader).

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Form, Query},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const LEGAL_GATE_TURNSTILE_VERIFY_URL: &str =
    "https://challenges.cloudflare.com/turnstile/v0/siteverify";
const LEGAL_GATE_COOKIE_NAME: &str = "twitch_legal_gate";
const LEGAL_GATE_COOKIE_TTL_SECONDS: u64 = 600;
const LEGAL_GATE_TURNSTILE_ACTION: &str = "legal_access";
const X_ROBOTS_TAG: &str = "noindex, nofollow, noarchive, nosnippet, noimageindex";

const LEGAL_GATE_ALLOWED_PATHS: [&str; 3] =
    ["/twitch/impressum", "/twitch/datenschutz", "/twitch/agb"];

const BLOCKED_LEGAL_PAGE_USER_AGENT_TOKENS: [&str; 20] = [
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
];
// Python prüft zusätzlich das Token "bot/" — hier separat, weil es einen
// Slash enthält und in der Liste oben optisch untergeht.
const BLOCKED_UA_BOT_SLASH: &str = "bot/";

// ---------------------------------------------------------------------------
// Seiten-Inhalte (Defaults, identisch zu _DEFAULT_LEGAL_PAGE_BODIES)
// ---------------------------------------------------------------------------

fn legal_page_title(slug: &str) -> Option<&'static str> {
    match slug {
        "impressum" => Some("Impressum"),
        "datenschutz" => Some("Datenschutzerklärung"),
        "agb" => Some("Allgemeine Geschäftsbedingungen"),
        "sicherheit" => Some("Sicherheitskonzept"),
        _ => None,
    }
}

const DEFAULT_BODY_IMPRESSUM: &str = concat!(
    "<p class='sub'>Angaben gemäß § 5 DDG</p>",
    "<h2>Betreiber</h2>",
    "<address>Nathanael Golla<br>Binger Straße 5<br>55263 Wackernheim</address>",
    "<h2>Kontakt</h2>",
    "<p><a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>",
    "<h2>Verantwortlich für den Inhalt</h2>",
    "<p>Verantwortlich für den Inhalt nach § 18 Abs. 2 MStV:<br>",
    "Nathanael Golla, Anschrift wie oben.</p>"
);

const DEFAULT_BODY_AGB: &str = concat!(
    "<p class='sub'>Stand: Juni 2026</p>",
    "<h2>§ 1 Geltungsbereich</h2>",
    "<p>Diese Allgemeinen Geschäftsbedingungen (AGB) gelten ausschließlich für den ",
    "<strong>Twitch-Bot der Deutschen Deadlock Community</strong> samt zugehöriger ",
    "Web-Dienste (Streamer-Dashboard, Statistik-Seiten, Abo-Verwaltung), betrieben ",
    "von Nathanael Golla, Binger Straße 5, 55263 Wackernheim (nachfolgend ",
    "<em>Anbieter</em>). Andere Angebote der Deutschen Deadlock Community — etwa ",
    "die Discord-Bots, der Steam-Bot oder der Community-Discord-Server — sind nicht ",
    "Gegenstand dieser AGB. Sie gelten sowohl für die unentgeltliche Nutzung des ",
    "Twitch-Bots als auch für kostenpflichtige Zusatzleistungen. ",
    "Nutzerinnen und Nutzer sind insbesondere ",
    "Streamerinnen und Streamer, die am Partnerprogramm teilnehmen (nachfolgend ",
    "<em>Partner</em>), sowie bei kostenpflichtigen Leistungen die <em>Kundschaft</em>. ",
    "Abweichende Bedingungen werden nur Vertragsbestandteil, wenn der Anbieter ihnen ",
    "ausdrücklich zustimmt.</p>",
    "<h2>§ 2 Leistungen des Anbieters</h2>",
    "<p>Der Anbieter betreibt einen Chat-Bot und zugehörige Dienste für Twitch-Streamer ",
    "der Deadlock-Community. Die kostenlose Basisnutzung kann insbesondere umfassen:</p>",
    "<ul>",
    "<li><strong>Chat-Bot:</strong> Chat-Befehle, Community-Hinweise und automatische ",
    "Nachrichten im Kanal des Partners.</li>",
    "<li><strong>Raid-Netzwerk:</strong> automatische Weiterleitung der Zuschauer (Raid) ",
    "an einen anderen Partner zum Stream-Ende. Die gegenseitigen Raids sind fester ",
    "Bestandteil des Partnernetzwerks.</li>",
    "<li><strong>Automatische Moderation:</strong> Erkennung und Moderation von Spam-, ",
    "Scam- und Bot-Aktivität (siehe § 4).</li>",
    "<li><strong>Statistiken und Dashboard:</strong> Stream- und Community-Auswertungen ",
    "für Partner.</li>",
    "<li><strong>Discord-Integration:</strong> z. B. Live-Ankündigungen im ",
    "Community-Discord.</li>",
    "</ul>",
    "<p>Kostenpflichtige Zusatzleistungen können insbesondere umfassen:</p>",
    "<ul>",
    "<li><strong>Raid Boost:</strong> bevorzugte Platzierung des Kanals im Raid-Netzwerk.</li>",
    "<li><strong>Analyse-Dashboard:</strong> Zugang zu erweiterten Statistiken, ",
    "Viewer-Verläufen und Wachstumsanalysen.</li>",
    "<li><strong>Bundle:</strong> Kombination aus Analyse-Dashboard und Raid Boost.</li>",
    "</ul>",
    "<p>Der konkrete Leistungsumfang kostenpflichtiger Leistungen ergibt sich aus der im ",
    "Checkout ausgewählten Option. Auf unentgeltliche Leistungen besteht kein Anspruch; ",
    "der Anbieter kann sie weiterentwickeln, einschränken oder einstellen.</p>",
    "<h2>§ 3 Teilnahme am Partnerprogramm</h2>",
    "<p>Die Nutzung setzt ein Twitch-Konto voraus. Die Nutzungsbedingungen von Twitch ",
    "bleiben unberührt und sind einzuhalten. Die Aufnahme in das Partnerprogramm erfolgt ",
    "nach Freischaltung durch den Anbieter; ein Anspruch auf Aufnahme besteht nicht.</p>",
    "<p>Funktionen, die der Bot im Namen des Partners ausführt (insbesondere Raids), ",
    "erfordern eine ausdrückliche Autorisierung über Twitch (OAuth). Der Umfang der ",
    "Berechtigungen wird im Twitch-Autorisierungsdialog angezeigt. Die Autorisierung ",
    "kann jederzeit in den Twitch-Kontoeinstellungen unter <em>Verbindungen</em> ",
    "widerrufen werden; der Bot erkennt den Widerruf und deaktiviert die betroffenen ",
    "Funktionen automatisch.</p>",
    "<h2>§ 4 Automatische Moderation und Bannliste</h2>",
    "<p>Der Bot führt in Partner-Kanälen automatische Moderation durch. Dazu gehören ",
    "das Löschen von Nachrichten, Timeouts und Banns bei erkannter Spam-, Scam- oder ",
    "Bot-Aktivität. Zum Schutz aller Partner führt der Anbieter eine kanalübergreifende ",
    "Bannliste; Einträge können in allen Partner-Kanälen vollzogen werden.</p>",
    "<p>Moderationsentscheidungen werden automatisiert getroffen. Wer eine Maßnahme für ",
    "fehlerhaft hält, kann sich per E-Mail an ",
    "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a> oder über den ",
    "Community-Discord melden; der Eintrag wird dann durch einen Menschen geprüft. ",
    "Die Verantwortung der Partner für ihren eigenen Kanal bleibt unberührt.</p>",
    "<h2>§ 5 KI-gestützte Funktionen</h2>",
    "<p>Einzelne Funktionen nutzen künstliche Intelligenz, etwa die Bewertung ",
    "verdächtiger Chat-Nachrichten, automatische Antworten, der Titel-Generator und ",
    "Stream-Analysen. Dabei können einzelne Inhalte (z. B. Chat-Nachrichten) zur ",
    "Verarbeitung an KI-Dienstleister übermittelt werden; Einzelheiten regelt die ",
    "<a href='/twitch/datenschutz'>Datenschutzerklärung</a>. KI-generierte Inhalte ",
    "können Fehler enthalten; der Anbieter übernimmt keine Gewähr für ihre Richtigkeit.</p>",
    "<h2>§ 6 Pflichten der Nutzerinnen und Nutzer</h2>",
    "<p>Es ist untersagt, den Dienst zu manipulieren, zu stören oder zu überlasten, ",
    "Moderations- oder Schutzmechanismen (einschließlich der KI-Schutzmechanismen) zu ",
    "umgehen oder den Dienst zur Verbreitung rechtswidriger Inhalte zu nutzen. Bei ",
    "Verstößen kann der Anbieter Nutzerinnen und Nutzer vom Dienst ausschließen und ",
    "Partner aus dem Partnerprogramm entfernen.</p>",
    "<h2>§ 7 Vertragsschluss bei kostenpflichtigen Leistungen</h2>",
    "<p>Die Darstellung der Dienste ist eine unverbindliche Aufforderung zur Bestellung. ",
    "Durch Absenden des Checkout-Formulars über Stripe gibt die Kundschaft ein verbindliches ",
    "Angebot ab. Der Vertrag kommt zustande, sobald die Zahlung durch Stripe bestätigt wurde ",
    "oder der Anbieter den Zugang freischaltet.</p>",
    "<h2>§ 8 Preise und Zahlung</h2>",
    "<p>Die im Checkout angegebenen Preise gelten zum Zeitpunkt der Bestellung. Soweit nicht ",
    "anders angegeben, verstehen sich Preise zuzüglich der gesetzlichen Umsatzsteuer. Die ",
    "Abrechnung erfolgt über den Zahlungsdienstleister Stripe. Der Rechnungsbetrag wird zu ",
    "Beginn des gebuchten Abrechnungszeitraums fällig.</p>",
    "<p>Bei Buchung eines Jahresabonnements wird der Jahresbetrag sofort berechnet. Sofern ",
    "im Angebot ausgewiesen, können zusätzliche Bonusmonate gewährt werden. Bonusmonate sind ",
    "nicht bar auszahlbar und nicht übertragbar.</p>",
    "<h2>§ 9 Laufzeit und Kündigung</h2>",
    "<p>Abonnements laufen für den gewählten Zeitraum und verlängern sich automatisch um den ",
    "gleichen Zeitraum, sofern sie nicht zum Ende der laufenden Periode gekündigt werden. ",
    "Die Kündigung ist über die Abo-Verwaltung unter ",
    "<a href='/twitch/dashboard'>/twitch/dashboard</a> oder per E-Mail an ",
    "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a> möglich.</p>",
    "<p>Die unentgeltliche Nutzung kann von beiden Seiten jederzeit ohne Einhaltung einer ",
    "Frist beendet werden — durch Partner insbesondere durch Austritt aus dem ",
    "Partnerprogramm oder Widerruf der Twitch-Autorisierung, durch den Anbieter ",
    "insbesondere bei Verstößen gegen diese AGB. Bestehende kostenpflichtige Abonnements ",
    "und zwingende gesetzliche Rechte bleiben davon unberührt.</p>",
    "<h2 id='widerruf'>§ 10 Widerrufsrecht und sofortige Leistungserbringung</h2>",
    "<p>Bei den angebotenen Diensten handelt es sich um digitale Leistungen, die unmittelbar ",
    "nach Vertragsschluss bereitgestellt werden können. Das Widerrufsrecht kann nach ",
    "<strong>§ 356 Abs. 5 BGB</strong> erlöschen, wenn Verbraucherinnen und Verbraucher ",
    "ausdrücklich zustimmen, dass der Anbieter vor Ablauf der Widerrufsfrist mit der ",
    "Ausführung beginnt, und bestätigen, dass sie dadurch ihr Widerrufsrecht verlieren.</p>",
    "<p>Diese Zustimmung wird im Bestellprozess gesondert abgefragt, sofern sie für den ",
    "jeweiligen Vertrag erforderlich ist. Zwingende gesetzliche Rechte bleiben unberührt.</p>",
    "<h2>§ 11 Verfügbarkeit und Haftung</h2>",
    "<p>Der Anbieter bemüht sich um einen stabilen Betrieb, kann aber keine ununterbrochene ",
    "Verfügbarkeit garantieren. Wartung, Störungen bei Drittanbietern wie Twitch, Discord ",
    "oder Stripe sowie technische Ausfälle können die Nutzung zeitweise einschränken.</p>",
    "<p>Die Haftung richtet sich nach den gesetzlichen Vorschriften. Für leicht fahrlässige ",
    "Pflichtverletzungen haftet der Anbieter nur bei Verletzung wesentlicher Vertragspflichten ",
    "und begrenzt auf den vertragstypischen, vorhersehbaren Schaden. Für unentgeltlich ",
    "erbrachte Leistungen haftet der Anbieter nur für Vorsatz und grobe Fahrlässigkeit. ",
    "Die Haftung für Schäden aus der Verletzung von Leben, Körper oder Gesundheit sowie ",
    "nach dem Produkthaftungsgesetz bleibt jeweils unberührt.</p>",
    "<h2>§ 12 Datenschutz</h2>",
    "<p>Informationen zur Verarbeitung personenbezogener Daten finden sich in der ",
    "<a href='/twitch/datenschutz'>Datenschutzerklärung</a>. Wie das Projekt mit ",
    "Sicherheit umgeht, beschreibt das öffentliche ",
    "<a href='/twitch/sicherheit'>Sicherheitskonzept</a>.</p>",
    "<h2>§ 13 Änderungen der AGB</h2>",
    "<p>Der Anbieter kann diese AGB anpassen, wenn sachliche Gründe vorliegen, zum Beispiel ",
    "gesetzliche Änderungen, technische Weiterentwicklungen oder Änderungen des ",
    "Leistungsumfangs. Wesentliche Änderungen werden rechtzeitig mitgeteilt. ",
    "Bestehende gesetzliche Rechte der Kundschaft bleiben unberührt.</p>",
    "<h2>§ 14 Schlussbestimmungen</h2>",
    "<p>Es gilt deutsches Recht unter Ausschluss des UN-Kaufrechts. Für Verbraucherinnen ",
    "und Verbraucher gelten zusätzlich die zwingenden Verbraucherschutzvorschriften ihres ",
    "gewöhnlichen Aufenthaltsortes. Sollten einzelne Bestimmungen dieser AGB unwirksam sein, ",
    "bleibt die Wirksamkeit der übrigen Bestimmungen unberührt.</p>"
);

const DEFAULT_BODY_DATENSCHUTZ: &str = concat!(
    "<p class='sub'>Stand: Mai 2026</p>",
    "<h2>Verantwortlicher</h2>",
    "<p>Nathanael Golla<br>Binger Straße 5, 55263 Wackernheim<br>",
    "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>",
    "<h2>Zwecke und Rechtsgrundlagen</h2>",
    "<p>Wir verarbeiten personenbezogene Daten, um Login, Abo-Verwaltung, Zahlungsabwicklung, ",
    "Dashboard-Funktionen, Support und den sicheren Betrieb des Dienstes bereitzustellen. ",
    "Rechtsgrundlagen sind insbesondere Art. 6 Abs. 1 lit. b DSGVO (Vertragserfüllung), ",
    "Art. 6 Abs. 1 lit. c DSGVO (gesetzliche Pflichten) und Art. 6 Abs. 1 lit. f DSGVO ",
    "(berechtigte Interessen an Sicherheit, Fehleranalyse und Missbrauchsschutz).</p>",
    "<h2>Verarbeitete Daten</h2>",
    "<p>Je nach Nutzung können insbesondere folgende Daten verarbeitet werden:</p>",
    "<ul>",
    "<li>Twitch-Daten: Twitch-Name, Twitch-ID, OAuth-Status und von Twitch ",
    "bereitgestellte Profildaten.</li>",
    "<li>Discord-Daten: Discord-ID, Anzeigename und Rollenstatus, soweit für Community- ",
    "oder Admin-Funktionen erforderlich.</li>",
    "<li>Abonnement- und Rechnungsdaten: Plan, Status, Buchungszeitpunkt, ",
    "Rechnungsreferenzen und steuerlich relevante Angaben.</li>",
    "<li>Nutzungs- und Analysedaten: Stream-Statistiken, Viewer-Verläufe, Chat- und ",
    "Dashboard-Metriken, soweit sie für gebuchte Funktionen benötigt werden.</li>",
    "<li>Technische Daten: IP-Adresse, User-Agent, Zeitstempel, Logdaten, ",
    "Sicherheitsereignisse und Session-Cookies.</li>",
    "</ul>",
    "<h2>Empfänger und Dienstleister</h2>",
    "<p>Zahlungen werden über Stripe Payments Europe Ltd. abgewickelt. Stripe verarbeitet ",
    "Zahlungsdaten nach eigener Datenschutzrichtlinie: ",
    "<a href='https://stripe.com/de/privacy' target='_blank' ",
    "rel='noopener noreferrer'>stripe.com/de/privacy</a>.</p>",
    "<p>Für Login- und Plattformfunktionen werden Daten mit Twitch, Discord und den jeweils ",
    "angebundenen Plattformen ausgetauscht, soweit dies technisch oder vertraglich ",
    "notwendig ist. Für den Schutz der Legal-Seiten kann Cloudflare Turnstile eingesetzt ",
    "werden, um automatisierte Zugriffe zu erkennen.</p>",
    "<h2>Cookies</h2>",
    "<p>Diese Website verwendet technisch notwendige Cookies, insbesondere für Login-Sessions, ",
    "Abo-Verwaltung und das Legal-Access-Gate. Es werden keine Marketing-Cookies eingesetzt. ",
    "Eine Einwilligung ist für unbedingt erforderliche Cookies gemäß § 25 Abs. 2 Nr. 2 TDDDG ",
    "nicht erforderlich. Stripe kann während des Bezahlvorgangs Cookies auf eigenen ",
    "Domains setzen.</p>",
    "<h2>Speicherdauer</h2>",
    "<p>Daten werden nur so lange gespeichert, wie sie für die genannten Zwecke ",
    "erforderlich sind. Abonnement- und Nutzungsdaten werden grundsätzlich für die Dauer ",
    "des Vertrags gespeichert. ",
    "Rechnungs- und Buchungsdaten können aufgrund gesetzlicher Aufbewahrungspflichten bis zu ",
    "10 Jahre gespeichert werden. Sicherheits- und Serverlogs werden regelmäßig gelöscht, ",
    "sofern keine längere Aufbewahrung zur Aufklärung von Missbrauch oder Störungen ",
    "erforderlich ist.</p>",
    "<h2>Deine Rechte (Art. 15-22 DSGVO)</h2>",
    "<ul>",
    "<li>Auskunft über gespeicherte Daten (Art. 15)</li>",
    "<li>Berichtigung unrichtiger Daten (Art. 16)</li>",
    "<li>Löschung deiner Daten (Art. 17)</li>",
    "<li>Einschränkung der Verarbeitung (Art. 18)</li>",
    "<li>Datenübertragbarkeit (Art. 20)</li>",
    "<li>Widerspruch gegen die Verarbeitung (Art. 21)</li>",
    "</ul>",
    "<p>Zur Wahrnehmung dieser Rechte wende dich an: ",
    "<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a></p>",
    "<h2>Beschwerderecht</h2>",
    "<p>Du hast das Recht, dich bei der zuständigen Datenschutz-Aufsichtsbehörde ",
    "zu beschweren. Zuständig ist der <em>Landesbeauftragte für den Datenschutz ",
    "und die Informationsfreiheit Rheinland-Pfalz (LfDI)</em>, ",
    "Hintere Bleiche 34, 55116 Mainz.</p>"
);

const DEFAULT_BODY_SICHERHEIT: &str = concat!(
    "<p class='sub'>Stand: Juni 2026</p>",
    "<p>Der Bot bekommt in Partner-Kanälen Moderator-Rechte und verarbeitet ",
    "Twitch-Autorisierungen. Wer so viel Vertrauen bekommt, sollte offenlegen, wie er ",
    "damit umgeht. Diese Seite beschreibt deshalb, wie das Projekt das Thema Sicherheit ",
    "angeht — zum Nachlesen für alle, die es genauer wissen wollen.</p>",
    "<h2>Grundprinzipien</h2>",
    "<ul>",
    "<li><strong>Minimale Berechtigungen:</strong> Der Bot fordert nur die Rechte an, ",
    "die eine Funktion wirklich braucht.</li>",
    "<li><strong>Datensparsamkeit:</strong> Gespeichert wird, was für Statistiken und ",
    "Moderation nötig ist — bevorzugt aggregiert statt als Rohdaten.</li>",
    "<li><strong>Verschlüsselung:</strong> Sensible Daten wie Zugriffs-Tokens liegen ",
    "nie im Klartext in der Datenbank.</li>",
    "<li><strong>Transparenz:</strong> Jede Berechtigung ist im ",
    "Twitch-Autorisierungsdialog sichtbar und jederzeit widerrufbar.</li>",
    "</ul>",
    "<h2>Wie der Bot selbst geschützt wird</h2>",
    "<p>Die wichtigste Frage ist nicht nur, was der Bot darf — sondern was passiert, ",
    "wenn etwas schiefgeht. Deshalb ist der Bot so gebaut, dass ein einzelner Fehler ",
    "oder ein kompromittierter Baustein möglichst wenig Schaden anrichten kann:</p>",
    "<ul>",
    "<li><strong>Begrenzter Schadensradius:</strong> Der Bot-Account besitzt keine ",
    "Broadcaster-Rechte. Selbst wenn jemand den Bot-Account übernehmen würde, könnte ",
    "er damit keine Kanäle umkonfigurieren — die mächtigeren Streamer-Autorisierungen ",
    "liegen separat und verschlüsselt.</li>",
    "<li><strong>Fail-safe statt fail-open:</strong> Fehlt eine sichere Konfiguration ",
    "(z. B. ein Verschlüsselungsschlüssel oder eine Datenbank-Anbindung), startet der ",
    "betroffene Dienst gar nicht erst, statt unsicher weiterzulaufen. Bei ",
    "Entschlüsselungsfehlern wird die Aktion abgebrochen — niemals mit Klartext-Daten ",
    "improvisiert.</li>",
    "<li><strong>Getrennte Dienste:</strong> Chat, Moderation, Raids und Dashboard ",
    "laufen als getrennte Prozesse mit klaren Schnittstellen. Ein Absturz oder Fehler ",
    "in einem Teil legt nicht das Ganze lahm und öffnet keinen Zugriff auf die anderen ",
    "Teile.</li>",
    "<li><strong>Schutz vor Doppel-Aktionen:</strong> Kritische Aktionen (z. B. Raids ",
    "oder Banns) sind idempotent abgesichert — ein Netzwerk-Wackler, der eine Anfrage ",
    "doppelt sendet, löst die Aktion trotzdem nur einmal aus.</li>",
    "<li><strong>Vorsichtige Rollouts:</strong> Neue automatische Funktionen (etwa ",
    "KI-Chat-Antworten) laufen zuerst im Beobachtungsmodus und werden überprüft, ",
    "bevor sie live wirken dürfen.</li>",
    "<li><strong>Laufende Selbstkontrolle:</strong> Automatische Log-Prüfungen laufen ",
    "stündlich, ein vollständiges Audit täglich. Auffälligkeiten alarmieren die ",
    "Betreiber sofort — Probleme sollen auffallen, bevor Nutzer sie bemerken.</li>",
    "</ul>",
    "<h2>Was der Bot darf — und was nicht</h2>",
    "<p>Es gibt zwei getrennte Berechtigungsebenen. Der Bot-eigene Twitch-Account ",
    "arbeitet ausschließlich mit Moderator-Rechten: Nachrichten lesen und schreiben, ",
    "Spam moderieren, Ankündigungen senden. Er besitzt bewusst keine ",
    "Broadcaster-Berechtigungen — er kann also weder Streamtitel noch ",
    "Kanaleinstellungen ändern.</p>",
    "<p>Funktionen, die im Namen des Streamers laufen (insbesondere der automatische ",
    "Raid zum Stream-Ende), erfordern eine separate, ausdrückliche OAuth-Autorisierung ",
    "durch den Streamer. Der angefragte Umfang steht im Twitch-Dialog. Die ",
    "Autorisierung lässt sich jederzeit in den Twitch-Kontoeinstellungen unter ",
    "<em>Verbindungen</em> widerrufen — der Bot erkennt das und deaktiviert die ",
    "betroffenen Funktionen automatisch, statt mit ungültigen Rechten weiterzulaufen.</p>",
    "<h2>Umgang mit Zugangsdaten und Tokens</h2>",
    "<p>Programme brauchen Geheimnisse, um zu funktionieren: den Zugang zur ",
    "Datenbank, Schlüssel für Twitch, Discord oder Stripe. Der bequeme — und weit ",
    "verbreitete — Weg ist eine sogenannte <em>.env-Datei</em>: eine einfache ",
    "Textdatei, in der diese Werte im Klartext stehen und beim Start als ",
    "Umgebungsvariablen an das Programm übergeben werden. Das ist riskant: So eine ",
    "Datei liegt lesbar auf der Festplatte, rutscht leicht versehentlich in ein ",
    "Backup oder in die Versionsverwaltung, und jeder Prozess auf dem System kann ",
    "die Umgebungsvariablen anderer Prozesse potenziell mitlesen.</p>",
    "<p><strong>Diesen Weg gehen wir bewusst nicht.</strong> Stattdessen liegen ",
    "alle Betriebsgeheimnisse in einem zentralen, verschlüsselten Tresor ",
    "(Secret-Manager). Dort sind sie verschlüsselt gespeichert; im Klartext ",
    "existieren sie nur flüchtig im Arbeitsspeicher des jeweiligen Dienstes, ",
    "während er läuft — und werden beim Start frisch aus dem Tresor geholt, statt ",
    "in einer Datei auf der Platte zu liegen. Zugriff auf den Tresor ist selbst ",
    "wieder abgesichert und auf das Nötigste beschränkt. Zusätzlich werden Tokens ",
    "und Verbindungsdaten in Logdateien maskiert, sodass sie auch bei der ",
    "Fehlersuche nicht im Klartext auftauchen, Schlüssel-Rotationen werden ",
    "protokolliert, und die Code-Historie wird automatisiert auf versehentlich ",
    "eingecheckte Geheimnisse gescannt.</p>",
    "<p>Die OAuth-Tokens der Streamer sind das Sensibelste, was wir speichern. ",
    "Deshalb liegen sie nicht einfach in einer pauschal verschlüsselten Datenbank, ",
    "sondern <strong>jeder Wert wird einzeln</strong> mit AES-256-GCM ",
    "verschlüsselt — so funktioniert das im Detail:</p>",
    "<ul>",
    "<li><strong>Frischer Zufallswert pro Verschlüsselung:</strong> Jeder Wert ",
    "bekommt eine eigene, zufällig erzeugte Nonce. Selbst zwei identische Tokens ",
    "ergeben damit komplett unterschiedliche verschlüsselte Daten — aus der ",
    "Datenbank lässt sich kein Muster ablesen.</li>",
    "<li><strong>Manipulation fällt sofort auf:</strong> Jeder Wert trägt ein ",
    "kryptographisches Echtheitssiegel. Wird auch nur ein Byte verändert, schlägt ",
    "die Entschlüsselung kontrolliert fehl, statt stillschweigend falsche Daten zu ",
    "liefern.</li>",
    "<li><strong>Fest an Spalte und Konto gebunden:</strong> Beim Verschlüsseln ",
    "wird der Kontext mitversiegelt — Tabelle, Spaltenname und die Twitch-Konto-ID ",
    "des Streamers. Ein verschlüsselter Wert lässt sich dadurch nicht an eine ",
    "andere Stelle kopieren: Der Token von Streamer A kann technisch nie als Token ",
    "von Streamer B entschlüsselt werden, und ein Refresh-Token geht nie als ",
    "Access-Token durch.</li>",
    "<li><strong>Schlüssel getrennt vom Datenbestand:</strong> Der ",
    "Master-Schlüssel liegt im Secret-Manager, niemals in der Datenbank. Ein ",
    "kopierter Datenbank-Dump enthält nur unlesbare Blöcke.</li>",
    "<li><strong>Rotation eingebaut:</strong> Jeder verschlüsselte Wert trägt eine ",
    "Schlüssel-Versions-Kennung. Der Schlüssel kann gewechselt werden, ohne alle ",
    "Daten auf einen Schlag neu verschlüsseln zu müssen.</li>",
    "</ul>",
    "<p>Wir setzen hier bewusst auf diese gezielte Verschlüsselung einzelner Felder ",
    "statt allein auf eine pauschale Verschlüsselung der gesamten Festplatte. Der ",
    "Grund: Eine Festplattenverschlüsselung schützt nur die ausgeschaltete Platte ",
    "vor physischem Diebstahl — sobald der Server läuft, ist sie entschlüsselt und ",
    "hilft gegen einen kopierten Datenbank-Auszug, ein verlorenes Backup oder eine ",
    "Datenbank-Lücke nicht. Die Feldverschlüsselung schützt genau dort: Selbst ein ",
    "vollständiger Datenbank-Auszug enthält bei den sensiblen Werten nur unlesbare ",
    "Blöcke, weil der Schlüssel eben nicht in der Datenbank liegt.</p>",
    "<p>So sieht ein solcher verschlüsselter Token in der Datenbank tatsächlich aus ",
    "— hier ein Beispielpaar (Access- und Refresh-Token) im echten Speicherformat:</p>",
    "<pre class='cryptobox'>",
    "Access&nbsp;&nbsp;: 01027631cc1a318fc855cd096f414d4eb9140a1efaabbe7033619b73",
    "76eb2ade6399c5600e0bde49e5a9a314708da992a6e6c430146335861b9766631c795c9379b40f0",
    "3da76d94ee642ec\n",
    "Refresh : 010276315877678bb2ed322b62caed1893dda920643dafb46c11418b47095a2276fa",
    "41c0b95787b2fba008224c4d32173bfd6631bbca88554ed73a66aea84ff9de382af161cde3f6539",
    "8480e</pre>",
    "<p class='cryptohint'>Aufbau: ein Versions-Byte, die Schlüssel-ID, ein ",
    "Zufalls-Nonce und schließlich der Chiffretext mit angehängtem Echtheits-Siegel ",
    "— alles als Hex. Ohne den Master-Schlüssel ist daraus nichts zu gewinnen: Es ",
    "gibt keine Struktur, kein Muster, keinen Angriffspunkt. (Das gezeigte Paar ist ",
    "ein Format-Beispiel mit Wegwerf-Schlüssel, kein echter Zugang.)</p>",
    "<h2>Netzwerk und Infrastruktur</h2>",
    "<p>Interne Dienste des Bots sind nur auf der lokalen Maschine erreichbar ",
    "(Loopback-Bindung) und zusätzlich per Firewall von außen blockiert. Öffentlich ",
    "erreichbar ist nur, was über den Reverse-Proxy mit einer expliziten ",
    "Pfad-Freigabeliste läuft. Interne Schnittstellen verlangen einen eigenen ",
    "Zugriffs-Token, dessen Prüfung in konstanter Zeit erfolgt (Schutz vor ",
    "Timing-Angriffen), sowie Idempotenz-Schlüssel, damit wiederholte Anfragen keine ",
    "doppelten Aktionen auslösen.</p>",
    "<h2>Wie der Server geschützt wird</h2>",
    "<p>Der Bot läuft auf einem eigenen Server, der in mehreren Schichten ",
    "abgesichert ist:</p>",
    "<ul>",
    "<li><strong>Mehrschichtige Firewall:</strong> Vor dem Server filtert bereits ",
    "die Firewall des Rechenzentrums (IONOS) den Verkehr; auf dem Server selbst ",
    "läuft eine zweite Firewall. Offen ist nur das Minimum, das Website und Bot ",
    "brauchen — alles andere wird verworfen, die internen Dienste sind zusätzlich ",
    "nur lokal erreichbar.</li>",
    "<li><strong>Fernzugriff nur privat:</strong> Die Verwaltung des Servers läuft ",
    "über ein privates VPN-Netz und ist nicht offen aus dem Internet erreichbar.</li>",
    "<li><strong>Schutz vor SQL-Injection:</strong> Alle Datenbank-Zugriffe laufen ",
    "über parametrisierte Abfragen — Nutzereingaben (z. B. Chat-Nachrichten oder ",
    "Formularfelder) werden der Datenbank immer als Daten übergeben, nie als Teil ",
    "des SQL-Befehls. Eingeschleuste Befehle laufen damit ins Leere.</li>",
    "<li><strong>Schutz vor eingeschleustem Code im Browser (XSS):</strong> ",
    "Ausgaben werden konsequent maskiert, und der Webserver setzt eine ",
    "Content-Security-Policy, die fremde Skripte blockiert.</li>",
    "<li><strong>Aktuelle Software:</strong> Sicherheitsupdates des Betriebssystems ",
    "werden automatisch eingespielt — inklusive kontrolliertem Neustart, wenn ein ",
    "Update ihn erfordert.</li>",
    "<li><strong>Wenig Rechte pro Dienst:</strong> Die Bot-Dienste laufen nicht ",
    "mit Administrator-Rechten, sondern als eingeschränkte Benutzer und jeweils in ",
    "eigenen, voneinander getrennten Prozessen. Was ein einzelner Dienst anrichten ",
    "kann, ist dadurch von vornherein begrenzt.</li>",
    "<li><strong>Verschlüsselte Verbindungen:</strong> Der gesamte Verkehr von ",
    "außen läuft über HTTPS/TLS — Daten zwischen Browser und Server sind unterwegs ",
    "verschlüsselt.</li>",
    "<li><strong>Selbstheilung und Überlastschutz:</strong> Stürzt ein Dienst ab, ",
    "wird er automatisch neu gestartet; ein Wächter greift ein, bevor ",
    "Speicher-Engpässe den Server lahmlegen.</li>",
    "<li><strong>Regelmäßige Backups:</strong> Von den Daten werden regelmäßig ",
    "Sicherungen gezogen, damit nach einem Ausfall nichts verloren geht.</li>",
    "</ul>",
    "<p>Das Leitprinzip dahinter ist <em>Verteidigung in mehreren Schichten</em>: ",
    "Es gibt nicht die eine Mauer, deren Fall alles preisgibt, sondern viele ",
    "kleine Hürden — fällt eine, stehen die anderen noch.</p>",
    "<h2>Logins und Sessions</h2>",
    "<p>Streamer melden sich am Dashboard per Twitch-OAuth an; die Anmelde-Abläufe ",
    "sind mit CSRF-Schutz abgesichert, und die Zwischen-Tokens des OAuth-Flows sind ",
    "einmalig verwendbar und verfallen nach zehn Minuten von selbst. Sessions werden ",
    "verschlüsselt gespeichert (auch hier liegt der Schlüssel außerhalb der ",
    "Datenbank) und laufen automatisch ab. Der Admin-Zugang ist nicht an ein ",
    "statisches Passwort, sondern an die Mitgliedschaft im Community-Discord-Team ",
    "gebunden. Und weil jeder Login über Twitch bzw. Discord läuft, gilt ",
    "grundsätzlich: <strong>Wir haben nie ein Passwort von dir</strong> — es ",
    "existiert bei uns schlicht keines, das gestohlen werden könnte.</p>",
    "<h2>Schutz der Betreiber-Zugänge</h2>",
    "<p>Die beste Technik nützt wenig, wenn jemand einfach die Zugänge des ",
    "Betreibers übernimmt. Deshalb gilt auch auf der menschlichen Ebene ein klarer ",
    "Standard:</p>",
    "<ul>",
    "<li><strong>Passwortmanager statt Merkbarkeit:</strong> Jeder Zugang — vom ",
    "Bot-Konto bis zum Server — hat ein eigenes, zufällig erzeugtes Passwort, ",
    "das nirgends wiederverwendet wird. Sie sind bewusst lang (das Twitch-Konto ",
    "des Bots etwa 40 Zeichen) und in einem Passwortmanager gespeichert, nicht im ",
    "Kopf und nicht in einer Datei.</li>",
    "<li><strong>Eingebauter Phishing-Schutz:</strong> Der Passwortmanager merkt ",
    "sich zu jedem Zugang die echte Adresse (Domain) und füllt das Passwort nur ",
    "dort aus. Führt ein gefälschter Link auf eine täuschend echt aussehende ",
    "Betrugsseite, bleibt das Feld leer — der Manager kennt diese Adresse nicht. ",
    "Damit dort überhaupt ein Passwort landet, müsste man es bewusst von Hand ",
    "eintippen oder die falsche Seite aktiv freigeben. Genau dieses ausbleibende ",
    "automatische Ausfüllen ist eine zweite Warnstufe: Es zwingt zum aktiven ",
    "Nachdenken, statt dass die Anmeldung einfach durchrutscht.</li>",
    "<li><strong>Zwei-Faktor überall:</strong> Alle wichtigen Konten sind mit ",
    "Zwei-Faktor-Authentifizierung geschützt. Ein gestohlenes Passwort allein ",
    "reicht nicht, um sich einzuloggen.</li>",
    "<li><strong>Benachrichtigung bei jedem Login:</strong> Anmeldungen an den ",
    "wichtigen Konten lösen eine E-Mail aus. Ein unbefugter Zugriff würde sofort ",
    "auffallen, statt unbemerkt zu bleiben.</li>",
    "<li><strong>Wachsam gegen Social Engineering:</strong> Die häufigste echte ",
    "Angriffsform ist nicht das Knacken von Technik, sondern das Austricksen von ",
    "Menschen — gefälschte E-Mails, vorgetäuschte Notlagen, angebliche ",
    "Support-Anfragen. Darauf ist der Betreiber bewusst geschult: Zugangsdaten ",
    "werden grundsätzlich nicht auf Zuruf herausgegeben, Aufforderungen mit ",
    "Zeitdruck werden misstrauisch behandelt.</li>",
    "</ul>",
    "<h2>Zahlungsdaten</h2>",
    "<p>Bezahlt wird ausschließlich über Stripe Checkout. Kreditkarten- oder ",
    "Kontodaten erreichen unseren Server zu keinem Zeitpunkt — sie werden direkt ",
    "bei Stripe eingegeben und verarbeitet. Bei uns gespeichert werden nur Plan, ",
    "Status und Rechnungsreferenzen. Eingehende Zahlungs-Ereignisse akzeptiert der ",
    "Server nur mit gültiger Stripe-Signatur, jedes Ereignis wird genau einmal ",
    "verarbeitet (Schutz vor eingespielten Wiederholungen), und die ",
    "Weiterleitungs-Adressen rund um den Checkout sind auf eine feste Liste ",
    "erlaubter Domains begrenzt. Die Zustimmung zum sofortigen Leistungsbeginn ",
    "(Widerruf) hält Stripe mit Zeitstempel pro Bestellung fest.</p>",
    "<h2>Ereignisse von außen nur mit Echtheitsnachweis</h2>",
    "<p>Von Stream-Starts und anderen Twitch-Ereignissen erfährt der Bot über ",
    "signierte Benachrichtigungen (EventSub). Jede eingehende Nachricht wird ",
    "kryptographisch geprüft — die Signatur wird über Nachrichten-ID, Zeitstempel ",
    "und Inhalt gebildet, was die Prüfung nicht besteht, wird verworfen. Niemand ",
    "kann dem Bot also gefälschte Ereignisse der Art ",
    "<em>Streamer XY ist live</em> unterschieben.</p>",
    "<h2>Sicherheit der KI-Funktionen</h2>",
    "<p>Die Frage-Box auf der Website hat einen mehrschichtigen Schutz gegen ",
    "Prompt-Injection: bekannte Manipulationsmuster werden erkannt, das Modell darf ",
    "nur mit einem festen Fakten-Steckbrief antworten (kein erfundenes Wissen), und ",
    "die Ausgabe wird geprüft, bevor sie angezeigt wird. Automatische Chat-Antworten ",
    "durchlaufen einen Review-Prozess, bevor sie live gehen. Der lernende Spam-Filter ",
    "hat einen eingebauten Fehlbann-Schutz: Er lernt nicht nur Spam-Muster, sondern ",
    "auch ausdrücklich unbedenkliche Muster, und normalisiert Zeichen-Tricks ",
    "(z. B. kyrillische Doppelgänger-Buchstaben), bevor er bewertet.</p>",
    "<h2>Moderation mit Augenmaß</h2>",
    "<p>Automatische Maßnahmen sind bewusst konservativ gebaut: Raid-Ereignisse werden ",
    "über mehrere unabhängige Signale korreliert, bevor reagiert wird; auffällige ",
    "externe Kanäle bekommen erst eine Karenzzeit statt eines Sofort-Banns; und wo der ",
    "Bot keine Rechte hat oder nicht willkommen ist, stellt er das Senden ein, statt ",
    "es immer wieder zu versuchen.</p>",
    "<h2>Überwachung und Reaktion</h2>",
    "<p>Der Betrieb wird laufend überwacht: strukturierte, rotierende Logs (mit ",
    "maskierten Geheimnissen), automatische Alarme an die Betreiber bei kritischen ",
    "Ereignissen, eine stündliche automatische Log-Prüfung und ein tägliches ",
    "Gesamt-Audit. Der Code wird regelmäßig mit statischer Analyse, ",
    "Abhängigkeits-Audits und Container-Scans geprüft.</p>",
    "<h2>Security Testing &amp; Responsible Disclosure</h2>",
    "<p>White-Hat-Testing ist ausdrücklich erlaubt — wer Schwachstellen im System ",
    "sucht und findet, tut uns einen Gefallen. Die gängigen Regeln gelten:</p>",
    "<ul>",
    "<li><strong>Kein Schaden:</strong> Keine Daten verändern, löschen oder ",
    "verschlüsseln. Kein Einschleusen von Backdoors oder Schadsoftware. Betrieb und ",
    "Verfügbarkeit des Systems dürfen nicht beeinträchtigt werden.</li>",
    "<li><strong>Keine Datenexfiltration:</strong> Gefundene fremde Daten ",
    "dokumentieren reicht — nicht herunterladen, nicht weitergeben.</li>",
    "<li><strong>Kein Social Engineering gegen Nutzer:</strong> Tests richten sich ",
    "gegen die Infrastruktur und den Code, nicht gegen Streamer oder ",
    "Community-Mitglieder.</li>",
    "<li><strong>Detaillierter Report Pflicht:</strong> Wir brauchen eine vollständige ",
    "Beschreibung des Angriffswegs — welche Schritte in welcher Reihenfolge, welche ",
    "Requests oder Eingaben, was als Ergebnis sichtbar wurde. Nur ",
    "&#x201E;ich hab was gefunden&#x201C; reicht nicht; ohne reproduzierbaren Weg ",
    "können wir nichts fixen.</li>",
    "<li><strong>Vertraulich melden, nicht veröffentlichen:</strong> Bitte keine ",
    "technischen Details öffentlich posten, bevor das Problem behoben ist.</li>",
    "</ul>",
    "<p>Report direkt an:<br>",
    "&nbsp;&bull;&nbsp;<a href='mailto:mail@earlysalty.com'>mail@earlysalty.com</a><br>",
    "&nbsp;&bull;&nbsp;Discord: <strong>earlysalty</strong></p>",
    "<p>Wer sich an diese Regeln hält, muss keine Konsequenzen befürchten. ",
    "Gezielte Denial-of-Service-Angriffe, das Abgreifen echter Nutzerdaten oder ",
    "das Einschleusen von Backdoors sind ausdrücklich <em>nicht</em> abgedeckt und ",
    "fallen nicht unter diese Freigabe.</p>",
    "<h2 id='melden'>Lücke melden</h2>",
    "<p class='sub'>Nur echte, selbst geprüfte Schwachstellen — keine KI-generierten Hypothesen, ",
    "keine Vermutungen ohne eigenen Nachweis.</p>",
    "<form method='post' action='/twitch/sicherheit/report'>",
    "<div class='fg'><label for='vtitle'>Kurztitel</label>",
    "<input type='text' id='vtitle' name='title' required maxlength='120' ",
    "placeholder='z.B. IDOR auf /twitch/api/streamer-data'></div>",
    "<div class='fg'><label for='vdesc'>Detaillierter Reproduktionsweg (Pflicht)</label>",
    "<textarea id='vdesc' name='description' required rows='9' minlength='100' maxlength='5000' ",
    "placeholder='Schritt f&#x00FC;r Schritt: welche URL, Eingabe oder Request, was sichtbar wurde. ",
    "Muss reproduzierbar sein.'></textarea></div>",
    "<div class='fg'><label for='vcontact'>Discord oder E-Mail (optional, f&#x00FC;r R&#x00FC;ckfragen)</label>",
    "<input type='text' id='vcontact' name='contact' maxlength='100' ",
    "placeholder='discord: deinname / mail@example.com'></div>",
    "<p class='fhint'>Durch Absenden best&#x00E4;tigst du: kein DoS, keine Datenexfiltration, ",
    "kein Social Engineering gegen Nutzer. Nur echte L&#x00FC;cken &#x2014; keine KI-Halluzinationen, ",
    "keine Vermutungen ohne eigene Pr&#x00FC;fung.</p>",
    "<button type='submit'>Report einreichen</button>",
    "</form>"
);

fn default_legal_page_body(slug: &str) -> Option<&'static str> {
    match slug {
        "impressum" => Some(DEFAULT_BODY_IMPRESSUM),
        "agb" => Some(DEFAULT_BODY_AGB),
        "datenschutz" => Some(DEFAULT_BODY_DATENSCHUTZ),
        "sicherheit" => Some(DEFAULT_BODY_SICHERHEIT),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Dokument-Laden (Default + optionaler JSON-Override wie in Python)
// ---------------------------------------------------------------------------

struct LegalDocument {
    title: String,
    body: String,
}

fn legal_pages_storage_path() -> std::path::PathBuf {
    std::env::var("TB_LEGAL_PAGES_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from("data/admin_dashboard/legal_pages.json")
        })
}

fn load_legal_page_document(slug: &str) -> Option<LegalDocument> {
    let title = legal_page_title(slug)?;
    let body = default_legal_page_body(slug)?;
    let mut document = LegalDocument {
        title: title.to_string(),
        body: body.to_string(),
    };

    let Ok(raw) = std::fs::read_to_string(legal_pages_storage_path()) else {
        return Some(document);
    };
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Some(document);
    };
    let Some(entry) = payload.get(slug) else {
        return Some(document);
    };
    if let Some(override_title) = entry.get("title").and_then(|v| v.as_str()) {
        if !override_title.trim().is_empty() {
            document.title = override_title.trim().to_string();
        }
    }
    if let Some(override_body) = entry.get("body").and_then(|v| v.as_str()) {
        if !override_body.trim().is_empty() {
            document.body = override_body.to_string();
        }
    }
    Some(document)
}

// ---------------------------------------------------------------------------
// HTML-Rendering (byte-identisch zu _render_legal_page / _render_legal_gate_page)
// ---------------------------------------------------------------------------

/// Entspricht Pythons `html.escape(s, quote=True)`.
fn escape_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

fn render_legal_page(
    title: &str,
    body: &str,
    footer_links: &[(&str, &str)],
    noindex: bool,
) -> String {
    let footer_html = footer_links
        .iter()
        .map(|(href, label)| {
            format!(
                "<a href='{}'>{}</a>",
                escape_html(href),
                escape_html(label)
            )
        })
        .collect::<Vec<_>>()
        .join(" &nbsp;&middot;&nbsp; ");
    let robots_meta = if noindex {
        "<meta name='robots' content='noindex, nofollow'>"
    } else {
        ""
    };
    format!(
        concat!(
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>",
            "<meta name='viewport' content='width=device-width,initial-scale=1'>",
            "{robots_meta}",
            "<title>{title} · EarlySalty</title>",
            "<style>",
            "body{{margin:0;background:#f8fafc;color:#0f172a;",
            "font-family:Segoe UI,Arial,sans-serif;line-height:1.7;}}",
            ".wrap{{max-width:700px;margin:0 auto;padding:40px 20px 60px;}}",
            "h1{{font-size:1.7rem;margin:0 0 6px;font-weight:800;}}",
            ".back{{font-size:13px;color:#64748b;margin-bottom:24px;display:block;",
            "text-decoration:none;}}",
            ".back:hover{{color:#2563eb;}}",
            "h2{{font-size:1.05rem;margin:26px 0 6px;color:#0f172a;font-weight:700;}}",
            "p,address{{font-size:15px;color:#334155;font-style:normal;margin:0 0 10px;}}",
            "ul{{font-size:15px;color:#334155;margin:0 0 10px;padding-left:22px;}}",
            "li{{margin-bottom:4px;}}",
            "a{{color:#2563eb;text-decoration:none;}}",
            "a:hover{{text-decoration:underline;}}",
            ".sub{{color:#64748b;font-size:14px;margin:0 0 20px;}}",
            ".cryptobox{{background:#0f172a;color:#7dd3fc;font-family:",
            "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12px;",
            "line-height:1.5;padding:14px 16px;border-radius:8px;overflow-x:auto;",
            "white-space:pre-wrap;word-break:break-all;margin:0 0 8px;}}",
            ".cryptohint{{font-size:13px;color:#64748b;margin:0 0 10px;}}",
            ".footer{{margin-top:40px;font-size:12px;color:#94a3b8;",
            "border-top:1px solid #e2e8f0;padding-top:14px;}}",
            ".fg{{margin-bottom:18px;}}",
            "label{{display:block;font-size:14px;font-weight:600;color:#0f172a;margin-bottom:6px;}}",
            "input[type=text],textarea{{width:100%;box-sizing:border-box;border:1px solid #cbd5e1;",
            "border-radius:8px;padding:10px 12px;font-size:14px;font-family:inherit;",
            "color:#0f172a;background:#fff;resize:vertical;}}",
            "input:focus,textarea:focus{{outline:2px solid #2563eb;outline-offset:1px;border-color:transparent;}}",
            ".fhint{{font-size:13px;color:#64748b;margin:0 0 16px;padding:10px 12px;",
            "background:#fffbeb;border-radius:6px;border:1px solid #fcd34d;}}",
            "button[type=submit]{{background:#2563eb;color:#fff;border:none;border-radius:8px;",
            "padding:11px 24px;font-size:15px;font-weight:600;cursor:pointer;}}",
            "button[type=submit]:hover{{background:#1d4ed8;}}",
            ".msg-ok{{padding:16px;background:#f0fdf4;border:1px solid #86efac;",
            "border-radius:8px;color:#166534;font-size:15px;margin-bottom:16px;}}",
            ".msg-err{{padding:16px;background:#fef2f2;border:1px solid #fca5a5;",
            "border-radius:8px;color:#991b1b;font-size:15px;margin-bottom:16px;}}",
            "</style></head><body><div class='wrap'>",
            "<a class='back' href='/twitch/pricing'>&larr; Zurück zu den Plänen</a>",
            "<h1>{title}</h1>",
            "{body}",
            "<div class='footer'>{footer}</div>",
            "</div></body></html>",
        ),
        robots_meta = robots_meta,
        title = escape_html(title),
        body = body,
        footer = footer_html,
    )
}

fn render_legal_gate_page(next_path: &str, site_key: &str) -> String {
    let escaped_next = escape_html(next_path);
    let escaped_site_key = escape_html(site_key);
    format!(
        concat!(
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>",
            "<meta name='viewport' content='width=device-width,initial-scale=1'>",
            "<meta name='robots' content='noindex, nofollow'>",
            "<title>Einen Moment bitte …</title>",
            "<script src='https://challenges.cloudflare.com/turnstile/v0/api.js' async defer></script>",
            "<style>",
            "*{{box-sizing:border-box;margin:0;padding:0}}",
            "body{{display:flex;align-items:center;justify-content:center;",
            "min-height:100vh;background:#f8fafc;font-family:Segoe UI,Arial,sans-serif;}}",
            ".loader{{display:flex;flex-direction:column;align-items:center;gap:18px;}}",
            ".spin{{width:36px;height:36px;border:3px solid #e2e8f0;",
            "border-top-color:#2563eb;border-radius:50%;animation:s .8s linear infinite;}}",
            "@keyframes s{{to{{transform:rotate(360deg)}}}}",
            "p{{font-size:14px;color:#64748b;letter-spacing:.01em;}}",
            ".hint{{font-size:12px;color:#94a3b8;}}",
            "</style></head><body>",
            "<div class='loader'>",
            "<div class='spin'></div>",
            "<p>Einen Moment bitte …</p>",
            "<span class='hint'>Der Server ist gerade etwas langsam.</span>",
            "<form id='lgf' method='post' action='/twitch/legal/verify' style='display:none'>",
            "<input type='hidden' name='next' value='{next}'>",
            "<div class='cf-turnstile' data-sitekey='{site_key}'",
            " data-action='{action}'",
            " data-appearance='interaction-only'",
            " data-callback='_tsOk'></div>",
            "</form>",
            "</div>",
            "<script>function _tsOk(){{document.getElementById('lgf').submit();}}</script>",
            "</body></html>",
        ),
        next = escaped_next,
        site_key = escaped_site_key,
        action = LEGAL_GATE_TURNSTILE_ACTION,
    )
}

// ---------------------------------------------------------------------------
// Gate-Konfiguration und Cookie-Signatur
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct LegalGateConfig {
    site_key: String,
    secret_key: String,
    cookie_secret: String,
}

impl LegalGateConfig {
    /// Liest die drei Gate-Secrets aus der Umgebung (Infisical-Env).
    /// Fallback-Namen identisch zum Python-Loader.
    fn from_env() -> Self {
        fn read(primary: &str, fallback: &str) -> String {
            std::env::var(primary)
                .or_else(|_| std::env::var(fallback))
                .unwrap_or_default()
                .trim()
                .to_string()
        }
        Self {
            site_key: read("TWITCH_LEGAL_TURNSTILE_SITE_KEY", "TURNSTILE_SITE_KEY"),
            secret_key: read("TWITCH_LEGAL_TURNSTILE_SECRET_KEY", "TURNSTILE_SECRET_KEY"),
            cookie_secret: read("TWITCH_LEGAL_GATE_COOKIE_SECRET", "LEGAL_GATE_COOKIE_SECRET"),
        }
    }

    fn is_enabled(&self) -> bool {
        !self.site_key.is_empty() && !self.secret_key.is_empty() && !self.cookie_secret.is_empty()
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn gate_cookie_signature(cookie_secret: &str, expires_raw: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(cookie_secret.as_bytes())
        .expect("HMAC akzeptiert beliebige Key-Längen");
    mac.update(expires_raw.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn gate_cookie_value(cookie_secret: &str, expires_at: u64) -> String {
    let expires_raw = expires_at.to_string();
    let signature = gate_cookie_signature(cookie_secret, &expires_raw);
    format!("{expires_raw}.{signature}")
}

fn gate_cookie_is_valid(config: &LegalGateConfig, headers: &HeaderMap) -> bool {
    if !config.is_enabled() {
        return false;
    }
    let raw_cookie = read_cookie(headers, LEGAL_GATE_COOKIE_NAME).unwrap_or_default();
    let Some((expires_raw, provided_signature)) = raw_cookie.split_once('.') else {
        return false;
    };
    if provided_signature.is_empty() {
        return false;
    }
    let Ok(expires_at) = expires_raw.parse::<u64>() else {
        return false;
    };
    if expires_at <= now_epoch_secs() {
        return false;
    }
    let Ok(provided_bytes) = hex::decode(provided_signature) else {
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(config.cookie_secret.as_bytes())
        .expect("HMAC akzeptiert beliebige Key-Längen");
    mac.update(expires_raw.as_bytes());
    // verify_slice vergleicht in konstanter Zeit (wie hmac.compare_digest).
    mac.verify_slice(&provided_bytes).is_ok()
}

fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let (key, value) = pair.split_once('=')?;
        if key.trim() == name {
            return Some(value.trim().to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Request-Hilfen (UA-Block, next-Pfad, Host, Secure-Erkennung)
// ---------------------------------------------------------------------------

fn is_blocked_legal_page_user_agent(headers: &HeaderMap) -> bool {
    let normalized = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    BLOCKED_LEGAL_PAGE_USER_AGENT_TOKENS
        .iter()
        .any(|token| normalized.contains(token))
        || normalized.contains(BLOCKED_UA_BOT_SLASH)
}

fn blocked_legal_page_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        [("X-Robots-Tag", X_ROBOTS_TAG)],
        "Forbidden",
    )
        .into_response()
}

fn gate_unavailable_response() -> Response {
    tracing::error!(
        "Legal human gate is unavailable: erwartete Secrets \
         TWITCH_LEGAL_TURNSTILE_SITE_KEY, TWITCH_LEGAL_TURNSTILE_SECRET_KEY, \
         TWITCH_LEGAL_GATE_COOKIE_SECRET"
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [("X-Robots-Tag", X_ROBOTS_TAG)],
        "Legal access gate is not configured.",
    )
        .into_response()
}

fn normalize_gate_next_path(raw_path: Option<&str>) -> &'static str {
    let candidate = raw_path.unwrap_or("").trim();
    LEGAL_GATE_ALLOWED_PATHS
        .iter()
        .find(|allowed| **allowed == candidate)
        .copied()
        .unwrap_or("/twitch/impressum")
}

fn gate_redirect_response(next_path: &str) -> Response {
    let encoded: String = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("next", next_path)
        .finish();
    (
        StatusCode::FOUND,
        [(header::LOCATION, format!("/twitch/legal/access?{encoded}"))],
    )
        .into_response()
}

fn request_host(headers: &HeaderMap) -> String {
    let raw_host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if raw_host.is_empty() {
        return String::new();
    }
    // Nur Hostname ohne Port, lowercased — wie urlsplit(...).hostname.
    raw_host
        .rsplit_once(':')
        .map(|(host, port)| {
            if port.chars().all(|c| c.is_ascii_digit()) {
                host
            } else {
                raw_host
            }
        })
        .unwrap_or(raw_host)
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase()
}

fn request_is_secure(headers: &HeaderMap) -> bool {
    headers
        .get("X-Forwarded-Proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

fn set_gate_cookie_header(headers: &HeaderMap, cookie_secret: &str) -> String {
    let value = gate_cookie_value(
        cookie_secret,
        now_epoch_secs() + LEGAL_GATE_COOKIE_TTL_SECONDS,
    );
    let mut cookie = format!(
        "{LEGAL_GATE_COOKIE_NAME}={value}; HttpOnly; Max-Age={LEGAL_GATE_COOKIE_TTL_SECONDS}; \
         Path=/twitch/; SameSite=Lax"
    );
    if request_is_secure(headers) {
        cookie.push_str("; Secure");
    }
    cookie
}

// ---------------------------------------------------------------------------
// Turnstile-Verifikation
// ---------------------------------------------------------------------------

async fn verify_turnstile_token(
    config: &LegalGateConfig,
    headers: &HeaderMap,
    token: &str,
) -> bool {
    let normalized_token = token.trim();
    if normalized_token.is_empty() || config.secret_key.is_empty() {
        tracing::warn!(
            token_empty = normalized_token.is_empty(),
            secret_empty = config.secret_key.is_empty(),
            "legal_verify: token oder secret fehlt"
        );
        return false;
    }

    let mut form: Vec<(&str, String)> = vec![
        ("secret", config.secret_key.clone()),
        ("response", normalized_token.to_string()),
    ];
    if let Some(remote_ip) = headers
        .get("CF-Connecting-IP")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        form.push(("remoteip", remote_ip.to_string()));
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "legal_verify: HTTP-Client-Aufbau fehlgeschlagen");
            return false;
        }
    };
    let result: serde_json::Value = match client
        .post(LEGAL_GATE_TURNSTILE_VERIFY_URL)
        .form(&form)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    {
        Ok(response) => match response.json().await {
            Ok(json) => json,
            Err(error) => {
                tracing::warn!(%error, "legal_verify: siteverify JSON unlesbar");
                return false;
            }
        },
        Err(error) => {
            tracing::warn!(%error, "legal_verify: siteverify request failed");
            return false;
        }
    };

    if !result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        tracing::warn!(
            error_codes = ?result.get("error-codes"),
            "legal_verify: siteverify success=false"
        );
        return false;
    }
    let action = result
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if action != LEGAL_GATE_TURNSTILE_ACTION {
        tracing::warn!(
            got = action,
            expected = LEGAL_GATE_TURNSTILE_ACTION,
            "legal_verify: action mismatch"
        );
        return false;
    }
    let hostname = result
        .get("hostname")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if hostname.is_empty() {
        tracing::warn!("legal_verify: hostname fehlt in siteverify-Antwort");
        return false;
    }
    let expected_host = request_host(headers);
    if hostname != expected_host {
        tracing::warn!(
            cf = hostname,
            request = expected_host,
            "legal_verify: hostname mismatch"
        );
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// GET /robots.txt — Legal-Seiten für Crawler sperren (Sicherheitskonzept bleibt frei).
pub async fn robots_txt_handler() -> Response {
    let robots = concat!(
        "User-agent: *\n",
        "Disallow: /twitch/impressum\n",
        "Disallow: /twitch/datenschutz\n",
        "Disallow: /twitch/agb\n",
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        robots,
    )
        .into_response()
}

/// GET /twitch/legal/access — Turnstile-Zwischenseite.
pub async fn legal_access_handler(
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if is_blocked_legal_page_user_agent(&headers) {
        return blocked_legal_page_response();
    }
    let next_path = normalize_gate_next_path(query.get("next").map(String::as_str));
    let config = LegalGateConfig::from_env();
    if !config.is_enabled() {
        return gate_unavailable_response();
    }
    if gate_cookie_is_valid(&config, &headers) {
        return (StatusCode::FOUND, [(header::LOCATION, next_path)]).into_response();
    }
    let page = render_legal_gate_page(next_path, &config.site_key);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE.as_str(), "text/html; charset=utf-8"),
            ("X-Robots-Tag", X_ROBOTS_TAG),
        ],
        page,
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct LegalVerifyForm {
    #[serde(default)]
    next: String,
    #[serde(rename = "cf-turnstile-response", default)]
    turnstile_response: String,
}

/// POST /twitch/legal/verify — Turnstile-Token prüfen, Gate-Cookie setzen.
pub async fn legal_verify_handler(
    headers: HeaderMap,
    Form(form): Form<LegalVerifyForm>,
) -> Response {
    if is_blocked_legal_page_user_agent(&headers) {
        return blocked_legal_page_response();
    }
    let next_path = normalize_gate_next_path(Some(&form.next));
    let config = LegalGateConfig::from_env();
    if !config.is_enabled() {
        return gate_unavailable_response();
    }
    if !verify_turnstile_token(&config, &headers, &form.turnstile_response).await {
        return (
            StatusCode::FORBIDDEN,
            [("X-Robots-Tag", X_ROBOTS_TAG)],
            "Turnstile verification failed.",
        )
            .into_response();
    }
    let cookie = set_gate_cookie_header(&headers, &config.cookie_secret);
    (
        StatusCode::FOUND,
        [
            (header::LOCATION.as_str(), next_path.to_string()),
            (header::SET_COOKIE.as_str(), cookie),
        ],
    )
        .into_response()
}

fn gated_legal_page_response(
    headers: &HeaderMap,
    request_path: &str,
    slug: &str,
    footer_links: &[(&str, &str)],
) -> Response {
    if is_blocked_legal_page_user_agent(headers) {
        return blocked_legal_page_response();
    }
    let config = LegalGateConfig::from_env();
    if !config.is_enabled() {
        return gate_unavailable_response();
    }
    if !gate_cookie_is_valid(&config, headers) {
        return gate_redirect_response(normalize_gate_next_path(Some(request_path)));
    }
    let Some(document) = load_legal_page_document(slug) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let page = render_legal_page(&document.title, &document.body, footer_links, true);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE.as_str(), "text/html; charset=utf-8"),
            ("X-Robots-Tag", X_ROBOTS_TAG),
        ],
        page,
    )
        .into_response()
}

/// GET /twitch/impressum — §5 DDG hinter dem Legal-Human-Gate.
pub async fn impressum_handler(headers: HeaderMap) -> Response {
    gated_legal_page_response(
        &headers,
        "/twitch/impressum",
        "impressum",
        &[
            ("/twitch/abbo", "Pläne"),
            ("/twitch/datenschutz", "Datenschutz"),
            ("/twitch/agb", "AGB"),
            ("/twitch/sicherheit", "Sicherheit"),
        ],
    )
}

/// GET /twitch/datenschutz — DSGVO Art. 13/14 hinter dem Legal-Human-Gate.
pub async fn datenschutz_handler(headers: HeaderMap) -> Response {
    gated_legal_page_response(
        &headers,
        "/twitch/datenschutz",
        "datenschutz",
        &[
            ("/twitch/abbo", "Pläne"),
            ("/twitch/impressum", "Impressum"),
            ("/twitch/agb", "AGB"),
            ("/twitch/sicherheit", "Sicherheit"),
        ],
    )
}

/// GET /twitch/agb — AGB hinter dem Legal-Human-Gate.
pub async fn agb_handler(headers: HeaderMap) -> Response {
    gated_legal_page_response(
        &headers,
        "/twitch/agb",
        "agb",
        &[
            ("/twitch/pricing", "Pläne"),
            ("/twitch/impressum", "Impressum"),
            ("/twitch/datenschutz", "Datenschutz"),
            ("/twitch/sicherheit", "Sicherheit"),
        ],
    )
}

/// GET /twitch/sicherheit — öffentliches Sicherheitskonzept, bewusst UNgegated
/// und ohne noindex (soll gelesen und gefunden werden).
pub async fn sicherheit_handler() -> Response {
    let Some(document) = load_legal_page_document("sicherheit") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let page = render_legal_page(
        &document.title,
        &document.body,
        &[
            ("/twitch/impressum", "Impressum"),
            ("/twitch/datenschutz", "Datenschutz"),
            ("/twitch/agb", "AGB"),
        ],
        false,
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        page,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Security-Report: Formular-Verarbeitung + Discord-DM
// ---------------------------------------------------------------------------

/// Discord User-ID des Bot-Eigentümers (earlysalty).
const DISCORD_OWNER_USER_ID: &str = "662995601738170389";

#[derive(Deserialize)]
pub struct SecurityReportForm {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    contact: String,
}

async fn send_security_report_dm(
    title: &str,
    description: &str,
    contact: &str,
) -> Result<(), String> {
    let token = std::env::var("DISCORD_TOKEN")
        .map_err(|_| "DISCORD_TOKEN nicht konfiguriert".to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let auth = format!("Bot {token}");

    // DM-Kanal öffnen
    let dm_resp: serde_json::Value = client
        .post("https://discord.com/api/v10/users/@me/channels")
        .header("Authorization", &auth)
        .json(&serde_json::json!({"recipient_id": DISCORD_OWNER_USER_ID}))
        .send()
        .await
        .map_err(|e| format!("DM-Kanal request: {e}"))?
        .json()
        .await
        .map_err(|e| format!("DM-Kanal JSON: {e}"))?;

    let channel_id = dm_resp["id"]
        .as_str()
        .ok_or_else(|| format!("Kein channel.id in Discord-Antwort: {dm_resp}"))?;

    let contact_line = if contact.is_empty() { "—" } else { contact };
    let message = format!(
        "🔒 **Security Report**\n\n**Titel:** {title}\n**Kontakt:** {contact_line}\n\n**Report:**\n{description}"
    );
    let message = &message[..message.len().min(2000)];

    let send_resp = client
        .post(format!(
            "https://discord.com/api/v10/channels/{channel_id}/messages"
        ))
        .header("Authorization", &auth)
        .json(&serde_json::json!({"content": message}))
        .send()
        .await
        .map_err(|e| format!("DM-Send request: {e}"))?;

    if send_resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Discord API {}", send_resp.status()))
    }
}

/// Blockierender Teil der Opus-Analyse — läuft in spawn_blocking.
fn run_opus_analysis_blocking(
    title: String,
    description: String,
    contact: String,
) -> Result<String, String> {
    let contact_line = if contact.is_empty() {
        "—".to_string()
    } else {
        contact
    };
    let prompt = format!(
        "Du bist ein Security-Analyst für das Deadlock-Twitch-Bot-Repo unter \
         /home/naniadm/Documents/Deadlock-Twitch-Bot.\n\n\
         Ein externer White-Hat hat folgende Schwachstelle gemeldet:\n\
         Titel: {title}\n\
         Kontakt: {contact_line}\n\
         Report:\n{description}\n\n\
         Aufgaben:\n\
         1. Lies den relevanten Code im Repo und prüfe, ob die beschriebene Lücke \
         real ist.\n\
         2. Bewerte: echte Lücke oder Fehlalarm? Begründe ausführlich anhand \
         konkreter Stellen im Code.\n\
         3. Falls echte Lücke: schätze Schweregrad (Low / Medium / High / Critical) \
         und erkläre das Angriffsszenario.\n\
         4. Falls die Lücke sicher fixbar ist (kein risikoreicher Eingriff): \
         repariere den Bug und erstelle einen Commit (kein Push).\n\
         5. Fasse alles für eine Discord-DM zusammen — maximal 1800 Zeichen, \
         klar strukturiert mit Bewertung, Begründung und Fix-Status.\n\
         Antworte NUR mit dem DM-Text, keine weitere Präambel.",
    );

    let output = std::process::Command::new("/home/naniadm/.local/bin/claude")
        .args(["-p", "--model", "opus", "--dangerously-skip-permissions"])
        .arg(&prompt)
        .current_dir("/home/naniadm/Documents/Deadlock-Twitch-Bot")
        .output()
        .map_err(|e| format!("claude CLI start: {e}"))?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Err("Opus hat keinen Output geliefert".into())
        } else {
            Ok(text)
        }
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Sendet die Opus-Analyse als Discord-DM nach Abschluss.
async fn send_analysis_dm(token: &str, analysis: &str) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Opus-DM client: {e}");
            return;
        }
    };
    let auth = format!("Bot {token}");
    let send_result = client
        .post("https://discord.com/api/v10/users/@me/channels")
        .header("Authorization", &auth)
        .json(&serde_json::json!({"recipient_id": DISCORD_OWNER_USER_ID}))
        .send()
        .await;
    let dm_resp: serde_json::Value = match send_result {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Opus-DM kanal JSON: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::warn!("Opus-DM kanal: {e}");
            return;
        }
    };
    let Some(channel_id) = dm_resp["id"].as_str() else {
        tracing::warn!("Opus-DM: kein channel_id");
        return;
    };
    let header_line = "🔍 **Opus Security-Analyse**\n\n";
    let max_body = 2000 - header_line.len();
    let body: String = analysis.chars().take(max_body).collect();
    let content = format!("{header_line}{body}");
    if let Err(e) = client
        .post(format!(
            "https://discord.com/api/v10/channels/{channel_id}/messages"
        ))
        .header("Authorization", &auth)
        .json(&serde_json::json!({"content": content}))
        .send()
        .await
    {
        tracing::warn!("Opus-DM senden: {e}");
    }
}

/// Startet die Opus-Analyse als Hintergrundtask — blockiert die HTTP-Response nicht.
fn spawn_opus_analysis(title: String, description: String, contact: String, token: String) {
    tokio::spawn(async move {
        let t = title.clone();
        let result =
            tokio::task::spawn_blocking(move || run_opus_analysis_blocking(title, description, contact))
                .await;
        match result {
            Ok(Ok(analysis)) => {
                tracing::info!(title = %t, "Opus-Analyse abgeschlossen, sende DM");
                send_analysis_dm(&token, &analysis).await;
            }
            Ok(Err(e)) => tracing::warn!(title = %t, error = %e, "Opus-Analyse fehlgeschlagen"),
            Err(e) => tracing::warn!(title = %t, error = %e, "spawn_blocking panic"),
        }
    });
}

fn render_report_result(ok: bool, msg: &str) -> Response {
    let (class, heading) = if ok {
        ("msg-ok", "Report eingegangen")
    } else {
        ("msg-err", "Fehler")
    };
    let body = format!(
        "<h2>{heading}</h2><div class='{class}'>{}</div>\
         <p><a href='/twitch/sicherheit'>&#x2190; Zur&#x00FC;ck</a></p>",
        escape_html(msg)
    );
    let page = render_legal_page(
        "Security Report",
        &body,
        &[
            ("/twitch/impressum", "Impressum"),
            ("/twitch/datenschutz", "Datenschutz"),
            ("/twitch/agb", "AGB"),
        ],
        false,
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        page,
    )
        .into_response()
}

/// POST /twitch/sicherheit/report — Security-Report-Formular verarbeiten.
pub async fn security_report_handler(Form(form): Form<SecurityReportForm>) -> Response {
    let title: String = form.title.trim().chars().take(120).collect();
    let description: String = form.description.trim().chars().take(5000).collect();
    let contact: String = form.contact.trim().chars().take(100).collect();

    if title.is_empty() || description.len() < 100 {
        return render_report_result(
            false,
            "Kurztitel und ein ausführlicher Reproduktionsweg (mind. 100 Zeichen) sind Pflicht.",
        );
    }

    let discord_token = std::env::var("DISCORD_TOKEN").unwrap_or_default();

    match send_security_report_dm(&title, &description, &contact).await {
        Ok(()) => {
            tracing::info!(title = %title, "Security Report eingegangen, starte Opus-Analyse");
            // Opus analysiert und fixt ggf. den Bug im Hintergrund — non-blocking.
            if !discord_token.is_empty() {
                spawn_opus_analysis(title, description, contact, discord_token);
            }
            render_report_result(
                true,
                "Report eingegangen — danke. Wir schauen uns das an und melden uns bei Rückfragen.",
            )
        }
        Err(e) => {
            tracing::warn!(error = %e, "Security Report DM fehlgeschlagen");
            render_report_result(
                false,
                "Technischer Fehler beim Weiterleiten — schreib bitte direkt an \
                 mail@earlysalty.com oder Discord: earlysalty.",
            )
        }
    }
}

/// Router für alle Legal-Routen (statuslos, kein DB-Pool nötig).
pub fn build_legal_router() -> Router {
    Router::new()
        .route("/robots.txt", get(robots_txt_handler))
        .route("/twitch/legal/access", get(legal_access_handler))
        .route("/twitch/legal/verify", post(legal_verify_handler))
        .route("/twitch/impressum", get(impressum_handler))
        .route("/twitch/datenschutz", get(datenschutz_handler))
        .route("/twitch/agb", get(agb_handler))
        .route("/twitch/sicherheit", get(sicherheit_handler))
        .route("/twitch/sicherheit/report", post(security_report_handler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_roundtrip_ist_gueltig() {
        let config = LegalGateConfig {
            site_key: "site".into(),
            secret_key: "secret".into(),
            cookie_secret: "cookie-secret".into(),
        };
        let value = gate_cookie_value(&config.cookie_secret, now_epoch_secs() + 600);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{LEGAL_GATE_COOKIE_NAME}={value}").parse().unwrap(),
        );
        assert!(gate_cookie_is_valid(&config, &headers));
    }

    #[test]
    fn abgelaufenes_cookie_ist_ungueltig() {
        let config = LegalGateConfig {
            site_key: "site".into(),
            secret_key: "secret".into(),
            cookie_secret: "cookie-secret".into(),
        };
        let value = gate_cookie_value(&config.cookie_secret, now_epoch_secs() - 1);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{LEGAL_GATE_COOKIE_NAME}={value}").parse().unwrap(),
        );
        assert!(!gate_cookie_is_valid(&config, &headers));
    }

    #[test]
    fn manipulierte_signatur_ist_ungueltig() {
        let config = LegalGateConfig {
            site_key: "site".into(),
            secret_key: "secret".into(),
            cookie_secret: "cookie-secret".into(),
        };
        let expires = (now_epoch_secs() + 600).to_string();
        let forged = format!("{expires}.{}", "0".repeat(64));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{LEGAL_GATE_COOKIE_NAME}={forged}").parse().unwrap(),
        );
        assert!(!gate_cookie_is_valid(&config, &headers));
    }

    #[test]
    fn next_pfad_wird_auf_allowlist_normalisiert() {
        assert_eq!(normalize_gate_next_path(Some("/twitch/agb")), "/twitch/agb");
        assert_eq!(
            normalize_gate_next_path(Some("https://evil.example/")),
            "/twitch/impressum"
        );
        assert_eq!(normalize_gate_next_path(None), "/twitch/impressum");
    }

    #[test]
    fn ua_blockliste_greift() {
        let mut headers = HeaderMap::new();
        headers.insert(header::USER_AGENT, "Mozilla/5.0 GPTBot/1.0".parse().unwrap());
        assert!(is_blocked_legal_page_user_agent(&headers));
        let mut ok = HeaderMap::new();
        ok.insert(header::USER_AGENT, "Mozilla/5.0 Firefox/127.0".parse().unwrap());
        assert!(!is_blocked_legal_page_user_agent(&ok));
    }

    #[test]
    fn escape_html_entspricht_python() {
        assert_eq!(
            escape_html("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&#x27;f"
        );
    }

    #[test]
    fn defaults_fuer_alle_slugs_vorhanden() {
        for slug in ["impressum", "datenschutz", "agb", "sicherheit"] {
            let doc = load_legal_page_document(slug).expect("Dokument vorhanden");
            assert!(!doc.title.is_empty());
            assert!(doc.body.contains("<h2>"));
        }
        assert!(load_legal_page_document("unbekannt").is_none());
    }

    /// Dev-Werkzeug für den Live-Diff gegen Python: schreibt alle gerenderten
    /// Seiten nach /tmp/rust_legal_<slug>.html. Gegenstück auf Python-Seite:
    /// `_DashboardLegalMixin._render_legal_page` mit identischen Footer-Links.
    /// Ausführen mit `cargo test -p tb-dashboard-api -- --ignored --nocapture`.
    /// Ein Render-Fall: Slug, Footer-Links (Pfad → Label), Impressum-Flag.
    type LegalPageCase = (&'static str, &'static [(&'static str, &'static str)], bool);

    #[test]
    #[ignore]
    fn dump_rendered_pages_fuer_live_diff() {
        let cases: [LegalPageCase; 4] = [
            (
                "impressum",
                &[
                    ("/twitch/abbo", "Pläne"),
                    ("/twitch/datenschutz", "Datenschutz"),
                    ("/twitch/agb", "AGB"),
                    ("/twitch/sicherheit", "Sicherheit"),
                ],
                true,
            ),
            (
                "datenschutz",
                &[
                    ("/twitch/abbo", "Pläne"),
                    ("/twitch/impressum", "Impressum"),
                    ("/twitch/agb", "AGB"),
                    ("/twitch/sicherheit", "Sicherheit"),
                ],
                true,
            ),
            (
                "agb",
                &[
                    ("/twitch/pricing", "Pläne"),
                    ("/twitch/impressum", "Impressum"),
                    ("/twitch/datenschutz", "Datenschutz"),
                    ("/twitch/sicherheit", "Sicherheit"),
                ],
                true,
            ),
            (
                "sicherheit",
                &[
                    ("/twitch/impressum", "Impressum"),
                    ("/twitch/datenschutz", "Datenschutz"),
                    ("/twitch/agb", "AGB"),
                ],
                false,
            ),
        ];
        for (slug, footer, noindex) in cases {
            let doc = load_legal_page_document(slug).unwrap();
            let page = render_legal_page(&doc.title, &doc.body, footer, noindex);
            std::fs::write(format!("/tmp/rust_legal_{slug}.html"), page).unwrap();
        }
        let gate = render_legal_gate_page("/twitch/agb", "DIFF-SITE-KEY");
        std::fs::write("/tmp/rust_legal_gate.html", gate).unwrap();
    }

    #[test]
    fn sicherheit_render_ist_indexierbar_agb_nicht() {
        let doc = load_legal_page_document("sicherheit").unwrap();
        let public = render_legal_page(&doc.title, &doc.body, &[], false);
        assert!(!public.contains("noindex"));
        let gated = render_legal_page("AGB", "<p>x</p>", &[], true);
        assert!(gated.contains("<meta name='robots' content='noindex, nofollow'>"));
    }
}
