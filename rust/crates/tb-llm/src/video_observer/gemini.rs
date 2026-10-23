use std::time::Duration;

use serde_json::{json, Value};
use tokio::time::{sleep, Instant};

use super::{
    cdp::{Cdp, Point},
    ObserverError, VideoObserverConfig,
};

const PICKER: &str = "button[aria-label^='Open mode picker']";
const MENU_ITEM: &str = "gem-menu-item, [role=menuitem]";
const UPLOAD_MENU: &str = "button[aria-label='Upload & tools']";
const UPLOAD_ITEM: &str = "[role=menuitem], button";
const FIND: &str = r#"((selector,text)=>{const el=[...document.querySelectorAll(selector)].find(e=>e.getClientRects().length&&(text===null||e.innerText.split('\n').map(s=>s.trim()).includes(text)));if(!el)return null;el.scrollIntoView({block:'center'});const r=el.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})"#;
const READY: &str = r#"(()=>{const prompt=document.querySelector("rich-textarea div[contenteditable='true'],div[contenteditable='true'][role='textbox'],textarea");const login=[...document.querySelectorAll('a[href*="accounts.google.com"]')].some(a=>a.getClientRects().length&&/^(sign in|anmelden)$/i.test(a.innerText.trim()));return login?'login':(prompt?'ready':'loading');})()"#;
const UPLOAD_STATE: &str = r#"(()=>{if(!document.querySelector('uploader-file-preview'))return 'missing';return document.querySelector('.gem-attachment-loading-container,.gem-attachment-content.loading')?'loading':'ready';})()"#;
const FOCUS: &str = r#"(()=>{const e=document.querySelector("rich-textarea div[contenteditable='true'],div[contenteditable='true'][role='textbox'],textarea");if(!e)return false;e.focus();return true;})()"#;
const COUNT: &str = "document.querySelectorAll('message-content').length";
const OBSERVE: &str = r#"(()=>{const a=[...document.querySelectorAll('message-content')];const errors=[...document.querySelectorAll('[role=alert],error-banner,error-message,mat-snack-bar-container')].filter(e=>e.getClientRects().length&&!e.closest('message-content,user-query,rich-textarea')).map(e=>e.innerText);return {count:a.length,text:a.length?a[a.length-1].innerText:'',errors,streaming:[...document.querySelectorAll("button[aria-label*='Stop'],button[aria-label*='stoppen'],model-response mat-progress-spinner,message-content mat-progress-spinner")].some(e=>e.getClientRects().length)};})()"#;
const POLL: Duration = Duration::from_millis(500);
const SETTLE: Duration = Duration::from_millis(1200);
const STABLE: Duration = Duration::from_secs(4);

pub(super) async fn ask(
    cdp: &mut Cdp,
    config: &VideoObserverConfig,
    path: &str,
    prompt: &str,
) -> Result<String, ObserverError> {
    let target = cdp.open_page(&config.chat_url).await?;
    let result = conversation(cdp, config, path, prompt).await;
    cdp.close_page(&target).await;
    result
}

async fn conversation(
    cdp: &mut Cdp,
    config: &VideoObserverConfig,
    path: &str,
    prompt: &str,
) -> Result<String, ObserverError> {
    wait_ready(cdp, config).await?;
    select_model(cdp, config).await?;
    attach(cdp, config, path).await?;
    let before = submit(cdp, config, prompt).await?;
    let answer = response(cdp, config, before).await?;
    Ok(truncate(clean(&answer), config.max_answer_bytes))
}

async fn wait_ready(cdp: &mut Cdp, config: &VideoObserverConfig) -> Result<(), ObserverError> {
    let deadline = Instant::now() + config.step();
    loop {
        match cdp.eval(READY).await?.as_str() {
            Some("ready") => return Ok(()),
            Some("login") => return Err(ObserverError::NotLoggedIn),
            _ if Instant::now() >= deadline => return Err(ObserverError::NotLoggedIn),
            _ => sleep(POLL).await,
        }
    }
}

async fn select_model(cdp: &mut Cdp, config: &VideoObserverConfig) -> Result<(), ObserverError> {
    if active(cdp, config).await? {
        return Ok(());
    }
    click_when(cdp, PICKER, None, config.step()).await?;
    click_when(cdp, MENU_ITEM, Some(&config.model_item), config.step())
        .await
        .map_err(|_| ObserverError::ModelUnavailable)?;
    sleep(SETTLE).await;
    click_when(cdp, PICKER, None, config.step()).await?;
    sleep(SETTLE).await;
    if let Some(point) = find(cdp, MENU_ITEM, Some(&config.thinking_menu)).await? {
        cdp.click(point).await?;
        sleep(SETTLE).await;
    }
    click_when(cdp, MENU_ITEM, Some(&config.thinking_item), config.step())
        .await
        .map_err(|_| ObserverError::ModelUnavailable)?;
    sleep(SETTLE).await;
    cdp.press("Escape", "Escape", 27).await?;
    let deadline = Instant::now() + config.step();
    while !active(cdp, config).await? {
        if Instant::now() >= deadline {
            return Err(ObserverError::ModelUnavailable);
        }
        sleep(POLL).await;
    }
    Ok(())
}

async fn active(cdp: &mut Cdp, config: &VideoObserverConfig) -> Result<bool, ObserverError> {
    let expression = format!(
        "document.querySelector({})?.getAttribute('aria-label')||''",
        json!(PICKER)
    );
    Ok(cdp
        .eval(&expression)
        .await?
        .as_str()
        .is_some_and(|label| label.trim_end().ends_with(config.active_label.as_str())))
}

async fn attach(
    cdp: &mut Cdp,
    config: &VideoObserverConfig,
    path: &str,
) -> Result<(), ObserverError> {
    cdp.page(
        "Page.setInterceptFileChooserDialog",
        json!({ "enabled": true }),
    )
    .await?;
    click_when(cdp, UPLOAD_MENU, None, config.step()).await?;
    sleep(SETTLE).await;
    click_when(cdp, UPLOAD_ITEM, Some("Upload files"), config.step()).await?;
    let chooser = cdp
        .wait_event("Page.fileChooserOpened", config.step())
        .await?;
    let node = chooser["params"]["backendNodeId"]
        .as_i64()
        .ok_or(ObserverError::Ui)?;
    cdp.page(
        "DOM.setFileInputFiles",
        json!({ "files": [path], "backendNodeId": node }),
    )
    .await?;
    let deadline = Instant::now() + Duration::from_millis(config.upload_timeout_ms);
    loop {
        if cdp.eval(UPLOAD_STATE).await?.as_str() == Some("ready") {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ObserverError::Timeout);
        }
        sleep(POLL).await;
    }
}

async fn submit(
    cdp: &mut Cdp,
    config: &VideoObserverConfig,
    prompt: &str,
) -> Result<u64, ObserverError> {
    let deadline = Instant::now() + config.step();
    while cdp.eval(FOCUS).await? != json!(true) {
        if Instant::now() >= deadline {
            return Err(ObserverError::Ui);
        }
        sleep(POLL).await;
    }
    cdp.page("Input.insertText", json!({ "text": prompt }))
        .await?;
    sleep(SETTLE).await;
    let before = cdp.eval(COUNT).await?.as_u64().ok_or(ObserverError::Ui)?;
    cdp.press("Enter", "Enter", 13).await?;
    Ok(before)
}

async fn response(
    cdp: &mut Cdp,
    config: &VideoObserverConfig,
    before: u64,
) -> Result<String, ObserverError> {
    let deadline = Instant::now() + Duration::from_millis(config.response_timeout_ms);
    let mut stable = Instant::now();
    let mut last = String::new();
    while Instant::now() < deadline {
        if let Some(point) = find(cdp, "button", Some("Agree")).await? {
            cdp.click(point).await?;
        }
        let state = cdp.eval(OBSERVE).await?;
        if let Some(error) = visible_failure(&state) {
            return Err(error);
        }
        let text = if state["count"].as_u64().unwrap_or(0) > before {
            state["text"].as_str().unwrap_or("").trim()
        } else {
            ""
        };
        if !text.is_empty() && text != last {
            last = text.to_owned();
            stable = Instant::now();
        }
        if !last.is_empty() && stable.elapsed() >= STABLE && state["streaming"] == json!(false) {
            return Ok(last);
        }
        sleep(Duration::from_secs(1)).await;
    }
    Err(ObserverError::Timeout)
}

async fn find(
    cdp: &mut Cdp,
    selector: &str,
    text: Option<&str>,
) -> Result<Option<Point>, ObserverError> {
    let expression = format!("{FIND}({},{})", json!(selector), json!(text));
    Ok(Point::from_value(&cdp.eval(&expression).await?))
}

async fn click_when(
    cdp: &mut Cdp,
    selector: &str,
    text: Option<&str>,
    timeout: Duration,
) -> Result<(), ObserverError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(point) = find(cdp, selector, text).await? {
            return cdp.click(point).await;
        }
        if Instant::now() >= deadline {
            return Err(ObserverError::Ui);
        }
        sleep(POLL).await;
    }
}

fn visible_failure(state: &Value) -> Option<ObserverError> {
    state.get("errors")?.as_array()?.iter().find_map(|value| {
        let text = value.as_str()?.to_lowercase();
        if [
            "rate limit",
            "too many requests",
            "quota",
            "try again later",
            "später erneut",
            "zu viele anfragen",
        ]
        .iter()
        .any(|marker| text.contains(marker))
        {
            Some(ObserverError::RateLimited)
        } else if [
            "i can't assist",
            "i can’t assist",
            "i cannot assist",
            "dabei kann ich nicht helfen",
        ]
        .iter()
        .any(|marker| text.contains(marker))
        {
            Some(ObserverError::Refused)
        } else {
            None
        }
    })
}

fn clean(answer: &str) -> String {
    answer
        .lines()
        .map(str::trim_end)
        .filter(|line| !is_citation(line.trim()))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn is_citation(line: &str) -> bool {
    line == "MP4"
        || line
            .strip_prefix("+ ")
            .is_some_and(|count| !count.is_empty() && count.bytes().all(|b| b.is_ascii_digit()))
}

fn truncate(mut text: String, max_bytes: usize) -> String {
    if text.len() > max_bytes {
        let mut end = max_bytes;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_text_is_never_a_browser_failure() {
        assert_eq!(
            visible_failure(&json!({ "text": "I can't assist", "errors": [] })),
            None
        );
        assert_eq!(
            visible_failure(&json!({ "errors": ["Too many requests; try again later"] })),
            Some(ObserverError::RateLimited)
        );
    }

    #[test]
    fn citation_chips_are_removed() {
        assert_eq!(
            clean("00:01: Warden rolls.   \nMP4\n\nKeep it.\nMP4\n+ 1\n+ 1 more"),
            "00:01: Warden rolls.\n\nKeep it.\n+ 1 more"
        );
    }

    #[test]
    fn truncation_keeps_utf8_boundaries() {
        assert_eq!(truncate("äöü".to_owned(), 3), "ä");
    }
}
