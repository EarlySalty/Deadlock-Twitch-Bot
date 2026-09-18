use super::super::community::matching::Schedule;
use super::*;
use axum::response::Html;
use chrono::Timelike;
use std::fmt::Write;

pub(super) fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn document(
    title: &str,
    description: &str,
    canonical: Option<&str>,
    body: &str,
    accent: &str,
) -> String {
    let metadata = canonical.map(|url| format!(r#"<link rel="canonical" href="{}"><meta property="og:url" content="{}"><meta property="og:type" content="profile"><meta property="og:title" content="{}"><meta property="og:description" content="{}">"#,escape(url),escape(url),escape(title),escape(description))).unwrap_or_else(|| "<meta name=\"robots\" content=\"noindex\">".into());
    format!(
        r#"<!doctype html><html lang="de"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><meta name="description" content="{}">{}<link rel="stylesheet" href="/twitch/profile-assets/profile.css"></head><body class="{}"><header class="site-head"><a href="/" class="brand">DDC<span>Deutsche Deadlock Community</span></a><nav aria-label="Community"><a href="/streamer#partner">Partner entdecken</a><a href="/twitch/verwaltung#profil">Mein Profil</a></nav></header><main>{}</main><footer><a href="/streamer">Teil des Partnernetzwerks werden</a><span><a href="/twitch/impressum">Impressum</a> · <a href="/twitch/datenschutz">Datenschutz</a></span></footer></body></html>"#,
        escape(title),
        escape(description),
        metadata,
        accent,
        body
    )
}
fn response(status: StatusCode, html: String) -> Response {
    let mut response = no_store((status, Html(html)).into_response());
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, "default-src 'none'; style-src 'self' 'unsafe-inline'; img-src https://static-cdn.jtvnw.net; base-uri 'none'; form-action 'self'; frame-ancestors 'none'".parse().unwrap());
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    if status != StatusCode::OK {
        response
            .headers_mut()
            .insert("x-robots-tag", "noindex, nofollow".parse().unwrap());
    }
    response
}
pub(super) fn missing(status: StatusCode) -> Response {
    let (title, text) = match status {
        StatusCode::SERVICE_UNAVAILABLE => (
            "Gerade nicht erreichbar",
            "Die Seite konnte gerade nicht geladen werden. Bitte versuche es erneut.",
        ),
        StatusCode::BAD_REQUEST => (
            "Ungültiger Kalendermonat",
            "Bitte wähle einen Monat zwischen 2000 und 2100.",
        ),
        _ => (
            "Dieses Profil ist nicht öffentlich",
            "Hier ist derzeit kein veröffentlichtes Profil eines aktiven Partners erreichbar.",
        ),
    };
    response(status,document(title,text,None,&format!("<section class=\"empty\"><p class=\"eyebrow\">Partnernetzwerk</p><h1>{title}</h1><p>{text}</p><a class=\"button\" href=\"/streamer#partner\">Andere Partner entdecken</a></section>"),"gold"))
}
fn timestamp(at: DateTime<Utc>) -> String {
    at.with_timezone(&Berlin)
        .format("%d.%m.%Y · %H:%M")
        .to_string()
}
fn event_card(event: &Event) -> String {
    format!("<article class=\"event-card\"><span class=\"eyebrow\">Geplant</span><h3>{}</h3><p><time datetime=\"{}\">{}</time> bis <time datetime=\"{}\">{}</time></p><p class=\"multiline\">{}</p></article>",escape(&event.title),event.starts_at.to_rfc3339(),timestamp(event.starts_at),event.ends_at.to_rfc3339(),timestamp(event.ends_at),escape(&event.description))
}
pub(super) fn page(
    record: &Record,
    observed: &Schedule,
    monthly: &[Session],
    directory: &[DirectoryEntry],
    month: NaiveDate,
    now: DateTime<Utc>,
    truncated: bool,
) -> Response {
    let p = &record.content.0;
    let login = escape(&record.login);
    let headline = if p.headline.is_empty() {
        "Ein Gesicht aus unserem Partnernetzwerk"
    } else {
        &p.headline
    };
    let accent = match p.accent {
        Accent::Gold => "gold",
        Accent::Violet => "violet",
        Accent::Teal => "teal",
    };
    let image = if !p.avatar_url.is_empty() && safe_url(&p.avatar_url) {
        format!("<img class=\"avatar\" src=\"{}\" alt=\"Profilbild von {}\" width=\"112\" height=\"112\">",escape(&p.avatar_url),login)
    } else {
        format!(
            "<div class=\"avatar initial\" aria-hidden=\"true\">{}</div>",
            login.chars().next().unwrap_or('?').to_ascii_uppercase()
        )
    };
    let live = if is_live(record, now) {
        format!(
            "<span class=\"live\">● Live auf Twitch{}</span>",
            record
                .last_game
                .as_ref()
                .map(|g| format!(" · {}", escape(g)))
                .unwrap_or_default()
        )
    } else {
        "<span class=\"badge\">DDC-Partner</span>".into()
    };
    let mut body = format!(
        r#"<a class="back" href="/streamer#partner">← Partnernetzwerk</a><section class="hero">{image}<div class="hero-copy">{live}<p class="eyebrow">@{login}</p><h1>{}</h1><p>Hier findest du mich, meine nächsten Streams und Menschen aus meinem Umfeld.</p><a class="button" href="https://www.twitch.tv/{login}" rel="noopener noreferrer" target="_blank">Auf Twitch vorbeischauen ↗</a></div></section><nav class="socials" aria-label="Social-Links">"#,
        escape(headline)
    );
    for social in &p.socials {
        if safe_url(&social.url) {
            let _ = write!(
                body,
                "<a href=\"{}\" target=\"_blank\" rel=\"me noopener noreferrer ugc\">{} ↗</a>",
                escape(&social.url),
                escape(&social.label)
            );
        }
    }
    body.push_str("</nav><div class=\"intro-grid\"><section class=\"panel\"><p class=\"eyebrow\">Das bin ich</p><h2>Über mich</h2>");
    if p.about.is_empty() {
        body.push_str("<p>Mein Stream erzählt den Rest. Schau gerne vorbei!</p>");
    } else {
        let _ = write!(body, "<p class=\"multiline\">{}</p>", escape(&p.about));
    }
    body.push_str("</section><section class=\"panel\"><p class=\"eyebrow\">Wir sehen uns</p><h2>Demnächst geplant</h2>");
    let upcoming: Vec<_> = p
        .events
        .iter()
        .filter(|e| e.ends_at > now)
        .take(3)
        .collect();
    if upcoming.is_empty() {
        body.push_str("<p>Noch keine kommenden Termine eingetragen. Spontane Streams sind trotzdem möglich.</p>");
    }
    for event in upcoming {
        body.push_str(&event_card(event));
    }
    body.push_str("<p class=\"hint\">Termine trägt der Streamer selbst ein. Änderungen und spontane Streams sind möglich. Alle Zeiten: Europe/Berlin.</p></section></div>");
    let next = month.checked_add_months(chrono::Months::new(1)).unwrap();
    let previous = month.checked_sub_months(chrono::Months::new(1)).unwrap();
    let month_names = [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ];
    let _ = write!(body,"<section class=\"panel calendar-section\" id=\"kalender\"><div class=\"section-head\"><div><p class=\"eyebrow\">Mein Streamkalender</p><h2>{} {}</h2></div><nav class=\"month-nav\" aria-label=\"Kalendermonat\">",month_names[month.month0() as usize],month.year());
    if previous.year() >= 2000 {
        let _ = write!(
            body,
            "<a href=\"?month={}#kalender\" aria-label=\"Vorheriger Monat\">←</a>",
            previous.format("%Y-%m")
        );
    }
    body.push_str("<a href=\"?month=");
    body.push_str(&now.with_timezone(&Berlin).format("%Y-%m").to_string());
    body.push_str("#kalender\">Heute</a>");
    if next.year() <= 2100 {
        let _ = write!(
            body,
            "<a href=\"?month={}#kalender\" aria-label=\"Nächster Monat\">→</a>",
            next.format("%Y-%m")
        );
    }
    body.push_str("</nav></div><p class=\"legend\"><span class=\"planned-key\">● Geplant</span>");
    if p.show_history {
        body.push_str("<span class=\"observed-key\">● Tatsächlich live gewesen</span>");
    }
    body.push_str("<span>Europe/Berlin</span></p><div class=\"calendar-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Monatskalender\"><div class=\"calendar\">");
    for day in ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"] {
        let _ = write!(body, "<div class=\"weekday\">{day}</div>");
    }
    for _ in 0..month.weekday().num_days_from_monday() {
        body.push_str("<div class=\"outside\" aria-hidden=\"true\"></div>");
    }
    let mut day = month;
    while day < next {
        let start = Berlin
            .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let end = Berlin
            .from_local_datetime(&day.succ_opt().unwrap().and_hms_opt(0, 0, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let today = if day == now.with_timezone(&Berlin).date_naive() {
            " today"
        } else {
            ""
        };
        let _ = write!(
            body,
            "<div class=\"day{today}\"><time class=\"date\" datetime=\"{day}\">{}</time>",
            day.day()
        );
        for event in p
            .events
            .iter()
            .filter(|e| e.starts_at < end && e.ends_at > start)
        {
            let label = format!(
                "{} · {} bis {}",
                event.title,
                timestamp(event.starts_at),
                timestamp(event.ends_at)
            );
            let _=write!(body,"<div class=\"calendar-event planned\" title=\"{}\"><span>{:02}:{:02} · Geplant</span><strong>{}</strong></div>",escape(&label),event.starts_at.max(start).with_timezone(&Berlin).hour(),event.starts_at.max(start).with_timezone(&Berlin).minute(),escape(&event.title));
        }
        for session in monthly
            .iter()
            .filter(|s| s.started_at < end && s.ended_at > start)
        {
            let game = session.game_name.as_deref().unwrap_or("Stream");
            let label = format!(
                "Tatsächlich live: {} bis {} · {}",
                timestamp(session.started_at),
                timestamp(session.ended_at),
                game
            );
            let _=write!(body,"<div class=\"calendar-event observed\" title=\"{}\"><span>{:02}:{:02} · War live</span><strong>{}</strong></div>",escape(&label),session.started_at.max(start).with_timezone(&Berlin).hour(),session.started_at.max(start).with_timezone(&Berlin).minute(),escape(game));
        }
        body.push_str("</div>");
        day = day.succ_opt().unwrap();
    }
    body.push_str("</div></div><p class=\"hint\">Vergangene Livestreams stammen aus unserer Erfassung, nicht aus den Kalendereinträgen. Fehlende Einträge bedeuten nicht sicher, dass es keinen Stream gab. Laufende Streams erscheinen nach ihrem Ende in der Historie.</p></section>");
    if p.show_history {
        let _=write!(body,"<section class=\"panel\"><p class=\"eyebrow\">Mein bisheriger Rhythmus</p><h2>Wann war ich live?</h2><p>{} erfasste Streams in den letzten 90 Tagen. Neuere Streams zählen stärker. Kein verbindlicher Sendeplan.</p><div class=\"heatmap-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Historische Livezeiten in Berliner Zeit\"><div class=\"hours\"><span>00 Uhr</span><span>06 Uhr</span><span>12 Uhr</span><span>18 Uhr</span><span>24 Uhr</span></div>",observed.sessions);
        for (weekday, label) in ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"]
            .iter()
            .enumerate()
        {
            let _ = write!(body, "<div class=\"heat-row\"><span>{label}</span>");
            for slot in 0..48 {
                let value = observed.slots[weekday * 48 + slot];
                let label = format!(
                    "{label} {:02}:{:02} · {:.0} % gewichtete Live-Häufigkeit",
                    slot / 2,
                    (slot % 2) * 30,
                    value * 100.0
                );
                let _=write!(body,"<span class=\"heat-slot\" role=\"img\" title=\"{label}\" aria-label=\"{label}\" style=\"opacity:{:.3}\"></span>",0.08+value*0.92);
            }
            body.push_str("</div>");
        }
        body.push_str("</div><p class=\"hint\">Je Kästchen 30 Minuten. Sommer- und Winterzeit werden in Europe/Berlin berücksichtigt.</p></section>");
    }
    if truncated {
        body.push_str("<p class=\"hint\">Die Darstellung ist auf die neuesten 2001 erfassten Streams je Zeitraum begrenzt.</p>");
    }
    let featured: Vec<_> = p
        .featured
        .iter()
        .filter_map(|login| {
            directory
                .iter()
                .find(|entry| entry.login == *login && entry.login != record.login)
        })
        .collect();
    if !featured.is_empty() {
        body.push_str("<section class=\"panel\"><p class=\"eyebrow\">Aus meinem Umfeld</p><h2>Schau auch hier vorbei</h2><div class=\"partner-grid\">");
        for entry in featured {
            let _=write!(body,"<a class=\"partner-card\" href=\"/streamer/@{}\"><strong>@{} ↗</strong><span>{}</span></a>",escape(&entry.login),escape(&entry.login),escape(&entry.headline));
        }
        body.push_str("</div></section>");
    }
    body.push_str("<aside class=\"network-cta\"><h2>Dein nächster Lieblingsstream wartet schon.</h2><a class=\"button\" href=\"/streamer#partner\">Mehr Partner entdecken →</a></aside>");
    response(
        StatusCode::OK,
        document(
            &format!("@{} · DDC-Partner", record.login),
            headline,
            Some(&format!(
                "https://deutsche-deadlock-community.de/streamer/@{}",
                record.login
            )),
            &body,
            accent,
        ),
    )
}
