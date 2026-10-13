use super::*;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension,
};
use tb_domain::vod_range::VodRange;
use tb_vod_archive::private_range::{cleanup, enqueue, extract, select_source, trusted_media_url};

fn partner(id: &str) -> PrivateVodOwner {
    PrivateVodOwner(id.into())
}

async fn fixture() -> (
    crate::test_database::Database,
    tempfile::TempDir,
    PrivateVodRuntime,
) {
    let database = crate::test_database::Database::new().await;
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261013130000_private_vod_ranges.sql"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let runtime = PrivateVodRuntime {
        paths: PrivateRangePaths {
            directory: directory.path().join("private"),
            yt_dlp: "yt-dlp".into(),
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
        },
        downloads: Arc::new(Semaphore::new(2)),
    };
    runtime.paths.prepare().await.unwrap();
    (database, directory, runtime)
}

async fn mounted_request(
    app: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    headers: &[(&str, &str)],
) -> Response {
    use tower::ServiceExt;
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "dashboard.fixture.test")
        .header("content-type", "application/json");
    if !headers.iter().any(|(name, _)| *name == "origin") {
        request = request.header("origin", "https://dashboard.fixture.test");
    }
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    app.clone()
        .oneshot(request.body(Body::from(r#"{"enabled":true}"#)).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn mounted_private_vod_routes_require_verified_twitch_session_provenance() {
    use crate::auth::session::{DashboardAuthState, ADMIN_COOKIE_NAME, PARTNER_COOKIE_NAME};
    let (database, _directory, runtime) = fixture().await;
    let pool = &database.pool;
    sqlx::raw_sql(
        "CREATE TABLE dashboard_sessions (
            session_id TEXT PRIMARY KEY, session_type TEXT NOT NULL,
            payload_enc BYTEA NOT NULL, created_at DOUBLE PRECISION NOT NULL,
            expires_at DOUBLE PRECISION NOT NULL
        );
        CREATE TABLE twitch_partners (
            id BIGINT PRIMARY KEY, twitch_login TEXT NOT NULL, twitch_user_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active', technical_pause_reason TEXT,
            admin_archived_at TIMESTAMPTZ, departnered_at TIMESTAMPTZ,
            partnered_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
            manual_partner_opt_out INTEGER DEFAULT 0
        );
        INSERT INTO twitch_partners (id,twitch_login,twitch_user_id) VALUES
            (42,'fixture-owner','42'),(99,'fixture-other','99');",
    )
    .execute(pool)
    .await
    .unwrap();
    let key = tb_crypto::random_urlsafe_token(32);
    let auth_state = DashboardAuthState::new(pool.clone(), key.clone())
        .with_admin_twitch_user_id(Some("42".into()));
    let master = auth_state
        .create_admin_session("fixture-discord", "Fixture Admin")
        .await
        .unwrap();
    let owner = auth_state
        .create_partner_session("fixture-owner", "42", "Fixture Owner")
        .await
        .unwrap();
    let other = auth_state
        .create_partner_session("fixture-other", "99", "Fixture Other")
        .await
        .unwrap();
    let master_cookie = format!("{ADMIN_COOKIE_NAME}={}", master.session_id);
    let owner_cookie = format!("{PARTNER_COOKIE_NAME}={}", owner.session_id);
    let other_cookie = format!("{PARTNER_COOKIE_NAME}={}", other.session_id);
    let access = auth_state
        .create_partner_access_session("fixture-owner", "42", "Fixture Owner", "")
        .await
        .unwrap();
    let access_cookie = format!(
        "{}={}; {master_cookie}",
        crate::auth::session::PARTNER_ACCESS_COOKIE_NAME,
        access.session_id
    );
    let app = crate::build_authed_router(
        pool.clone(),
        "fixture-internal".into(),
        crate::RateLimiter::new(pool.clone(), key),
    )
    .layer(Extension(auth_state.clone()))
    .layer(Extension(runtime.clone()));
    let route = "/twitch/api/v2/streamer/private-vod-clips";
    let id = uuid::Uuid::new_v4().to_string();
    let file = runtime.paths.file(&id).unwrap();
    tokio::fs::write(&file, b"fixture").await.unwrap();
    sqlx::query("INSERT INTO twitch_private_vod_jobs (id,owner_id,video_id,start_seconds,end_seconds,status,file_bytes) VALUES ($1,'42','123',15,75,'ready',7)")
        .bind(&id).execute(pool).await.unwrap();
    let download = format!("{route}/{id}/download");
    for cookie in [
        access_cookie,
        format!(
            "{PARTNER_COOKIE_NAME}={}; {master_cookie}",
            master.session_id
        ),
        format!(
            "{PARTNER_COOKIE_NAME}={}; {master_cookie}",
            access.session_id
        ),
    ] {
        for (method, uri) in [("GET", route), ("POST", route), ("GET", download.as_str())] {
            assert_eq!(
                mounted_request(&app, method, uri, Some(&cookie), &[])
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
    }
    for mode in ["", "; tb_admin_mode=2"] {
        let cookie = format!("{master_cookie}{mode}");
        let mut parts = axum::http::Request::builder()
            .uri(route)
            .header("cookie", &cookie)
            .body(())
            .unwrap()
            .into_parts()
            .0;
        parts.extensions.insert(auth_state.clone());
        let auth = DashboardAuthLevel::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(owner_id(&auth).unwrap(), "42");
        assert_eq!(auth.is_privileged(), !mode.is_empty());
        for (method, uri) in [("GET", route), ("POST", route), ("GET", download.as_str())] {
            let response = mounted_request(&app, method, uri, Some(&cookie), &[]).await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "private, no-store"
            );
        }
    }
    for headers in [
        vec![("x-dashboard-context", "admin")],
        vec![("x-internal-token", "fixture-internal")],
    ] {
        for (method, uri) in [("GET", route), ("POST", route), ("GET", download.as_str())] {
            assert_eq!(
                mounted_request(&app, method, uri, Some(&master_cookie), &headers)
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
    }
    for (method, uri) in [("GET", route), ("GET", download.as_str())] {
        assert_eq!(
            mounted_request(&app, method, uri, None, &[]).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let enabled: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM twitch_private_vod_settings)")
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(!enabled);
    for mode in ["", "; tb_admin_mode=2"] {
        let cookie = format!("{owner_cookie}{mode}");
        assert_eq!(
            mounted_request(
                &app,
                "POST",
                &format!("{route}?owner_id=99&streamer=fixture-other"),
                Some(&cookie),
                &[]
            )
            .await
            .status(),
            StatusCode::OK
        );
        let response = mounted_request(
            &app,
            "GET",
            &format!("{route}?owner_id=99&streamer=fixture-other"),
            Some(&cookie),
            &[],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["enabled"], true);
        assert_eq!(value["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(value["jobs"][0]["owner_id"], "42");
        let response = mounted_request(&app, "GET", &download, Some(&cookie), &[]).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 7)
                .await
                .unwrap()
                .as_ref(),
            b"fixture"
        );
    }
    let response = mounted_request(
        &app,
        "GET",
        &format!("{route}?owner_id=42"),
        Some(&other_cookie),
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["enabled"], false);
    assert!(value["jobs"].as_array().unwrap().is_empty());
    assert_eq!(
        mounted_request(
            &app,
            "GET",
            &format!("{download}?owner_id=42"),
            Some(&other_cookie),
            &[]
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mounted_request(
            &app,
            "POST",
            &format!("{route}?owner_id=42"),
            Some(&other_cookie),
            &[]
        )
        .await
        .status(),
        StatusCode::OK
    );
    let owners: Vec<String> =
        sqlx::query_scalar("SELECT owner_id FROM twitch_private_vod_settings ORDER BY owner_id")
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(owners, ["42", "99"]);
    let response = mounted_request(
        &app,
        "POST",
        route,
        Some(&owner_cookie),
        &[("origin", "https://attacker.fixture.test")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"], "invalid_csrf");
    assert_eq!(
        mounted_request(&app, "POST", route, None, &[])
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn completed_file_keeps_download_capacity_until_body_is_consumed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("download.mp4");
    tokio::fs::write(&path, b"fixture").await.unwrap();
    let file = tokio::fs::File::open(&path).await.unwrap();
    let semaphore = Arc::new(Semaphore::new(1));
    let permit = semaphore.clone().acquire_owned().await.unwrap();
    let body = download_body(
        file,
        7,
        chrono::Utc::now() + chrono::Duration::hours(24),
        permit,
    );
    tokio::task::yield_now().await;
    assert_eq!(semaphore.available_permits(), 0);
    assert_eq!(
        axum::body::to_bytes(body, 7).await.unwrap().as_ref(),
        b"fixture"
    );
    tokio::task::yield_now().await;
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn stalled_download_releases_private_file_and_capacity_on_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("download.mp4");
    tokio::fs::write(&path, vec![1u8; 4 * 65536]).await.unwrap();
    let file = tokio::fs::File::open(&path).await.unwrap();
    let semaphore = Arc::new(Semaphore::new(1));
    let permit = semaphore.clone().acquire_owned().await.unwrap();
    tokio::time::pause();
    let body = download_body(
        file,
        4 * 65536,
        chrono::Utc::now() + chrono::Duration::hours(24),
        permit,
    );
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(301)).await;
    tokio::task::yield_now().await;
    assert_eq!(semaphore.available_permits(), 1);
    assert!(axum::body::to_bytes(body, 4 * 65536).await.is_err());
}

#[tokio::test]
async fn helix_submission_pins_the_actual_current_archive() {
    use tb_transport_twitch::{HelixClient, HelixConfig};
    use wiremock::{
        matchers::{method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };
    let (database, _directory, _runtime) = fixture().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"access_token":"fixture-only","expires_in":3600,"token_type":"bearer"}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/streams"))
        .and(query_param("user_id", "42"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"id":"current","user_id":"42"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/videos")).and(query_param("user_id","42")).and(query_param("type","archive")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"123","user_id":"42","stream_id":"old","duration":"3h"},{"id":"456","user_id":"42","stream_id":"current","duration":"2h"}]}))).mount(&server).await;
    let mut config = HelixConfig::new("fixture-client", "fixture-secret");
    config.token_url = format!("{}/token", server.uri());
    config.helix_base = server.uri();
    let helix = HelixClient::new(config).unwrap();
    settings_handler(
        partner("42"),
        State(database.pool.clone()),
        Json(Settings { enabled: true }),
    )
    .await;
    let id = tb_vod_archive::private_range::submit(
        &database.pool,
        &helix,
        "42",
        VodRange::parse("1:55:00 1:58:00").unwrap(),
    )
    .await
    .unwrap();
    let source: String =
        sqlx::query_scalar("SELECT video_id FROM twitch_private_vod_jobs WHERE id=$1")
            .bind(id)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(source, "456");
}

#[tokio::test]
async fn owner_scope_expiry_and_physical_cleanup() {
    let (database, _directory, runtime) = fixture().await;
    let pool = &database.pool;
    assert_eq!(
        owner_id(&DashboardAuthLevel::None).unwrap_err().0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        owner_id(&DashboardAuthLevel::admin()).unwrap_err().0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        settings_handler(
            partner("42"),
            State(pool.clone()),
            Json(Settings { enabled: true })
        )
        .await
        .status(),
        StatusCode::OK
    );
    let id = enqueue(
        pool,
        "42",
        "123",
        VodRange {
            start: 3300,
            end: 3480,
        },
    )
    .await
    .unwrap();
    let path = runtime.paths.file(&id).unwrap();
    tokio::fs::write(&path, b"fixture mp4 bytes").await.unwrap();
    sqlx::query("UPDATE twitch_private_vod_jobs SET status='ready',file_bytes=17 WHERE id=$1")
        .bind(&id)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        download_handler(
            partner("99"),
            State(pool.clone()),
            Path(id.clone()),
            Some(Extension(runtime.clone()))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let response = download_handler(
        partner("42"),
        State(pool.clone()),
        Path(id.clone()),
        Some(Extension(runtime.clone())),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(response.headers()[header::CONTENT_TYPE], "video/mp4");
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .as_ref(),
        b"fixture mp4 bytes"
    );
    sqlx::query(
        "UPDATE twitch_private_vod_jobs SET expires_at=now()-INTERVAL '1 second' WHERE id=$1",
    )
    .bind(&id)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(
        download_handler(
            partner("42"),
            State(pool.clone()),
            Path(id.clone()),
            Some(Extension(runtime.clone()))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let partial = path.with_extension("part.mp4");
    tokio::fs::write(&partial, b"partial").await.unwrap();
    cleanup(pool, &runtime.paths).await.unwrap();
    assert!(!path.exists());
    assert!(!partial.exists());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_private_vod_jobs")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let orphan = runtime
        .paths
        .file(&uuid::Uuid::new_v4().to_string())
        .unwrap();
    tokio::fs::write(&orphan, b"orphan").await.unwrap();
    cleanup(pool, &runtime.paths).await.unwrap();
    assert!(!orphan.exists());
}

#[tokio::test]
async fn admission_is_atomic_bounded_and_opt_in() {
    let (database, _directory, runtime) = fixture().await;
    let pool = &database.pool;
    let range = VodRange {
        start: 3300,
        end: 3480,
    };
    assert!(matches!(
        enqueue(pool, "42", "123", range).await,
        Err(tb_vod_archive::private_range::RangeError::Rejected(
            "disabled"
        ))
    ));
    sqlx::query("INSERT INTO twitch_private_vod_settings (owner_id,enabled) SELECT value::text,true FROM generate_series(42,62) value").execute(pool).await.unwrap();
    let (first, second) = tokio::join!(
        enqueue(pool, "42", "123", range),
        enqueue(pool, "42", "123", range)
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert!(matches!(
        first.err().or(second.err()),
        Some(tb_vod_archive::private_range::RangeError::Rejected(
            "active"
        ))
    ));
    sqlx::query("UPDATE twitch_private_vod_jobs SET status='failed'")
        .execute(pool)
        .await
        .unwrap();
    for _ in 0..2 {
        enqueue(pool, "42", "123", range).await.unwrap();
        sqlx::query("UPDATE twitch_private_vod_jobs SET status='failed'")
            .execute(pool)
            .await
            .unwrap();
    }
    assert!(matches!(
        enqueue(pool, "42", "123", range).await,
        Err(tb_vod_archive::private_range::RangeError::Rejected("rate"))
    ));
    for owner in 43..60 {
        enqueue(pool, &owner.to_string(), "123", range)
            .await
            .unwrap();
    }
    assert!(matches!(
        enqueue(pool, "60", "123", range).await,
        Err(tb_vod_archive::private_range::RangeError::Rejected(
            "capacity"
        ))
    ));
    sqlx::query("UPDATE twitch_private_vod_jobs SET expires_at=now()-INTERVAL '1 second'")
        .execute(pool)
        .await
        .unwrap();
    cleanup(pool, &runtime.paths).await.unwrap();
    assert!(enqueue(pool, "60", "123", range).await.is_ok());
}

#[test]
fn source_is_bound_to_owner_current_stream_and_available_range() {
    use tb_transport_twitch::streams::HelixArchiveVideo;
    let video = |id: &str, owner: &str, stream: &str, duration: &str| HelixArchiveVideo {
        id: id.into(),
        user_id: owner.into(),
        stream_id: Some(stream.into()),
        duration: duration.into(),
    };
    let range = VodRange {
        start: 6900,
        end: 7080,
    };
    let videos = vec![
        video("1", "99", "current", "3h"),
        video("2", "42", "old", "3h"),
        video("3", "42", "current", "2h"),
    ];
    assert_eq!(
        select_source(videos.clone(), "42", Some("current"), range)
            .unwrap()
            .id,
        "3"
    );
    assert_eq!(
        select_source(videos.clone(), "42", None, range).unwrap().id,
        "2"
    );
    assert!(select_source(videos, "42", Some("missing"), range).is_err());
    assert!(select_source(
        vec![video("4", "42", "current", "1h")],
        "42",
        Some("current"),
        range
    )
    .is_err());
    assert!(select_source(
        vec![video("5", "42", "current", "broken")],
        "42",
        None,
        range
    )
    .is_err());
    assert!(trusted_media_url(
        "https://vod-secure.twitch.tv/fixture/index.m3u8"
    ));
    assert!(trusted_media_url(
        "https://example.cloudfront.ttvnw.net/index.m3u8"
    ));
    for url in [
        "file:///etc/passwd",
        "http://vod-secure.twitch.tv/foo",
        "https://127.0.0.1/foo",
        "https://ttvnw.net.attacker.test/foo",
        "https://vod-secure.twitch.tv:8769/foo",
        "https://user:password@vod-secure.twitch.tv/foo",
    ] {
        assert!(!trusted_media_url(url));
    }
}

async fn decoded_frame(path: &std::path::Path, seconds: &str) -> Vec<u8> {
    let output = tokio::process::Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-ss", seconds, "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=160:90",
            "-pix_fmt",
            "rgb24",
            "-threads",
            "1",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 160 * 90 * 3);
    output.stdout
}

fn frame_difference(left: &[u8], right: &[u8]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| f64::from(left.abs_diff(*right)))
        .sum::<f64>()
        / left.len() as f64
}

#[cfg(unix)]
#[tokio::test]
async fn partial_file_is_private_before_ffmpeg_completion_and_cancellation() {
    use std::os::unix::fs::MetadataExt;
    let (database, directory, runtime) = fixture().await;
    let source = directory.path().join("blocked-input.fifo");
    assert!(tokio::process::Command::new("mkfifo")
        .arg(&source)
        .status()
        .await
        .unwrap()
        .success());
    let id = uuid::Uuid::new_v4().to_string();
    let output = runtime.paths.file(&id).unwrap();
    let partial = output.with_extension("part.mp4");
    let paths = runtime.paths.clone();
    let source_text = source.to_str().unwrap().to_string();
    let task_id = id.clone();
    let task = tokio::spawn(async move {
        extract(
            &paths,
            &source_text,
            VodRange { start: 15, end: 75 },
            &task_id,
        )
        .await
    });
    let metadata = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match tokio::fs::metadata(&partial).await {
                Ok(metadata) => break metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tokio::task::yield_now().await;
                }
                Err(error) => panic!("partial metadata failed: {error}"),
            }
        }
    })
    .await
    .unwrap();
    assert!(!task.is_finished());
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(
        tokio::fs::metadata(&runtime.paths.directory)
            .await
            .unwrap()
            .mode()
            & 0o777,
        0o700
    );
    assert!(!output.exists());
    let collision = extract(
        &runtime.paths,
        source.to_str().unwrap(),
        VodRange { start: 15, end: 75 },
        &id,
    )
    .await;
    assert!(
        matches!(collision, Err(tb_vod_archive::private_range::RangeError::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists)
    );
    assert_eq!(
        tokio::fs::metadata(&partial).await.unwrap().ino(),
        metadata.ino()
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(
        tokio::fs::metadata(&partial).await.unwrap().mode() & 0o777,
        0o600
    );
    cleanup(&database.pool, &runtime.paths).await.unwrap();
    assert!(!partial.exists());
    assert!(!output.exists());
}

#[tokio::test]
async fn synthetic_hls_is_seeked_and_trimmed_by_actual_ffmpeg() {
    let (_database, directory, runtime) = fixture().await;
    let source = directory.path().join("index-dvr.m3u8");
    assert!(tokio::process::Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x90:rate=2",
            "-t",
            "90",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-threads",
            "1",
            "-g",
            "10",
            "-sc_threshold",
            "0",
            "-hls_time",
            "5",
            "-hls_playlist_type",
            "vod",
            "-f",
            "hls",
        ])
        .arg(&source)
        .status()
        .await
        .unwrap()
        .success());
    let mut entries = tokio::fs::read_dir(directory.path()).await.unwrap();
    let mut input_bytes = 0;
    let mut segments = 0;
    while let Some(entry) = entries.next_entry().await.unwrap() {
        if entry.file_type().await.unwrap().is_file() {
            input_bytes += entry.metadata().await.unwrap().len();
            if entry.path().extension().is_some_and(|ext| ext == "ts") {
                segments += 1;
            }
        }
    }
    assert!(input_bytes < 2 * 1024 * 1024);
    assert_eq!(segments, 18);
    let id = uuid::Uuid::new_v4().to_string();
    let size = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        extract(
            &runtime.paths,
            source.to_str().unwrap(),
            VodRange { start: 15, end: 75 },
            &id,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(size > 0 && size < tb_domain::vod_range::MAX_FILE_BYTES);
    let output = runtime.paths.file(&id).unwrap();
    let first = decoded_frame(&output, "0").await;
    let expected = decoded_frame(&source, "15").await;
    let wrong_start = decoded_frame(&source, "0").await;
    assert!(frame_difference(&first, &expected) < 6.0);
    assert!(frame_difference(&first, &wrong_start) > 10.0);
    let probe = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(&output)
        .output()
        .await
        .unwrap();
    assert!(probe.status.success());
    let duration: f64 = String::from_utf8(probe.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!((duration - 60.0).abs() <= 1.0);
}

#[tokio::test]
async fn actual_three_minute_video_is_playable_and_delivered_privately() {
    let (database, directory, runtime) = fixture().await;
    let source = directory.path().join("synthetic-source.mp4");
    assert!(tokio::process::Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x90:rate=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=400:sample_rate=8000",
            "-t",
            "210",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-threads",
            "1",
            "-c:a",
            "aac"
        ])
        .arg(&source)
        .status()
        .await
        .unwrap()
        .success());
    settings_handler(
        partner("42"),
        State(database.pool.clone()),
        Json(Settings { enabled: true }),
    )
    .await;
    let range = VodRange::parse("00:15 03:15").unwrap();
    let id = enqueue(&database.pool, "42", "123", range).await.unwrap();
    let size = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        extract(&runtime.paths, source.to_str().unwrap(), range, &id),
    )
    .await
    .unwrap()
    .unwrap();
    sqlx::query("UPDATE twitch_private_vod_jobs SET status='ready',file_bytes=$2 WHERE id=$1")
        .bind(&id)
        .bind(size as i64)
        .execute(&database.pool)
        .await
        .unwrap();
    let response = download_handler(
        partner("42"),
        State(database.pool.clone()),
        Path(id.clone()),
        Some(Extension(runtime.clone())),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let data = axum::body::to_bytes(
        response.into_body(),
        tb_domain::vod_range::MAX_FILE_BYTES as usize,
    )
    .await
    .unwrap();
    assert_eq!(data.len() as u64, size);
    let downloaded = directory.path().join("downloaded.mp4");
    tokio::fs::write(&downloaded, &data).await.unwrap();
    let first = decoded_frame(&downloaded, "0").await;
    let expected = decoded_frame(&source, "15").await;
    let wrong_start = decoded_frame(&source, "0").await;
    assert!(frame_difference(&first, &expected) < 6.0);
    assert!(frame_difference(&first, &wrong_start) > 10.0);
    let probe = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(downloaded)
        .output()
        .await
        .unwrap();
    assert!(probe.status.success());
    let duration: f64 = String::from_utf8(probe.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!((duration - 180.0).abs() <= 1.0);
    sqlx::query("UPDATE twitch_private_vod_jobs SET expires_at=now()-INTERVAL '1 second'")
        .execute(&database.pool)
        .await
        .unwrap();
    cleanup(&database.pool, &runtime.paths).await.unwrap();
    assert!(!runtime.paths.file(&id).unwrap().exists());
}
