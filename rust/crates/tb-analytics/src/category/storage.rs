#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;

use chrono::{DateTime, NaiveDate, Utc};
use futures_util::TryStreamExt;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool, Row};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tb_crypto::field::FieldCipher;
use uuid::Uuid;
use zeroize::Zeroizing;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
const BLOCK: usize = 1024 * 1024;
const MAX_ROW: usize = BLOCK / 2;
const MAGIC: &[u8; 8] = b"TBCAT001";

pub use tb_config::category_archive::CategoryArchive as ArchiveConfig;
fn reserve(config: &ArchiveConfig, required: u64) -> Result<(), Error> {
    let disk = nix::sys::statvfs::statvfs(&config.temporary_directory)?;
    let available = disk.blocks_available().saturating_mul(disk.fragment_size());
    if available < config.min_free_bytes.saturating_add(required) {
        return Err("Plattenreserve für Archiv unterschritten".into());
    }
    Ok(())
}

fn reserve_database(config: &ArchiveConfig, required: u64, minimum: u64) -> Result<(), Error> {
    let disk = nix::sys::statvfs::statvfs("/var/lib/postgresql")?;
    let available = disk.blocks_available().saturating_mul(disk.fragment_size());
    if available < config.min_free_bytes.max(minimum).saturating_add(required) {
        return Err("Plattenreserve für PostgreSQL-Verdichtung unterschritten".into());
    }
    Ok(())
}

#[derive(Clone, Debug, sqlx::FromRow, Serialize)]
pub struct Manifest {
    pub id: Uuid,
    pub day: NaiveDate,
    pub kind: String,
    pub object_path: String,
    pub state: String,
    pub rows: i64,
    pub checksum: Option<String>,
    pub cipher_checksum: Option<String>,
    pub cipher_bytes: Option<i64>,
    pub key_id: String,
}
impl Manifest {
    fn aad(&self, block: u64, last: bool) -> Vec<u8> {
        format!(
            "category-archive:v1:{}:{}:{}:{}:{block}:{last}",
            self.id, self.day, self.kind, self.key_id
        )
        .into_bytes()
    }
    fn validate(&self, cipher: &FieldCipher) -> Result<(), Error> {
        let expected = format!(
            "gdrive:category/{}/{}-{}.jsonl.zst.gcm",
            self.day, self.kind, self.id
        );
        if !matches!(self.kind.as_str(), "chat" | "snapshots")
            || self.key_id != cipher.kid()
            || self.object_path != expected
        {
            return Err("Ungültiger Archivvertrag".into());
        }
        Ok(())
    }
}

struct ArchiveWriter<'a> {
    file: File,
    config: &'a ArchiveConfig,
    manifest: &'a Manifest,
    cipher: &'a FieldCipher,
    buffer: Zeroizing<Vec<u8>>,
    digest: Sha256,
    rows: i64,
    block: u64,
    bytes: u64,
}
impl<'a> ArchiveWriter<'a> {
    fn new(
        path: &Path,
        config: &'a ArchiveConfig,
        manifest: &'a Manifest,
        cipher: &'a FieldCipher,
    ) -> Result<Self, Error> {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(MAGIC)?;
        Ok(Self {
            file,
            config,
            manifest,
            cipher,
            buffer: Zeroizing::new(Vec::with_capacity(BLOCK)),
            digest: Sha256::new(),
            rows: 0,
            block: 0,
            bytes: 8,
        })
    }
    fn row(&mut self, row: &str) -> Result<(), Error> {
        if row.len() > MAX_ROW || row.contains('\n') {
            return Err("Archivzeile überschreitet Formatgrenze".into());
        }
        if self.buffer.len() + row.len() + 1 > BLOCK {
            self.flush(false)?;
        }
        self.buffer.extend_from_slice(row.as_bytes());
        self.buffer.push(b'\n');
        self.digest.update(row.as_bytes());
        self.digest.update(b"\n");
        self.rows += 1;
        Ok(())
    }
    fn flush(&mut self, last: bool) -> Result<(), Error> {
        let compressed = Zeroizing::new(zstd::stream::encode_all(self.buffer.as_slice(), 3)?);
        let encrypted = self
            .cipher
            .encrypt_bytes(&compressed, &self.manifest.aad(self.block, last))?;
        let next = self.bytes + 5 + encrypted.len() as u64;
        if next > self.config.max_file_bytes {
            return Err("Archivdatei überschreitet Platzgrenze".into());
        }
        reserve(self.config, encrypted.len() as u64 + 5)?;
        self.file.write_all(&[u8::from(last)])?;
        self.file
            .write_all(&(encrypted.len() as u32).to_be_bytes())?;
        self.file.write_all(&encrypted)?;
        self.buffer.clear();
        self.bytes = next;
        self.block += 1;
        Ok(())
    }
    fn finish(mut self) -> Result<(i64, String, u64), Error> {
        if !self.buffer.is_empty() {
            self.flush(false)?;
        }
        let checksum = hex::encode(self.digest.clone().finalize());
        self.buffer
            .extend_from_slice(serde_json::to_string(&(self.rows, &checksum))?.as_bytes());
        self.flush(true)?;
        self.file.sync_all()?;
        Ok((self.rows, checksum, self.bytes))
    }
}

#[derive(Default)]
struct CancelRead(Arc<AtomicBool>);
impl Drop for CancelRead {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[cfg(test)]
fn file_checksum(path: &Path) -> Result<(String, u64), Error> {
    file_checksum_with_cancel(path, &|| false)
}

fn file_checksum_with_cancel(
    path: &Path,
    cancelled: &impl Fn() -> bool,
) -> Result<(String, u64), Error> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0;
    loop {
        if cancelled() {
            return Err("Archivprüfung abgebrochen".into());
        }
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        bytes += n as u64;
    }
    Ok((hex::encode(digest.finalize()), bytes))
}

async fn file_checksum_async(path: &Path) -> Result<(String, u64), Error> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0;
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            return Ok((hex::encode(digest.finalize()), bytes));
        }
        digest.update(&buffer[..n]);
        bytes += n as u64;
    }
}

#[cfg(test)]
fn read_archive(
    path: &Path,
    manifest: &Manifest,
    cipher: &FieldCipher,
    consume: impl FnMut(&str) -> Result<(), Error>,
) -> Result<(), Error> {
    read_archive_with_cancel(path, manifest, cipher, consume, &|| false)
}

fn read_archive_with_cancel(
    path: &Path,
    manifest: &Manifest,
    cipher: &FieldCipher,
    mut consume: impl FnMut(&str) -> Result<(), Error>,
    cancelled: &impl Fn() -> bool,
) -> Result<(), Error> {
    manifest.validate(cipher)?;
    if manifest.cipher_bytes != Some(std::fs::metadata(path)?.len() as i64) {
        return Err("Archivgröße stimmt nicht mit Manifest überein".into());
    }
    let (checksum, bytes) = file_checksum_with_cancel(path, cancelled)?;
    if manifest.cipher_checksum.as_ref() != Some(&checksum)
        || manifest.cipher_bytes != Some(bytes as i64)
    {
        return Err("Archivdatei stimmt nicht mit Manifest überein".into());
    }
    let mut file = File::open(path)?;
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err("Unbekanntes Archivformat".into());
    }
    let mut digest = Sha256::new();
    let mut rows = 0i64;
    let mut index = 0u64;
    loop {
        if cancelled() {
            return Err("Archivprüfung abgebrochen".into());
        }
        let mut header = [0u8; 5];
        file.read_exact(&mut header)?;
        let last = match header[0] {
            0 => false,
            1 => true,
            _ => return Err("Ungültiger Archivblock".into()),
        };
        let length = u32::from_be_bytes(header[1..].try_into()?) as usize;
        if length > BLOCK + 65536 {
            return Err("Archivblock zu groß".into());
        }
        let mut encrypted = vec![0; length];
        file.read_exact(&mut encrypted)?;
        let compressed =
            Zeroizing::new(cipher.decrypt_bytes(&encrypted, &manifest.aad(index, last))?);
        let decoder = zstd::stream::read::Decoder::new(compressed.as_slice())?;
        let mut plain = Zeroizing::new(Vec::new());
        decoder.take((BLOCK + 1) as u64).read_to_end(&mut plain)?;
        if plain.len() > BLOCK {
            return Err("Archivblock überschreitet Entpackgrenze".into());
        }
        if last {
            let footer: (i64, String) = serde_json::from_slice(&plain)?;
            let actual = hex::encode(digest.finalize());
            if footer != (rows, actual.clone())
                || manifest.rows != rows
                || manifest.checksum.as_ref() != Some(&actual)
            {
                return Err("Archivintegrität verletzt".into());
            }
            if file.read(&mut [0u8; 1])? != 0 {
                return Err("Zusätzliche Archivdaten".into());
            }
            return Ok(());
        }
        let text = std::str::from_utf8(&plain)?;
        if !text.ends_with('\n') {
            return Err("Unvollständige Archivzeile".into());
        }
        for row in text
            .strip_suffix('\n')
            .ok_or("Archivzeile fehlt")?
            .split('\n')
        {
            if row.len() > MAX_ROW {
                return Err("Archivzeile zu groß".into());
            }
            digest.update(row.as_bytes());
            digest.update(b"\n");
            rows += 1;
            consume(row)?;
        }
        index += 1;
    }
}

pub async fn manifest(pool: &PgPool, id: Uuid) -> Result<Manifest, Error> {
    Ok(sqlx::query_as("SELECT id,day,kind,object_path,state,rows,checksum,cipher_checksum,cipher_bytes,key_id FROM category_archive_manifest WHERE id=$1").bind(id).fetch_one(pool).await?)
}

pub async fn dry_run(pool: &PgPool, from: NaiveDate, to: NaiveDate) -> Result<Value, Error> {
    if from > to {
        return Err("Ungültiger UTC-Zeitraum".into());
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL TIME ZONE 'UTC'")
        .execute(&mut *tx)
        .await?;
    let unresolved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT FROM twitch_streamers_partner_state WHERE is_partner=1 AND nullif(btrim(twitch_user_id),'') IS NULL)").fetch_one(&mut *tx).await?;
    if unresolved {
        return Err("Aktiver Partner ohne stabile Twitch-ID; Trockenlauf bleibt gesperrt".into());
    }
    let mut result = Vec::new();
    let mut day = from;
    while day <= to {
        for (kind,sql) in [
            ("chat","SELECT EXISTS(SELECT FROM twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=m.room_user_id) AS partner,count(*)::bigint AS rows,coalesce(sum(pg_column_size(m)),0)::bigint AS bytes FROM category_chat_messages m WHERE sent_at >= $1::date::timestamptz AND sent_at < ($1::date+1)::timestamptz GROUP BY 1"),
            ("snapshots","WITH selected AS (SELECT EXISTS(SELECT FROM twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=m.user_id) AS partner,pg_column_size(m) AS wire_bytes,n.version_id,CASE WHEN n.version_id IS NULL THEN 0 ELSE pg_column_size(n) END AS sample_bytes,CASE WHEN n.version_id IS NULL THEN pg_column_size(m) ELSE 0 END AS legacy_bytes FROM category_snapshots_read m LEFT JOIN category_snapshot_samples n USING(snapshot_at,stream_id) WHERE m.snapshot_at >= $1::date::timestamptz AND m.snapshot_at < ($1::date+1)::timestamptz), versions AS (SELECT k.partner,sum(pg_column_size(v))::bigint AS version_bytes FROM (SELECT DISTINCT partner,version_id FROM selected WHERE version_id IS NOT NULL) k JOIN category_snapshot_versions v ON v.id=k.version_id GROUP BY 1) SELECT s.partner,count(*)::bigint AS rows,coalesce(sum(s.wire_bytes),0)::bigint AS bytes,coalesce(sum(s.sample_bytes),0)::bigint AS sample_bytes,coalesce(sum(s.legacy_bytes),0)::bigint AS legacy_bytes,coalesce(max(v.version_bytes),0)::bigint AS version_bytes FROM selected s LEFT JOIN versions v USING(partner) GROUP BY 1")
        ] {
            let groups=sqlx::query(sql).bind(day).fetch_all(&mut *tx).await?;
            for partner in [false,true] {
                let row=groups.iter().find(|r|r.get::<bool,_>("partner")==partner);
                let mut item=serde_json::json!({"day":day,"kind":kind,"partner":partner,"rows":row.map_or(0,|r|r.get::<i64,_>("rows")),"row_bytes":row.map_or(0,|r|r.get::<i64,_>("bytes"))});
                if kind=="snapshots" {
                    for field in ["sample_bytes","version_bytes","legacy_bytes"] { item[field]=serde_json::json!(row.map_or(0,|r|r.get::<i64,_>(field))); }
                }
                result.push(item);
            }
        }
        day = day.succ_opt().ok_or("Datumsüberlauf")?;
    }
    tx.rollback().await?;
    Ok(serde_json::json!(result))
}

pub async fn backfill(pool: &PgPool, day: NaiveDate) -> Result<Value, Error> {
    let normalized: bool =
        sqlx::query_scalar("SELECT normalized FROM category_storage_state WHERE singleton")
            .fetch_one(pool)
            .await?;
    if normalized {
        let proof: Option<(i64, String)> =
            sqlx::query_as("SELECT rows,checksum FROM category_normalization_proofs WHERE day=$1")
                .bind(day)
                .fetch_optional(pool)
                .await?;
        return Ok(
            serde_json::json!({"day":day,"backfilled":0,"normalized":true,"proof":proof.map(|(rows,checksum)|serde_json::json!({"rows":rows,"checksum":checksum,"mismatches":0}))}),
        );
    }
    let start = day.and_hms_opt(0, 0, 0).ok_or("Datum")?.and_utc();
    let end = start + chrono::Duration::days(1);
    let mut total = 0u64;
    loop {
        let rows: Vec<(DateTime<Utc>,String)>=sqlx::query_as("SELECT o.snapshot_at,o.stream_id FROM category_stream_snapshots o WHERE snapshot_at >= $1 AND snapshot_at < $2 AND NOT EXISTS(SELECT FROM category_snapshot_samples n WHERE (n.snapshot_at,n.stream_id)=(o.snapshot_at,o.stream_id)) ORDER BY o.snapshot_at,o.stream_id LIMIT 1000").bind(start).bind(end).fetch_all(pool).await?;
        if rows.is_empty() {
            break;
        }
        let mut tx = pool.begin().await?;
        for (at, id) in &rows {
            sqlx::query("SELECT category_snapshot_put(snapshot_at,stream_id,user_id,user_login,viewer_count,title,language,started_at,tags,thumbnail_url,is_mature,sample_seconds) FROM category_stream_snapshots WHERE snapshot_at=$1 AND stream_id=$2").bind(at).bind(id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        total += rows.len() as u64;
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await?;
    let proof = prove_day(&mut tx, day).await?;
    tx.commit().await?;
    Ok(serde_json::json!({"day":day,"backfilled":total,"proof":proof}))
}

async fn prove_day(connection: &mut PgConnection, day: NaiveDate) -> Result<Value, Error> {
    let start = day.and_hms_opt(0, 0, 0).ok_or("Datum")?.and_utc();
    let end = start + chrono::Duration::days(1);
    let mut count = 0i64;
    let mut digest = Sha256::new();
    let mut stream=sqlx::query("SELECT category_snapshot_wire(ROW(o.*)::category_snapshots_normalized) AS original,category_snapshot_wire(n) AS reconstructed FROM category_stream_snapshots o LEFT JOIN category_snapshots_normalized n USING(snapshot_at,stream_id) WHERE o.snapshot_at >= $1 AND o.snapshot_at < $2 ORDER BY o.snapshot_at,o.stream_id").bind(start).bind(end).fetch(&mut *connection);
    while let Some(row) = stream.try_next().await? {
        let original: String = row.try_get("original")?;
        let reconstructed: Option<String> = row.try_get("reconstructed")?;
        if reconstructed.as_ref() != Some(&original) {
            return Err("Snapshot-Rekonstruktion stimmt nicht zeilenweise überein".into());
        }
        digest.update(original.as_bytes());
        digest.update(b"\n");
        count += 1;
    }
    drop(stream);
    let checksum = hex::encode(digest.finalize());
    sqlx::query("INSERT INTO category_normalization_proofs(day,rows,checksum) VALUES($1,$2,$3) ON CONFLICT(day) DO UPDATE SET rows=excluded.rows,checksum=excluded.checksum,proved_at=now()").bind(day).bind(count).bind(&checksum).execute(connection).await?;
    Ok(serde_json::json!({"rows":count,"checksum":checksum,"mismatches":0}))
}

pub async fn finalize_normalization(pool: &PgPool) -> Result<Value, Error> {
    let normalized: bool =
        sqlx::query_scalar("SELECT normalized FROM category_storage_state WHERE singleton")
            .fetch_one(pool)
            .await?;
    if normalized {
        let proofs: Vec<(NaiveDate, i64, String)> = sqlx::query_as(
            "SELECT day,rows,checksum FROM category_normalization_proofs ORDER BY day",
        )
        .fetch_all(pool)
        .await?;
        return Ok(serde_json::json!(proofs.into_iter().map(|(day,rows,checksum)|serde_json::json!({"day":day,"proof":{"rows":rows,"checksum":checksum,"mismatches":0}})).collect::<Vec<_>>()));
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("LOCK TABLE category_collection_runs IN SHARE MODE")
        .execute(&mut *tx)
        .await?;
    sqlx::query("LOCK TABLE category_stream_snapshots IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    sqlx::query("LOCK TABLE category_snapshot_samples IN SHARE MODE")
        .execute(&mut *tx)
        .await?;
    let native: bool = sqlx::query_scalar("SELECT writer_snapshot_at IS NOT NULL AND writer_snapshot_at=(SELECT max(snapshot_at) FROM category_collection_runs) FROM category_storage_state WHERE singleton").fetch_one(&mut *tx).await?;
    if !native {
        return Err("Letzter Messlauf stammt nicht nachweislich vom normalisierten Writer".into());
    }
    let days:Vec<NaiveDate>=sqlx::query_scalar("SELECT DISTINCT (snapshot_at AT TIME ZONE 'UTC')::date FROM category_stream_snapshots ORDER BY 1").fetch_all(&mut *tx).await?;
    let mut proofs = Vec::new();
    for day in days {
        proofs.push(serde_json::json!({"day":day,"proof":prove_day(&mut tx,day).await?}));
    }
    sqlx::raw_sql("CREATE OR REPLACE VIEW category_snapshots_read AS SELECT * FROM category_snapshots_normalized; DROP TABLE category_stream_snapshots; UPDATE category_storage_state SET normalized=true WHERE singleton;").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(serde_json::json!(proofs))
}

pub async fn backfill_chat_metrics(pool: &PgPool, day: NaiveDate) -> Result<u64, Error> {
    let start = day.and_hms_opt(0, 0, 0).ok_or("Datum")?.and_utc();
    let end = start + chrono::Duration::days(1);
    let mut count = 0;
    loop {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(782363991808)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("LOCK TABLE category_chat_messages IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let n=sqlx::query("INSERT INTO category_chat_metrics SELECT m.sent_at,m.room_user_id,m.message_id,m.chatter_user_id,m.detected_lang,m.message_len,m.shared_chat_copy,m.tags->>'source-room-id',m.tags->>'source-id' FROM category_chat_messages m WHERE sent_at >= $1 AND sent_at < $2 AND NOT EXISTS(SELECT FROM category_chat_metrics k WHERE (k.sent_at,k.room_user_id,k.message_id)=(m.sent_at,m.room_user_id,m.message_id)) ORDER BY sent_at,room_user_id,message_id LIMIT 1000 ON CONFLICT DO NOTHING").bind(start).bind(end).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        count += n;
        if n == 0 {
            break;
        }
    }
    loop {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(782363991808)")
            .execute(&mut *tx)
            .await?;
        let n=sqlx::query("WITH keys AS (SELECT sent_at,room_user_id,message_id FROM category_chat_messages WHERE sent_at >= $1 AND sent_at < $2 AND tags IS DISTINCT FROM category_compact_tags(tags) ORDER BY sent_at,room_user_id,message_id LIMIT 1000) UPDATE category_chat_messages m SET tags=category_compact_tags(m.tags) FROM keys k WHERE (m.sent_at,m.room_user_id,m.message_id)=(k.sent_at,k.room_user_id,k.message_id)").bind(start).bind(end).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        if n == 0 {
            break;
        }
    }
    Ok(count)
}

pub async fn finalize_chat_metrics(pool: &PgPool) -> Result<(), Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(782363991808)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("LOCK TABLE category_chat_messages IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    let missing:bool=sqlx::query_scalar("SELECT EXISTS(SELECT FROM category_chat_messages m LEFT JOIN category_chat_metrics k USING(sent_at,room_user_id,message_id) WHERE k.message_id IS NULL OR (m.chatter_user_id,m.detected_lang,m.message_len,m.shared_chat_copy,m.tags->>'source-room-id',m.tags->>'source-id') IS DISTINCT FROM (k.chatter_user_id,k.detected_lang,k.message_len,k.shared_chat_copy,k.source_room_id,k.source_id))").fetch_one(&mut *tx).await?;
    if missing {
        return Err("Chat-Kennzahlen unvollständig".into());
    }
    sqlx::query("UPDATE category_storage_state SET chat_metrics_ready=true WHERE singleton")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn create_manifest(
    pool: &PgPool,
    day: NaiveDate,
    kind: &str,
    cipher: &FieldCipher,
) -> Result<Manifest, Error> {
    let today: NaiveDate = sqlx::query_scalar("SELECT (now() AT TIME ZONE 'UTC')::date")
        .fetch_one(pool)
        .await?;
    if day >= today || !matches!(kind, "chat" | "snapshots") {
        return Err("Nur abgeschlossene UTC-Tage archivieren".into());
    }
    let unresolved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT FROM twitch_streamers_partner_state WHERE is_partner=1 AND nullif(btrim(twitch_user_id),'') IS NULL)").fetch_one(pool).await?;
    if unresolved {
        return Err("Aktiver Partner ohne stabile Twitch-ID; Auslagerung bleibt gesperrt".into());
    }
    let ready: bool = sqlx::query_scalar(
        "SELECT normalized AND chat_metrics_ready FROM category_storage_state WHERE singleton",
    )
    .fetch_one(pool)
    .await?;
    if !ready {
        return Err("Speicherübergang nicht abgeschlossen".into());
    }
    if let Some(id)=sqlx::query_scalar("SELECT id FROM category_archive_manifest WHERE day=$1 AND kind=$2 AND state IN ('exporting','exported') ORDER BY created_at LIMIT 1").bind(day).bind(kind).fetch_optional(pool).await? { return manifest(pool,id).await; }
    let id = Uuid::new_v4();
    let path = format!("gdrive:category/{day}/{kind}-{id}.jsonl.zst.gcm");
    sqlx::query("INSERT INTO category_archive_manifest(id,day,kind,object_path,state,key_id) VALUES($1,$2,$3,$4,'exporting',$5)").bind(id).bind(day).bind(kind).bind(path).bind(cipher.kid()).execute(pool).await?;
    manifest(pool, id).await
}

async fn export(
    pool: &PgPool,
    m: &Manifest,
    path: &Path,
    config: &ArchiveConfig,
    cipher: &FieldCipher,
) -> Result<(), Error> {
    m.validate(cipher)?;
    sqlx::query("UPDATE category_archive_manifest SET state='exporting',rows=0,checksum=NULL,cipher_checksum=NULL,cipher_bytes=NULL,verified_at=NULL WHERE id=$1 AND state IN ('exporting','exported')").bind(m.id).execute(pool).await?;
    sqlx::query("DELETE FROM category_archive_members WHERE manifest_id=$1")
        .bind(m.id)
        .execute(pool)
        .await?;
    let start = m.day.and_hms_opt(0, 0, 0).ok_or("Datum")?.and_utc();
    let end = start + chrono::Duration::days(1);
    let sql = if m.kind == "chat" {
        "SELECT c.sent_at AS at,c.room_user_id,c.message_id AS item_id,category_chat_wire(c) AS wire FROM category_chat_messages c WHERE sent_at >= $1 AND sent_at < $2 AND NOT EXISTS(SELECT FROM twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=c.room_user_id) ORDER BY sent_at,room_user_id,message_id"
    } else {
        "SELECT c.snapshot_at AS at,c.user_id AS room_user_id,c.stream_id AS item_id,category_snapshot_wire(c) AS wire FROM category_snapshots_normalized c WHERE snapshot_at >= $1 AND snapshot_at < $2 AND NOT EXISTS(SELECT FROM twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=c.user_id) ORDER BY snapshot_at,stream_id"
    };
    let mut writer = ArchiveWriter::new(path, config, m, cipher)?;
    let mut stream = sqlx::query(sql).bind(start).bind(end).fetch(pool);
    let mut batch = Vec::with_capacity(500);
    while let Some(row) = stream.try_next().await? {
        let wire: String = row.try_get("wire")?;
        writer.row(&wire)?;
        batch.push((
            row.try_get::<DateTime<Utc>, _>("at")?,
            row.try_get::<String, _>("room_user_id")?,
            row.try_get::<String, _>("item_id")?,
            hex::encode(Sha256::digest(wire.as_bytes())),
        ));
        if batch.len() == 500 {
            store_members(pool, m.id, &batch).await?;
            batch.clear();
        }
    }
    drop(stream);
    store_members(pool, m.id, &batch).await?;
    let (rows, checksum, bytes) = writer.finish()?;
    let (cipher_checksum, actual_bytes) = file_checksum_async(path).await?;
    if actual_bytes != bytes {
        return Err("Archivgröße geändert".into());
    }
    sqlx::query("UPDATE category_archive_manifest SET state='exported',rows=$2,checksum=$3,cipher_checksum=$4,cipher_bytes=$5 WHERE id=$1 AND state='exporting'").bind(m.id).bind(rows).bind(checksum).bind(cipher_checksum).bind(bytes as i64).execute(pool).await?;
    Ok(())
}
async fn store_members(
    pool: &PgPool,
    id: Uuid,
    batch: &[(DateTime<Utc>, String, String, String)],
) -> Result<(), Error> {
    if batch.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for (at, room, item, checksum) in batch {
        sqlx::query("INSERT INTO category_archive_members VALUES($1,$2,$3,$4,$5)")
            .bind(id)
            .bind(at)
            .bind(room)
            .bind(item)
            .bind(checksum)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub trait ObjectStore: Send + Sync {
    fn upload<'a>(
        &'a self,
        local: &'a Path,
        remote: &'a str,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send + 'a;
    fn download<'a>(
        &'a self,
        remote: &'a str,
        local: &'a Path,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send + 'a;
}
pub struct Drive {
    config: ArchiveConfig,
}
impl Drive {
    pub fn new(config: ArchiveConfig) -> Self {
        Self { config }
    }
    async fn execute(&self, args: Vec<String>) -> Result<(), Error> {
        let mut command = tokio::process::Command::new("/usr/local/bin/rclone");
        command
            .args(args)
            .args(["--max-size", &self.config.max_file_bytes.to_string()])
            .args([
                "--max-transfer",
                &self.config.max_file_bytes.to_string(),
                "--cutoff-mode",
                "hard",
            ])
            .args([
                "--transfers",
                "1",
                "--checkers",
                "1",
                "--buffer-size",
                "4Mi",
                "--retries",
                "2",
                "--low-level-retries",
                "2",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let status = match tokio::time::timeout(
            Duration::from_secs(self.config.command_timeout_seconds),
            child.wait(),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                child.kill().await?;
                return Err("rclone überschreitet Zeitgrenze".into());
            }
        };
        if !status.success() {
            return Err(format!("rclone fehlgeschlagen: {status}").into());
        }
        Ok(())
    }
}
impl ObjectStore for Drive {
    async fn upload(&self, local: &Path, remote: &str) -> Result<(), Error> {
        let folder = remote.rsplit_once('/').ok_or("Archivpfad")?.0;
        let args = tb_stream_audit::archiv::rclone_datei_args(local, folder);
        self.execute(args).await
    }
    async fn download(&self, remote: &str, local: &Path) -> Result<(), Error> {
        self.execute(vec![
            "copyto".into(),
            remote.into(),
            local.to_string_lossy().into_owned(),
        ])
        .await
    }
}

async fn lease(pool: &PgPool) -> Result<sqlx::pool::PoolConnection<sqlx::Postgres>, Error> {
    if pool.options().get_max_connections() < 3 {
        return Err("Archiv benötigt mindestens drei Poolverbindungen".into());
    }
    let mut connection = pool.acquire().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(782363991807)")
        .fetch_one(&mut *connection)
        .await?;
    if !locked {
        return Err("Archivar läuft bereits".into());
    }
    connection.close_on_drop();
    Ok(connection)
}

async fn serialized<T>(
    pool: &PgPool,
    operation: impl std::future::Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    let mut connection = lease(pool).await?;
    let result = operation.await;
    let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock(782363991807)")
        .fetch_one(&mut *connection)
        .await?;
    if !released {
        return Err("Archiv-Sperre konnte nicht bestätigt freigegeben werden".into());
    }
    result
}

pub async fn archive_day(
    pool: &PgPool,
    day: NaiveDate,
    kind: &str,
    config: &ArchiveConfig,
    cipher: &FieldCipher,
    store: &impl ObjectStore,
) -> Result<Manifest, Error> {
    config.validate()?;
    reserve(config, config.max_file_bytes.saturating_mul(2))?;
    serialized(pool, async {
    let m = create_manifest(pool, day, kind, cipher).await?;
    m.validate(cipher)?;
    let directory = tempfile::Builder::new()
        .prefix("tb-category-")
        .tempdir_in(&config.temporary_directory)?;
    let path = directory
        .path()
        .join(m.object_path.rsplit('/').next().ok_or("Archivdatei")?);
    if matches!(m.state.as_str(), "exporting" | "exported") {
        export(pool, &m, &path, config, cipher).await?;
        store.upload(&path, &m.object_path).await?;
    }
    let m = manifest(pool, m.id).await?;
    verify_remote(&m, directory.path(), config, cipher, store).await?;
    sqlx::query("UPDATE category_archive_manifest SET state='verified',verified_at=now() WHERE id=$1 AND state IN ('exported','verified')").bind(m.id).execute(pool).await?;
    manifest(pool, m.id).await
    }).await
}
async fn verify_remote(
    m: &Manifest,
    directory: &Path,
    config: &ArchiveConfig,
    cipher: &FieldCipher,
    store: &impl ObjectStore,
) -> Result<PathBuf, Error> {
    let bytes = m.cipher_bytes.ok_or("Archivgröße fehlt")?;
    if bytes <= 0 || bytes as u64 > config.max_file_bytes {
        return Err("Archivgröße ungültig".into());
    }
    reserve(config, bytes as u64)?;
    let path = directory.join("verified.gcm");
    store.download(&m.object_path, &path).await?;
    let cancelled = CancelRead::default();
    let stopped = cancelled.0.clone();
    let input = path.clone();
    let manifest = m.clone();
    let cipher = cipher.clone();
    tokio::task::spawn_blocking(move || {
        read_archive_with_cancel(&input, &manifest, &cipher, |_| Ok(()), &|| {
            stopped.load(Ordering::Acquire)
        })
    })
    .await??;
    Ok(path)
}

pub async fn remove_local(
    pool: &PgPool,
    id: Uuid,
    config: &ArchiveConfig,
    cipher: &FieldCipher,
    store: &impl ObjectStore,
) -> Result<u64, Error> {
    config.validate()?;
    if !config.remove_enabled {
        return Err("Lokales Entfernen ist in der Betriebskonfiguration gesperrt".into());
    }
    serialized(pool, async {
        let m = manifest(pool, id).await?;
        if !matches!(m.state.as_str(), "verified" | "removed") {
            return Err("Manifest nicht bestätigt".into());
        }
        let directory = tempfile::Builder::new()
            .prefix("tb-category-")
            .tempdir_in(&config.temporary_directory)?;
        verify_remote(&m, directory.path(), config, cipher, store).await?;
        let members: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM category_archive_members WHERE manifest_id=$1",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;
        if members != m.rows {
            return Err("Archiv-Mitgliederzahl stimmt nicht mit Manifest überein".into());
        }
        let mut total = 0;
        loop {
            let n: i64 = sqlx::query_scalar("SELECT category_archive_remove($1,1000)")
                .bind(id)
                .fetch_one(pool)
                .await?;
            total += n as u64;
            let state: String =
                sqlx::query_scalar("SELECT state FROM category_archive_manifest WHERE id=$1")
                    .bind(id)
                    .fetch_one(pool)
                    .await?;
            if state == "removed" {
                break;
            }
        }
        if m.kind == "chat" {
            compact_chat(pool, id, config).await?;
        }
        Ok(total)
    })
    .await
}

async fn compact_chat(pool: &PgPool, id: Uuid, config: &ArchiveConfig) -> Result<(), Error> {
    let (size, minimum): (i64,i64) = sqlx::query_as("SELECT coalesce(pg_total_relation_size(to_regclass('public.category_chat_messages_p'||to_char(m.day,'YYYYMMDD'))),0),c.min_free_bytes FROM category_archive_manifest m CROSS JOIN category_collector_config c WHERE m.id=$1 AND c.singleton").bind(id).fetch_one(pool).await?;
    reserve_database(config, (size as u64).saturating_mul(2), minimum as u64)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('statement_timeout',$1,true)")
        .bind(format!("{}s", config.command_timeout_seconds))
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT category_archive_compact($1)")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn timestamp(value: &Value) -> Result<DateTime<Utc>, Error> {
    let bytes = hex::decode(value.as_str().ok_or("Archivzeit")?)?;
    let micros = i64::from_be_bytes(bytes.try_into().map_err(|_| "Archivzeitgröße")?)
        .checked_add(946_684_800_000_000)
        .ok_or("Archivzeitüberlauf")?;
    DateTime::from_timestamp_micros(micros).ok_or_else(|| "Archivzeit außerhalb Bereich".into())
}
fn float(value: &Value) -> Result<f64, Error> {
    let bytes = hex::decode(value.as_str().ok_or("Archivzahl")?)?;
    Ok(f64::from_be_bytes(
        bytes.try_into().map_err(|_| "Archivzahlgröße")?,
    ))
}

pub async fn restore(
    pool: &PgPool,
    id: Uuid,
    config: &ArchiveConfig,
    cipher: &FieldCipher,
    store: &impl ObjectStore,
) -> Result<u64, Error> {
    config.validate()?;
    serialized(pool, async {
    let m = manifest(pool, id).await?;
    if !matches!(m.state.as_str(), "verified" | "removed" | "restored") {
        return Err("Archiv noch nicht bestätigt".into());
    }
    let directory = tempfile::Builder::new()
        .prefix("tb-category-")
        .tempdir_in(&config.temporary_directory)?;
    let path = verify_remote(&m, directory.path(), config, cipher, store).await?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL TIME ZONE 'UTC'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(782363991808)")
        .execute(&mut *tx)
        .await?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<String>(16);
    let reader_cipher = cipher.clone();
    let reader_manifest = m.clone();
    let cancelled = CancelRead::default();
    let stopped = cancelled.0.clone();
    let reader = tokio::task::spawn_blocking(move || {
        let result = read_archive_with_cancel(&path, &reader_manifest, &reader_cipher, |row| {
            sender
                .blocking_send(row.to_owned())
                .map_err(|_| "Rückholung abgebrochen".into())
        }, &|| stopped.load(Ordering::Acquire));
        drop(sender);
        result
    });
    let tx_ref = &mut tx;
    let import = async move {
        let mut count = 0u64;
        while let Some(wire) = receiver.recv().await {
            if m.kind == "chat" {
                let message: super::RawMessage = serde_json::from_str(&wire)?;
                if message.sent_at.date_naive() != m.day {
                    return Err::<u64, Error>("Archivtag stimmt nicht".into());
                }
                sqlx::query("SELECT category_restore_chat($1::jsonb)")
                    .bind(wire)
                    .execute(&mut **tx_ref)
                    .await?;
            } else {
                let row: Value = serde_json::from_str(&wire)?;
                let at = timestamp(&row["snapshot_at"])?;
                if at.date_naive() != m.day {
                    return Err("Archivtag stimmt nicht".into());
                }
                sqlx::query("SELECT category_snapshot_put($1,$2,$3,$4,$5,$6,$7,$8,$9::text::text[],$10,$11,$12)")
                    .bind(at).bind(row["stream_id"].as_str()).bind(row["user_id"].as_str()).bind(row["user_login"].as_str())
                    .bind(row["viewer_count"].as_i64()).bind(row["title"].as_str()).bind(row["language"].as_str())
                    .bind(timestamp(&row["started_at"])?).bind(row["tags"].as_str()).bind(row["thumbnail_url"].as_str())
                    .bind(row["is_mature"].as_bool()).bind(float(&row["sample_seconds"])?).execute(&mut **tx_ref).await?;
            }
            count += 1;
        }
        Ok::<u64, Error>(count)
    };
    let (read, imported) = tokio::join!(reader, import);
    read??;
    let count = imported?;
    sqlx::query("UPDATE category_archive_manifest SET state='restored' WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(count)
    }).await
}

pub fn archive_cipher(secret: &dyn Fn(&str) -> Option<String>) -> Result<FieldCipher, Error> {
    let key = Zeroizing::new(secret("CATEGORY_ARCHIVE_KEY_V1").ok_or("Archivschlüssel fehlt")?);
    FieldCipher::from_hex_key(key.trim(), "category-v1")
        .map_err(|_| "Archivschlüssel ungültig".into())
}

pub async fn pending_removals(pool: &PgPool) -> Result<Vec<Uuid>, Error> {
    Ok(sqlx::query_scalar("SELECT m.id FROM category_archive_manifest m WHERE m.state IN ('verified','removed') AND (m.state='verified' OR (m.kind='chat' AND m.compacted_at IS NULL) OR
        (m.kind='chat' AND EXISTS(SELECT FROM category_archive_members k JOIN category_chat_messages c ON (c.sent_at,c.room_user_id,c.message_id)=(k.at,k.room_user_id,k.item_id) WHERE k.manifest_id=m.id AND NOT EXISTS(SELECT FROM twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=c.room_user_id))) OR
        (m.kind='snapshots' AND EXISTS(SELECT FROM category_archive_members k JOIN category_snapshots_normalized c ON (c.snapshot_at,c.user_id,c.stream_id)=(k.at,k.room_user_id,k.item_id) WHERE k.manifest_id=m.id AND NOT EXISTS(SELECT FROM twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=c.user_id)))) ORDER BY m.day,m.kind LIMIT 2").fetch_all(pool).await?)
}

pub async fn run(
    pool: PgPool,
    config: ArchiveConfig,
    cipher: Option<Arc<FieldCipher>>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    if !config.enabled {
        return;
    }
    let mut interval = tokio::time::interval(Duration::from_secs(3600));
    loop {
        tokio::select! { biased; _=stop.changed()=>return, _=interval.tick()=>{} }
        let attempt = async {
            let cipher = cipher
                .as_ref()
                .ok_or("Archivschlüssel fehlt im vorhandenen Secret-Weg")?;
            let days:Vec<(NaiveDate,String)>=sqlx::query_as("SELECT DISTINCT day,kind FROM (
                SELECT (c.sent_at AT TIME ZONE 'UTC')::date AS day,'chat'::text AS kind FROM category_chat_messages c
                WHERE NOT EXISTS(SELECT FROM twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=c.room_user_id)
                AND NOT EXISTS(SELECT FROM category_archive_members k JOIN category_archive_manifest m ON m.id=k.manifest_id WHERE m.kind='chat' AND m.state IN ('verified','removed','restored') AND (k.at,k.room_user_id,k.item_id)=(c.sent_at,c.room_user_id,c.message_id))
                UNION ALL SELECT (c.snapshot_at AT TIME ZONE 'UTC')::date,'snapshots'::text FROM category_snapshots_normalized c
                WHERE NOT EXISTS(SELECT FROM twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=c.user_id)
                AND NOT EXISTS(SELECT FROM category_archive_members k JOIN category_archive_manifest m ON m.id=k.manifest_id WHERE m.kind='snapshots' AND m.state IN ('verified','removed','restored') AND (k.at,k.room_user_id,k.item_id)=(c.snapshot_at,c.user_id,c.stream_id))
                ) x WHERE day<(now() AT TIME ZONE 'UTC')::date ORDER BY day,kind LIMIT 2").fetch_all(&pool).await?;
            let drive = Drive::new(config.clone());
            for (day, kind) in days {
                archive_day(&pool, day, &kind, &config, cipher, &drive).await?;
            }
            if config.remove_enabled {
                let pending = pending_removals(&pool).await?;
                for id in pending {
                    remove_local(&pool, id, &config, cipher, &drive).await?;
                }
            }
            Ok::<(), Error>(())
        };
        let outcome = tokio::select! { biased; _=stop.changed()=>return, result=attempt=>result };
        if let Err(error) = outcome {
            tracing::error!(error = %error, "Kategorie-Auslagerung fehlgeschlagen");
            if record_failure(&pool, Utc::now()).await.is_err() {
                tracing::error!("Archivfehler konnte nicht dauerhaft entprellt werden");
            }
        }
    }
}

pub async fn record_failure(pool: &PgPool, now: DateTime<Utc>) -> Result<(), Error> {
    let mut tx = pool.begin().await?;
    let previous: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT last_alert_at FROM category_storage_state WHERE singleton FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("UPDATE category_storage_state SET failures=failures+1 WHERE singleton")
        .execute(&mut *tx)
        .await?;
    if previous.is_none_or(|at| at <= now - chrono::Duration::days(1)) {
        let content="Bei der Auslagerung der Kategorie- und Chatdaten ist ein Fehler aufgetreten. Die Auslagerung wird nicht ungeprüft fortgesetzt. Bitte Verschlüsselung, Plattenreserve und Drive-Verbindung prüfen.";
        sqlx::query("INSERT INTO category_archive_notifications(notification_day,content) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(now.with_timezone(&chrono_tz::Europe::Berlin).date_naive()).bind(content).execute(&mut *tx).await?;
        sqlx::query("UPDATE category_storage_state SET last_alert_at=$1 WHERE singleton")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tracing::error!(
            "Kategorie-Auslagerung angehalten; Meldung für den bestehenden Watchdog vorgemerkt"
        );
    }
    tx.commit().await?;
    Ok(())
}
