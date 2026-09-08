use super::{EvalError, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path};

pub fn check_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
    {
        return Err(EvalError("private_path"));
    }
    for ancestor in path.ancestors() {
        if ancestor.join(".git").exists() {
            return Err(EvalError("repository_output_forbidden"));
        }
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(EvalError("symlink_forbidden"))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && ancestor == path => {}
            Err(_) => return Err(EvalError("private_path")),
        }
    }
    Ok(())
}

fn check_private(path: &Path, directory: bool) -> Result<()> {
    check_path(path)?;
    let meta = fs::symlink_metadata(path).map_err(|_| EvalError("private_metadata"))?;
    if meta.is_dir() != directory || meta.mode() & 0o077 != 0 || (!directory && !meta.is_file()) {
        return Err(EvalError("private_permissions"));
    }
    Ok(())
}

pub fn read_private(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    check_private(path, false)?;
    check_private(path.parent().ok_or(EvalError("private_parent"))?, true)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| EvalError("private_read"))?;
    let metadata = file.metadata().map_err(|_| EvalError("private_metadata"))?;
    if !metadata.is_file() || metadata.mode() & 0o077 != 0 || metadata.len() > max_bytes {
        return Err(EvalError("private_input_size"));
    }
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| EvalError("private_read"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(EvalError("private_input_size"));
    }
    Ok(bytes)
}

pub fn create_directory(path: &Path) -> Result<()> {
    check_path(path)?;
    check_private(path.parent().ok_or(EvalError("private_parent"))?, true)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(|_| EvalError("output_directory_exists_or_unavailable"))
}

pub fn create_file(path: &Path) -> Result<File> {
    check_path(path)?;
    check_private(path.parent().ok_or(EvalError("private_parent"))?, true)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| EvalError("private_output"))
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = create_file(path)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| EvalError("private_write"))
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn checkpoint(path: &Path, bytes: &[u8]) -> Result<()> {
    check_path(path)?;
    let pending = path.with_extension("pending");
    write(&pending, bytes)?;
    fs::rename(&pending, path).map_err(|_| EvalError("checkpoint_rename"))?;
    File::open(path.parent().ok_or(EvalError("private_parent"))?)
        .and_then(|f| f.sync_all())
        .map_err(|_| EvalError("checkpoint_sync"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn private_outputs_reject_overwrite_symlinks_and_public_parent() {
        let root = std::env::temp_dir().join(format!(
            "tb-replay-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let output = root.join("run");
        create_directory(&output).unwrap();
        write(&output.join("data"), b"synthetic").unwrap();
        assert_eq!(fs::metadata(&output).unwrap().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(output.join("data")).unwrap().mode() & 0o777,
            0o600
        );
        assert!(write(&output.join("data"), b"overwrite").is_err());
        std::os::unix::fs::symlink(output.join("data"), output.join("link")).unwrap();
        assert!(read_private(&output.join("link"), 100).is_err());
        fs::set_permissions(&output, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(write(&output.join("public"), b"synthetic").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
