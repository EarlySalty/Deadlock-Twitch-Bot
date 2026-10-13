//! Öffentliche Basisadresse (Paket O, Multi-Server-Plan 3.5/4.3).
//!
//! Der Twitch-Bot wird über zwei öffentliche Domains erreicht: die deutsche
//! Hauptdomain und die internationale Domain `dead-lock.de`. Ein Backend bedient
//! beide. Diese Datei ist die **einzige** Stelle, die die beiden Hostnamen kennt;
//! alle Links, Redirect-URLs und Herkunftsprüfungen leiten sich von hier ab.
//!
//! Sicherheitsregel: Die Domain einer Anfrage wird nur aus der festen Allowlist
//! [`PublicSite`] gewählt, nie frei aus einem Host-Header übernommen. Unbekannte
//! Hosts fallen auf die Hauptdomain zurück.

/// Hostname der Hauptdomain als Literal, damit Konstanten per `concat!`
/// daraus gebaut werden können (siehe [`primary_url!`](crate::primary_url)).
#[macro_export]
macro_rules! primary_host {
    () => {
        "deutsche-deadlock-community.de"
    };
}

/// Absolute URL auf der Hauptdomain als `&'static str`, z. B.
/// `primary_url!("/twitch/dashboard")`. Für Konstanten und Bot-Texte, die
/// (noch) keinem Streamer-Heimatserver zugeordnet sind; zur Laufzeit gibt es
/// dafür [`PublicSite::url`].
#[macro_export]
macro_rules! primary_url {
    () => {
        concat!("https://", $crate::primary_host!())
    };
    ($path:literal) => {
        concat!("https://", $crate::primary_host!(), $path)
    };
}

/// Absolute URL auf dem internen Admin-Host der Hauptdomain.
#[macro_export]
macro_rules! admin_url {
    () => {
        concat!("https://admin.", $crate::primary_host!())
    };
    ($path:literal) => {
        concat!("https://admin.", $crate::primary_host!(), $path)
    };
}

/// Hostname der deutschen Hauptdomain.
pub const PRIMARY_HOST: &str = crate::primary_host!();
/// Hostname der internationalen Domain.
pub const INTERNATIONAL_HOST: &str = "dead-lock.de";
/// Origin der deutschen Hauptdomain.
pub const PRIMARY_ORIGIN: &str = crate::primary_url!();
/// Origin der internationalen Domain.
pub const INTERNATIONAL_ORIGIN: &str = "https://dead-lock.de";
/// Hostname des internen Admin-Dashboards (bleibt auf der Hauptdomain).
pub const ADMIN_HOST: &str = concat!("admin.", crate::primary_host!());
/// Origin des internen Admin-Dashboards.
pub const ADMIN_ORIGIN: &str = crate::admin_url!();

/// Öffentliche Domain, auf der eine Anfrage oder ein Ablauf läuft.
///
/// `Primary` ist der Standard: Alles, was keiner Anfrage zuzuordnen ist
/// (Bot-Nachrichten, Hintergrundjobs), zeigt auf die Hauptdomain, solange der
/// Heimat-Server eines Streamers nichts anderes vorgibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PublicSite {
    #[default]
    Primary,
    International,
}

impl PublicSite {
    /// Alle bekannten Domains (Allowlist), Hauptdomain zuerst.
    pub const ALL: [PublicSite; 2] = [PublicSite::Primary, PublicSite::International];

    /// Reiner Hostname ohne Schema und Port.
    pub const fn host(self) -> &'static str {
        match self {
            PublicSite::Primary => PRIMARY_HOST,
            PublicSite::International => INTERNATIONAL_HOST,
        }
    }

    /// Origin (`https://host`) ohne abschließenden Schrägstrich.
    pub const fn origin(self) -> &'static str {
        match self {
            PublicSite::Primary => PRIMARY_ORIGIN,
            PublicSite::International => INTERNATIONAL_ORIGIN,
        }
    }

    /// Absolute URL auf dieser Domain. `path` beginnt mit `/`; fehlt er,
    /// wird er ergänzt.
    pub fn url(self, path: &str) -> String {
        if path.is_empty() {
            return self.origin().to_string();
        }
        if path.starts_with('/') {
            format!("{}{path}", self.origin())
        } else {
            format!("{}/{path}", self.origin())
        }
    }

    /// Ordnet einen `Host`-Header (oder Hostnamen) der Allowlist zu.
    ///
    /// Groß-/Kleinschreibung, Port und ein abschließender Punkt werden
    /// ignoriert. Alles andere (fremde Hosts, Subdomains, Userinfo) → `None`.
    pub fn from_host(raw: &str) -> Option<Self> {
        let host = normalize_host(raw)?;
        PublicSite::ALL.into_iter().find(|site| site.host() == host)
    }

    /// Wie [`from_host`](Self::from_host), fällt aber auf die Hauptdomain zurück.
    pub fn from_host_or_primary(raw: Option<&str>) -> Self {
        raw.and_then(Self::from_host).unwrap_or_default()
    }

    /// Ordnet eine absolute URL (oder Origin) der Allowlist zu. Nur `https`
    /// ohne Userinfo und ohne abweichenden Port zählt.
    pub fn from_url(raw: &str) -> Option<Self> {
        let parsed = url::Url::parse(raw.trim()).ok()?;
        if parsed.scheme() != "https"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some()
        {
            return None;
        }
        Self::from_host(parsed.host_str()?)
    }

    /// Überträgt eine konfigurierte URL auf diese Domain.
    ///
    /// Zeigt `configured` auf eine der beiden öffentlichen Domains (https, ohne
    /// Userinfo und Port), wird nur der Host ersetzt; Pfad, Query und Fragment
    /// bleiben. Jede andere URL (lokale Entwicklung, Admin-Host, fremde Hosts)
    /// bleibt unverändert. Für die Hauptdomain ist das Ergebnis bei einer auf
    /// die Hauptdomain konfigurierten URL immer exakt die Eingabe.
    pub fn rebase(self, configured: &str) -> String {
        let trimmed = configured.trim();
        if Self::from_url(trimmed).is_none() {
            return configured.to_string();
        }
        let Ok(mut parsed) = url::Url::parse(trimmed) else {
            return configured.to_string();
        };
        if parsed.host_str() == Some(self.host()) {
            return configured.to_string();
        }
        if parsed.set_host(Some(self.host())).is_err() {
            return configured.to_string();
        }
        let rebased = parsed.to_string();
        // `Url` hängt bei reinen Origins einen `/` an; die Eingabeform bleibt.
        if !trimmed.ends_with('/') && rebased.ends_with('/') && parsed.path() == "/" {
            rebased.trim_end_matches('/').to_string()
        } else {
            rebased
        }
    }

    /// Stabiler Kurzname für Persistenz und interne Query-Parameter.
    pub const fn as_str(self) -> &'static str {
        match self {
            PublicSite::Primary => "primary",
            PublicSite::International => "international",
        }
    }

    /// Gegenstück zu [`as_str`](Self::as_str); akzeptiert auch den Hostnamen.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "primary" => Some(PublicSite::Primary),
            "international" => Some(PublicSite::International),
            other => Self::from_host(other),
        }
    }
}

/// Liefert alle Domain-Varianten einer auf die Hauptdomain konfigurierten
/// Redirect-URL (Hauptdomain zuerst, ohne Duplikate). Grundlage für die Liste,
/// die in den OAuth-Apps (Twitch, Discord, Google, Kick, Stripe) einzutragen ist.
pub fn redirect_variants(configured: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for site in PublicSite::ALL {
        let candidate = site.rebase(configured);
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

fn normalize_host(raw: &str) -> Option<String> {
    let value = raw.split(',').next()?.trim();
    if value.is_empty() || value.contains('@') || value.contains('/') {
        return None;
    }
    let without_port = match value.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') && port.chars().all(|c| c.is_ascii_digit()) => {
            host
        }
        Some(_) => return None,
        None => value,
    };
    let host = without_port.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makros_und_konstanten_passen_zusammen() {
        assert_eq!(PRIMARY_HOST, "deutsche-deadlock-community.de");
        assert_eq!(PRIMARY_ORIGIN, "https://deutsche-deadlock-community.de");
        assert_eq!(
            crate::primary_url!("/streamer"),
            "https://deutsche-deadlock-community.de/streamer"
        );
        assert_eq!(ADMIN_HOST, "admin.deutsche-deadlock-community.de");
        assert_eq!(
            crate::admin_url!("/twitch"),
            "https://admin.deutsche-deadlock-community.de/twitch"
        );
    }

    #[test]
    fn host_header_wird_nur_aus_allowlist_gewaehlt() {
        assert_eq!(
            PublicSite::from_host("dead-lock.de"),
            Some(PublicSite::International)
        );
        assert_eq!(
            PublicSite::from_host("DEAD-LOCK.DE:443"),
            Some(PublicSite::International)
        );
        assert_eq!(
            PublicSite::from_host("dead-lock.de."),
            Some(PublicSite::International)
        );
        assert_eq!(
            PublicSite::from_host("deutsche-deadlock-community.de"),
            Some(PublicSite::Primary)
        );
        assert_eq!(PublicSite::from_host("evil.dead-lock.de"), None);
        assert_eq!(PublicSite::from_host("dead-lock.de.evil.test"), None);
        assert_eq!(
            PublicSite::from_host("admin.deutsche-deadlock-community.de"),
            None
        );
        assert_eq!(PublicSite::from_host("user@dead-lock.de"), None);
        assert_eq!(PublicSite::from_host(""), None);
        assert_eq!(
            PublicSite::from_host_or_primary(Some("evil.test")),
            PublicSite::Primary
        );
        assert_eq!(PublicSite::from_host_or_primary(None), PublicSite::Primary);
    }

    #[test]
    fn url_baut_absolute_adressen() {
        assert_eq!(
            PublicSite::Primary.url("/twitch/dashboard"),
            "https://deutsche-deadlock-community.de/twitch/dashboard"
        );
        assert_eq!(
            PublicSite::International.url("streamer"),
            "https://dead-lock.de/streamer"
        );
        assert_eq!(PublicSite::International.url(""), "https://dead-lock.de");
    }

    #[test]
    fn rebase_tauscht_nur_oeffentliche_hosts() {
        let configured = "https://deutsche-deadlock-community.de/callback/twitch";
        assert_eq!(PublicSite::Primary.rebase(configured), configured);
        assert_eq!(
            PublicSite::International.rebase(configured),
            "https://dead-lock.de/callback/twitch"
        );
        assert_eq!(
            PublicSite::Primary.rebase("https://dead-lock.de/twitch/uplink?verbunden=twitch"),
            "https://deutsche-deadlock-community.de/twitch/uplink?verbunden=twitch"
        );
        assert_eq!(
            PublicSite::International.rebase("https://deutsche-deadlock-community.de"),
            "https://dead-lock.de"
        );
        for fremd in [
            "http://localhost:8769/twitch/auth/callback",
            "https://admin.deutsche-deadlock-community.de/twitch/auth/callback",
            "https://deutsche-deadlock-community.de:8443/callback/twitch",
            "http://deutsche-deadlock-community.de/callback/twitch",
            "https://user@deutsche-deadlock-community.de/callback/twitch",
            "https://example.test/callback/twitch",
            "kein url",
        ] {
            assert_eq!(PublicSite::International.rebase(fremd), fremd, "{fremd}");
        }
    }

    #[test]
    fn redirect_varianten_fuer_app_konfiguration() {
        assert_eq!(
            redirect_variants("https://deutsche-deadlock-community.de/callback/twitch"),
            vec![
                "https://deutsche-deadlock-community.de/callback/twitch".to_string(),
                "https://dead-lock.de/callback/twitch".to_string(),
            ]
        );
        assert_eq!(
            redirect_variants("http://localhost:8769/callback/twitch"),
            vec!["http://localhost:8769/callback/twitch".to_string()]
        );
    }

    #[test]
    fn kurzname_ist_umkehrbar() {
        for site in PublicSite::ALL {
            assert_eq!(PublicSite::parse(site.as_str()), Some(site));
            assert_eq!(PublicSite::parse(site.host()), Some(site));
        }
        assert_eq!(PublicSite::parse("evil.test"), None);
    }
}
