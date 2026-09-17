//! Steam OpenID 2.0 relying party. Provider, realm and return URL are pinned;
//! claimed IDs from the browser are never accepted without direct verification.
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};
use url::Url;

const ENDPOINT: &str = "https://steamcommunity.com/openid/login";
const NS: &str = "http://specs.openid.net/auth/2.0";
const IDENTIFIER_SELECT: &str = "http://specs.openid.net/auth/2.0/identifier_select";
const CLAIM_PREFIX: &str = "https://steamcommunity.com/openid/id/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenIdError {
    Invalid,
    Unavailable,
}

pub fn authorize_url(return_to: &str) -> Result<String, OpenIdError> {
    let callback = Url::parse(return_to).map_err(|_| OpenIdError::Invalid)?;
    if callback.scheme() != "https"
        || callback.host_str().is_none()
        || callback.fragment().is_some()
        || !callback.username().is_empty()
        || callback.password().is_some()
    {
        return Err(OpenIdError::Invalid);
    }
    let realm = format!("{}/", callback.origin().ascii_serialization());
    let mut url = Url::parse(ENDPOINT).map_err(|_| OpenIdError::Invalid)?;
    url.query_pairs_mut().extend_pairs([
        ("openid.ns", NS),
        ("openid.mode", "checkid_setup"),
        ("openid.claimed_id", IDENTIFIER_SELECT),
        ("openid.identity", IDENTIFIER_SELECT),
        ("openid.return_to", return_to),
        ("openid.realm", &realm),
    ]);
    Ok(url.into())
}

pub fn parse_query(raw: &str) -> Result<BTreeMap<String, String>, OpenIdError> {
    if raw.len() > 8192 {
        return Err(OpenIdError::Invalid);
    }
    let mut params = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        if key.len() > 80
            || value.len() > 2048
            || params.len() >= 40
            || (key != "state" && !key.starts_with("openid."))
            || params
                .insert(key.into_owned(), value.into_owned())
                .is_some()
        {
            return Err(OpenIdError::Invalid);
        }
    }
    Ok(params)
}

fn validate_assertion(
    params: &BTreeMap<String, String>,
    return_to: &str,
    now: i64,
) -> Result<(i64, String), OpenIdError> {
    let get = |key: &str| {
        params
            .get(key)
            .map(String::as_str)
            .ok_or(OpenIdError::Invalid)
    };
    if get("openid.ns")? != NS
        || get("openid.mode")? != "id_res"
        || get("openid.op_endpoint")? != ENDPOINT
        || get("openid.return_to")? != return_to
        || get("openid.claimed_id")? != get("openid.identity")?
        || get("openid.sig")?.is_empty()
        || get("openid.assoc_handle")?.is_empty()
    {
        return Err(OpenIdError::Invalid);
    }
    let signed = get("openid.signed")?.split(',').collect::<Vec<_>>();
    for key in [
        "op_endpoint",
        "claimed_id",
        "identity",
        "return_to",
        "response_nonce",
        "assoc_handle",
    ] {
        if !signed.contains(&key) {
            return Err(OpenIdError::Invalid);
        }
    }
    let id = get("openid.claimed_id")?
        .strip_prefix(CLAIM_PREFIX)
        .or_else(|| {
            get("openid.claimed_id")
                .ok()?
                .strip_prefix("http://steamcommunity.com/openid/id/")
        })
        .ok_or(OpenIdError::Invalid)?;
    if id.len() != 17 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(OpenIdError::Invalid);
    }
    let steam_id64 = id.parse::<i64>().map_err(|_| OpenIdError::Invalid)?;
    if !(76561197960265729..=76561202255233023).contains(&steam_id64) {
        return Err(OpenIdError::Invalid);
    }
    let nonce = get("openid.response_nonce")?;
    if nonce.len() <= 20 || nonce.len() > 255 || !nonce.is_ascii() {
        return Err(OpenIdError::Invalid);
    }
    let timestamp = chrono::DateTime::parse_from_rfc3339(&nonce[..20])
        .map_err(|_| OpenIdError::Invalid)?
        .timestamp();
    if timestamp > now + 60 || timestamp < now - 600 {
        return Err(OpenIdError::Invalid);
    }
    let nonce_hash = Sha256::digest(nonce.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok((steam_id64, nonce_hash))
}

#[derive(Clone)]
pub struct SteamOpenIdClient {
    endpoint: String,
}
impl Default for SteamOpenIdClient {
    fn default() -> Self {
        Self {
            endpoint: ENDPOINT.into(),
        }
    }
}
impl SteamOpenIdClient {
    pub async fn verify(
        &self,
        params: &BTreeMap<String, String>,
        return_to: &str,
    ) -> Result<(i64, String), OpenIdError> {
        let verified = validate_assertion(params, return_to, chrono::Utc::now().timestamp())?;
        let mut form = params
            .iter()
            .filter(|(key, _)| key.starts_with("openid."))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<BTreeMap<_, _>>();
        form.insert("openid.mode".into(), "check_authentication".into());
        // Never follow redirects or send an assertion to a client-provided endpoint.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| OpenIdError::Unavailable)?;
        let mut response = client
            .post(&self.endpoint)
            .form(&form)
            .send()
            .await
            .map_err(|_| OpenIdError::Unavailable)?;
        if !response.status().is_success() {
            return Err(OpenIdError::Unavailable);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| OpenIdError::Unavailable)?
        {
            if body.len() + chunk.len() > 8192 {
                return Err(OpenIdError::Invalid);
            }
            body.extend_from_slice(&chunk);
        }
        let body = std::str::from_utf8(&body).map_err(|_| OpenIdError::Invalid)?;
        let validity = body
            .lines()
            .filter_map(|line| line.strip_prefix("is_valid:"))
            .collect::<Vec<_>>();
        if validity != ["true"] {
            return Err(OpenIdError::Invalid);
        }
        Ok(verified)
    }

    #[cfg(test)]
    pub(crate) fn at(endpoint: String) -> Self {
        Self { endpoint }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_string_contains, method},
        Mock, MockServer, ResponseTemplate,
    };

    pub(crate) fn assertion(return_to: &str) -> BTreeMap<String, String> {
        let nonce = format!("{}abc123", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"));
        [
            ("openid.ns", NS),
            ("openid.mode", "id_res"),
            ("openid.op_endpoint", ENDPOINT),
            (
                "openid.claimed_id",
                "https://steamcommunity.com/openid/id/76561197960265770",
            ),
            (
                "openid.identity",
                "https://steamcommunity.com/openid/id/76561197960265770",
            ),
            ("openid.return_to", return_to),
            ("openid.response_nonce", &nonce),
            ("openid.sig", "signature"),
            ("openid.assoc_handle", "handle"),
            (
                "openid.signed",
                "op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle",
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
    }
    #[test]
    fn steam_openid_rejects_duplicate_parameters_and_wrong_identity_fields() {
        assert!(parse_query("state=a&state=b").is_err());
        assert!(parse_query("openid.mode=id_res&openid.mode=cancel").is_err());
        let callback = "https://example.test/twitch/connect/steam/callback?state=a";
        for (key, value) in [
            ("openid.ns", "wrong"),
            ("openid.mode", "cancel"),
            ("openid.op_endpoint", "http://127.0.0.1/"),
            ("openid.return_to", "https://evil.test/"),
            (
                "openid.identity",
                "https://steamcommunity.com/openid/id/76561197960265771",
            ),
            ("openid.signed", "identity"),
            ("openid.sig", ""),
            ("openid.response_nonce", "2000-01-01T00:00:00Zold"),
        ] {
            let mut params = assertion(callback);
            params.insert(key.into(), value.into());
            assert!(
                validate_assertion(&params, callback, chrono::Utc::now().timestamp()).is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn steam_openid_rejects_foreign_identity_hosts_and_invalid_steam_ids() {
        for identity in [
            "https://evil.test/openid/id/76561197960265770",
            "https://steamcommunity.com/openid/id/76561197960265770/",
            "https://steamcommunity.com/openid/id/76561197960265728",
            "https://steamcommunity.com/openid/id/76561202255233024",
        ] {
            let mut params = assertion("https://example.test/cb");
            params.insert("openid.claimed_id".into(), identity.into());
            params.insert("openid.identity".into(), identity.into());
            assert!(validate_assertion(
                &params,
                "https://example.test/cb",
                chrono::Utc::now().timestamp()
            )
            .is_err());
        }
    }
    #[tokio::test]
    async fn steam_openid_requires_positive_server_verification_not_just_browser_claims() {
        let server = MockServer::start().await;
        let callback = "https://example.test/cb";
        Mock::given(method("POST"))
            .and(body_string_contains("openid.mode=check_authentication"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("ns:http://specs.openid.net/auth/2.0\nis_valid:true\n"),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            SteamOpenIdClient::at(server.uri())
                .verify(&assertion(callback), callback)
                .await
                .unwrap()
                .0,
            76561197960265770
        );
    }
    #[tokio::test]
    async fn steam_openid_rejects_false_ambiguous_oversized_and_redirected_verification() {
        for (status, body) in [
            (200, "is_valid:false\n".into()),
            (200, "is_valid:true\nis_valid:false\n".into()),
            (200, "x".repeat(8193)),
            (302, "is_valid:true\n".into()),
            (503, "".into()),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .mount(&server)
                .await;
            assert!(SteamOpenIdClient::at(server.uri())
                .verify(
                    &assertion("https://example.test/cb"),
                    "https://example.test/cb"
                )
                .await
                .is_err());
        }
    }
    #[test]
    fn steam_openid_authorize_url_uses_pinned_provider_and_exact_callback() {
        let callback = "https://example.test/twitch/connect/steam/callback?state=a";
        let url = Url::parse(&authorize_url(callback).unwrap()).unwrap();
        assert_eq!(url.host_str(), Some("steamcommunity.com"));
        let q = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(q["openid.return_to"], callback);
        assert_eq!(q["openid.realm"], "https://example.test/");
        assert!(authorize_url("http://example.test/cb").is_err());
    }
}
