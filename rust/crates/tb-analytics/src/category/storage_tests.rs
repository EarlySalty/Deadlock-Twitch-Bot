use super::*;

fn example() -> (tempfile::TempDir, Manifest, FieldCipher, ArchiveConfig) {
    let directory = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let day = "2026-09-27".parse().unwrap();
    let manifest = Manifest {
        id,
        day,
        kind: "chat".into(),
        object_path: format!("gdrive:category/{day}/chat-{id}.jsonl.zst.gcm"),
        state: "exporting".into(),
        rows: 0,
        checksum: None,
        cipher_checksum: None,
        cipher_bytes: None,
        key_id: "category-v1".into(),
    };
    let cipher = archive_cipher(&|name| {
        assert_eq!(name, "CATEGORY_ARCHIVE_KEY_V1");
        Some("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".into())
    })
    .unwrap();
    (directory, manifest, cipher, ArchiveConfig::default())
}
fn record(path: &Path, manifest: &mut Manifest, config: &ArchiveConfig, cipher: &FieldCipher) {
    let mut writer = ArchiveWriter::new(path, config, manifest, cipher).unwrap();
    for i in 0..12 {
        writer
            .row(&format!(
                "{{\"index\":{i},\"synthetic\":\"{}\"}}",
                "Ä".repeat(100000)
            ))
            .unwrap();
        assert!(writer.buffer.len() <= BLOCK);
    }
    let (rows, checksum, bytes) = writer.finish().unwrap();
    manifest.rows = rows;
    manifest.checksum = Some(checksum);
    manifest.cipher_bytes = Some(bytes as i64);
    manifest.cipher_checksum = Some(file_checksum(path).unwrap().0);
}
#[test]
fn bounded_compressed_binary_blocks_roundtrip_with_protected_temporary_files() {
    use std::os::unix::fs::PermissionsExt;
    let (directory, mut manifest, cipher, config) = example();
    let path = directory.path().join("archive.gcm");
    record(&path, &mut manifest, &config, &cipher);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let mut rows = 0;
    read_archive(&path, &manifest, &cipher, |_| {
        rows += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(rows, 12);
    assert!(manifest.cipher_bytes.unwrap() < 5000);
    assert!(ArchiveWriter::new(&path, &config, &manifest, &cipher).is_err());
}
#[test]
fn truncated_reordered_wrong_object_and_tampered_ciphertext_fail_even_with_updated_file_hash() {
    let (directory, mut manifest, cipher, config) = example();
    let path = directory.path().join("archive.gcm");
    record(&path, &mut manifest, &config, &cipher);
    let bytes = std::fs::read(&path).unwrap();
    let first_end = 13 + u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
    let second_end = first_end
        + 5
        + u32::from_be_bytes(bytes[first_end + 1..first_end + 5].try_into().unwrap()) as usize;
    let reordered = [
        bytes[..8].to_vec(),
        bytes[first_end..second_end].to_vec(),
        bytes[8..first_end].to_vec(),
        bytes[second_end..].to_vec(),
    ]
    .concat();
    for changed in [reordered, bytes[..bytes.len() - 1].to_vec(), {
        let mut changed = bytes.clone();
        changed[30] ^= 1;
        changed
    }] {
        std::fs::write(&path, &changed).unwrap();
        manifest.cipher_checksum = Some(file_checksum(&path).unwrap().0);
        manifest.cipher_bytes = Some(changed.len() as i64);
        assert!(read_archive(&path, &manifest, &cipher, |_| Ok(())).is_err());
    }
    std::fs::write(&path, &bytes).unwrap();
    manifest.cipher_checksum = Some(file_checksum(&path).unwrap().0);
    manifest.cipher_bytes = Some(bytes.len() as i64);
    manifest.id = Uuid::new_v4();
    manifest.object_path = format!(
        "gdrive:category/{}/chat-{}.jsonl.zst.gcm",
        manifest.day, manifest.id
    );
    assert!(read_archive(&path, &manifest, &cipher, |_| Ok(())).is_err());
}
#[test]
fn oversized_rows_missing_key_and_wrong_key_slot_fail_closed() {
    let (directory, mut manifest, cipher, config) = example();
    let path = directory.path().join("archive.gcm");
    let mut writer = ArchiveWriter::new(&path, &config, &manifest, &cipher).unwrap();
    assert!(writer.row(&"x".repeat(MAX_ROW + 1)).is_err());
    drop(writer);
    assert!(archive_cipher(&|_| None).is_err());
    assert!(archive_cipher(&|_| Some("bad".into())).is_err());
    manifest.key_id = "different-key".into();
    assert!(manifest.validate(&cipher).is_err());
}

#[tokio::test]
async fn asynchronous_hash_and_cancelled_integrity_checks_use_the_same_archive() {
    let (directory, mut manifest, cipher, config) = example();
    let path = directory.path().join("archive.gcm");
    record(&path, &mut manifest, &config, &cipher);
    assert_eq!(
        file_checksum_async(&path).await.unwrap(),
        file_checksum(&path).unwrap()
    );
    let cancelled = CancelRead::default();
    let stopped = cancelled.0.clone();
    drop(cancelled);
    let mut rows = 0;
    assert!(read_archive_with_cancel(
        &path,
        &manifest,
        &cipher,
        |_| {
            rows += 1;
            Ok(())
        },
        &|| stopped.load(Ordering::Acquire)
    )
    .is_err());
    assert_eq!(rows, 0);
}
