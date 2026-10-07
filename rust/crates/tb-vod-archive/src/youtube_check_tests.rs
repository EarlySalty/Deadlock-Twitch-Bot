use super::*;

async fn synthetic_manager(pool: &PgPool, auth_id: i32) -> CredentialManager {
    use std::sync::Arc;
    use tb_crypto::{aad, FieldCipher};

    let cipher = Arc::new(FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap());
    let encrypted = cipher
        .encrypt_field(
            "synthetic-access",
            &aad::social_media("access_token", "youtube", Some("synthetic"), 1),
        )
        .unwrap();
    sqlx::query("INSERT INTO social_media_platform_auth(id,platform,twitch_user_id,platform_user_id,streamer_login,access_token_enc) VALUES ($1,'youtube','42','channel','synthetic',$2)")
        .bind(auth_id).bind(encrypted).execute(pool).await.unwrap();
    CredentialManager::new(pool.clone(), cipher)
}

async fn local_snapshot(pool: &PgPool) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_array((SELECT jsonb_agg(to_jsonb(v) ORDER BY id) FROM twitch_vod_archive_vods v),(SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM twitch_vod_archive_parts p))")
        .fetch_one(pool).await.unwrap()
}

fn provider_video(id: &str, description: &str, seconds: u32, state: &str) -> Value {
    json!({
        "id": id,
        "snippet": {"channelId": "channel", "title": "Identical title", "description": description},
        "contentDetails": {"duration": format!("PT{seconds}S")},
        "status": {"uploadStatus": state, "privacyStatus": "private"}
    })
}

#[tokio::test]
async fn existing_ids_are_reconciled_independently_of_all_legacy_part_states() {
    use tb_social_media::uploaders::youtube::YouTubeUploader;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    let pool = crate::store::tests::pool("t_youtube_legacy_ids")
        .await
        .expect("isolated token_db test configuration required");
    let manager = synthetic_manager(&pool, 101).await;
    for (index, local_state) in ["pending", "failed", "uploading", "done"]
        .iter()
        .enumerate()
    {
        let twitch_id = format!("{}", 100 + index);
        crate::store::merke_vod(&pool, &twitch_id, "synthetic", "42", "Identical title", 120)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_vod_archive_parts (vod_id,part_index,file_path,status,youtube_video_id,last_error,upload_offset) SELECT id,0,$2,$3,$4,'synthetic recovery',10 FROM twitch_vod_archive_vods WHERE twitch_id=$1")
            .bind(&twitch_id).bind(format!("/synthetic/{index}.mp4")).bind(local_state)
            .bind(format!("video{index}")).execute(&pool).await.unwrap();
    }
    crate::store::merke_vod(&pool, "200", "synthetic", "42", "Identical title", 480)
        .await
        .unwrap();
    for (index, local_state) in ["pending", "failed", "uploading", "done"]
        .iter()
        .enumerate()
    {
        sqlx::query("INSERT INTO twitch_vod_archive_parts (vod_id,part_index,file_path,status,youtube_video_id,last_error) SELECT id,$1,'/synthetic/multipart.mp4',$2,$3,'synthetic recovery' FROM twitch_vod_archive_vods WHERE twitch_id='200'")
            .bind(index as i32).bind(local_state).bind(format!("multipart{index}"))
            .execute(&pool).await.unwrap();
    }
    sqlx::query(
        "UPDATE twitch_vod_archive_vods SET status='archived',last_error='synthetic recovery'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let before = local_snapshot(&pool).await;
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":"channel","contentDetails":{"relatedPlaylists":{"uploads":"uploads"}}}]})))
        .expect(1).mount(&server).await;
    let mut items = (0..4)
        .map(|i| provider_video(&format!("multipart{i}"), "", 120, "processed"))
        .collect::<Vec<_>>();
    for (index, state) in ["processed", "uploaded", "rejected"].iter().enumerate() {
        items.push(provider_video(&format!("video{index}"), "", 120, state));
    }
    Mock::given(method("GET"))
        .and(path("/videos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":items})))
        .expect(1)
        .mount(&server)
        .await;
    let config = VodArchiveOptions {
        youtube_requests_per_run: 3,
        ..Default::default()
    };
    run_with(&pool, &manager, &config, |credentials| {
        YouTubeUploader::new(&credentials.access_token).with_bases(server.uri(), server.uri())
    })
    .await
    .unwrap();
    let states: Vec<(String, String, bool, i32)> = sqlx::query_as("SELECT v.twitch_id,c.state,c.complete,jsonb_array_length(c.observations) FROM twitch_vod_archive_vods v JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id ORDER BY v.twitch_id")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(
        states,
        vec![
            ("100".into(), "confirmed".into(), true, 1),
            ("101".into(), "processing".into(), false, 1),
            ("102".into(), "rejected".into(), false, 1),
            ("103".into(), "unavailable".into(), false, 1),
            ("200".into(), "confirmed".into(), true, 4),
        ]
    );
    assert_eq!(before, local_snapshot(&pool).await);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.method.as_str() == "GET"));
    eprintln!("YOUTUBE_DB_PROOF: existing IDs with pending, failed, uploading and done legacy states persist processed, processing, rejected and unavailable observations; multipart coverage independent of local completion; all local rows and queues unchanged");
}

#[tokio::test]
async fn minimum_budget_advances_mixed_ids_and_preserves_completed_inventory_for_known_reads() {
    use tb_social_media::uploaders::youtube::YouTubeUploader;
    use wiremock::{
        matchers::{method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    let pool = crate::store::tests::pool("t_youtube_mixed_budget")
        .await
        .expect("isolated token_db test configuration required");
    let manager = synthetic_manager(&pool, 102).await;
    for index in 0..51 {
        let twitch_id = format!("{}", 100 + index);
        crate::store::merke_vod(&pool, &twitch_id, "synthetic", "42", "Identical title", 120)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_vod_archive_parts (vod_id,part_index,file_path,status,youtube_video_id) SELECT id,0,'/synthetic/known.mp4','pending',$2 FROM twitch_vod_archive_vods WHERE twitch_id=$1")
            .bind(twitch_id).bind(format!("known{index:02}")).execute(&pool).await.unwrap();
    }
    for twitch_id in ["900", "901"] {
        crate::store::merke_vod(&pool, twitch_id, "synthetic", "42", "Identical title", 120)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO twitch_vod_archive_parts (vod_id,part_index,file_path,status,youtube_video_id) SELECT id,0,'/synthetic/mixed-0.mp4','failed','mixed-known' FROM twitch_vod_archive_vods WHERE twitch_id='900'")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_vod_archive_parts (vod_id,part_index,file_path,status) SELECT id,1,'/synthetic/mixed-1.mp4','pending' FROM twitch_vod_archive_vods WHERE twitch_id='900'")
        .execute(&pool).await.unwrap();
    sqlx::query(
        "UPDATE twitch_vod_archive_vods SET status='archived',last_error='synthetic recovery'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let before = local_snapshot(&pool).await;
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":"channel","contentDetails":{"relatedPlaylists":{"uploads":"uploads"}}}]})))
        .expect(3).mount(&server).await;
    Mock::given(method("GET")).and(path("/playlistItems"))
        .and(|request: &wiremock::Request| !request.url.query_pairs().any(|(key, _)| key == "pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"contentDetails":{"videoId":"mixed-missing"}}],"nextPageToken":"second"})))
        .expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/playlistItems"))
        .and(query_param("pageToken", "second"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"items":[{"contentDetails":{"videoId":"historical"}}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/videos"))
        .respond_with(|request: &wiremock::Request| {
            let ids = request.url.query_pairs().find(|(key, _)| key == "id").unwrap().1;
            let items = ids.split(',').map(|id| match id {
                "mixed-missing" => provider_video(id, "Archivquelle: Twitch-VOD 900; Teil 2/2\nOriginal: https://www.twitch.tv/videos/900", 60, "processed"),
                "historical" => provider_video(id, "Original: https://www.twitch.tv/videos/901", 120, "processed"),
                "mixed-known" => provider_video(id, "", 60, "processed"),
                _ => provider_video(id, "", 120, "processed"),
            }).collect::<Vec<_>>();
            ResponseTemplate::new(200).set_body_json(json!({"items":items}))
        }).expect(4).mount(&server).await;
    let config = VodArchiveOptions {
        youtube_requests_per_run: 3,
        ..Default::default()
    };
    let factory = |credentials: &tb_social_media::credentials::SocialMediaCredentials| {
        YouTubeUploader::new(&credentials.access_token).with_bases(server.uri(), server.uri())
    };
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let first: (Option<String>, bool, i64) = sqlx::query_as(
        "SELECT cursor,complete,generation FROM twitch_vod_youtube_scans WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(first.0.as_deref(), Some("second"));
    assert!(!first.1);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM twitch_vod_youtube_checks WHERE complete"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    sqlx::query("UPDATE twitch_vod_youtube_checks SET next_check_at=NOW()")
        .execute(&pool)
        .await
        .unwrap();
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let second: (Option<String>, bool, i64) = sqlx::query_as(
        "SELECT cursor,complete,generation FROM twitch_vod_youtube_scans WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(second, (None, true, first.2));
    assert_eq!(server.received_requests().await.unwrap().len(), 6);
    let mixed: (String, bool, bool) = sqlx::query_as("SELECT c.state,c.complete,c.next_check_at<=NOW()+INTERVAL '11 minutes' FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE v.twitch_id='900'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(mixed, ("partial".into(), false, true));
    sqlx::query("UPDATE twitch_vod_youtube_checks SET next_check_at=NOW() WHERE NOT complete")
        .execute(&pool)
        .await
        .unwrap();
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM twitch_vod_youtube_checks WHERE complete AND state='confirmed'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        53
    );
    let third: (Option<String>, bool, i64) = sqlx::query_as(
        "SELECT cursor,complete,generation FROM twitch_vod_youtube_scans WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(third, second);
    assert_eq!(before, local_snapshot(&pool).await);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 9);
    assert!(requests
        .iter()
        .all(|request| request.method.as_str() == "GET"));
    eprintln!("YOUTUBE_DB_PROOF: minimum three-read budget reserves durable playlist progress despite 52 known IDs; two-page scan completed, inventory retained for subsequent direct-ID batches, mixed known/missing parts confirmed without fabricating local completion or timestamps; nine GETs in three bounded runs");
}

fn observation(id: &str, index: Option<i32>, total: Option<i32>, seconds: i64) -> Beobachtung {
    Beobachtung {
        video_id: id.into(),
        part_index: index,
        part_total: total,
        duration_sec: Some(seconds),
        state: "processed".into(),
        privacy: Some("private".into()),
        observed_at: DateTime::parse_from_rfc3339("2026-10-08T01:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
    }
}

fn synthetic_vod() -> PruefVod {
    PruefVod {
        id: 1,
        twitch_id: "123".into(),
        duration_sec: 120,
        snapshot: json!([]),
        parts: json!([]),
    }
}

#[test]
fn source_identity_rejects_titles_and_conflicting_sources() {
    let mut video = ArchivVideo {
        id: "abc".into(),
        channel_id: "channel".into(),
        title: "Gleicher Titel".into(),
        description: String::new(),
        duration_sec: Some(120),
        state: "processed".into(),
        privacy: None,
    };
    assert!(source(&video).is_none());
    video.description = "Original: https://www.twitch.tv/videos/123".into();
    assert_eq!(source(&video), Some(("123".into(), None)));
    video.title = "Gleicher Titel (Teil 2/3)".into();
    assert_eq!(source(&video), Some(("123".into(), Some((1, 3)))));
    video
        .description
        .push_str("\nhttps://www.twitch.tv/videos/456");
    assert!(source(&video).is_none());
    video.description = "https://www.twitch.tv/videos/123abc".into();
    assert!(source(&video).is_none());
    video.description =
        "Archivquelle: Twitch-VOD 123; Teil 0/2\nOriginal: https://www.twitch.tv/videos/123".into();
    assert!(source(&video).is_none());
    video.description =
        "Archivquelle: Twitch-VOD 123; Teil 1/3\nOriginal: https://www.twitch.tv/videos/123".into();
    assert!(source(&video).is_none());
    assert_eq!(
        read_error(&UploadError::Request("timeout".into())),
        "request"
    );
    assert_eq!(
        read_error(&UploadError::Api("HTTP 403".into())),
        "connection"
    );
}

#[test]
fn full_coverage_requires_completed_scan_unique_parts_and_duration() {
    let vod = synthetic_vod();
    let full = vec![observation("abc", None, None, 120)];
    assert_eq!(decision(&vod, &full, true, false), ("confirmed", true));
    assert_eq!(decision(&vod, &full, false, false), ("partial", false));
    assert_eq!(
        decision(&vod, &[observation("abc", None, None, 60)], true, false),
        ("partial", false)
    );
    let parts = vec![
        observation("abc", Some(0), Some(2), 60),
        observation("def", Some(1), Some(2), 60),
    ];
    assert_eq!(decision(&vod, &parts, true, false), ("confirmed", true));
    assert_eq!(decision(&vod, &parts[..1], true, false), ("partial", false));
    let duplicate = vec![parts[0].clone(), parts[0].clone()];
    assert_eq!(decision(&vod, &duplicate, true, false), ("partial", false));
    let mut states = parts.clone();
    for (state, expected) in [
        ("processing", "processing"),
        ("rejected", "rejected"),
        ("unavailable", "unavailable"),
    ] {
        states[1].state = state.into();
        assert_eq!(decision(&vod, &states, true, false), (expected, false));
    }
    assert_eq!(decision(&vod, &[], false, false), ("searching", false));
    assert_eq!(decision(&vod, &[], true, false), ("unresolved", false));
}

#[tokio::test]
async fn postgres_evidence_errors_and_stale_writers_are_separate_from_uploads() {
    let pool = crate::store::tests::pool("t_youtube_evidence")
        .await
        .expect("isolated token_db test configuration required");
    sqlx::query("INSERT INTO social_media_platform_auth(platform,twitch_user_id,platform_user_id) VALUES ('youtube','42','channel')").execute(&pool).await.unwrap();
    crate::store::merke_vod(&pool, "123", "synthetic", "42", "Identical title", 120)
        .await
        .unwrap();
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='archived' WHERE twitch_id='123'")
        .execute(&pool)
        .await
        .unwrap();
    let target = ziel(&pool, "42").await.unwrap().unwrap();
    let vod = vods(&pool, "42", &target).await.unwrap().remove(0);
    let mut proof = observation("abc", None, None, 120);
    proof.observed_at = Utc::now();
    assert!(save(
        &pool,
        "42",
        &target,
        Some("channel"),
        &vod,
        &[proof.clone()],
        "confirmed",
        true,
        None,
        86400
    )
    .await
    .unwrap());
    let (uploaded,parts): (Option<DateTime<Utc>>,i64)=sqlx::query_as("SELECT uploaded_at,(SELECT COUNT(*) FROM twitch_vod_archive_parts) FROM twitch_vod_archive_vods WHERE id=$1").bind(vod.id).fetch_one(&pool).await.unwrap();
    assert!(uploaded.is_none());
    assert_eq!(parts, 0);
    let before: Value=sqlx::query_scalar("SELECT jsonb_build_array(observations,last_success_at) FROM twitch_vod_youtube_checks WHERE vod_id=$1").bind(vod.id).fetch_one(&pool).await.unwrap();
    assert!(save(
        &pool,
        "42",
        &target,
        Some("channel"),
        &vod,
        &[],
        "error",
        false,
        Some("quota"),
        86400
    )
    .await
    .unwrap());
    let after: Value=sqlx::query_scalar("SELECT jsonb_build_array(observations,last_success_at) FROM twitch_vod_youtube_checks WHERE vod_id=$1").bind(vod.id).fetch_one(&pool).await.unwrap();
    assert_eq!(before, after);
    sqlx::query("UPDATE social_media_platform_auth SET platform_user_id='different' WHERE id=$1")
        .bind(target.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(!save(
        &pool,
        "42",
        &target,
        Some("channel"),
        &vod,
        &[proof.clone()],
        "confirmed",
        true,
        None,
        86400
    )
    .await
    .unwrap());
    let changed = ziel(&pool, "42").await.unwrap().unwrap();
    sqlx::query(
        "UPDATE twitch_vod_archive_vods SET updated_at=updated_at+INTERVAL '1 second' WHERE id=$1",
    )
    .bind(vod.id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(!save(
        &pool,
        "42",
        &changed,
        Some("different"),
        &vod,
        &[proof],
        "confirmed",
        true,
        None,
        86400
    )
    .await
    .unwrap());
    eprintln!("YOUTUBE_DB_PROOF: successful observation write, historical timestamp untouched, zero synthetic parts, quota preserves success, channel and concurrent upload guards reject stale writes");
}

#[tokio::test]
async fn cleanup_requires_current_target_fresh_complete_processed_evidence() {
    let pool = crate::store::tests::pool("t_youtube_cleanup")
        .await
        .expect("isolated token_db test configuration required");
    sqlx::query("INSERT INTO social_media_platform_auth(platform,twitch_user_id,platform_user_id) VALUES ('youtube','42','channel')").execute(&pool).await.unwrap();
    crate::store::merke_vod(&pool, "123", "synthetic", "42", "Identical title", 120)
        .await
        .unwrap();
    let id: i64 =
        sqlx::query_scalar("SELECT id FROM twitch_vod_archive_vods WHERE twitch_id='123'")
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='uploaded' WHERE id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(cleanup_proof(&pool, "42", id, 24).await.unwrap().is_none());
    let target = ziel(&pool, "42").await.unwrap().unwrap();
    let vod = vods(&pool, "42", &target).await.unwrap().remove(0);
    let mut proof = observation("abc", None, None, 120);
    proof.observed_at = Utc::now();
    save(
        &pool,
        "42",
        &target,
        Some("channel"),
        &vod,
        &[proof.clone()],
        "confirmed",
        true,
        None,
        86400,
    )
    .await
    .unwrap();
    let guard = cleanup_proof(&pool, "42", id, 24).await.unwrap();
    assert!(guard.is_some());
    drop(guard);
    save(
        &pool,
        "42",
        &target,
        Some("channel"),
        &vod,
        &[],
        "error",
        false,
        Some("request"),
        3600,
    )
    .await
    .unwrap();
    assert!(cleanup_proof(&pool, "42", id, 24).await.unwrap().is_none());
    proof.state = "processing".into();
    save(
        &pool,
        "42",
        &target,
        Some("channel"),
        &vod,
        &[proof],
        "processing",
        false,
        None,
        600,
    )
    .await
    .unwrap();
    assert!(cleanup_proof(&pool, "42", id, 24).await.unwrap().is_none());
    eprintln!("YOUTUBE_DB_PROOF: cleanup denied without current complete evidence, allowed after processed proof, denied on request failure and processing");
}

#[tokio::test]
async fn reconciliation_resumes_real_database_cursor_with_encrypted_credentials_and_read_only_requests(
) {
    use std::sync::Arc;
    use tb_crypto::{aad, FieldCipher};
    use tb_social_media::uploaders::youtube::YouTubeUploader;
    use wiremock::{
        matchers::{method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    let pool = crate::store::tests::pool("t_youtube_pagination")
        .await
        .expect("isolated token_db test configuration required");
    let cipher = Arc::new(FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap());
    let encrypted = cipher
        .encrypt_field(
            "synthetic-access",
            &aad::social_media("access_token", "youtube", Some("synthetic"), 1),
        )
        .unwrap();
    sqlx::query("INSERT INTO social_media_platform_auth(platform,twitch_user_id,platform_user_id,streamer_login,access_token_enc) VALUES ('youtube','42','channel','synthetic',$1)").bind(encrypted).execute(&pool).await.unwrap();
    for id in ["123", "456", "789"] {
        crate::store::merke_vod(&pool, id, "synthetic", "42", "Identical title", 120)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='archived'")
        .execute(&pool)
        .await
        .unwrap();
    let before: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(v) ORDER BY id) FROM twitch_vod_archive_vods v",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let manager = CredentialManager::new(pool.clone(), cipher);
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/channels")).and(query_param("mine","true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":"channel","contentDetails":{"relatedPlaylists":{"uploads":"uploads"}}}]}))).expect(2).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/playlistItems"))
        .and(|request: &wiremock::Request| {
            !request.url.query_pairs().any(|(key, _)| key == "pageToken")
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"items":[{"contentDetails":{"videoId":"abc"}}],"nextPageToken":"second"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/playlistItems"))
        .and(query_param("pageToken", "second"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"items":[{"contentDetails":{"videoId":"def"}}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    for (video, source_id) in [("abc", "123"), ("def", "456")] {
        Mock::given(method("GET")).and(path("/videos")).and(query_param("id",video))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":video,"snippet":{"channelId":"channel","title":"Identical title","description":format!("Original: https://www.twitch.tv/videos/{source_id}")},"contentDetails":{"duration":"PT120S"},"status":{"uploadStatus":"processed","privacyStatus":"private"},"processingDetails":{"processingStatus":"succeeded"}}]}))).expect(1).mount(&server).await;
    }
    let config = VodArchiveOptions {
        youtube_requests_per_run: 3,
        ..Default::default()
    };
    let factory = |credentials: &tb_social_media::credentials::SocialMediaCredentials| {
        YouTubeUploader::new(&credentials.access_token).with_bases(server.uri(), server.uri())
    };
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let first: (Option<String>, bool) = sqlx::query_as(
        "SELECT cursor,complete FROM twitch_vod_youtube_scans WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(first, (Some("second".into()), false));
    let complete: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_vod_youtube_checks WHERE complete")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(complete, 0);
    let continuation:bool=sqlx::query_scalar("SELECT bool_and(next_check_at<=NOW()+INTERVAL '11 minutes') FROM twitch_vod_youtube_checks").fetch_one(&pool).await.unwrap();
    assert!(continuation);
    sqlx::query("UPDATE twitch_vod_youtube_checks SET next_check_at=NOW()")
        .execute(&pool)
        .await
        .unwrap();
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let second: (Option<String>, bool) = sqlx::query_as(
        "SELECT cursor,complete FROM twitch_vod_youtube_scans WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(second, (None, true));
    let states:Vec<(String,String,bool)>=sqlx::query_as("SELECT v.twitch_id,c.state,c.complete FROM twitch_vod_archive_vods v JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id ORDER BY v.twitch_id").fetch_all(&pool).await.unwrap();
    assert_eq!(
        states,
        vec![
            ("123".into(), "confirmed".into(), true),
            ("456".into(), "confirmed".into(), true),
            ("789".into(), "unresolved".into(), false)
        ]
    );
    let after: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(v) ORDER BY id) FROM twitch_vod_archive_vods v",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, after);
    let parts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_vod_archive_parts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(parts, 0);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 6);
    assert!(requests
        .iter()
        .all(|request| request.method.as_str() == "GET"));
    eprintln!("YOUTUBE_DB_PROOF: encrypted credential path, six read-only calls, durable two-page scan, no premature absence or completion, identical titles separated by source ID, historical rows unchanged");
}
