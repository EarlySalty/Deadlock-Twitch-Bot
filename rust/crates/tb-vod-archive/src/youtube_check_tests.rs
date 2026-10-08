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
    let mixed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE v.twitch_id='900'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(mixed, 0);
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

#[tokio::test]
async fn completed_inventory_rechecks_refresh_deletions_processing_and_manual_requests() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tb_social_media::uploaders::youtube::YouTubeUploader;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    let pool = crate::store::tests::pool("t_youtube_fresh_inventory")
        .await
        .expect("isolated token_db test configuration required");
    let manager = synthetic_manager(&pool, 103).await;
    for id in ["100", "200", "300", "400"] {
        crate::store::merke_vod(&pool, id, "synthetic", "42", "Identical title", 120)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO twitch_vod_archive_parts (vod_id,part_index,file_path,status,youtube_video_id) SELECT id,0,'/synthetic/mixed-0.mp4','failed','mixed-known' FROM twitch_vod_archive_vods WHERE twitch_id='300'")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_vod_archive_parts (vod_id,part_index,file_path,status) SELECT id,1,'/synthetic/mixed-1.mp4','pending' FROM twitch_vod_archive_vods WHERE twitch_id='300'")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='archived'")
        .execute(&pool)
        .await
        .unwrap();
    let before = local_snapshot(&pool).await;
    let server = MockServer::start().await;
    let phase = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET")).and(path("/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":"channel","contentDetails":{"relatedPlaylists":{"uploads":"uploads"}}}]})))
        .expect(4).mount(&server).await;
    Mock::given(method("GET")).and(path("/playlistItems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"contentDetails":{"videoId":"gone"}},{"contentDetails":{"videoId":"processing"}},{"contentDetails":{"videoId":"mixed-missing"}},{"contentDetails":{"videoId":"changed-source"}}]})))
        .expect(1).mount(&server).await;
    let provider_phase = phase.clone();
    Mock::given(method("GET")).and(path("/videos"))
        .respond_with(move |request: &wiremock::Request| {
            let phase = provider_phase.load(Ordering::SeqCst);
            if phase == 3 { return ResponseTemplate::new(403); }
            let ids = request.url.query_pairs().find(|(key, _)| key == "id").unwrap().1;
            let items = ids.split(',').filter_map(|id| {
                let (source, seconds, state) = match id {
                    "gone" if phase > 0 => return None,
                    "gone" => ("Original: https://www.twitch.tv/videos/100", 120, "processed"),
                    "processing" => ("Original: https://www.twitch.tv/videos/200", 120, if phase == 0 { "uploaded" } else { "processed" }),
                    "mixed-known" => ("", 60, "processed"),
                    "mixed-missing" => ("Archivquelle: Twitch-VOD 300; Teil 2/2\nOriginal: https://www.twitch.tv/videos/300", 60, if phase == 1 { "uploaded" } else { "processed" }),
                    "changed-source" => (if phase == 0 { "Original: https://www.twitch.tv/videos/400" } else { "Original: https://www.twitch.tv/videos/999" }, 120, "processed"),
                    _ => panic!("unexpected synthetic video"),
                };
                Some(provider_video(id, source, seconds, state))
            }).collect::<Vec<_>>();
            ResponseTemplate::new(200).set_body_json(json!({"items":items}))
        }).expect(5).mount(&server).await;
    let config = VodArchiveOptions {
        youtube_requests_per_run: 4,
        ..Default::default()
    };
    let factory = |credentials: &tb_social_media::credentials::SocialMediaCredentials| {
        YouTubeUploader::new(&credentials.access_token).with_bases(server.uri(), server.uri())
    };
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    sqlx::query("UPDATE twitch_vod_youtube_inventory SET observed_at=NOW()-INTERVAL '1 hour'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE twitch_vod_youtube_checks SET last_success_at=NOW()-INTERVAL '1 hour',requested_at=NOW()").execute(&pool).await.unwrap();
    let scan_before: Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM twitch_vod_youtube_scans s WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(scan_before["complete"], true);
    phase.store(1, Ordering::SeqCst);
    let fresh_after: DateTime<Utc> = sqlx::query_scalar("SELECT NOW()")
        .fetch_one(&pool)
        .await
        .unwrap();
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let states: Vec<(String, String, bool, bool)> = sqlx::query_as("SELECT v.twitch_id,c.state,c.complete,c.last_success_at>=$1 AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(c.observations) o WHERE (o->>'observed_at')::timestamptz<$1) FROM twitch_vod_archive_vods v JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id ORDER BY v.twitch_id")
        .bind(fresh_after).fetch_all(&pool).await.unwrap();
    assert_eq!(
        states,
        vec![
            ("100".into(), "unavailable".into(), false, false),
            ("200".into(), "confirmed".into(), true, true),
            ("300".into(), "processing".into(), false, true),
            ("400".into(), "unresolved".into(), false, false)
        ]
    );
    let gone: Value = sqlx::query_scalar("SELECT c.observations FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE v.twitch_id='100'").fetch_one(&pool).await.unwrap();
    assert_eq!(gone[0]["state"], "unavailable");
    assert!(gone[0]["privacy"].is_null());
    assert!(
        gone[0]["observed_at"]
            .as_str()
            .unwrap()
            .parse::<DateTime<Utc>>()
            .unwrap()
            >= fresh_after
    );
    let absence_time: Value = sqlx::query_scalar("SELECT to_jsonb(c.last_success_at) FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE v.twitch_id='400'").fetch_one(&pool).await.unwrap();
    assert_eq!(absence_time, scan_before["last_success_at"]);
    phase.store(2, Ordering::SeqCst);
    sqlx::query(
        "UPDATE twitch_vod_youtube_checks SET next_check_at=NOW() WHERE state='processing'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let retry_after: DateTime<Utc> = sqlx::query_scalar("SELECT NOW()")
        .fetch_one(&pool)
        .await
        .unwrap();
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let retried: (String, bool, bool) = sqlx::query_as("SELECT c.state,c.complete,c.last_success_at>=$1 AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(c.observations) o WHERE (o->>'observed_at')::timestamptz<$1 OR o->>'state'<>'processed') FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE v.twitch_id='300'")
        .bind(retry_after).fetch_one(&pool).await.unwrap();
    assert_eq!(retried, ("confirmed".into(), true, true));
    let successes: Value = sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_array(vod_id,last_success_at,observations) ORDER BY vod_id) FROM twitch_vod_youtube_checks").fetch_one(&pool).await.unwrap();
    phase.store(3, Ordering::SeqCst);
    sqlx::query("UPDATE twitch_vod_youtube_checks SET requested_at=NOW()")
        .execute(&pool)
        .await
        .unwrap();
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let preserved: Value = sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_array(vod_id,last_success_at,observations) ORDER BY vod_id) FROM twitch_vod_youtube_checks").fetch_one(&pool).await.unwrap();
    assert_eq!(successes, preserved);
    let errors: bool = sqlx::query_scalar("SELECT bool_and(state='error' AND NOT complete AND last_error='connection') FROM twitch_vod_youtube_checks").fetch_one(&pool).await.unwrap();
    assert!(errors);
    let scan_after: Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM twitch_vod_youtube_scans s WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(scan_after["complete"], false);
    assert_eq!(
        scan_after["generation"].as_i64().unwrap(),
        scan_before["generation"].as_i64().unwrap() + 1
    );
    assert_eq!(
        scan_after["last_success_at"],
        scan_before["last_success_at"]
    );
    assert_eq!(before, local_snapshot(&pool).await);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 10);
    assert!(requests
        .iter()
        .all(|request| request.method.as_str() == "GET"));
    eprintln!("YOUTUBE_DB_PROOF: completed inventory supplies identities only; manual rechecks read unavailable, processed, mixed processing and changed-source videos freshly; unresolved search times remain tied to the completed scan; automatic processing retry refreshes all parts; read errors preserve previous success while a new absence search remains incomplete; local legacy rows unchanged");
}

#[tokio::test]
async fn bounded_refresh_keeps_unfetched_inventory_proofs_and_requests_unchanged() {
    use tb_social_media::uploaders::youtube::YouTubeUploader;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    let pool = crate::store::tests::pool("t_youtube_refresh_budget")
        .await
        .expect("isolated token_db test configuration required");
    let manager = synthetic_manager(&pool, 104).await;
    for index in 0..101 {
        crate::store::merke_vod(
            &pool,
            &format!("{}", 1000 + index),
            "synthetic",
            "42",
            "Identical title",
            120,
        )
        .await
        .unwrap();
    }
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='archived'")
        .execute(&pool)
        .await
        .unwrap();
    let target = ziel(&pool, "42").await.unwrap().unwrap();
    sqlx::query("INSERT INTO twitch_vod_youtube_scans (twitch_user_id,auth_id,auth_revision,channel_id,playlist_id,complete,next_scan_at) VALUES ('42',$1,$2,'channel','uploads',TRUE,NOW()+INTERVAL '1 day')")
        .bind(target.id).bind(&target.revision).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_vod_youtube_inventory (twitch_user_id,video_id,generation,twitch_id,duration_sec,state,privacy,observed_at) SELECT '42','candidate'||twitch_id,1,twitch_id,120,'processing','private',NOW()-INTERVAL '1 hour' FROM twitch_vod_archive_vods")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_vod_youtube_checks (vod_id,auth_id,auth_revision,channel_id,state,complete,observations,last_success_at,last_attempt_at,next_check_at,requested_at,upload_snapshot) SELECT id,$1,$2,'channel','processing',FALSE,jsonb_build_array(jsonb_build_object('video_id','candidate'||twitch_id,'part_index',NULL,'part_total',NULL,'duration_sec',120,'state','processing','privacy','private','observed_at',NOW()-INTERVAL '1 hour')),NOW()-INTERVAL '1 hour',NOW()-INTERVAL '1 hour',NOW()+INTERVAL '1 day',NOW(),'[]'::jsonb FROM twitch_vod_archive_vods")
        .bind(target.id).bind(&target.revision).execute(&pool).await.unwrap();
    let before = local_snapshot(&pool).await;
    let skipped_before: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE v.twitch_id='1100'").fetch_one(&pool).await.unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":"channel","contentDetails":{"relatedPlaylists":{"uploads":"uploads"}}}]})))
        .expect(2).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/videos"))
        .respond_with(|request: &wiremock::Request| {
            let ids = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "id")
                .unwrap()
                .1;
            assert!(ids.split(',').count() <= 50);
            let items = ids
                .split(',')
                .filter(|id| *id != "candidate1100")
                .map(|id| {
                    provider_video(
                        id,
                        &format!(
                            "Original: https://www.twitch.tv/videos/{}",
                            id.strip_prefix("candidate").unwrap()
                        ),
                        120,
                        "processed",
                    )
                })
                .collect::<Vec<_>>();
            ResponseTemplate::new(200).set_body_json(json!({"items":items}))
        })
        .expect(3)
        .mount(&server)
        .await;
    let config = VodArchiveOptions {
        youtube_requests_per_run: 3,
        ..Default::default()
    };
    let factory = |credentials: &tb_social_media::credentials::SocialMediaCredentials| {
        YouTubeUploader::new(&credentials.access_token).with_bases(server.uri(), server.uri())
    };
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
    let skipped_after: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE v.twitch_id='1100'").fetch_one(&pool).await.unwrap();
    assert_eq!(skipped_before, skipped_after);
    let confirmed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_vod_youtube_checks WHERE complete AND state='confirmed' AND requested_at IS NULL").fetch_one(&pool).await.unwrap();
    assert_eq!(confirmed, 100);
    let second_read_at: DateTime<Utc> = sqlx::query_scalar("SELECT NOW()")
        .fetch_one(&pool)
        .await
        .unwrap();
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let unavailable: (String, bool, bool, Option<DateTime<Utc>>, bool) = sqlx::query_as("SELECT c.state,c.complete,c.requested_at IS NULL,c.last_success_at,(c.observations->0->>'observed_at')::timestamptz>=$1 FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE v.twitch_id='1100'")
        .bind(second_read_at).fetch_one(&pool).await.unwrap();
    assert_eq!(unavailable, ("unavailable".into(), false, true, None, true));
    assert_eq!(before, local_snapshot(&pool).await);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 5);
    assert!(requests
        .iter()
        .all(|request| request.method.as_str() == "GET"));
    eprintln!("YOUTUBE_DB_PROOF: 101 completed-inventory candidates refreshed in batches of at most 50 under a three-request budget; unfetched processing proof, success time and manual request unchanged; later fresh read records unavailability without inventing a completed-search time or changing local rows");
}

async fn oversized_vod(recovered: bool) {
    use tb_social_media::uploaders::youtube::YouTubeUploader;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    let pool = crate::store::tests::pool(if recovered {
        "t_youtube_large_recovered"
    } else {
        "t_youtube_large_local"
    })
    .await
    .unwrap();
    let manager = synthetic_manager(&pool, if recovered { 108 } else { 105 }).await;
    crate::store::merke_vod(&pool, "123", "synthetic", "42", "Synthetic", 101)
        .await
        .unwrap();
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='archived'")
        .execute(&pool)
        .await
        .unwrap();
    if recovered {
        let target = ziel(&pool, "42").await.unwrap().unwrap();
        sqlx::query("INSERT INTO twitch_vod_youtube_scans(twitch_user_id,auth_id,auth_revision,channel_id,playlist_id,complete,last_success_at,next_scan_at) VALUES ('42',$1,$2,'channel','uploads',TRUE,NOW(),NOW()+INTERVAL '1 day')")
            .bind(target.id).bind(&target.revision).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_vod_youtube_inventory(twitch_user_id,video_id,twitch_id,generation,part_index,part_total,duration_sec,state) SELECT '42','video'||lpad(i::text,3,'0'),'123',1,0,1,1,'processed' FROM generate_series(0,100) i")
            .execute(&pool).await.unwrap();
    } else {
        sqlx::query("INSERT INTO twitch_vod_archive_parts(vod_id,part_index,file_path,status,youtube_video_id) SELECT v.id,i,'/synthetic/part','done','video'||lpad(i::text,3,'0') FROM twitch_vod_archive_vods v CROSS JOIN generate_series(0,100) i")
            .execute(&pool).await.unwrap();
    }
    let before = local_snapshot(&pool).await;
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":"channel","contentDetails":{"relatedPlaylists":{"uploads":"uploads"}}}]}))).expect(2).mount(&server).await;
    Mock::given(method("GET")).and(path("/videos")).respond_with(move |request: &wiremock::Request| {
        let ids = request.url.query_pairs().find(|(key,_)| key == "id").unwrap().1;
        let items: Vec<_> = ids.split(',').map(|id| provider_video(id, if recovered { "Archivquelle: Twitch-VOD 123; Teil 1/1\nOriginal: https://www.twitch.tv/videos/123" } else { "" }, 1, "processed")).collect();
        ResponseTemplate::new(200).set_body_json(json!({"items":items}))
    }).expect(3).mount(&server).await;
    let config = VodArchiveOptions {
        youtube_requests_per_run: 3,
        ..Default::default()
    };
    let factory = |credentials: &tb_social_media::credentials::SocialMediaCredentials| {
        YouTubeUploader::new(&credentials.access_token).with_bases(server.uri(), server.uri())
    };
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
    let progress: (i32, i32) = sqlx::query_as("SELECT jsonb_array_length(refreshed),jsonb_array_length(observations) FROM twitch_vod_youtube_continuations").fetch_one(&pool).await.unwrap();
    assert_eq!(progress, (100, 100));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_vod_youtube_checks")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    let first_time: DateTime<Utc> = sqlx::query_scalar("SELECT MIN((o->>'observed_at')::timestamptz) FROM twitch_vod_youtube_continuations p CROSS JOIN LATERAL jsonb_array_elements(p.observations) o").fetch_one(&pool).await.unwrap();
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let outcome: (String,bool,i32,DateTime<Utc>) = sqlx::query_as("SELECT state,complete,jsonb_array_length(observations),last_success_at FROM twitch_vod_youtube_checks").fetch_one(&pool).await.unwrap();
    assert_eq!(outcome.0, if recovered { "partial" } else { "confirmed" });
    assert_eq!(outcome.1, !recovered);
    assert_eq!(outcome.2, 101);
    assert!(outcome.3 <= first_time);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_vod_youtube_continuations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(before, local_snapshot(&pool).await);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 5);
    assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
    let ids: Vec<_> = requests
        .iter()
        .filter(|r| r.url.path() == "/videos")
        .flat_map(|r| {
            r.url
                .query_pairs()
                .find(|(key, _)| key == "id")
                .unwrap()
                .1
                .split(',')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids.len(), 101);
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 101);
    eprintln!("YOUTUBE_DB_PROOF: 101 distinct IDs in one VOD advance durably in two minimum-budget runs, original per-batch observation times retained, ambiguous recovered candidates finish as partial, no upload mutation");
}

#[tokio::test]
async fn minimum_budget_finishes_one_vod_with_101_local_ids() {
    oversized_vod(false).await;
}

#[tokio::test]
async fn minimum_budget_finishes_one_vod_with_101_ambiguous_recovered_candidates() {
    oversized_vod(true).await;
}

#[tokio::test]
async fn manual_incomplete_search_refreshes_empty_and_mixed_inventory_and_processing_retries() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tb_social_media::uploaders::youtube::YouTubeUploader;
    use wiremock::{
        matchers::{method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    let pool = crate::store::tests::pool("t_youtube_search_refresh")
        .await
        .unwrap();
    let manager = synthetic_manager(&pool, 106).await;
    for id in ["100", "200", "300"] {
        crate::store::merke_vod(
            &pool,
            id,
            "synthetic",
            "42",
            "Synthetic",
            if id == "300" { 180 } else { 120 },
        )
        .await
        .unwrap();
    }
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='archived'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO twitch_vod_archive_parts(vod_id,part_index,file_path,status,youtube_video_id) SELECT id,i,'/synthetic/part','done',CASE WHEN i=0 THEN 'mixed-known' END FROM twitch_vod_archive_vods CROSS JOIN generate_series(0,2) i WHERE twitch_id='300'").execute(&pool).await.unwrap();
    let target = ziel(&pool, "42").await.unwrap().unwrap();
    sqlx::query("INSERT INTO twitch_vod_youtube_scans(twitch_user_id,auth_id,auth_revision,channel_id,playlist_id,complete,last_success_at,next_scan_at) VALUES ('42',$1,$2,'channel','uploads',TRUE,NOW()-INTERVAL '1 hour',NOW()+INTERVAL '1 day')").bind(target.id).bind(&target.revision).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_vod_youtube_inventory(twitch_user_id,video_id,generation,twitch_id,part_index,part_total,duration_sec,state) VALUES ('42','old200',1,'200',0,2,60,'processing')").execute(&pool).await.unwrap();
    for vod in vods(&pool, "42", &target).await.unwrap() {
        let old = if vod.twitch_id == "200" {
            vec![Beobachtung {
                state: "processing".into(),
                observed_at: Utc::now() - chrono::Duration::hours(1),
                ..observation("old200", Some(0), Some(2), 60)
            }]
        } else {
            vec![]
        };
        sqlx::query("INSERT INTO twitch_vod_youtube_checks(vod_id,auth_id,auth_revision,channel_id,state,observations,upload_snapshot,last_success_at,last_attempt_at,requested_at,next_check_at) VALUES ($1,$2,$3,'channel',$4,$5,$6,NOW()-INTERVAL '1 hour',NOW()-INTERVAL '1 hour',NOW(),NOW()+INTERVAL '1 day')")
            .bind(vod.id).bind(target.id).bind(&target.revision).bind(if vod.twitch_id=="200" { "processing" } else { "unresolved" }).bind(json!(old)).bind(vod.parts).execute(&pool).await.unwrap();
    }
    let before = local_snapshot(&pool).await;
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":"channel","contentDetails":{"relatedPlaylists":{"uploads":"uploads"}}}]}))).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/playlistItems"))
        .and(|r: &wiremock::Request| !r.url.query_pairs().any(|(k, _)| k == "pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"items":[{"contentDetails":{"videoId":"old200"}}],"nextPageToken":"next"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/playlistItems")).and(query_param("pageToken","next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"contentDetails":{"videoId":"new100"}},{"contentDetails":{"videoId":"new200"}},{"contentDetails":{"videoId":"mixed1"}},{"contentDetails":{"videoId":"mixed2"}}]}))).expect(1).mount(&server).await;
    let processed = Arc::new(AtomicBool::new(false));
    let phase = processed.clone();
    Mock::given(method("GET")).and(path("/videos")).respond_with(move |r:&wiremock::Request| {
        let ids=r.url.query_pairs().find(|(k,_)| k=="id").unwrap().1;
        let items:Vec<_>=ids.split(',').map(|id| {
            let (vod,index,total,seconds) = match id { "old200"=>("200",0,2,60),"new200"=>("200",1,2,60),"mixed1"=>("300",1,3,60),"mixed2"=>("300",2,3,60),"mixed-known"=>("300",0,3,60),"new100"=>("100",0,1,120),_=>panic!("unexpected synthetic ID") };
            provider_video(id,&format!("Archivquelle: Twitch-VOD {vod}; Teil {}/{}\nOriginal: https://www.twitch.tv/videos/{vod}",index+1,total),seconds,if id=="old200" && !phase.load(Ordering::SeqCst) { "uploaded" } else { "processed" })
        }).collect();
        ResponseTemplate::new(200).set_body_json(json!({"items":items}))
    }).mount(&server).await;
    let config = VodArchiveOptions {
        youtube_requests_per_run: 3,
        ..Default::default()
    };
    let factory = |c: &tb_social_media::credentials::SocialMediaCredentials| {
        YouTubeUploader::new(&c.access_token).with_bases(server.uri(), server.uri())
    };
    let mut previous = 0;
    for round in 0..3 {
        run_with(&pool, &manager, &config, &factory).await.unwrap();
        let requests = server.received_requests().await.unwrap();
        assert!(requests.len() - previous <= 3);
        previous = requests.len();
        if round == 0 {
            let scan: (Option<String>, bool) =
                sqlx::query_as("SELECT cursor,complete FROM twitch_vod_youtube_scans")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(scan, (Some("next".into()), false));
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM twitch_vod_youtube_checks WHERE complete OR state='unresolved' AND last_success_at>NOW()-INTERVAL '30 minutes'").fetch_one(&pool).await.unwrap(),0);
        }
        sqlx::query("UPDATE twitch_vod_youtube_checks SET next_check_at=NOW() WHERE NOT complete")
            .execute(&pool)
            .await
            .unwrap();
    }
    let states:Vec<(String,String,bool)>=sqlx::query_as("SELECT v.twitch_id,c.state,c.complete FROM twitch_vod_archive_vods v JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id ORDER BY v.twitch_id").fetch_all(&pool).await.unwrap();
    assert_eq!(
        states,
        vec![
            ("100".into(), "confirmed".into(), true),
            ("200".into(), "processing".into(), false),
            ("300".into(), "confirmed".into(), true)
        ]
    );
    processed.store(true, Ordering::SeqCst);
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT bool_and(complete AND state='confirmed') FROM twitch_vod_youtube_checks"
    )
    .fetch_one(&pool)
    .await
    .unwrap());
    assert_eq!(before, local_snapshot(&pool).await);
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| r.method.as_str() == "GET"));
    eprintln!("YOUTUBE_DB_PROOF: manual request replaces completed empty/incomplete search, paginated continuation discovers new uploads for ID-less and missing mixed parts; unfinished search never final absence; processing retry confirms fresh provider state without upload mutation");
}

#[tokio::test]
async fn changed_parts_provider_error_retains_original_evidence_snapshot() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tb_social_media::uploaders::youtube::YouTubeUploader;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };
    let pool = crate::store::tests::pool("t_youtube_changed_error")
        .await
        .unwrap();
    let manager = synthetic_manager(&pool, 107).await;
    crate::store::merke_vod(&pool, "123", "synthetic", "42", "Synthetic", 120)
        .await
        .unwrap();
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='archived'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO twitch_vod_archive_parts(vod_id,part_index,file_path,status,youtube_video_id) SELECT id,0,'/synthetic/part','done','original' FROM twitch_vod_archive_vods").execute(&pool).await.unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":"channel","contentDetails":{"relatedPlaylists":{"uploads":"uploads"}}}]}))).expect(2).mount(&server).await;
    let failing = Arc::new(AtomicBool::new(false));
    let phase = failing.clone();
    Mock::given(method("GET"))
        .and(path("/videos"))
        .respond_with(move |_: &wiremock::Request| {
            if phase.load(Ordering::SeqCst) {
                ResponseTemplate::new(403)
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"items":[provider_video("original","",120,"processed")]}))
            }
        })
        .expect(2)
        .mount(&server)
        .await;
    let config = VodArchiveOptions {
        youtube_requests_per_run: 3,
        ..Default::default()
    };
    let factory = |c: &tb_social_media::credentials::SocialMediaCredentials| {
        YouTubeUploader::new(&c.access_token).with_bases(server.uri(), server.uri())
    };
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let evidence:Value=sqlx::query_scalar("SELECT jsonb_build_array(observations,last_success_at,upload_snapshot) FROM twitch_vod_youtube_checks").fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE twitch_vod_archive_parts SET youtube_video_id='new',updated_at=NOW()")
        .execute(&pool)
        .await
        .unwrap();
    let before = local_snapshot(&pool).await;
    failing.store(true, Ordering::SeqCst);
    run_with(&pool, &manager, &config, &factory).await.unwrap();
    let retained:Value=sqlx::query_scalar("SELECT jsonb_build_array(observations,last_success_at,upload_snapshot) FROM twitch_vod_youtube_checks").fetch_one(&pool).await.unwrap();
    assert_eq!(evidence, retained);
    let current:bool=sqlx::query_scalar("SELECT c.upload_snapshot=(SELECT jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index) FROM twitch_vod_archive_parts p WHERE p.vod_id=c.vod_id) FROM twitch_vod_youtube_checks c").fetch_one(&pool).await.unwrap();
    assert!(!current);
    assert_eq!(before, local_snapshot(&pool).await);
    assert!(sqlx::query_scalar::<_,bool>("SELECT state='error' AND last_error='connection' AND NOT complete FROM twitch_vod_youtube_checks").fetch_one(&pool).await.unwrap());
    eprintln!("YOUTUBE_DB_PROOF: changed parts plus real provider HTTP error retain original observation snapshot and success time; stale evidence does not match current API/cleanup guard");
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
        check: None,
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
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":"channel","contentDetails":{"relatedPlaylists":{"uploads":"uploads"}}}]}))).expect(3).mount(&server).await;
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":video,"snippet":{"channelId":"channel","title":"Identical title","description":format!("Original: https://www.twitch.tv/videos/{source_id}")},"contentDetails":{"duration":"PT120S"},"status":{"uploadStatus":"processed","privacyStatus":"private"},"processingDetails":{"processingStatus":"succeeded"}}]}))).expect(if video == "abc" { 2 } else { 1 }).mount(&server).await;
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
    let retained: (String, bool) = sqlx::query_as("SELECT c.state,c.complete FROM twitch_vod_youtube_checks c JOIN twitch_vod_archive_vods v ON v.id=c.vod_id WHERE v.twitch_id='123'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(retained, ("partial".into(), false));
    sqlx::query("UPDATE twitch_vod_youtube_checks SET next_check_at=NOW() WHERE NOT complete")
        .execute(&pool)
        .await
        .unwrap();
    run_with(&pool, &manager, &config, &factory).await.unwrap();
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
    assert_eq!(requests.len(), 8);
    assert!(requests
        .iter()
        .all(|request| request.method.as_str() == "GET"));
    eprintln!("YOUTUBE_DB_PROOF: encrypted credential path, eight read-only calls, durable two-page scan and fresh prior-page recheck, no premature absence or completion, identical titles separated by source ID, historical rows unchanged");
}
