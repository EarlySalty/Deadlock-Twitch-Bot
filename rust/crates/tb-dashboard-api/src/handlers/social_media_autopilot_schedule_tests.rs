use super::*;
use axum::body::to_bytes;
use serde_json::{json, Value};

#[test]
fn schedule_body_rejects_foreign_settings_and_preserves_false_values() {
    assert!(serde_json::from_value::<ScheduleWrite>(json!({"publish_mode":"full_auto"})).is_err());
    assert!(
        serde_json::from_value::<ScheduleWrite>(json!({"clip_date_filter":{"kind":"all"}}))
            .is_err()
    );
    let body: ScheduleWrite = serde_json::from_value(
        json!({"twitch_user_id":"42","enabled":false,"ai_editing_enabled":false}),
    )
    .expect("narrow schedule request");
    let patch = body.patch();
    assert_eq!(patch.enabled, Some(false));
    assert_eq!(patch.ai_editing_enabled, Some(false));
    assert!(patch.timezone.is_none());
}

#[test]
fn queue_actions_do_not_accept_mode_or_another_job_payload() {
    assert!(serde_json::from_value::<QueueActionWrite>(json!({"mode":"static"})).is_err());
    assert!(
        serde_json::from_value::<QueueOrderWrite>(json!({"job_ids":[1,2],"enabled":true})).is_err()
    );
    assert!(serde_json::from_value::<QueueOrderWrite>(json!({"job_ids":["1"]})).is_err());
}

#[tokio::test]
async fn handler_errors_keep_stable_status_and_safe_response_shapes() {
    for (error, status, code) in [
        (
            ScheduleError::NotFound,
            StatusCode::NOT_FOUND,
            "job_not_found",
        ),
        (ScheduleError::Started, StatusCode::CONFLICT, "job_started"),
        (
            ScheduleError::QueueChanged,
            StatusCode::CONFLICT,
            "queue_changed",
        ),
        (
            ScheduleError::Invalid("invalid".into()),
            StatusCode::BAD_REQUEST,
            "invalid_schedule",
        ),
    ] {
        let response = failure(error);
        assert_eq!(response.status(), status);
        let bytes = to_bytes(response.into_body(), 4096)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON response");
        assert_eq!(body["error"], code);
        assert!(body["message"].is_string());
    }
}
