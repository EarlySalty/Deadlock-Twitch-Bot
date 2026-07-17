use std::future::Future;

use tokio::sync::{mpsc, oneshot};
use tokio::task::{AbortHandle, JoinHandle, JoinSet};

struct TaskRecord {
    name: &'static str,
    handle: JoinHandle<()>,
    abort: AbortHandle,
}

enum SupervisorCommand {
    Spawn(TaskRecord),
    Shutdown(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct TaskSupervisor {
    tx: mpsc::UnboundedSender<SupervisorCommand>,
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
        let abort = handle.abort_handle();
        let record = TaskRecord {
            name,
            handle,
            abort,
        };
        if let Err(error) = self.tx.send(SupervisorCommand::Spawn(record)) {
            if let SupervisorCommand::Spawn(record) = error.0 {
                record.abort.abort();
            }
            tracing::error!(
                task = name,
                "Task-Supervisor nicht verfuegbar; Background-Task abgebrochen"
            );
        }
    }

    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(SupervisorCommand::Shutdown(tx)).is_err() {
            return;
        }
        let _ = rx.await;
    }
}

async fn run_supervisor(mut rx: mpsc::UnboundedReceiver<SupervisorCommand>) {
    let mut watchers = JoinSet::new();
    let mut aborts = Vec::new();
    loop {
        tokio::select! {
            Some(command) = rx.recv() => {
                match command {
                    SupervisorCommand::Spawn(record) => {
                        aborts.push(record.abort.clone());
                        watch_record(&mut watchers, record);
                    }
                    SupervisorCommand::Shutdown(reply) => {
                        rx.close();
                        while let Ok(command) = rx.try_recv() {
                            match command {
                                SupervisorCommand::Spawn(record) => {
                                    record.abort.abort();
                                    watch_record(&mut watchers, record);
                                }
                                SupervisorCommand::Shutdown(extra_reply) => {
                                    let _ = extra_reply.send(());
                                }
                            }
                        }
                        for abort in &aborts {
                            abort.abort();
                        }
                        while let Some(joined) = watchers.join_next().await {
                            log_joined(joined, true);
                        }
                        let _ = reply.send(());
                        break;
                    }
                }
            }
            Some(joined) = watchers.join_next(), if !watchers.is_empty() => {
                log_joined(joined, false);
            }
            else => break,
        }
    }
}

fn watch_record(
    watchers: &mut JoinSet<(&'static str, Result<(), tokio::task::JoinError>)>,
    record: TaskRecord,
) {
    watchers.spawn(async move {
        let result = record.handle.await;
        (record.name, result)
    });
}

fn log_joined(
    joined: Result<(&'static str, Result<(), tokio::task::JoinError>), tokio::task::JoinError>,
    shutting_down: bool,
) {
    match joined {
        Ok((name, Ok(()))) if !shutting_down => {
            tracing::error!(task = name, "Background-Task unerwartet beendet");
        }
        Ok((_name, Ok(()))) => {}
        Ok((name, Err(error))) if error.is_panic() => {
            tracing::error!(task = name, %error, "Background-Task ist gepanikt");
        }
        Ok((name, Err(error))) if !shutting_down => {
            tracing::error!(task = name, %error, "Background-Task wurde abgebrochen");
        }
        Ok((_name, Err(_error))) => {}
        Err(error) if error.is_panic() => {
            tracing::error!(%error, "Task-Supervisor-Watcher ist gepanikt");
        }
        Err(error) if !shutting_down => {
            tracing::error!(%error, "Task-Supervisor-Watcher wurde abgebrochen");
        }
        Err(_error) => {}
    }
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::oneshot;

    struct DropSignal(Arc<Mutex<Option<oneshot::Sender<()>>>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.0.lock().unwrap().take() {
                let _ = tx.send(());
            }
        }
    }

    #[tokio::test]
    async fn shutdown_bricht_registrierte_tasks_ab_wartet_auf_drop_und_leakt_keine_spaeten_spawns()
    {
        let supervisor = super::TaskSupervisor::start();
        let (drop_tx, drop_rx) = oneshot::channel();
        let guard = DropSignal(Arc::new(Mutex::new(Some(drop_tx))));
        supervisor.spawn("pending_test_task", async move {
            let _guard = guard;
            future::pending::<()>().await;
        });

        supervisor.shutdown().await;
        tokio::time::timeout(Duration::from_secs(1), drop_rx)
            .await
            .expect("shutdown muss den Task joinen")
            .expect("drop signal");

        let (late_drop_tx, late_drop_rx) = oneshot::channel();
        let late_guard = DropSignal(Arc::new(Mutex::new(Some(late_drop_tx))));
        let late_body_ran = Arc::new(AtomicBool::new(false));
        let late_body_ran_task = Arc::clone(&late_body_ran);
        supervisor.spawn("late_test_task", async move {
            let _guard = late_guard;
            future::pending::<()>().await;
            late_body_ran_task.store(true, Ordering::SeqCst);
        });
        tokio::time::timeout(Duration::from_secs(1), late_drop_rx)
            .await
            .expect("late spawn muss sofort abgebrochen werden")
            .expect("late drop signal");
        tokio::task::yield_now().await;
        assert!(!late_body_ran.load(Ordering::SeqCst));
    }
}
