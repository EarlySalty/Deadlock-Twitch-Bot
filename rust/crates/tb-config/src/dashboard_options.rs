//! Betriebswerte der vorhandenen Dashboard-, Auth- und Billingpfade.
//! Zugangsdaten, Preisbeträge und Modellwahl gehören nicht in diese Optionen.
use crate::file::FileError;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StripePlanPriceIds {
    pub monthly: Option<String>,
    pub yearly: Option<String>,
}
impl StripePlanPriceIds {
    pub fn cycles(&self) -> impl Iterator<Item = (u32, &str)> {
        [(1, self.monthly.as_deref()), (12, self.yearly.as_deref())]
            .into_iter()
            .filter_map(|(cycle, id)| id.map(|id| (cycle, id)))
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DashboardOptions {
    pub affiliate_mail: crate::affiliate_options::AffiliateMailOptions,
    pub affiliate_seller: crate::affiliate_options::AffiliateSellerOptions,
    pub runtime_enforce: bool,
    pub runtime_role: String,
    pub runtime_lock_dir: PathBuf,
    pub legacy_fallback_url: Option<String>,
    pub cookie_insecure: bool,
    pub noauth_readiness: bool,
    pub pentest_disable_rate_limits: bool,
    pub oauth_redirect_uri: Option<String>,
    pub affiliate_oauth_redirect_uri: Option<String>,
    pub public_dashboard_url: Option<String>,
    pub admin_public_url: Option<String>,
    pub discord_oauth_broker_url: String,
    pub admin_owner_user_id: Option<u64>,
    pub admin_guild_ids: Vec<u64>,
    pub shared_admin_cookie_domain: String,
    pub demo_login_twitch_user_id: Option<String>,
    pub demo_login_display_name: String,
    pub demo_embed_origins: Vec<String>,
    pub billing_public_origin: Option<String>,
    pub stripe_price_ids: BTreeMap<String, StripePlanPriceIds>,
    pub stripe_product_ids: BTreeMap<String, String>,
    pub discord_ref_code: String,
    pub steam_rank_url: String,
    pub steam_link_start_base_url: String,
    pub turnier_internal_base_url: String,
    pub deadlock_assets_base_url: String,
    pub helix_base_url: Option<String>,
    pub admin_dist_path: PathBuf,
    pub dashboard_dist_path: PathBuf,
    pub website_dist_path: PathBuf,
    pub legal_pages_path: PathBuf,
    pub legal_turnstile_site_key: String,
    pub excluded_bot_logins: Vec<String>,
    pub additional_bot_user_ids: Vec<String>,
}
impl Default for DashboardOptions {
    fn default() -> Self {
        Self {
            affiliate_mail: Default::default(),
            affiliate_seller: Default::default(),
            runtime_enforce: true,
            runtime_role: String::new(),
            runtime_lock_dir: "data/runtime/locks".into(),
            legacy_fallback_url: None,
            cookie_insecure: false,
            noauth_readiness: false,
            pentest_disable_rate_limits: false,
            oauth_redirect_uri: None,
            affiliate_oauth_redirect_uri: None,
            public_dashboard_url: None,
            admin_public_url: None,
            discord_oauth_broker_url: "http://127.0.0.1:8770".into(),
            admin_owner_user_id: None,
            admin_guild_ids: Vec::new(),
            shared_admin_cookie_domain: "deutsche-deadlock-community.de".into(),
            demo_login_twitch_user_id: None,
            demo_login_display_name: String::new(),
            demo_embed_origins: vec!["https://deutsche-deadlock-community.de".into()],
            billing_public_origin: None,
            stripe_price_ids: BTreeMap::new(),
            stripe_product_ids: BTreeMap::new(),
            discord_ref_code: "DE-Deadlock-Discord".into(),
            steam_rank_url: "http://127.0.0.1:8783/rank".into(),
            steam_link_start_base_url: "https://deutsche-deadlock-community.de/link".into(),
            turnier_internal_base_url: "http://127.0.0.1:8900".into(),
            deadlock_assets_base_url: "https://assets.deadlock-api.com".into(),
            helix_base_url: None,
            admin_dist_path: "bot/admin_dashboard/dist".into(),
            dashboard_dist_path: "bot/analytics/dashboard_v2/dist".into(),
            website_dist_path: "website/dist".into(),
            legal_pages_path: "data/admin_dashboard/legal_pages.json".into(),
            legal_turnstile_site_key: String::new(),
            excluded_bot_logins: Vec::new(),
            additional_bot_user_ids: Vec::new(),
        }
    }
}
impl DashboardOptions {
    pub(crate) fn validate(&self) -> Result<(), FileError> {
        if self.affiliate_mail.port == 0 {
            return Err(FileError::invalid("dashboard.options.affiliate_mail.port"));
        }
        for value in [
            &self.affiliate_mail.from_name,
            &self.affiliate_seller.name,
            &self.affiliate_seller.company,
            &self.affiliate_seller.street,
            &self.affiliate_seller.postal_code,
            &self.affiliate_seller.city,
            &self.affiliate_seller.country,
            &self.affiliate_seller.email,
            &self.affiliate_seller.tax_id,
        ]
        .into_iter()
        .chain(self.affiliate_mail.host.iter())
        .chain(self.affiliate_mail.from_email.iter())
        {
            if value.chars().any(char::is_control) {
                return Err(FileError::invalid(
                    "dashboard.options.affiliate_mail_or_seller",
                ));
            }
        }
        if let Some(host) = &self.affiliate_mail.host {
            if host.trim().is_empty()
                || host.contains("://")
                || host.chars().any(char::is_whitespace)
            {
                return Err(FileError::invalid("dashboard.options.affiliate_mail.host"));
            }
        }
        if let Some(email) = &self.affiliate_mail.from_email {
            if email.trim().is_empty() {
                return Err(FileError::invalid(
                    "dashboard.options.affiliate_mail.from_email",
                ));
            }
        }
        if let Some(website) = &self.affiliate_seller.website {
            validate_url(website, "dashboard.options.affiliate_seller.website")?;
        }
        for (name, value) in [
            (
                "dashboard.options.legacy_fallback_url",
                &self.legacy_fallback_url,
            ),
            (
                "dashboard.options.oauth_redirect_uri",
                &self.oauth_redirect_uri,
            ),
            (
                "dashboard.options.affiliate_oauth_redirect_uri",
                &self.affiliate_oauth_redirect_uri,
            ),
            (
                "dashboard.options.public_dashboard_url",
                &self.public_dashboard_url,
            ),
            ("dashboard.options.admin_public_url", &self.admin_public_url),
            (
                "dashboard.options.billing_public_origin",
                &self.billing_public_origin,
            ),
            ("dashboard.options.helix_base_url", &self.helix_base_url),
        ] {
            if let Some(value) = value {
                validate_url(value, name)?;
            }
        }
        for (name, value) in [
            (
                "dashboard.options.discord_oauth_broker_url",
                &self.discord_oauth_broker_url,
            ),
            ("dashboard.options.steam_rank_url", &self.steam_rank_url),
            (
                "dashboard.options.steam_link_start_base_url",
                &self.steam_link_start_base_url,
            ),
            (
                "dashboard.options.turnier_internal_base_url",
                &self.turnier_internal_base_url,
            ),
            (
                "dashboard.options.deadlock_assets_base_url",
                &self.deadlock_assets_base_url,
            ),
        ] {
            validate_url(value, name)?;
        }
        for path in [
            &self.runtime_lock_dir,
            &self.admin_dist_path,
            &self.dashboard_dist_path,
            &self.website_dist_path,
            &self.legal_pages_path,
        ] {
            validate_path(path)?;
        }
        if self.admin_owner_user_id == Some(0) || self.admin_guild_ids.contains(&0) {
            return Err(FileError::invalid("dashboard.options.admin_ids"));
        }
        for id in self
            .additional_bot_user_ids
            .iter()
            .chain(self.demo_login_twitch_user_id.iter())
        {
            crate::global::positive_id(id, "dashboard.options.twitch_user_id")?;
        }
        for origin in &self.demo_embed_origins {
            validate_url(origin, "dashboard.options.demo_embed_origins")?;
        }
        if self
            .shared_admin_cookie_domain
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, ';' | '/' | '\\' | ':'))
        {
            return Err(FileError::invalid(
                "dashboard.options.shared_admin_cookie_domain",
            ));
        }
        for (plan, cycles) in &self.stripe_price_ids {
            if plan.trim().is_empty()
                || cycles
                    .cycles()
                    .any(|(_, id)| id.trim().is_empty() || id.chars().any(char::is_control))
            {
                return Err(FileError::invalid("dashboard.options.stripe_price_ids"));
            }
        }
        if self.stripe_product_ids.iter().any(|(plan, id)| {
            plan.trim().is_empty() || id.trim().is_empty() || id.chars().any(char::is_control)
        }) {
            return Err(FileError::invalid("dashboard.options.stripe_product_ids"));
        }
        Ok(())
    }
}
pub(crate) fn validate_url(value: &str, field: &'static str) -> Result<(), FileError> {
    let url = url::Url::parse(value.trim()).map_err(|_| FileError::invalid(field))?;
    let host = url.host_str().ok_or_else(|| FileError::invalid(field))?;
    let local = host
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
        || host.eq_ignore_ascii_case("localhost");
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
        || !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || value.chars().any(char::is_control)
        || value.contains('\\')
    {
        return Err(FileError::invalid(field));
    }
    Ok(())
}
fn validate_path(path: &Path) -> Result<(), FileError> {
    let value = path
        .to_str()
        .ok_or_else(|| FileError::invalid("dashboard.options.paths"))?;
    if value.trim().is_empty() || value.contains("://") || value.chars().any(char::is_control) {
        return Err(FileError::invalid("dashboard.options.paths"));
    }
    Ok(())
}
