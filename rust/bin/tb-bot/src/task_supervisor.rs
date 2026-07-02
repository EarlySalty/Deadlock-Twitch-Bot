use std::future::Future;

use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

struct TaskRecord {
    name: &'static str,
    handle: JoinHandle<()>,
}

#[derive(Clone)]
pub struct TaskSupervisor {
    tx: mpsc::UnboundedSender<TaskRecord>,
}

impl TaskSupervisor {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(run_supervisor(rx));
        Self { tx }
    }

    pub fn spawn<F>(&self, name: &'static str, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(future);
        if self.tx.send(TaskRecord { name, handle }).is_err() {
            tracing::error!(
                task = name,
                "Task-Supervisor nicht verfuegbar; Background-Task laeuft unueberwacht"
            );
        }
    }
}

async fn run_supervisor(mut rx: mpsc::UnboundedReceiver<TaskRecord>) {
    let mut watchers = JoinSet::new();
    loop {
        tokio::select! {
            Some(record) = rx.recv() => {
                watchers.spawn(async move {
                    let result = record.handle.await;
                    (record.name, result)
                });
            }
            Some(joined) = watchers.join_next(), if !watchers.is_empty() => {
                match joined {
                    Ok((name, Ok(()))) => {
                        tracing::error!(task = name, "Background-Task unerwartet beendet");
                    }
                    Ok((name, Err(error))) if error.is_panic() => {
                        tracing::error!(task = name, %error, "Background-Task ist gepanikt");
                    }
                    Ok((name, Err(error))) => {
                        tracing::error!(task = name, %error, "Background-Task wurde abgebrochen");
                    }
                    Err(error) if error.is_panic() => {
                        tracing::error!(%error, "Task-Supervisor-Watcher ist gepanikt");
                    }
                    Err(error) => {
                        tracing::error!(%error, "Task-Supervisor-Watcher wurde abgebrochen");
                    }
                }
            }
            else => break,
        }
    }
}
