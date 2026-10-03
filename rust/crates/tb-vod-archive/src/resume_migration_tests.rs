//! Transaktionaler Umstieg anhand synthetischer Uploads, ohne YouTube-Aufrufe.
use super::tests::{pool, test_cipher};
use super::*;

#[tokio::test]
async fn resume_migration_preserves_offsets_and_rolls_back_on_invalid_ciphertext() {
    let pool = pool("t_token_resume_migration")
        .await
        .expect("Wegwerf-Datenbank erforderlich");
    let cipher = test_cipher();
    merke_vod(&pool, "synthetic-vod", "synthetic_owner", "42", "Test", 120)
        .await
        .unwrap();
    let vod = offene_vods(&pool, "42", 1).await.unwrap().remove(0);
    setze_teile(
        &pool,
        vod.id,
        "synthetic_owner",
        &["/synthetic/part0".into(), "/synthetic/part1".into()],
    )
    .await
    .unwrap();
    let rows = teile(&pool, vod.id, &cipher).await.unwrap();
    let a = rows[0].id;
    let b = rows[1].id;
    sqlx::query("UPDATE twitch_vod_archive_parts SET status='uploading',upload_offset=1048576,upload_session_uri='https://synthetic.invalid/resume' WHERE id=$1")
        .bind(a).execute(&pool).await.unwrap();
    sqlx::query(
        "UPDATE twitch_vod_archive_parts SET upload_session_uri='enc:v1:broken' WHERE id=$1",
    )
    .bind(b)
    .execute(&pool)
    .await
    .unwrap();
    assert!(migrate_resume_sessions(&pool, &cipher).await.is_err());
    let raw: String =
        sqlx::query_scalar("SELECT upload_session_uri FROM twitch_vod_archive_parts WHERE id=$1")
            .bind(a)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(raw, "https://synthetic.invalid/resume");
    assert!(teile(&pool, vod.id, &cipher).await.is_err());
    sqlx::query("UPDATE twitch_vod_archive_parts SET status='done',youtube_video_id='synthetic-video',upload_session_uri='https://synthetic.invalid/finished' WHERE id=$1")
        .bind(b).execute(&pool).await.unwrap();
    assert_eq!(migrate_resume_sessions(&pool, &cipher).await.unwrap(), 2);
    assert_eq!(migrate_resume_sessions(&pool, &cipher).await.unwrap(), 0);
    let restored = teile(&pool, vod.id, &cipher).await.unwrap();
    assert_eq!(
        restored[0].upload_session_uri.as_deref(),
        Some("https://synthetic.invalid/resume")
    );
    assert_eq!(restored[0].upload_offset, 1048576);
    assert_eq!(
        restored[1].youtube_video_id.as_deref(),
        Some("synthetic-video")
    );
    assert!(restored[1].upload_session_uri.is_none());
    let encoded: String =
        sqlx::query_scalar("SELECT upload_session_uri FROM twitch_vod_archive_parts WHERE id=$1")
            .bind(a)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!encoded.contains("https://"));
    assert!(tb_crypto::text::decrypt(&cipher, &encoded, &session_aad(b, "synthetic-vod")).is_err());
    assert!(tb_crypto::text::decrypt(&cipher, &encoded, &session_aad(a, "other-vod")).is_err());
    assert!(!format!("{:?}", restored[0]).contains("synthetic.invalid"));
    setze_teil_fertig(&pool, a, "synthetic-second-video")
        .await
        .unwrap();
    assert!(teile(&pool, vod.id, &cipher).await.unwrap()[0]
        .upload_session_uri
        .is_none());
    pool.close().await;
}
