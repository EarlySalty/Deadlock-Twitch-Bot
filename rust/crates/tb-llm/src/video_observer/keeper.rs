use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::time::{sleep, Instant};
use zeroize::Zeroizing;

use super::{
    cdp::Cdp,
    gemini::{self, READY},
    ObserverError, VideoObserverConfig, BROWSER,
};

const PASSKEY_ENV: &str = "GOOGLE_STEEL_PASSKEY";
const GEMINI: &str = "https://gemini.google.com/";
const SIGN_IN: &str = "https://accounts.google.com/ServiceLogin?hl=en&continue=";
const INTERVAL: Duration = Duration::from_secs(300);
const LOAD: Duration = Duration::from_secs(4);
const STEP_PAUSE: Duration = Duration::from_secs(5);
const KEY_DELAY: Duration = Duration::from_millis(90);
const LOGIN_STEPS: usize = 15;
const CLICKABLE: &str = "button,[role=button],[role=link],a,li";
const CHOICES: [&str; 5] = [
    "Use your passkey",
    "Try another way",
    "Next",
    "Continue",
    "Try again",
];
const CREDENTIAL_FIELDS: [&str; 6] = [
    "credentialId",
    "isResidentCredential",
    "rpId",
    "privateKey",
    "userHandle",
    "signCount",
];
const SPOOF: &str = "PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable=()=>Promise.resolve(true);if(PublicKeyCredential.getClientCapabilities){const o=PublicKeyCredential.getClientCapabilities.bind(PublicKeyCredential);PublicKeyCredential.getClientCapabilities=async()=>Object.assign({},await o(),{passkeyPlatformAuthenticator:true,userVerifyingPlatformAuthenticator:true});}";
const ACCOUNT: &str = r#"(()=>{for(const e of document.querySelectorAll('[aria-label]')){const m=/^(?:Google Account|Google-Konto):.*\(([^()\s]+@[^()\s]+)\)/.exec((e.getAttribute('aria-label')||'').replace(/\s+/g,' '));if(m)return m[1].toLowerCase();}return null;})()"#;
const EMPTY_IDENTIFIER: &str = "(()=>{const e=document.querySelector('#identifierId');return !!(e&&e.getClientRects().length&&!e.value);})()";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    Ready,
    Relogged,
}

#[derive(Deserialize)]
struct Passkey {
    account: String,
    credentials: Vec<Map<String, Value>>,
}

enum Status {
    SignedIn(Option<String>),
    SignedOut,
}

pub async fn check_session(config: &VideoObserverConfig) -> Result<Session, ObserverError> {
    let _browser = BROWSER.lock().await;
    ensure(config).await
}

pub async fn keep_alive(config: VideoObserverConfig) {
    if !config.valid() {
        tracing::warn!("gemini session keeper disabled: invalid observer configuration");
        return;
    }
    loop {
        match check_session(&config).await {
            Ok(Session::Ready) => tracing::debug!("gemini session ready"),
            Ok(Session::Relogged) => tracing::info!("gemini session signed in again via passkey"),
            Err(error) => tracing::warn!(%error, "gemini session keeper failed"),
        }
        sleep(INTERVAL).await;
    }
}

pub(super) async fn ensure(config: &VideoObserverConfig) -> Result<Session, ObserverError> {
    let passkey = load_passkey();
    let url = browser_socket(&config.devtools_endpoint, config.step()).await?;
    let mut cdp = Cdp::connect(&url, config.step()).await?;
    let target = single_tab(&mut cdp, &config.chat_url).await?;
    cdp.attach_page(&target).await?;
    let account = passkey.as_ref().map(|key| key.account.to_lowercase());
    if open_chat(&mut cdp, config, account.as_deref()).await? {
        return Ok(Session::Ready);
    }
    let passkey = passkey.ok_or(ObserverError::NotLoggedIn)?;
    let signed_in = sign_in(&mut cdp, config, &passkey).await;
    let _ = cdp
        .page("Page.navigate", json!({ "url": config.chat_url }))
        .await;
    if signed_in? {
        Ok(Session::Relogged)
    } else {
        Err(ObserverError::NotLoggedIn)
    }
}

fn load_passkey() -> Option<Passkey> {
    let raw = Zeroizing::new(std::env::var(PASSKEY_ENV).ok()?);
    let passkey: Passkey = serde_json::from_str(&raw).ok()?;
    (passkey.account.contains('@') && !passkey.credentials.is_empty()).then_some(passkey)
}

async fn browser_socket(endpoint: &str, step: Duration) -> Result<String, ObserverError> {
    let version: Value = reqwest::Client::builder()
        .timeout(step)
        .build()
        .map_err(|_| ObserverError::Unavailable)?
        .get(format!("{endpoint}/json/version"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| ObserverError::Unavailable)?
        .json()
        .await
        .map_err(|_| ObserverError::Unavailable)?;
    socket_url(endpoint, &version).ok_or(ObserverError::Unavailable)
}

fn socket_url(endpoint: &str, version: &Value) -> Option<String> {
    let advertised = reqwest::Url::parse(version["webSocketDebuggerUrl"].as_str()?).ok()?;
    let host = reqwest::Url::parse(endpoint).ok()?;
    Some(format!(
        "ws://{}:{}{}",
        host.host_str()?,
        host.port()?,
        advertised.path()
    ))
}

async fn single_tab(cdp: &mut Cdp, chat_url: &str) -> Result<String, ObserverError> {
    let listed = cdp.browser("Target.getTargets", json!({})).await?;
    let pages = pages(&listed);
    let keep = pages
        .iter()
        .find(|(_, url)| url.starts_with(GEMINI))
        .or(pages.first())
        .map(|(id, _)| id.clone());
    for (id, _) in &pages {
        if Some(id) != keep.as_ref() {
            cdp.close_page(id).await;
        }
    }
    match keep {
        Some(id) => Ok(id),
        None => cdp
            .browser("Target.createTarget", json!({ "url": chat_url }))
            .await?["targetId"]
            .as_str()
            .map(str::to_owned)
            .ok_or(ObserverError::Ui),
    }
}

fn pages(listed: &Value) -> Vec<(String, String)> {
    listed["targetInfos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|target| target["type"] == "page")
        .filter_map(|target| {
            Some((
                target["targetId"].as_str()?.to_owned(),
                target["url"].as_str().unwrap_or("").to_owned(),
            ))
        })
        .collect()
}

async fn open_chat(
    cdp: &mut Cdp,
    config: &VideoObserverConfig,
    account: Option<&str>,
) -> Result<bool, ObserverError> {
    cdp.page("Page.navigate", json!({ "url": config.chat_url }))
        .await?;
    sleep(LOAD).await;
    Ok(match status(cdp, config.step()).await? {
        Status::SignedIn(active) => account.is_none_or(|wanted| active.as_deref() == Some(wanted)),
        Status::SignedOut => false,
    })
}

async fn status(cdp: &mut Cdp, step: Duration) -> Result<Status, ObserverError> {
    let deadline = Instant::now() + step;
    loop {
        let href = cdp.eval("location.href").await?;
        if !href.as_str().is_some_and(|url| url.starts_with(GEMINI)) {
            return Ok(Status::SignedOut);
        }
        let ready = cdp.eval(READY).await?;
        if ready.as_str() == Some("login") {
            return Ok(Status::SignedOut);
        }
        let active = cdp.eval(ACCOUNT).await?.as_str().map(str::to_owned);
        if ready.as_str() == Some("ready") && active.is_some() {
            return Ok(Status::SignedIn(active));
        }
        if Instant::now() >= deadline {
            return match ready.as_str() {
                Some("ready") => Ok(Status::SignedIn(active)),
                _ => Err(ObserverError::Timeout),
            };
        }
        sleep(Duration::from_millis(500)).await;
    }
}

async fn sign_in(
    cdp: &mut Cdp,
    config: &VideoObserverConfig,
    passkey: &Passkey,
) -> Result<bool, ObserverError> {
    let script = cdp
        .page(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": SPOOF }),
        )
        .await?["identifier"]
        .as_str()
        .map(str::to_owned);
    let login = format!("{SIGN_IN}{}", encode(&config.chat_url));
    cdp.page("Page.navigate", json!({ "url": login })).await?;
    sleep(LOAD).await;
    let result = match arm(cdp, passkey).await {
        Ok(authenticator) => {
            let walked = walk(cdp, config, passkey).await;
            let _ = cdp
                .page(
                    "WebAuthn.removeVirtualAuthenticator",
                    json!({ "authenticatorId": authenticator }),
                )
                .await;
            walked
        }
        Err(error) => Err(error),
    };
    let _ = cdp.page("WebAuthn.disable", json!({})).await;
    if let Some(identifier) = script {
        let _ = cdp
            .page(
                "Page.removeScriptToEvaluateOnNewDocument",
                json!({ "identifier": identifier }),
            )
            .await;
    }
    result
}

async fn arm(cdp: &mut Cdp, passkey: &Passkey) -> Result<String, ObserverError> {
    cdp.page("WebAuthn.enable", json!({ "enableUI": false }))
        .await?;
    let authenticator = cdp
        .page(
            "WebAuthn.addVirtualAuthenticator",
            json!({ "options": {
                "protocol": "ctap2",
                "transport": "internal",
                "hasResidentKey": true,
                "hasUserVerification": true,
                "isUserVerified": true,
                "automaticPresenceSimulation": true
            } }),
        )
        .await?["authenticatorId"]
        .as_str()
        .ok_or(ObserverError::Ui)?
        .to_owned();
    for credential in &passkey.credentials {
        let filtered: Map<String, Value> = CREDENTIAL_FIELDS
            .iter()
            .filter_map(|field| Some(((*field).to_owned(), credential.get(*field)?.clone())))
            .collect();
        cdp.page(
            "WebAuthn.addCredential",
            json!({ "authenticatorId": authenticator, "credential": filtered }),
        )
        .await?;
    }
    cdp.page("Page.reload", json!({})).await?;
    sleep(LOAD).await;
    Ok(authenticator)
}

async fn walk(
    cdp: &mut Cdp,
    config: &VideoObserverConfig,
    passkey: &Passkey,
) -> Result<bool, ObserverError> {
    let account = passkey.account.to_lowercase();
    for _ in 0..LOGIN_STEPS {
        let href = cdp.eval("location.href").await?;
        if href.as_str().is_some_and(|url| url.starts_with(GEMINI)) {
            return Ok(matches!(
                status(cdp, config.step()).await?,
                Status::SignedIn(Some(active)) if active == account
            ));
        }
        advance(cdp, &passkey.account).await?;
        sleep(STEP_PAUSE).await;
    }
    Ok(false)
}

async fn advance(cdp: &mut Cdp, account: &str) -> Result<(), ObserverError> {
    if cdp.eval(EMPTY_IDENTIFIER).await? == json!(true) {
        if let Some(point) = gemini::find(cdp, "#identifierId", None).await? {
            cdp.click(point).await?;
        }
        cdp.type_text(account, KEY_DELAY).await?;
        sleep(Duration::from_millis(800)).await;
        return cdp.press("Enter", "Enter", 13).await;
    }
    let chooser = format!("[data-identifier={}]", json!(account));
    if let Some(point) = gemini::find(cdp, &chooser, None).await? {
        return cdp.click(point).await;
    }
    for choice in CHOICES {
        if let Some(point) = gemini::find(cdp, CLICKABLE, Some(choice)).await? {
            return cdp.click(point).await;
        }
    }
    Ok(())
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_config() -> VideoObserverConfig {
        VideoObserverConfig {
            steel_endpoint: "http://127.0.0.1:3000".into(),
            devtools_endpoint: "http://127.0.0.1:9223".into(),
            chat_url: "https://gemini.google.com/app".into(),
            model_item: "4 Argon".into(),
            thinking_menu: "Thinking level".into(),
            thinking_item: "High".into(),
            active_label: "Argon High".into(),
            max_video_bytes: 1024,
            max_answer_bytes: 24_000,
            step_timeout_ms: 45_000,
            upload_timeout_ms: 180_000,
            response_timeout_ms: 420_000,
            total_deadline_ms: 720_000,
        }
    }

    #[tokio::test]
    #[ignore = "needs the live Steel browser and GOOGLE_STEEL_PASSKEY"]
    async fn signs_in_a_fresh_browser_context_with_the_passkey() {
        let config = live_config();
        let passkey = load_passkey().expect("passkey");
        let url = browser_socket(&config.devtools_endpoint, config.step())
            .await
            .expect("devtools");
        let mut cdp = Cdp::connect(&url, config.step()).await.expect("connect");
        let context = cdp
            .browser("Target.createBrowserContext", json!({}))
            .await
            .expect("context")["browserContextId"]
            .clone();
        let target = cdp
            .browser(
                "Target.createTarget",
                json!({ "url": "about:blank", "browserContextId": context }),
            )
            .await
            .expect("target")["targetId"]
            .as_str()
            .expect("target id")
            .to_owned();
        cdp.attach_page(&target).await.expect("attach");
        let signed = sign_in(&mut cdp, &config, &passkey).await;
        let _ = cdp
            .browser(
                "Target.disposeBrowserContext",
                json!({ "browserContextId": context }),
            )
            .await;
        assert_eq!(signed, Ok(true));
    }

    #[tokio::test]
    #[ignore = "needs the live Steel browser"]
    async fn keeps_the_live_session_on_one_signed_in_tab() {
        let config = live_config();
        let session = check_session(&config).await;
        assert!(session.is_ok(), "{session:?}");
        let url = browser_socket(&config.devtools_endpoint, config.step())
            .await
            .expect("devtools");
        let mut cdp = Cdp::connect(&url, config.step()).await.expect("connect");
        let listed = cdp
            .browser("Target.getTargets", json!({}))
            .await
            .expect("targets");
        assert_eq!(pages(&listed).len(), 1);
    }

    #[test]
    fn rewrites_the_advertised_socket_to_the_devtools_port() {
        let version = json!({ "webSocketDebuggerUrl": "ws://127.0.0.1/devtools/browser/abc" });
        assert_eq!(
            socket_url("http://127.0.0.1:9223", &version).as_deref(),
            Some("ws://127.0.0.1:9223/devtools/browser/abc")
        );
    }

    #[test]
    fn keeps_only_page_targets() {
        let listed = json!({ "targetInfos": [
            { "targetId": "a", "type": "page", "url": "https://gemini.google.com/app" },
            { "targetId": "b", "type": "iframe", "url": "https://accounts.google.com/x" },
            { "targetId": "c", "type": "page", "url": "chrome-error://chromewebdata/" }
        ] });
        let ids: Vec<String> = pages(&listed).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["a", "c"]);
    }

    #[test]
    fn encodes_the_continue_url() {
        assert_eq!(
            encode("https://gemini.google.com/app"),
            "https%3A%2F%2Fgemini.google.com%2Fapp"
        );
    }
}
