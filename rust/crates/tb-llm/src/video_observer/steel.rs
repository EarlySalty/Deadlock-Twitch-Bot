use std::time::Duration;

use serde_json::Value;

use super::ObserverError;

pub(super) struct SteelSession {
    client: reqwest::Client,
    endpoint: String,
    id: String,
}

impl SteelSession {
    pub(super) async fn attach(endpoint: &str, step: Duration) -> Result<Self, ObserverError> {
        let client = reqwest::Client::builder()
            .timeout(step)
            .build()
            .map_err(|_| ObserverError::Unavailable)?;
        let listed: Value = client
            .get(format!("{endpoint}/v1/sessions"))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| ObserverError::Unavailable)?
            .json()
            .await
            .map_err(|_| ObserverError::Unavailable)?;
        let id = running_session(&listed).ok_or(ObserverError::Unavailable)?;
        Ok(Self {
            client,
            endpoint: endpoint.to_owned(),
            id,
        })
    }

    pub(super) fn websocket_url(&self) -> String {
        format!(
            "ws://{}/",
            self.endpoint
                .trim_start_matches("http://")
                .trim_end_matches('/')
        )
    }

    pub(super) async fn upload(
        &self,
        name: &str,
        bytes: &[u8],
        timeout: Duration,
    ) -> Result<String, ObserverError> {
        let boundary = format!("steel{}", self.id.replace('-', ""));
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"path\"\r\n\r\n{name}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: video/mp4\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let stored: Value = self
            .client
            .post(format!("{}/v1/sessions/{}/files", self.endpoint, self.id))
            .timeout(timeout)
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| ObserverError::Unavailable)?
            .json()
            .await
            .map_err(|_| ObserverError::Unavailable)?;
        stored["path"]
            .as_str()
            .filter(|path| path.starts_with('/'))
            .map(str::to_owned)
            .ok_or(ObserverError::Unavailable)
    }

    pub(super) async fn clear_files(self) {
        let _ = self
            .client
            .delete(format!("{}/v1/sessions/{}/files", self.endpoint, self.id))
            .send()
            .await;
    }
}

fn running_session(listed: &Value) -> Option<String> {
    listed["sessions"]
        .as_array()?
        .iter()
        .filter(|session| session["status"].as_str() != Some("released"))
        .filter_map(|session| session["id"].as_str())
        .find(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn attaches_only_to_a_running_session() {
        let listed = json!({ "sessions": [
            { "id": "aaaa-1111", "status": "released" },
            { "id": "bbbb-2222", "status": "idle" }
        ] });
        assert_eq!(running_session(&listed).as_deref(), Some("bbbb-2222"));
        assert_eq!(
            running_session(&json!({ "sessions": [{ "id": "aaaa", "status": "released" }] })),
            None
        );
    }
}
