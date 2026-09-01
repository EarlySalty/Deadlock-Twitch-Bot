//! Versioniertes SOCK_SEQPACKET-/SCM_RIGHTS-Protokoll zum Downloader-Helper.
//!
//! Der Bot legt den Ziel-Inode sicher an und übergibt nur diesen FD. Der
//! socket-aktivierte Helper läuft unter einer eigenen UID ohne DB-/Provider-
//! Zugänge und antwortet ausschließlich mit einem kurzen stabilen Code.

use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::media_sandbox::{create_output_file_nofollow, run_downloader_to_file};
use crate::preparation::validate_twitch_clip_url;

pub const DOWNLOADER_SOCKET_PATH: &str = "/run/deadlock-twitch-media-downloader/control.sock";
const REQUEST_MAGIC: &[u8; 4] = b"TBDL";
const RESPONSE_MAGIC: &[u8; 4] = b"TBDR";
const PROTOCOL_VERSION: u8 = 1;
const MAX_URL_BYTES: usize = 2048;
const MAX_CODE_BYTES: usize = 63;
const IPC_TIMEOUT: Duration = Duration::from_secs(11 * 60);
const MAX_OUTPUT_BYTES: u64 = 512 * 1024 * 1024;
const HELPER_USER: &str = "twitchdownload";
const CLIENT_USER: &str = "twitchbot";

#[derive(Debug, thiserror::Error)]
pub enum DownloaderIpcError {
    #[error("Downloader-Dienst ist nicht erreichbar")]
    Unavailable,
    #[error("Downloader-Protokoll ist ungültig")]
    Protocol,
    #[error("Downloader hat den Auftrag abgelehnt: {0}")]
    Rejected(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

struct SeqPacket {
    fd: OwnedFd,
}

impl AsRawFd for SeqPacket {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl SeqPacket {
    fn connect(path: &Path) -> io::Result<Self> {
        use std::os::unix::ffi::OsStrExt;
        let path = path.as_os_str().as_bytes();
        if path.is_empty() || path.len() >= 108 || path.contains(&0) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let raw =
            unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        address.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (target, source) in address.sun_path.iter_mut().zip(path.iter().copied()) {
            *target = source as libc::c_char;
        }
        let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + path.len() + 1;
        let result = unsafe {
            libc::connect(
                fd.as_raw_fd(),
                (&address as *const libc::sockaddr_un).cast(),
                length as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        set_pass_credentials(fd.as_raw_fd())?;
        set_timeouts(fd.as_raw_fd(), IPC_TIMEOUT)?;
        Ok(Self { fd })
    }

    fn duplicate(raw: RawFd) -> io::Result<Self> {
        let duplicated = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
        if duplicated < 0 {
            return Err(io::Error::last_os_error());
        }
        let packet = Self {
            fd: unsafe { OwnedFd::from_raw_fd(duplicated) },
        };
        let mut socket_type = 0;
        let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                packet.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_TYPE,
                (&mut socket_type as *mut libc::c_int).cast(),
                &mut length,
            )
        } != 0
            || socket_type != libc::SOCK_SEQPACKET
        {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        set_pass_credentials(packet.as_raw_fd())?;
        set_timeouts(packet.as_raw_fd(), IPC_TIMEOUT)?;
        Ok(packet)
    }

    #[cfg(test)]
    fn pair() -> io::Result<(Self, Self)> {
        let mut fds = [-1; 2];
        if unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok((
            Self {
                fd: unsafe { OwnedFd::from_raw_fd(fds[0]) },
            },
            Self {
                fd: unsafe { OwnedFd::from_raw_fd(fds[1]) },
            },
        ))
    }
}

fn set_pass_credentials(fd: RawFd) -> io::Result<()> {
    let enabled: libc::c_int = 1;
    if unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PASSCRED,
            (&enabled as *const libc::c_int).cast(),
            std::mem::size_of_val(&enabled) as libc::socklen_t,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_timeouts(fd: RawFd, timeout: Duration) -> io::Result<()> {
    let timeout = libc::timeval {
        tv_sec: timeout.as_secs() as libc::time_t,
        tv_usec: 0,
    };
    for option in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
        if unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                option,
                (&timeout as *const libc::timeval).cast(),
                std::mem::size_of_val(&timeout) as libc::socklen_t,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub async fn download_to_path(
    clip_url: &str,
    destination: &Path,
) -> Result<u64, DownloaderIpcError> {
    validate_twitch_clip_url(clip_url).map_err(|_| DownloaderIpcError::Protocol)?;
    let output = File::from(create_output_file_nofollow(destination)?);
    let result = async {
        let url = clip_url.to_string();
        let request_file = output.try_clone()?;
        tokio::task::spawn_blocking(move || request_over_socket(&url, &request_file))
            .await
            .map_err(|_| DownloaderIpcError::Unavailable)??;
        validate_downloaded_output(&output).await
    }
    .await;
    if result.is_err() && remove_owned_output(destination, &output).is_err() {
        tracing::warn!(
            code = "downloader_output_cleanup_failed",
            "Downloader-Ziel konnte nach Fehler nicht bereinigt werden"
        );
    }
    result
}

/// Entfernt ausschließlich den Namen des von diesem Aufruf angelegten Inodes.
/// Der Produktionspfad liegt in der bot-eigenen 0700-Arbeitsdirectory; die
/// dev-/ino-Prüfung verhindert zusätzlich, dass ein fremder Ersatzpfad gelöscht
/// wird, falls der Name wider Erwarten ausgetauscht wurde.
fn remove_owned_output(path: &Path, held: &File) -> io::Result<()> {
    let held_metadata = held.metadata()?;
    let named_metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !named_metadata.file_type().is_file()
        || named_metadata.dev() != held_metadata.dev()
        || named_metadata.ino() != held_metadata.ino()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Downloader-Ziel wurde ausgetauscht",
        ));
    }
    std::fs::remove_file(path)
}

async fn validate_downloaded_output(output: &File) -> Result<u64, DownloaderIpcError> {
    output.sync_all()?;
    let metadata = output.metadata()?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_OUTPUT_BYTES
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o640
    {
        return Err(DownloaderIpcError::Protocol);
    }
    validate_iso_bmff_header(output, metadata.len())?;
    let held_path = format!("/proc/{}/fd/{}", std::process::id(), output.as_raw_fd());
    let info = crate::video_processor::VideoProcessor::new("/usr/bin/ffmpeg", "/usr/bin/ffprobe")
        .get_video_info(&held_path)
        .await
        .map_err(|_| DownloaderIpcError::Protocol)?;
    if !info.duration.is_finite()
        || info.duration <= 0.0
        || info.duration > 180.0
        || !info.fps.is_finite()
        || info.fps <= 0.0
        || info.fps > 120.0
        || info.width <= 0
        || info.height <= 0
        || info.width > 3840
        || info.height > 2160
        || (info.width as u64).saturating_mul(info.height as u64) > 3840 * 2160
    {
        return Err(DownloaderIpcError::Protocol);
    }
    Ok(metadata.len())
}

fn validate_iso_bmff_header(output: &File, file_len: u64) -> Result<(), DownloaderIpcError> {
    use std::os::unix::fs::FileExt;

    let mut header = [0u8; 20];
    if file_len < 16 || output.read_exact_at(&mut header[..16], 0).is_err() {
        return Err(DownloaderIpcError::Protocol);
    }
    if &header[4..8] != b"ftyp" {
        return Err(DownloaderIpcError::Protocol);
    }
    let short_size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
    let (box_size, minimum_size) = if short_size == 1 {
        if file_len < 20 || output.read_exact_at(&mut header[16..20], 16).is_err() {
            return Err(DownloaderIpcError::Protocol);
        }
        (
            u64::from_be_bytes([
                header[8], header[9], header[10], header[11], header[12], header[13], header[14],
                header[15],
            ]),
            20,
        )
    } else {
        (short_size, 16)
    };
    if box_size < minimum_size || box_size > file_len {
        return Err(DownloaderIpcError::Protocol);
    }
    Ok(())
}

fn request_over_socket(url: &str, output: &File) -> Result<(), DownloaderIpcError> {
    let socket = SeqPacket::connect(Path::new(DOWNLOADER_SOCKET_PATH))
        .map_err(|_| DownloaderIpcError::Unavailable)?;
    send_request(&socket, url, output.as_raw_fd())?;
    if unsafe { libc::shutdown(socket.as_raw_fd(), libc::SHUT_WR) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let helper = account_ids(HELPER_USER).map_err(|_| DownloaderIpcError::Unavailable)?;
    read_response(&socket, Some(helper))
}

fn send_request(socket: &SeqPacket, url: &str, output_fd: RawFd) -> io::Result<()> {
    let url = url.as_bytes();
    if url.is_empty() || url.len() > MAX_URL_BYTES {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let mut payload = Vec::with_capacity(7 + url.len());
    payload.extend_from_slice(REQUEST_MAGIC);
    payload.push(PROTOCOL_VERSION);
    payload.extend_from_slice(&(url.len() as u16).to_be_bytes());
    payload.extend_from_slice(url);
    send_fd_frame(socket.as_raw_fd(), &payload, output_fd)
}

fn read_response(
    socket: &SeqPacket,
    expected_sender: Option<(u32, u32)>,
) -> Result<(), DownloaderIpcError> {
    let mut payload = [0u8; 7 + MAX_CODE_BYTES];
    let (length, credentials) = receive_response_frame(socket.as_raw_fd(), &mut payload)?;
    if length < 7 || &payload[..4] != RESPONSE_MAGIC || payload[4] != PROTOCOL_VERSION {
        return Err(DownloaderIpcError::Protocol);
    }
    if let Some((uid, gid)) = expected_sender {
        let credentials = credentials.ok_or(DownloaderIpcError::Protocol)?;
        if credentials.pid <= 1 || credentials.uid != uid || credentials.gid != gid {
            return Err(DownloaderIpcError::Protocol);
        }
    }
    let status = payload[5];
    let code_len = payload[6] as usize;
    if code_len > MAX_CODE_BYTES || length != 7 + code_len {
        return Err(DownloaderIpcError::Protocol);
    }
    let code =
        std::str::from_utf8(&payload[7..length]).map_err(|_| DownloaderIpcError::Protocol)?;
    if !stable_code(code) {
        return Err(DownloaderIpcError::Protocol);
    }
    match (status, code) {
        (0, "ok") => Ok(()),
        (1, code) => Err(DownloaderIpcError::Rejected(code.to_string())),
        _ => Err(DownloaderIpcError::Protocol),
    }
}

struct DownloadRequest {
    url: String,
    output: File,
}

fn receive_request(
    socket: &SeqPacket,
    expected_client: Option<(u32, u32)>,
) -> Result<DownloadRequest, DownloaderIpcError> {
    let mut payload = vec![0u8; 7 + MAX_URL_BYTES];
    let (length, mut fds, frame_credentials) =
        receive_request_frame(socket.as_raw_fd(), &mut payload)?;
    if length < 7 || &payload[..4] != REQUEST_MAGIC || payload[4] != PROTOCOL_VERSION {
        return Err(DownloaderIpcError::Protocol);
    }
    let url_len = u16::from_be_bytes([payload[5], payload[6]]) as usize;
    if url_len == 0 || url_len > MAX_URL_BYTES || length != 7 + url_len || fds.len() != 1 {
        return Err(DownloaderIpcError::Protocol);
    }
    require_request_eof(socket.as_raw_fd())?;
    let url =
        String::from_utf8(payload[7..length].to_vec()).map_err(|_| DownloaderIpcError::Protocol)?;
    validate_twitch_clip_url(&url).map_err(|_| DownloaderIpcError::Protocol)?;
    let peer = peer_credentials(socket.as_raw_fd())?;
    let frame_credentials = frame_credentials.ok_or(DownloaderIpcError::Protocol)?;
    if frame_credentials.pid != peer.pid
        || frame_credentials.uid != peer.uid
        || frame_credentials.gid != peer.gid
    {
        return Err(DownloaderIpcError::Protocol);
    }
    if let Some((uid, gid)) = expected_client {
        if peer.pid <= 1 || peer.uid != uid || peer.gid != gid {
            return Err(DownloaderIpcError::Protocol);
        }
    }
    let output = File::from(fds.pop().ok_or(DownloaderIpcError::Protocol)?);
    validate_received_output(&output, peer.uid)?;
    Ok(DownloadRequest { url, output })
}

fn validate_received_output(output: &File, expected_uid: u32) -> Result<(), DownloaderIpcError> {
    let metadata = output.metadata()?;
    let flags = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_GETFL) };
    let offset = unsafe { libc::lseek(output.as_raw_fd(), 0, libc::SEEK_CUR) };
    if flags < 0 || offset < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let writable = matches!(flags & libc::O_ACCMODE, libc::O_WRONLY | libc::O_RDWR);
    if !metadata.file_type().is_file()
        || metadata.len() != 0
        || metadata.uid() != expected_uid
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o640
        || offset != 0
        || !writable
        || flags & libc::O_APPEND != 0
    {
        return Err(DownloaderIpcError::Protocol);
    }
    Ok(())
}

fn receive_request_frame(
    fd: RawFd,
    payload: &mut [u8],
) -> io::Result<(usize, Vec<OwnedFd>, Option<libc::ucred>)> {
    let control_len =
        (unsafe { libc::CMSG_SPACE((std::mem::size_of::<RawFd>() * 4) as libc::c_uint) }
            + unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::ucred>() as u32) })
            as usize;
    let mut control = AlignedControl::new(control_len);
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = control.len_bytes();
    let received = unsafe { libc::recvmsg(fd, &mut message, libc::MSG_CMSG_CLOEXEC) };
    if received <= 0 {
        return Err(if received == 0 {
            io::Error::from(io::ErrorKind::UnexpectedEof)
        } else {
            io::Error::last_os_error()
        });
    }
    // Auch bei MSG_CTRUNC sind die im sichtbaren Teil gelieferten SCM_RIGHTS-
    // Deskriptoren bereits im Prozess installiert. Wir sammeln sie deshalb
    // zuerst als OwnedFd ein; jeder Fehler schließt sie anschließend per RAII.
    let (fds, credentials, mut invalid) = collect_control_messages(&message);
    invalid |= message.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0;
    if invalid {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok((received as usize, fds, credentials))
}

fn send_fd_frame(fd: RawFd, payload: &[u8], output_fd: RawFd) -> io::Result<()> {
    send_fds_frame(fd, payload, &[output_fd])
}

fn send_fds_frame(fd: RawFd, payload: &[u8], output_fds: &[RawFd]) -> io::Result<()> {
    if output_fds.is_empty() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let mut iov = libc::iovec {
        iov_base: payload.as_ptr().cast_mut().cast(),
        iov_len: payload.len(),
    };
    let fd_bytes = output_fds
        .len()
        .checked_mul(std::mem::size_of::<RawFd>())
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let control_len = unsafe { libc::CMSG_SPACE(fd_bytes as u32) } as usize;
    let mut control = AlignedControl::new(control_len);
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = control.len_bytes();
    let cmsg = unsafe { libc::CMSG_FIRSTHDR(&message) };
    if cmsg.is_null() {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    unsafe {
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(fd_bytes as u32) as usize;
        std::ptr::copy_nonoverlapping(
            output_fds.as_ptr(),
            libc::CMSG_DATA(cmsg).cast::<RawFd>(),
            output_fds.len(),
        );
    }
    let sent = unsafe { libc::sendmsg(fd, &message, libc::MSG_NOSIGNAL) };
    if sent < 0 {
        return Err(io::Error::last_os_error());
    }
    if sent as usize != payload.len() {
        return Err(io::Error::from(io::ErrorKind::WriteZero));
    }
    Ok(())
}

fn receive_response_frame(
    fd: RawFd,
    payload: &mut [u8],
) -> io::Result<(usize, Option<libc::ucred>)> {
    let control_len = (unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::ucred>() as u32) }
        + unsafe { libc::CMSG_SPACE((std::mem::size_of::<RawFd>() * 4) as u32) })
        as usize;
    let mut control = AlignedControl::new(control_len);
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = control.len_bytes();
    let received = unsafe { libc::recvmsg(fd, &mut message, libc::MSG_CMSG_CLOEXEC) };
    if received <= 0 {
        return Err(if received == 0 {
            io::Error::from(io::ErrorKind::UnexpectedEof)
        } else {
            io::Error::last_os_error()
        });
    }
    let (unexpected_fds, credentials, mut invalid) = collect_control_messages(&message);
    invalid |= message.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0;
    invalid |= !unexpected_fds.is_empty();
    if invalid {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok((received as usize, credentials))
}

/// `cmsghdr` und seine Nutzdaten verlangen mindestens Wortausrichtung. Ein
/// `Vec<u8>` garantiert diese für die FFI-Casts nicht.
struct AlignedControl {
    words: Vec<usize>,
}

impl AlignedControl {
    fn new(required_bytes: usize) -> Self {
        let words = required_bytes.div_ceil(std::mem::size_of::<usize>());
        Self {
            words: vec![0; words],
        }
    }

    fn as_mut_ptr(&mut self) -> *mut usize {
        self.words.as_mut_ptr()
    }

    fn len_bytes(&self) -> usize {
        self.words.len() * std::mem::size_of::<usize>()
    }
}

fn collect_control_messages(message: &libc::msghdr) -> (Vec<OwnedFd>, Option<libc::ucred>, bool) {
    let mut fds = Vec::new();
    let mut credentials = None;
    let mut invalid = false;
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(message) };
    while !cmsg.is_null() {
        let level = unsafe { (*cmsg).cmsg_level };
        let kind = unsafe { (*cmsg).cmsg_type };
        if level != libc::SOL_SOCKET {
            invalid = true;
        } else if kind == libc::SCM_RIGHTS {
            let header_len = unsafe { libc::CMSG_LEN(0) } as usize;
            let cmsg_len = unsafe { (*cmsg).cmsg_len };
            let data_len = cmsg_len.saturating_sub(header_len);
            if cmsg_len < header_len
                || data_len == 0
                || data_len % std::mem::size_of::<RawFd>() != 0
            {
                invalid = true;
            }
            let raw = unsafe { libc::CMSG_DATA(cmsg).cast::<RawFd>() };
            for index in 0..data_len / std::mem::size_of::<RawFd>() {
                let received_fd = unsafe { raw.add(index).read() };
                if received_fd < 0 {
                    invalid = true;
                } else {
                    // recvmsg(MSG_CMSG_CLOEXEC) hat den FD bereits in diesen
                    // Prozess installiert; OwnedFd garantiert Schließen auf
                    // jedem folgenden Protokollfehler.
                    fds.push(unsafe { OwnedFd::from_raw_fd(received_fd) });
                }
            }
        } else if kind == libc::SCM_CREDENTIALS {
            let expected =
                unsafe { libc::CMSG_LEN(std::mem::size_of::<libc::ucred>() as u32) } as usize;
            if credentials.is_some() || unsafe { (*cmsg).cmsg_len } != expected {
                invalid = true;
            } else {
                credentials = Some(unsafe { libc::CMSG_DATA(cmsg).cast::<libc::ucred>().read() });
            }
        } else {
            invalid = true;
        }
        cmsg = unsafe { libc::CMSG_NXTHDR(message, cmsg) };
    }
    (fds, credentials, invalid)
}

fn require_request_eof(fd: RawFd) -> io::Result<()> {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut poll, 1, 1_000) };
    if ready <= 0 {
        return Err(if ready == 0 {
            io::Error::from(io::ErrorKind::TimedOut)
        } else {
            io::Error::last_os_error()
        });
    }
    let mut byte = 0u8;
    let result = unsafe { libc::recv(fd, (&mut byte as *mut u8).cast(), 1, libc::MSG_DONTWAIT) };
    if result == 0 {
        Ok(())
    } else if result > 0 {
        Err(io::Error::from(io::ErrorKind::InvalidData))
    } else {
        Err(io::Error::last_os_error())
    }
}

fn peer_credentials(fd: RawFd) -> io::Result<libc::ucred> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    } != 0
        || length as usize != std::mem::size_of::<libc::ucred>()
    {
        return Err(io::Error::last_os_error());
    }
    Ok(credentials)
}

fn write_response(socket: &SeqPacket, status: u8, code: &str) -> io::Result<()> {
    if !stable_code(code) || code.len() > MAX_CODE_BYTES {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let mut response = Vec::with_capacity(7 + code.len());
    response.extend_from_slice(RESPONSE_MAGIC);
    response.push(PROTOCOL_VERSION);
    response.push(status);
    response.push(code.len() as u8);
    response.extend_from_slice(code.as_bytes());
    let sent = unsafe {
        libc::send(
            socket.as_raw_fd(),
            response.as_ptr().cast(),
            response.len(),
            libc::MSG_NOSIGNAL,
        )
    };
    if sent < 0 {
        return Err(io::Error::last_os_error());
    }
    if sent as usize != response.len() {
        return Err(io::Error::from(io::ErrorKind::WriteZero));
    }
    Ok(())
}

fn stable_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= MAX_CODE_BYTES
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

fn account_ids(name: &str) -> io::Result<(u32, u32)> {
    let name = CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 16 * 1024];
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if result.is_null() {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    Ok((entry.pw_uid, entry.pw_gid))
}

fn sibling_yt_dlp() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let parent = executable
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    let program = parent.join(OsStr::new("yt-dlp"));
    let metadata = std::fs::symlink_metadata(&program)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != 0
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o022 != 0
        || metadata.permissions().mode() & 0o111 == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unsicheres yt-dlp-Release-Artefakt",
        ));
    }
    Ok(program)
}

fn require_process_account(expected: (u32, u32)) -> io::Result<()> {
    let (uid, gid) = expected;
    let matches = unsafe {
        libc::getuid() == uid
            && libc::geteuid() == uid
            && libc::getgid() == gid
            && libc::getegid() == gid
    };
    if matches {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Downloader läuft nicht unter der vorgesehenen Dienstidentität",
        ))
    }
}

pub async fn serve_stdio() -> Result<(), DownloaderIpcError> {
    let helper = account_ids(HELPER_USER).map_err(|_| DownloaderIpcError::Protocol)?;
    require_process_account(helper).map_err(|_| DownloaderIpcError::Protocol)?;
    let socket = SeqPacket::duplicate(0)?;
    let client = account_ids(CLIENT_USER).map_err(|_| DownloaderIpcError::Protocol)?;
    serve_connection(socket, sibling_yt_dlp()?, Some(client)).await
}

async fn serve_connection(
    socket: SeqPacket,
    program: PathBuf,
    expected_client: Option<(u32, u32)>,
) -> Result<(), DownloaderIpcError> {
    let request = receive_request(&socket, expected_client)?;
    let outcome = run_downloader_to_file(&program, &request.url, &request.output).await;
    let outcome = match outcome {
        Ok(length) if length > 0 => (0, "ok"),
        Ok(_) => (1, "empty_output"),
        Err(_) => (1, "download_failed"),
    };
    write_response(&socket, outcome.0, outcome.1)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{FileExt, OpenOptionsExt};

    fn current_ids() -> (u32, u32) {
        (unsafe { libc::geteuid() }, unsafe { libc::getegid() })
    }

    #[test]
    fn client_bereinigt_nur_seinen_eigenen_ziel_inode() {
        let root =
            std::env::temp_dir().join(format!("tb-downloader-cleanup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("output.mp4");

        let owned = File::from(create_output_file_nofollow(&path).unwrap());
        remove_owned_output(&path, &owned).unwrap();
        assert!(!path.exists());

        let old = File::from(create_output_file_nofollow(&path).unwrap());
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"fremder Ersatz").unwrap();
        assert!(remove_owned_output(&path, &old).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"fremder Ersatz");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn helper_identitaet_wird_vor_jeder_verarbeitung_erzwungen() {
        let current = current_ids();
        assert!(require_process_account(current).is_ok());
        assert!(require_process_account((current.0.wrapping_add(1), current.1)).is_err());
        assert!(require_process_account((current.0, current.1.wrapping_add(1))).is_err());
    }

    #[test]
    fn scm_rights_transportiert_genau_den_gehaltenen_inode() {
        let root = std::env::temp_dir().join(format!("tb-downloader-ipc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("output.mp4");
        let output = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o640)
            .open(&path)
            .unwrap();
        let (client, server) = SeqPacket::pair().unwrap();
        set_pass_credentials(server.as_raw_fd()).unwrap();
        send_request(
            &client,
            "https://clips.twitch.tv/FancyClip",
            output.as_raw_fd(),
        )
        .unwrap();
        unsafe { libc::shutdown(client.as_raw_fd(), libc::SHUT_WR) };
        let request = receive_request(&server, Some(current_ids())).unwrap();
        let descriptor_flags = unsafe { libc::fcntl(request.output.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(descriptor_flags & libc::FD_CLOEXEC, 0);
        request.output.write_all_at(b"ipc", 0).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"ipc");
        std::fs::remove_dir_all(root).unwrap();
    }

    fn inode_fd_count(file: &File) -> usize {
        let metadata = file.metadata().unwrap();
        std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                std::fs::metadata(entry.path())
                    .map(|candidate| {
                        candidate.dev() == metadata.dev() && candidate.ino() == metadata.ino()
                    })
                    .unwrap_or(false)
            })
            .count()
    }

    #[test]
    fn truncierte_control_frames_leaken_keine_sichtbaren_rights() {
        let root =
            std::env::temp_dir().join(format!("tb-downloader-trunc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("marker.mp4");
        let marker = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o640)
            .open(&path)
            .unwrap();
        let repeated = vec![marker.as_raw_fd(); 16];

        let (request_sender, request_receiver) = SeqPacket::pair().unwrap();
        set_pass_credentials(request_receiver.as_raw_fd()).unwrap();
        send_fds_frame(request_sender.as_raw_fd(), b"request", &repeated).unwrap();
        let before_request = inode_fd_count(&marker);
        let mut request_payload = [0u8; 32];
        assert!(receive_request_frame(request_receiver.as_raw_fd(), &mut request_payload).is_err());
        assert_eq!(inode_fd_count(&marker), before_request);

        let (response_sender, response_receiver) = SeqPacket::pair().unwrap();
        set_pass_credentials(response_receiver.as_raw_fd()).unwrap();
        send_fds_frame(response_sender.as_raw_fd(), b"response", &repeated).unwrap();
        let before_response = inode_fd_count(&marker);
        let mut response_payload = [0u8; 32];
        assert!(
            receive_response_frame(response_receiver.as_raw_fd(), &mut response_payload).is_err()
        );
        assert_eq!(inode_fd_count(&marker), before_response);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn helper_lehnt_readonly_und_bereits_beschriebene_fds_ab() {
        let root =
            std::env::temp_dir().join(format!("tb-downloader-badfd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("output.mp4");
        std::fs::write(&path, b"vorhanden").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let readonly = File::open(&path).unwrap();
        assert!(validate_received_output(&readonly, current_ids().0).is_err());
        let writable = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        assert!(validate_received_output(&writable, current_ids().0).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn postgate_akzeptiert_nur_plausiblen_iso_bmff_ftyp_header() {
        let root =
            std::env::temp_dir().join(format!("tb-downloader-ftyp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();

        let valid_path = root.join("valid.mp4");
        std::fs::write(
            &valid_path,
            [
                0, 0, 0, 20, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 0, 0, b'i',
                b's', b'o', b'm',
            ],
        )
        .unwrap();
        let valid = File::open(valid_path).unwrap();
        assert!(validate_iso_bmff_header(&valid, 20).is_ok());

        let webm_path = root.join("video.webm");
        std::fs::write(
            &webm_path,
            [
                0x1a, 0x45, 0xdf, 0xa3, 0x9f, 0x42, 0x86, 0x81, 1, 0, 0, 0, 0, 0, 0, 0,
            ],
        )
        .unwrap();
        let webm = File::open(webm_path).unwrap();
        assert!(validate_iso_bmff_header(&webm, 16).is_err());

        let oversized_path = root.join("oversized.mp4");
        std::fs::write(
            &oversized_path,
            [
                0, 0, 1, 0, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 0, 0,
            ],
        )
        .unwrap();
        let oversized = File::open(oversized_path).unwrap();
        assert!(validate_iso_bmff_header(&oversized, 16).is_err());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn zweiter_frame_wird_abgelehnt() {
        let root =
            std::env::temp_dir().join(format!("tb-downloader-frames-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("output.mp4");
        let output = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o640)
            .open(&path)
            .unwrap();
        let (client, server) = SeqPacket::pair().unwrap();
        set_pass_credentials(server.as_raw_fd()).unwrap();
        send_request(
            &client,
            "https://clips.twitch.tv/FancyClip",
            output.as_raw_fd(),
        )
        .unwrap();
        unsafe {
            libc::send(
                client.as_raw_fd(),
                b"zweiter".as_ptr().cast(),
                7,
                libc::MSG_NOSIGNAL,
            );
            libc::shutdown(client.as_raw_fd(), libc::SHUT_WR);
        }
        assert!(receive_request(&server, Some(current_ids())).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn antworten_enthalten_credentials_und_nur_stabile_codes() {
        let (client, server) = SeqPacket::pair().unwrap();
        set_pass_credentials(server.as_raw_fd()).unwrap();
        set_pass_credentials(client.as_raw_fd()).unwrap();
        write_response(&server, 1, "download_failed").unwrap();
        assert!(matches!(
            read_response(&client, Some(current_ids())),
            Err(DownloaderIpcError::Rejected(code)) if code == "download_failed"
        ));
        assert!(write_response(&server, 1, "URL=https://intern").is_err());
    }

    #[tokio::test]
    #[ignore = "benötigt gestagtes offizielles yt-dlp und Netzwerk"]
    async fn offizieller_helper_liefert_einen_probe_gueltigen_twitch_clip() {
        let program = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/release/yt-dlp")
            .canonicalize()
            .expect("gestagtes yt-dlp fehlt");
        assert!(program.is_file(), "gestagtes yt-dlp fehlt");
        let root = std::env::temp_dir().join(format!("tb-helper-real-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let destination = root.join("clip.mp4");
        let output = File::from(create_output_file_nofollow(&destination).unwrap());
        let validation_output = output.try_clone().unwrap();
        let (client, server) = SeqPacket::pair().unwrap();
        set_pass_credentials(server.as_raw_fd()).unwrap();
        set_pass_credentials(client.as_raw_fd()).unwrap();
        let server_task = tokio::spawn(async move {
            let request = receive_request(&server, Some(current_ids())).unwrap();
            let outcome = run_downloader_to_file(&program, &request.url, &request.output).await;
            assert!(outcome.is_ok(), "Sandbox-Downloadfehler: {outcome:?}");
            write_response(&server, 0, "ok").unwrap();
            Ok::<(), DownloaderIpcError>(())
        });
        let client_task = tokio::task::spawn_blocking(move || {
            send_request(
                &client,
                "https://clips.twitch.tv/FamousRoundHorseAMPTropPunch-jjgWeEWUq2Yw7caL",
                output.as_raw_fd(),
            )?;
            if unsafe { libc::shutdown(client.as_raw_fd(), libc::SHUT_WR) } != 0 {
                return Err(DownloaderIpcError::Io(io::Error::last_os_error()));
            }
            read_response(&client, Some(current_ids()))
        });
        let client_result = client_task.await.unwrap();
        let server_result = server_task.await.unwrap();
        assert!(server_result.is_ok(), "Helperfehler: {server_result:?}");
        client_result.unwrap();
        validate_downloaded_output(&validation_output)
            .await
            .unwrap();
        let metadata = std::fs::symlink_metadata(&destination).unwrap();
        assert!(metadata.file_type().is_file());
        assert!(metadata.len() > 0 && metadata.len() <= 512 * 1024 * 1024);
        std::fs::remove_dir_all(root).unwrap();
    }
}
