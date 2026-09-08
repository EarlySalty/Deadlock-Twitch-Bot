//! Begrenzte Unix-Gegenstelle vor dem vorhandenen synthetischen HTTP-Mock.
//! Ausschließlich Testcode: Der Produktionsclient bekommt keinen TCP-Fallback.
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    time::Duration,
};

pub struct InfisicalMock {
    pub path: PathBuf,
    pub owner: u32,
    task: tokio::task::JoinHandle<()>,
    _directory: tempfile::TempDir,
}
impl InfisicalMock {
    pub fn start(server: &wiremock::MockServer) -> Self {
        let directory = tempfile::Builder::new()
            .prefix("tb-uds-")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("api.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let owner = std::fs::metadata(&path).unwrap().uid();
        let address = *server.address();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept(), if connections.len() < 8 => {
                        let Ok((mut socket, _)) = accepted else { break };
                        connections.spawn(async move {
                            let _ = tokio::time::timeout(Duration::from_secs(15), async {
                                let mut upstream = tokio::net::TcpStream::connect(address).await?;
                                tokio::io::copy_bidirectional(&mut socket, &mut upstream).await
                            }).await;
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Self {
            path,
            owner,
            task,
            _directory: directory,
        }
    }
}
impl Drop for InfisicalMock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
