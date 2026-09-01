//! Enger Bubblewrap-Läufer für fremde Medien.
//!
//! Die Bot- und Dashboard-Prozesse tragen DB-/Provider-Zugänge. Deshalb dürfen
//! yt-dlp, ffprobe und FFmpeg weder diese Umgebung noch den Dienst-Dateibaum
//! erben. Alle Medien werden als bereits geöffnete FDs in einen neuen User-/PID-
//! Namespace gebunden; FFmpeg/ffprobe bekommen zusätzlich kein Netzwerk.

#![cfg(target_os = "linux")]

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

const BWRAP: &str = "/usr/bin/bwrap";
const O_DIRECTORY: i32 = 0o200000;
const O_NOFOLLOW: i32 = 0o400000;
const O_NONBLOCK: i32 = 0o4000;
const MAX_MEDIA_FILE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CAPTURED_STDOUT_BYTES: u64 = 64 * 1024;
const MAX_ADDRESS_SPACE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_CPU_SECONDS: u64 = 30 * 60;
const MAX_OPEN_FILES: u64 = 128;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SandboxError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("Medienprozess hat das Zeitlimit überschritten")]
    Timeout,
    #[error("Medienprozess hat zu viele Ausgabedaten geliefert")]
    OutputLimit,
}

pub(crate) struct SandboxCommand {
    command: tokio::process::Command,
    output_file: Option<File>,
    stdout_to_output: bool,
    capture_stdout: bool,
    // Die FDs müssen bis nach dem exec von bwrap offen und ohne CLOEXEC sein.
    _inherited_fds: Vec<OwnedFd>,
}

pub(crate) struct SandboxOutput {
    pub(crate) process: Output,
    pub(crate) output_len: Option<u64>,
}

impl SandboxCommand {
    pub(crate) fn args<I, S>(&mut self, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(args);
    }

    pub(crate) fn capture_stdout(&mut self) {
        self.capture_stdout = true;
    }

    pub(crate) async fn output(mut self, timeout: Duration) -> Result<SandboxOutput, SandboxError> {
        self.command
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        if self.capture_stdout {
            self.command.stdout(Stdio::piped());
        } else if self.stdout_to_output {
            let stdout = self
                .output_file
                .as_ref()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Ausgabedatei fehlt"))?
                .try_clone()?;
            self.command.stdout(Stdio::from(stdout));
        } else {
            self.command.stdout(Stdio::null());
        }
        let mut child = self.command.spawn()?;
        let capture_stdout = self.capture_stdout;
        let run = async {
            let stdout = if capture_stdout {
                use tokio::io::AsyncReadExt;
                let mut stdout = child.stdout.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "Sandbox-stdout fehlt")
                })?;
                let mut bounded = (&mut stdout).take(MAX_CAPTURED_STDOUT_BYTES + 1);
                let mut bytes = Vec::new();
                bounded.read_to_end(&mut bytes).await?;
                if bytes.len() as u64 > MAX_CAPTURED_STDOUT_BYTES {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(SandboxError::OutputLimit);
                }
                bytes
            } else {
                Vec::new()
            };
            let status = child.wait().await?;
            Ok::<Output, SandboxError>(Output {
                status,
                stdout,
                stderr: Vec::new(),
            })
        };
        let process = tokio::time::timeout(timeout, run)
            .await
            .map_err(|_| SandboxError::Timeout)??;
        if let Some(file) = self.output_file.as_ref() {
            file.sync_all()?;
        }
        let output_len = self
            .output_file
            .as_ref()
            .map(|file| file.metadata().map(|metadata| metadata.len()))
            .transpose()?;
        Ok(SandboxOutput {
            process,
            output_len,
        })
    }
}

/// Startet ffprobe/FFmpeg ausschließlich mit einer einzelnen, gehaltenen
/// Eingabedatei. Falls ein Ziel geschrieben wird, wird dessen Inode atomar und
/// no-follow angelegt und als einzelne `/output.mp4`-Datei gebunden. Geschwister-
/// jobs im Arbeitsverzeichnis bleiben für den Parser unsichtbar.
pub(crate) fn media_command(
    program: &Path,
    input: &Path,
    output: Option<&Path>,
) -> Result<SandboxCommand, SandboxError> {
    let sandbox_program = match program {
        path if path == Path::new("/usr/bin/ffmpeg") => "/usr/bin/ffmpeg",
        path if path == Path::new("/usr/bin/ffprobe") => "/usr/bin/ffprobe",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Nur fest verdrahtete Medienprogramme sind erlaubt",
            )
            .into());
        }
    };
    let binary = inheritable(open_regular_nofollow(program, false)?)?;
    let input = inheritable(open_regular_nofollow(input, true)?)?;
    let mut inherited = vec![binary, input];
    let (output_file, output_fd) = if let Some(output) = output {
        let file = File::from(create_regular_nofollow(output)?);
        let inherited_file = inheritable(duplicate_raw_fd(file.as_raw_fd())?)?;
        let raw = inherited_file.as_raw_fd();
        inherited.push(inherited_file);
        (Some(file), Some(raw))
    } else {
        (None, None)
    };

    let mut command = base_command(true);
    add_runtime_mounts(&mut command);
    command.args([
        "--ro-bind-fd",
        &inherited[0].as_raw_fd().to_string(),
        sandbox_program,
        "--ro-bind-fd",
        &inherited[1].as_raw_fd().to_string(),
        "/input.mp4",
    ]);
    if let Some(fd) = output_fd {
        command.args(["--bind-fd", &fd.to_string(), "/output.mp4"]);
    }
    command.args(["--", sandbox_program]);
    let allowed_fds: Vec<i32> = inherited.iter().map(AsRawFd::as_raw_fd).collect();
    install_child_limits_and_fd_allowlist(&mut command, &allowed_fds);
    Ok(SandboxCommand {
        command,
        output_file,
        stdout_to_output: false,
        capture_stdout: false,
        _inherited_fds: inherited,
    })
}

/// Startet das explizit injizierte yt-dlp-Binary. Anders als beim Parser bleibt
/// das Netzwerk bewusst geteilt; sichtbar sind trotzdem nur das Binary und die
/// minimale Laufzeitumgebung. Die bereits sicher angelegte Ausgabedatei ist
/// kein Mountpunkt: yt-dlp schreibt mit `-o -` direkt auf dessen parentseitig
/// gehaltenen stdout-FD. Dadurch sieht der Parser überhaupt keinen
/// beschreibbaren Hostpfad und kann auch kein Ziel per rename austauschen.
fn downloader_command(
    program: &Path,
    resolver_file: File,
    nsswitch_file: File,
    output_file: File,
) -> Result<SandboxCommand, SandboxError> {
    if !program.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "yt-dlp benötigt einen absoluten Release-Pfad",
        )
        .into());
    }
    let binary = inheritable(open_regular_nofollow(program, true)?)?;
    let resolver = inheritable(resolver_file.into())?;
    let nsswitch = inheritable(nsswitch_file.into())?;
    let binary_fd = binary.as_raw_fd();
    let resolver_fd = resolver.as_raw_fd();
    let nsswitch_fd = nsswitch.as_raw_fd();
    let inherited = vec![binary, resolver, nsswitch];

    // Der socket-aktivierte Helper besitzt eine eigene UID, keine Dienst-
    // Secrets/DB-Rechte und ein systemd-erzwungenes öffentliches Egress. Die
    // bwrap-Sicht bleibt zusätzlich auf Runtime + Resolver beschränkt.
    let mut command = base_command(false);
    add_runtime_mounts(&mut command);
    command.args([
        "--dir",
        "/etc/ssl",
        "--ro-bind",
        "/etc/ssl/certs",
        "/etc/ssl/certs",
        "--ro-bind",
        "/etc/ssl/openssl.cnf",
        "/etc/ssl/openssl.cnf",
        "--file",
        &resolver_fd.to_string(),
        "/etc/resolv.conf",
        "--file",
        &nsswitch_fd.to_string(),
        "/etc/nsswitch.conf",
        "--ro-bind",
        "/dev/null",
        "/etc/hosts",
        "--ro-bind-fd",
        &binary_fd.to_string(),
        "/yt-dlp",
        "--",
        "/yt-dlp",
    ]);
    let allowed_fds: Vec<i32> = inherited.iter().map(AsRawFd::as_raw_fd).collect();
    install_child_limits_and_fd_allowlist(&mut command, &allowed_fds);
    Ok(SandboxCommand {
        command,
        output_file: Some(output_file),
        stdout_to_output: true,
        capture_stdout: false,
        _inherited_fds: inherited,
    })
}

pub(crate) fn create_output_file_nofollow(path: &Path) -> io::Result<OwnedFd> {
    create_regular_nofollow(path)
}

/// Führt yt-dlp im credential-freien Helper aus. Der einzige beschreibbare FD
/// ist stdout und zeigt auf den vom Bot übergebenen Ziel-Inode.
pub(crate) async fn run_downloader_to_file(
    program: &Path,
    clip_url: &str,
    output_file: &File,
) -> Result<u64, SandboxError> {
    let resolver = open_regular_nofollow(Path::new("/etc/resolv.conf"), true)?;
    let resolver_file = File::from(resolver);
    let resolver_metadata = resolver_file.metadata()?;
    if resolver_metadata.uid() != 0
        || resolver_metadata.nlink() != 1
        || resolver_metadata.permissions().mode() & 0o022 != 0
        || resolver_metadata.len() > 16 * 1024
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unsicherer Downloader-Resolver",
        )
        .into());
    }
    let nsswitch_file = minimal_nsswitch_file()?;
    let mut command = downloader_command(
        program,
        resolver_file,
        nsswitch_file,
        output_file.try_clone()?,
    )?;
    command.args([
        "--ignore-config",
        "--no-playlist",
        "--no-part",
        "--force-overwrites",
        "--no-continue",
        "--quiet",
        "--no-warnings",
        "--max-filesize",
        "512M",
        "-f",
        "b",
        "-o",
        "-",
        clip_url,
    ]);
    let output = command.output(Duration::from_secs(10 * 60)).await?;
    if !output.process.status.success() {
        return Err(io::Error::other("Downloader fehlgeschlagen").into());
    }
    output
        .output_len
        .filter(|length| *length > 0 && *length <= MAX_MEDIA_FILE_BYTES)
        .ok_or_else(|| io::Error::other("Ungültige Downloader-Ausgabe").into())
}

fn minimal_nsswitch_file() -> io::Result<File> {
    let name = std::ffi::CString::new("tb-media-nsswitch")
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let fd =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(b"hosts: dns\n")?;
    file.rewind()?;
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

fn base_command(unshare_network: bool) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(BWRAP);
    command
        .env_clear()
        .args(["--unshare-user", "--unshare-pid"]);
    if unshare_network {
        command.arg("--unshare-net");
    }
    command.args([
        "--new-session",
        "--die-with-parent",
        "--uid",
        "0",
        "--gid",
        "0",
        "--cap-drop",
        "ALL",
        "--disable-userns",
        "--clearenv",
        "--setenv",
        "LC_ALL",
        "C",
        "--size",
        "134217728",
        "--tmpfs",
        "/tmp",
        "--dev",
        "/dev",
        "--chdir",
        "/tmp",
    ]);
    command
}

fn install_child_limits_and_fd_allowlist(
    command: &mut tokio::process::Command,
    allowed_fds: &[i32],
) {
    // Alles wird vor dem Fork vorbereitet. Im Child laufen ausschließlich die
    // async-signal-sicheren close_range/fcntl/setrlimit-Syscalls.
    let allowed_fds = allowed_fds.to_vec();
    unsafe {
        command.pre_exec(move || {
            #[repr(C)]
            struct RLimit {
                // Linux `rlim_t` ist `unsigned long`; c_ulong hält die ABI
                // damit auch auf anderen Pointerbreiten korrekt.
                current: std::os::raw::c_ulong,
                maximum: std::os::raw::c_ulong,
            }
            const RLIMIT_CPU: i32 = 0;
            const RLIMIT_FSIZE: i32 = 1;
            const RLIMIT_NOFILE: i32 = 7;
            const RLIMIT_AS: i32 = 9;
            const CLOSE_RANGE_CLOEXEC: u32 = 1 << 2;
            const FD_CLOEXEC: i32 = 1;
            const F_SETFD: i32 = 2;
            const EBADF: i32 = 9;
            const ENOSYS: i32 = 38;
            unsafe extern "C" {
                fn setrlimit(resource: i32, limit: *const RLimit) -> i32;
                fn getrlimit(resource: i32, limit: *mut RLimit) -> i32;
                fn close_range(first: u32, last: u32, flags: u32) -> i32;
                fn fcntl(fd: i32, command: i32, argument: i32) -> i32;
                fn __errno_location() -> *mut i32;
            }
            if close_range(3, u32::MAX, CLOSE_RANGE_CLOEXEC) != 0 {
                let errno = *__errno_location();
                if errno != ENOSYS {
                    return Err(io::Error::from_raw_os_error(errno));
                }
                // Manche eng gefilterten Laufzeitumgebungen melden den
                // Kernel-Syscall als ENOSYS. Dann markieren wir fail-closed
                // jeden möglichen FD bis zum bestehenden NOFILE-Hardlimit.
                let mut nofile = RLimit {
                    current: 0,
                    maximum: 0,
                };
                if getrlimit(RLIMIT_NOFILE, &mut nofile) != 0 {
                    return Err(io::Error::last_os_error());
                }
                let last = nofile.maximum.min(i32::MAX as std::os::raw::c_ulong) as i32;
                for fd in 3..last {
                    if fcntl(fd, F_SETFD, FD_CLOEXEC) != 0 && *__errno_location() != EBADF {
                        return Err(io::Error::last_os_error());
                    }
                }
            }
            for fd in &allowed_fds {
                if *fd >= 3 && fcntl(*fd, F_SETFD, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            for (resource, value) in [
                (RLIMIT_CPU, MAX_CPU_SECONDS),
                (RLIMIT_FSIZE, MAX_MEDIA_FILE_BYTES),
                (RLIMIT_NOFILE, MAX_OPEN_FILES),
                (RLIMIT_AS, MAX_ADDRESS_SPACE_BYTES),
            ] {
                let limit = RLimit {
                    current: value as std::os::raw::c_ulong,
                    maximum: value as std::os::raw::c_ulong,
                };
                if setrlimit(resource, &limit) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
}

fn add_runtime_mounts(command: &mut tokio::process::Command) {
    // Auf dem Zielhost praktisch getestete, kleinste Laufzeitmenge für die
    // Debian-FFmpeg-Binaries und das offizielle yt-dlp-Standalone.
    command.args([
        "--dir",
        "/usr",
        "--dir",
        "/usr/bin",
        "--dir",
        "/usr/lib",
        "--ro-bind",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/x86_64-linux-gnu",
        "--ro-bind",
        "/usr/lib64",
        "/usr/lib64",
        "--symlink",
        "usr/lib",
        "/lib",
        "--symlink",
        "usr/lib64",
        "/lib64",
        "--dir",
        "/etc",
        "--ro-bind",
        "/etc/ld.so.cache",
        "/etc/ld.so.cache",
        "--dir",
        "/etc/alternatives",
        "--ro-bind",
        "/etc/alternatives/libblas.so.3-x86_64-linux-gnu",
        "/etc/alternatives/libblas.so.3-x86_64-linux-gnu",
        "--ro-bind",
        "/etc/alternatives/liblapack.so.3-x86_64-linux-gnu",
        "/etc/alternatives/liblapack.so.3-x86_64-linux-gnu",
    ]);
}

fn inheritable(fd: OwnedFd) -> io::Result<OwnedFd> {
    unsafe extern "C" {
        fn dup(oldfd: i32) -> i32;
    }
    let duplicated = unsafe { dup(fd.as_raw_fd()) };
    if duplicated < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: dup liefert einen neuen, allein besessenen FD ohne FD_CLOEXEC.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
}

fn open_regular_nofollow(path: &Path, require_nonempty: bool) -> io::Result<OwnedFd> {
    let fd = open_componentwise(path, false)?;
    let file = File::from(fd);
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file()
        || (require_nonempty && metadata.len() == 0)
        || metadata.nlink() != 1
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsichere Mediendatei",
        ));
    }
    Ok(file.into())
}

fn open_directory_nofollow(path: &Path) -> io::Result<OwnedFd> {
    let fd = open_componentwise(path, true)?;
    let file = File::from(fd);
    if !file.metadata()?.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsicheres Medienverzeichnis",
        ));
    }
    Ok(file.into())
}

fn create_regular_nofollow(path: &Path) -> io::Result<OwnedFd> {
    if let Some((directory_fd, name)) = held_process_fd_child(path) {
        return create_at(directory_fd, &name);
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let directory = open_directory_nofollow(parent)?;
    create_at(directory.as_raw_fd(), name)
}

fn create_at(directory_fd: i32, name: &OsStr) -> io::Result<OwnedFd> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    const O_RDWR: i32 = 0o2;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const O_CLOEXEC: i32 = 0o2000000;
    unsafe extern "C" {
        fn openat(dirfd: i32, path: *const std::os::raw::c_char, flags: i32, mode: u32) -> i32;
    }
    if name.is_empty() || name == "." || name == ".." || name.as_bytes().contains(&b'/') {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let name =
        CString::new(name.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let fd = unsafe {
        openat(
            directory_fd,
            name.as_ptr(),
            O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK,
            0o640,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: openat liefert einen neuen, allein besessenen FD.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

fn open_componentwise(path: &Path, final_is_directory: bool) -> io::Result<OwnedFd> {
    if let Some(fd) = held_process_fd(path) {
        return duplicate_raw_fd(fd);
    }
    if let Some((directory_fd, name)) = held_process_fd_child(path) {
        let directory = duplicate_raw_fd(directory_fd)?;
        let child = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())).join(name);
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(if final_is_directory {
            O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK
        } else {
            O_NOFOLLOW | O_NONBLOCK
        });
        return Ok(options.open(child)?.into());
    }

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK);
    let mut current = if path.is_absolute() {
        options.open("/")?
    } else {
        options.open(".")?
    };
    let components: Vec<_> = path
        .components()
        .filter(|component| !matches!(component, Component::RootDir | Component::CurDir))
        .collect();
    if components.is_empty() {
        return Ok(current.into());
    }
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        };
        let is_last = index + 1 == components.len();
        let child = PathBuf::from(format!("/proc/self/fd/{}", current.as_raw_fd())).join(name);
        if is_last && !final_is_directory {
            let mut file_options = OpenOptions::new();
            file_options
                .read(true)
                .custom_flags(O_NOFOLLOW | O_NONBLOCK);
            return Ok(file_options.open(child)?.into());
        }
        current = options.open(child)?;
    }
    Ok(current.into())
}

fn held_process_fd(path: &Path) -> Option<i32> {
    let components: Vec<_> = path.components().collect();
    if components.len() != 5
        || components[0] != Component::RootDir
        || components[1].as_os_str() != "proc"
        || components[3].as_os_str() != "fd"
    {
        return None;
    }
    let process = components[2].as_os_str().to_str()?;
    if process != "self" && process.parse::<u32>().ok()? != std::process::id() {
        return None;
    }
    components[4].as_os_str().to_str()?.parse().ok()
}

fn held_process_fd_child(path: &Path) -> Option<(i32, std::ffi::OsString)> {
    let components: Vec<_> = path.components().collect();
    if components.len() != 6
        || components[0] != Component::RootDir
        || components[1].as_os_str() != "proc"
        || components[3].as_os_str() != "fd"
    {
        return None;
    }
    let process = components[2].as_os_str().to_str()?;
    if process != "self" && process.parse::<u32>().ok()? != std::process::id() {
        return None;
    }
    let fd = components[4].as_os_str().to_str()?.parse().ok()?;
    let Component::Normal(name) = components[5] else {
        return None;
    };
    Some((fd, name.to_os_string()))
}

fn duplicate_raw_fd(fd: i32) -> io::Result<OwnedFd> {
    unsafe extern "C" {
        fn dup(oldfd: i32) -> i32;
    }
    let duplicated = unsafe { dup(fd) };
    if duplicated < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: dup liefert einen neuen, allein besessenen FD.
        Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::AsRawFd;

    #[test]
    fn nsswitch_memfd_ist_inhaltlich_fest_und_versiegelt() {
        let mut file = minimal_nsswitch_file().unwrap();
        let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
        assert!(seals >= 0);
        let expected =
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        assert_eq!(seals & expected, expected);
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        assert_eq!(content, "hosts: dns\n");
        assert!(file.write_all(b"hosts: files\n").is_err());
    }

    #[test]
    fn gehalte_parent_fd_pfade_werden_vor_der_sandbox_dupliziert() {
        let root = std::env::temp_dir().join(format!("tb-media-fd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file_path = root.join("input.mp4");
        std::fs::write(&file_path, b"mp4").unwrap();
        let file = File::open(&file_path).unwrap();
        let path = PathBuf::from(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            file.as_raw_fd()
        ));
        let opened = open_regular_nofollow(&path, true).unwrap();
        assert!(File::from(opened).metadata().unwrap().is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn test_program(program: &Path, unshare_network: bool) -> SandboxCommand {
        let binary = inheritable(open_regular_nofollow(program, false).unwrap()).unwrap();
        let binary_fd = binary.as_raw_fd();
        let mut command = base_command(unshare_network);
        add_runtime_mounts(&mut command);
        command.args([
            "--ro-bind-fd",
            &binary_fd.to_string(),
            program.to_str().unwrap(),
            "--",
            program.to_str().unwrap(),
        ]);
        install_child_limits_and_fd_allowlist(&mut command, &[binary_fd]);
        SandboxCommand {
            command,
            output_file: None,
            stdout_to_output: false,
            capture_stdout: false,
            _inherited_fds: vec![binary],
        }
    }

    #[tokio::test]
    async fn child_umgebung_enthaelt_keinen_dienst_marker() {
        let mut command = test_program(Path::new("/usr/bin/env"), true);
        // Der Marker wird nur für diesen Test in die bwrap-Umgebung gelegt.
        // `--clearenv` muss ihn vor dem eigentlichen Medienprozess entfernen.
        command
            .command
            .env("TB_SANDBOX_TEST_MARKER", "darf-nicht-sichtbar-sein");
        command.capture_stdout();
        let output = command.output(Duration::from_secs(10)).await.unwrap();
        assert!(output.process.status.success());
        assert!(!String::from_utf8_lossy(&output.process.stdout).contains("TB_SANDBOX_TEST_MARKER"));
    }

    #[tokio::test]
    async fn parser_sandbox_kann_host_listener_nicht_erreichen() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut command = test_program(Path::new("/usr/bin/bash"), true);
        command.args(["-c", &format!("printf x >/dev/tcp/127.0.0.1/{port}")]);
        let output = command.output(Duration::from_secs(10)).await.unwrap();
        assert!(!output.process.status.success());
        listener.set_nonblocking(true).unwrap();
        assert!(matches!(
            listener.accept(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
        ));
    }

    #[tokio::test]
    async fn stdout_und_dateigroesse_sind_hart_begrenzt() {
        let mut noisy = test_program(Path::new("/usr/bin/yes"), true);
        noisy.capture_stdout();
        assert!(matches!(
            noisy.output(Duration::from_secs(10)).await,
            Err(SandboxError::OutputLimit)
        ));

        let mut limit = test_program(Path::new("/usr/bin/bash"), true);
        limit.args([
            "-c",
            "printf '%s %s %s %s' \"$(ulimit -f)\" \"$(ulimit -v)\" \"$(ulimit -t)\" \"$(ulimit -n)\"",
        ]);
        limit.capture_stdout();
        let output = limit.output(Duration::from_secs(10)).await.unwrap();
        assert!(output.process.status.success());
        // bash meldet RLIMIT_FSIZE auf diesem Zielsystem in KiB.
        assert_eq!(
            String::from_utf8_lossy(&output.process.stdout).trim(),
            format!(
                "{} {} {} {}",
                MAX_MEDIA_FILE_BYTES / 1024,
                MAX_ADDRESS_SPACE_BYTES / 1024,
                MAX_CPU_SECONDS,
                MAX_OPEN_FILES
            )
        );
    }

    #[tokio::test]
    async fn fremder_inheritable_fd_wird_vor_bwrap_exec_geschlossen() {
        let root = std::env::temp_dir().join(format!("tb-media-fd-leak-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("marker");
        std::fs::write(&marker, b"darf-nicht-lesbar-sein\n").unwrap();
        let leaked = inheritable(File::open(&marker).unwrap().into()).unwrap();
        let leaked_fd = leaked.as_raw_fd();
        let mut command = test_program(Path::new("/usr/bin/bash"), true);
        // Der FD bleibt im Parent absichtlich offen, gehört aber nicht zur in
        // pre_exec eingefrorenen Allowlist des bwrap-Aufrufs.
        command._inherited_fds.push(leaked);
        command.args([
            "-c",
            &format!("IFS= read -r value <&{leaked_fd} && test -n \"$value\""),
        ]);
        let output = command.output(Duration::from_secs(10)).await.unwrap();
        assert!(!output.process.status.success());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn downloader_stdout_befuellt_nur_den_gehaltenen_output_inode() {
        let root =
            std::env::temp_dir().join(format!("tb-media-downloader-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let destination = root.join("output.mp4");
        let sentinel = root.join("sentinel");
        std::fs::write(&sentinel, b"bleibt").unwrap();

        let output_file = File::from(create_output_file_nofollow(&destination).unwrap());
        let resolver = File::open("/etc/resolv.conf").unwrap();
        let mut command = downloader_command(
            Path::new("/usr/bin/bash"),
            resolver,
            minimal_nsswitch_file().unwrap(),
            output_file,
        )
        .unwrap();
        command.args(["-c", "printf 'sandbox-output'"]);
        let output = command.output(Duration::from_secs(10)).await.unwrap();

        assert!(output.process.status.success());
        assert_eq!(output.output_len, Some(14));
        assert_eq!(std::fs::read(&destination).unwrap(), b"sandbox-output");
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"bleibt");
        std::fs::remove_dir_all(root).unwrap();
    }
}
