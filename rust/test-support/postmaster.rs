use std::{
    fs::{self, File},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const RAM_ROOT: &str = "/dev/shm";
const RAM_PREFIX: &str = "pgtest-";
const OWNER_LOCK: &str = "owner.lock";
const ORPHAN_AGE: Duration = Duration::from_secs(60);
const START_LIMIT: Duration = Duration::from_secs(30);
const STOP_LIMIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(10);

pub struct RamDir {
    path: PathBuf,
    _owner: File,
}

impl RamDir {
    pub fn new(label: &str) -> Self {
        reclaim_orphans();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = Path::new(RAM_ROOT).join(format!(
            "{RAM_PREFIX}{label}-{}-{}-{nonce}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .expect("RAM test directory");
        let owner = File::create(path.join(OWNER_LOCK)).expect("RAM test directory owner lock");
        owner
            .try_lock()
            .expect("RAM test directory owner lock held");
        Self {
            path,
            _owner: owner,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RamDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn reclaim_orphans() {
    let Ok(entries) = fs::read_dir(RAM_ROOT) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(RAM_PREFIX) {
            reclaim_orphan(&entry.path());
        }
    }
}

fn reclaim_orphan(path: &Path) {
    let lock = path.join(OWNER_LOCK);
    let old = fs::metadata(&lock)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age > ORPHAN_AGE));
    if !old {
        return;
    }
    let Ok(owner) = File::open(&lock) else {
        return;
    };
    if owner.try_lock().is_ok() && !postmaster_alive(path) {
        let _ = fs::remove_dir_all(path);
    }
}

fn postmaster_alive(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries.flatten().any(|entry| {
        fs::read_to_string(entry.path().join("postmaster.pid"))
            .ok()
            .and_then(|pid| pid.lines().next().map(|line| line.trim().to_owned()))
            .is_some_and(|pid| Path::new("/proc").join(pid).exists())
    })
}

pub struct Postmaster(Child);

impl Postmaster {
    pub fn start(data: &Path, socket: &Path, settings: &[&str], log: Option<&Path>) -> Self {
        let mut command = Command::new("/usr/bin/setpriv");
        command
            .args(["--pdeathsig", "INT", "--"])
            .arg("/usr/lib/postgresql/16/bin/postgres")
            .arg("-D")
            .arg(data)
            .arg("-k")
            .arg(socket)
            .args([
                "-c",
                "listen_addresses=",
                "-c",
                "dynamic_shared_memory_type=mmap",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log.map_or_else(Stdio::null, |log| {
                Stdio::from(File::create(log).expect("PostgreSQL log file"))
            }));
        for setting in settings {
            command.arg("-c").arg(setting);
        }
        let mut postmaster = Self(command.spawn().expect("PostgreSQL 16 postmaster start"));
        postmaster.wait_ready(data);
        postmaster
    }

    fn wait_ready(&mut self, data: &Path) {
        let deadline = Instant::now() + START_LIMIT;
        while !accepts_connections(data) {
            assert!(self.running(), "PostgreSQL postmaster exited during start");
            assert!(
                Instant::now() < deadline,
                "PostgreSQL postmaster start timed out"
            );
            thread::sleep(POLL);
        }
    }

    fn running(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }

    fn interrupt(&self) {
        let _ = Command::new("/usr/bin/kill")
            .args(["-INT", "--"])
            .arg(self.0.id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

impl Drop for Postmaster {
    fn drop(&mut self) {
        if self.running() {
            self.interrupt();
            let deadline = Instant::now() + STOP_LIMIT;
            while self.running() && Instant::now() < deadline {
                thread::sleep(POLL);
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn accepts_connections(data: &Path) -> bool {
    fs::read_to_string(data.join("postmaster.pid")).is_ok_and(|pid| {
        pid.lines()
            .nth(7)
            .is_some_and(|status| status.trim() == "ready")
    })
}
