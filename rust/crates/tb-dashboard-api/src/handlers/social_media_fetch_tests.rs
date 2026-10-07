use super::*;
use tb_social_media::clip::model::StreamerFetchResult;

#[tokio::test]
async fn fetch_error_is_not_reported_as_success() {
    let response = clip_fetch_response(
        StreamerFetchResult {
            clips_found: 3,
            error: Some("upstream_failure".into()),
            ..Default::default()
        },
        json!({"genutzt": 3}),
    );
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["success"], false);
    assert_eq!(value["error"], "upstream_failure");
    assert_eq!(value["clips_found"], 3);
    assert_eq!(value["kontingent"]["genutzt"], 3);
}

#[tokio::test]
async fn empty_fetch_is_a_success() {
    let response = clip_fetch_response(StreamerFetchResult::default(), json!({"genutzt": 0}));
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["success"], true);
    assert_eq!(value["clips_found"], 0);
    assert!(value.get("error").is_none());
}
