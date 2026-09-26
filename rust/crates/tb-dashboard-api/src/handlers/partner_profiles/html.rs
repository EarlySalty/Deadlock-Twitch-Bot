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
    let metadata = canonical
        .map(|url| {
            format!(
                r#"<link rel="canonical" href="{}"><meta property="og:url" content="{}"><meta property="og:type" content="profile"><meta property="og:title" content="{}"><meta property="og:description" content="{}">"#,
                escape(url),
                escape(url),
                escape(title),
                escape(description)
            )
        })
        .unwrap_or_else(|| "<meta name=\"robots\" content=\"noindex\">".into());

    format!(
        r#"<!doctype html><html lang="de"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><meta name="description" content="{}">{}<link rel="stylesheet" href="/twitch/profile-assets/profile.css"></head><body class="{}"><header class="site-head"><a href="/" class="brand" aria-label="Deutsche Deadlock Community"><img class="brand-mark" src="/streamer/brand/deadlock-d-logo.png" alt="" width="38" height="38"><span class="brand-copy"><strong>DDC</strong><span>Deutsche Deadlock Community</span></span></a><nav aria-label="Community"><a href="/streamer#partner">Partner entdecken</a><a href="/twitch/verwaltung#profil">Mein Profil</a></nav></header><main>{}</main><footer><a href="/streamer">Teil des Partnernetzwerks werden</a><span><a href="/twitch/impressum">Impressum</a> · <a href="/twitch/datenschutz">Datenschutz</a></span></footer></body></html>"#,
        escape(title),
        escape(description),
        metadata,
        accent,
        body
    )
}

fn response(status: StatusCode, html: String) -> Response {
    let mut response = no_store((status, Html(html)).into_response());
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'none'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' https://static-cdn.jtvnw.net https://clips-media-assets.twitch.tv; frame-src https://clips.twitch.tv; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"
            .parse()
            .unwrap(),
    );
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
    response(
        status,
        document(
            title,
            text,
            None,
            &format!(
                "<section class=\"empty\"><p class=\"eyebrow\">Partnernetzwerk</p><h1>{title}</h1><p>{text}</p><a class=\"button\" href=\"/streamer#partner\">Andere Partner entdecken</a></section>"
            ),
            "gold",
        ),
    )
}

fn timestamp(at: DateTime<Utc>) -> String {
    at.with_timezone(&Berlin)
        .format("%d.%m.%Y · %H:%M")
        .to_string()
}

fn short_day(at: DateTime<Utc>) -> String {
    at.with_timezone(&Berlin).format("%a, %d.%m.").to_string()
}

fn safe_clip_url(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("clips.twitch.tv") | Some("www.twitch.tv") | Some("twitch.tv")
        )
}

fn safe_clip_id(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= 100
        && raw
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || value == b'-' || value == b'_')
}

fn playstyle_label(value: &str) -> &str {
    match value {
        "competitive" => "Competitive",
        "tryhard" => "Tryhard",
        "chill" => "Chill",
        "community" => "Community",
        "educational" => "Erklärend",
        "variety" => "Variety",
        _ => value,
    }
}

fn preferred_time_label(value: &str) -> &str {
    match value {
        "weekday_day" => "Unter der Woche tagsüber",
        "weekday_evening" => "Unter der Woche abends",
        "weekday_late" => "Unter der Woche spät",
        "weekend_day" => "Am Wochenende tagsüber",
        "weekend_evening" => "Am Wochenende abends",
        "spontaneous" => "Spontan",
        _ => value,
    }
}

#[derive(Clone)]
struct Upcoming {
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    title: String,
    source: &'static str,
    recurring: bool,
}

fn upcoming_items(p: &Content, twitch: &TwitchProfileSnapshot, now: DateTime<Utc>) -> Vec<Upcoming> {
    let mut items: Vec<Upcoming> = p
        .events
        .iter()
        .filter(|event| event.ends_at > now)
        .map(|event| Upcoming {
            starts_at: event.starts_at,
            ends_at: event.ends_at,
            title: event.title.clone(),
            source: "Profil",
            recurring: false,
        })
        .collect();

    if p.sync_twitch_schedule {
        items.extend(
            twitch
                .schedule
                .iter()
                .filter(|segment| segment.ends_at > now)
                .map(|segment| Upcoming {
                    starts_at: segment.starts_at,
                    ends_at: segment.ends_at,
                    title: if segment.title.trim().is_empty() {
                        "Twitch Stream".to_string()
                    } else {
                        segment.title.clone()
                    },
                    source: "Twitch",
                    recurring: segment.is_recurring,
                }),
        );
    }

    items.sort_by_key(|item| item.starts_at);
    items.dedup_by(|a, b| {
        (a.starts_at - b.starts_at).num_minutes().abs() <= 5
            && a.title.eq_ignore_ascii_case(&b.title)
    });
    items.truncate(6);
    items
}

#[allow(clippy::too_many_arguments)]
pub(super) fn page(
    record: &Record,
    twitch: &TwitchProfileSnapshot,
    observed: &Schedule,
    monthly: &[Session],
    directory: &[DirectoryEntry],
    month: NaiveDate,
    now: DateTime<Utc>,
    truncated: bool,
) -> Response {
    let p = &record.content.0;
    let login = escape(&record.login);
    let display_name = if twitch.display_name.trim().is_empty() {
        record.login.as_str()
    } else {
        twitch.display_name.trim()
    };
    let headline = if p.headline.is_empty() {
        "Deadlock Streamer aus dem DDC Partnernetzwerk"
    } else {
        &p.headline
    };
    let about = if p.about.trim().is_empty() && !twitch.description.trim().is_empty() {
        twitch.description.trim()
    } else if p.about.trim().is_empty() {
        "Schau im Stream vorbei und lerne mich und meine Community kennen."
    } else {
        p.about.trim()
    };
    let accent = match p.accent {
        Accent::Gold => "gold",
        Accent::Violet => "violet",
        Accent::Teal => "teal",
    };

    let avatar_url = (!twitch.profile_image_url.is_empty()
        && safe_twitch_image(&twitch.profile_image_url))
    .then_some(twitch.profile_image_url.as_str())
    .or_else(|| {
        (!p.avatar_url.is_empty() && safe_twitch_image(&p.avatar_url))
            .then_some(p.avatar_url.as_str())
    });
    let banner_url = twitch
        .live
        .as_ref()
        .map(|live| live.thumbnail_url.as_str())
        .filter(|url| safe_twitch_image(url))
        .or_else(|| {
            (!twitch.banner_url.is_empty() && safe_twitch_image(&twitch.banner_url))
                .then_some(twitch.banner_url.as_str())
        })
        .or_else(|| {
            (!p.banner_url.is_empty() && safe_twitch_image(&p.banner_url))
                .then_some(p.banner_url.as_str())
        });

    let image = if let Some(avatar_url) = avatar_url {
        format!(
            "<img class=\"avatar\" src=\"{}\" alt=\"Profilbild von {}\" width=\"112\" height=\"112\">",
            escape(avatar_url),
            escape(display_name)
        )
    } else {
        format!(
            "<div class=\"avatar initial\" aria-hidden=\"true\">{}</div>",
            login.chars().next().unwrap_or('?').to_ascii_uppercase()
        )
    };
    let banner = banner_url
        .map(|url| {
            format!(
                "<img class=\"hero-banner\" src=\"{}\" alt=\"\" loading=\"eager\" fetchpriority=\"high\">",
                escape(url)
            )
        })
        .unwrap_or_default();

    let live_now = twitch.live.is_some() || is_live(record, now);
    let live_badge = if live_now {
        let game = twitch
            .live
            .as_ref()
            .map(|live| live.game_name.trim())
            .filter(|game| !game.is_empty())
            .or(record.last_game.as_deref())
            .unwrap_or("Twitch");
        format!(
            "<span class=\"live\"><span class=\"live-dot\"></span>Live · {}</span>",
            escape(game)
        )
    } else {
        "<span class=\"badge\">Partner</span>".into()
    };

    let mut tags = String::new();
    if !p.rank.trim().is_empty() {
        let _ = write!(
            tags,
            "<span class=\"profile-tag rank-tag\">Rang: {}</span>",
            escape(&p.rank)
        );
    }
    for hero in &p.main_heroes {
        let _ = write!(
            tags,
            "<span class=\"profile-tag\">Main: {}</span>",
            escape(hero)
        );
    }
    for playstyle in &p.playstyles {
        let _ = write!(
            tags,
            "<span class=\"profile-tag\">{}</span>",
            escape(playstyle_label(playstyle))
        );
    }

    let live_context = twitch
        .live
        .as_ref()
        .filter(|live| !live.title.trim().is_empty())
        .map(|live| {
            format!(
                "<div class=\"live-context\"><span>Gerade live</span><strong>{}</strong></div>",
                escape(live.title.trim())
            )
        })
        .unwrap_or_default();

    let mut body = format!(
        r##"<a class="back" href="/streamer#partner">← Partnernetzwerk</a><section class="hero">{banner}<div class="hero-shade"></div><div class="hero-content"><div class="hero-avatar">{image}</div><div class="hero-copy"><div class="hero-status">{live_badge}<span class="network-mark">Deutsche Deadlock Community</span></div><p class="eyebrow">Streamerprofil · @{login}</p><h1>{}</h1><p class="streamer-name">{}</p><p class="hero-lead">{}</p><div class="profile-tags">{tags}</div>{live_context}<div class="hero-actions"><a class="button" href="https://www.twitch.tv/{login}" rel="noopener noreferrer" target="_blank">Twitch öffnen ↗</a><a class="button secondary" href="#streamplan">Nächste Streams</a></div></div></div></section><nav class="socials" aria-label="Social Links">"##,
        escape(headline),
        escape(display_name),
        escape(about),
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
    body.push_str("</nav>");

    body.push_str("<div class=\"intro-grid\"><section class=\"panel about-panel\"><p class=\"eyebrow\">Das bin ich</p><h2>Über meinen Stream</h2>");
    let _ = write!(body, "<p class=\"multiline about-copy\">{}</p>", escape(about));
    if !p.preferred_times.is_empty() {
        body.push_str("<div class=\"meta-block\"><span>Typische Zeiten</span><div class=\"profile-tags\">");
        for value in &p.preferred_times {
            let _ = write!(
                body,
                "<span class=\"profile-tag subtle\">{}</span>",
                escape(preferred_time_label(value))
            );
        }
        body.push_str("</div></div>");
    }
    body.push_str("</section>");

    let upcoming = upcoming_items(p, twitch, now);
    body.push_str("<section class=\"panel schedule-panel\" id=\"streamplan\"><div class=\"section-head\"><div><p class=\"eyebrow\">Diese Woche und danach</p><h2>Nächste Streams</h2></div>");
    if p.sync_twitch_schedule && twitch.available {
        body.push_str("<span class=\"sync-badge\">Twitch-Streamplan aktiv</span>");
    }
    body.push_str("</div><div class=\"upcoming-list\">");
    if upcoming.is_empty() {
        body.push_str("<div class=\"empty-inline\"><strong>Noch kein Termin eingetragen</strong><p>Spontane Streams sind trotzdem möglich. Auf Twitch siehst du sofort, wenn der Kanal live geht.</p></div>");
    } else {
        for item in &upcoming {
            let recurring = if item.recurring {
                "<span class=\"recurring\">wiederkehrend</span>"
            } else {
                ""
            };
            let _ = write!(
                body,
                "<article class=\"upcoming-card\"><time datetime=\"{}\"><strong>{}</strong><span>{} bis {} Uhr</span></time><div><span class=\"source\">{}</span>{recurring}<h3>{}</h3></div></article>",
                item.starts_at.to_rfc3339(),
                escape(&short_day(item.starts_at)),
                item.starts_at.with_timezone(&Berlin).format("%H:%M"),
                item.ends_at.with_timezone(&Berlin).format("%H:%M"),
                item.source,
                escape(&item.title),
            );
        }
    }
    body.push_str("</div><p class=\"hint\">Zeiten werden in Europe/Berlin angezeigt. Twitch-Termine werden automatisch aktualisiert, wenn die Synchronisierung im Profil aktiv ist.</p></section></div>");

    if !twitch.clips.is_empty() {
        body.push_str("<section class=\"panel highlights\"><div class=\"section-head\"><div><p class=\"eyebrow\">Highlights</p><h2>Clips aus den letzten 30 Tagen</h2></div><span class=\"sync-badge\">Automatisch von Twitch</span></div><div class=\"clip-grid\">");
        for clip in twitch.clips.iter().take(3) {
            if !safe_clip_url(&clip.url) || !safe_clip_id(&clip.id) {
                continue;
            }
            let title = if clip.title.trim().is_empty() {
                "Twitch Clip"
            } else {
                clip.title.trim()
            };
            let _ = write!(
                body,
                "<article class=\"clip-card\"><div class=\"clip-media\"><iframe src=\"https://clips.twitch.tv/embed?clip={}&amp;parent=deutsche-deadlock-community.de&amp;autoplay=false&amp;muted=true\" title=\"{}\" loading=\"lazy\" allow=\"fullscreen\" referrerpolicy=\"no-referrer\"></iframe></div><a class=\"clip-copy\" href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\"><strong>{}</strong><span>{} Aufrufe · Auf Twitch ansehen ↗</span></a></article>",
                clip.id,
                escape(title),
                escape(&clip.url),
                escape(title),
                clip.view_count,
            );
        }
        body.push_str("</div></section>");
    }

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

    body.push_str("<details class=\"history-details\"><summary><span><strong>Kalender und bisherige Livezeiten</strong><small>Monatsansicht und 90 Tage Rhythmus</small></span><span class=\"summary-action\">Anzeigen</span></summary><div class=\"history-body\">");
    let _ = write!(
        body,
        "<section class=\"panel calendar-section\" id=\"kalender\"><div class=\"section-head\"><div><p class=\"eyebrow\">Monatsansicht</p><h2>{} {}</h2></div><nav class=\"month-nav\" aria-label=\"Kalendermonat\">",
        month_names[month.month0() as usize],
        month.year()
    );
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

        let planned_events: Vec<_> = p
            .events
            .iter()
            .filter(|event| event.starts_at < end && event.ends_at > start)
            .collect();
        let observed_sessions: Vec<_> = monthly
            .iter()
            .filter(|session| session.started_at < end && session.ended_at > start)
            .collect();

        let mut shown = 0usize;
        for event in planned_events.iter().take(2) {
            let label = format!(
                "{} · {} bis {}",
                event.title,
                timestamp(event.starts_at),
                timestamp(event.ends_at)
            );
            let _ = write!(
                body,
                "<div class=\"calendar-event planned\" title=\"{}\"><span>{:02}:{:02} · Geplant</span><strong>{}</strong></div>",
                escape(&label),
                event.starts_at.max(start).with_timezone(&Berlin).hour(),
                event.starts_at.max(start).with_timezone(&Berlin).minute(),
                escape(&event.title)
            );
            shown += 1;
        }
        for session in observed_sessions.iter().take(3usize.saturating_sub(shown)) {
            let game = session.game_name.as_deref().unwrap_or("Stream");
            let label = format!(
                "Tatsächlich live: {} bis {} · {}",
                timestamp(session.started_at),
                timestamp(session.ended_at),
                game
            );
            let _ = write!(
                body,
                "<div class=\"calendar-event observed\" title=\"{}\"><span>{:02}:{:02} · War live</span><strong>{}</strong></div>",
                escape(&label),
                session.started_at.max(start).with_timezone(&Berlin).hour(),
                session.started_at.max(start).with_timezone(&Berlin).minute(),
                escape(game)
            );
            shown += 1;
        }
        let hidden = planned_events.len() + observed_sessions.len() - shown;
        if hidden > 0 {
            let _ = write!(body, "<span class=\"calendar-more\">+{hidden} weitere</span>");
        }
        body.push_str("</div>");
        day = day.succ_opt().unwrap();
    }
    body.push_str("</div></div><p class=\"hint\">Vergangene Livestreams stammen aus unserer Erfassung. Fehlende Einträge sind kein Beleg dafür, dass kein Stream stattgefunden hat.</p></section>");

    if p.show_history {
        let _ = write!(
            body,
            "<section class=\"panel\"><p class=\"eyebrow\">Mein bisheriger Rhythmus</p><h2>Wann war ich live?</h2><p>{} erfasste Streams in den letzten 90 Tagen. Neuere Streams zählen stärker.</p><div class=\"heatmap-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Historische Livezeiten in Berliner Zeit\"><div class=\"hours\"><span>00 Uhr</span><span>06 Uhr</span><span>12 Uhr</span><span>18 Uhr</span><span>24 Uhr</span></div>",
            observed.sessions
        );
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
                let _ = write!(
                    body,
                    "<span class=\"heat-slot\" role=\"img\" title=\"{label}\" aria-label=\"{label}\" style=\"opacity:{:.3}\"></span>",
                    0.08 + value * 0.92
                );
            }
            body.push_str("</div>");
        }
        body.push_str("</div><p class=\"hint\">Je Kästchen 30 Minuten. Sommerzeit und Winterzeit werden in Europe/Berlin berücksichtigt.</p></section>");
    }
    body.push_str("</div></details>");

    if truncated {
        body.push_str("<p class=\"hint\">Die Darstellung ist auf die neuesten 2001 erfassten Streams je Zeitraum begrenzt.</p>");
    }

    let featured: Vec<_> = p
        .featured
        .iter()
        .filter_map(|featured_login| {
            directory
                .iter()
                .find(|entry| entry.login == *featured_login && entry.login != record.login)
        })
        .collect();
    if !featured.is_empty() {
        body.push_str("<section class=\"panel\"><p class=\"eyebrow\">Aus meinem Umfeld</p><h2>Streamer, mit denen ich gerne unterwegs bin</h2><div class=\"partner-grid\">");
        for entry in featured {
            let _ = write!(
                body,
                "<a class=\"partner-card\" href=\"/streamer/{}\"><strong>@{} ↗</strong><span>{}</span></a>",
                escape(&entry.login),
                escape(&entry.login),
                escape(&entry.headline)
            );
        }
        body.push_str("</div></section>");
    }

    body.push_str("<aside class=\"network-cta\"><h2>Noch mehr Deadlock Streams aus der Community</h2><a class=\"button\" href=\"/streamer#partner\">Partner entdecken →</a></aside>");

    response(
        StatusCode::OK,
        document(
            &format!("@{} · Deutsche Deadlock Community", record.login),
            headline,
            Some(&format!(
                "https://deutsche-deadlock-community.de/streamer/{}",
                record.login
            )),
            &body,
            accent,
        ),
    )
}
