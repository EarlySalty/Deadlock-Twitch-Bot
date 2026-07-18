use std::future::Future;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::AbortHandle;
use tokio::task::{JoinHandle, JoinSet};

struct TaskRecord {
    name: &'static str,
    handle: JoinHandle<()>,
    finite: bool,
}

enum SupervisorCommand {
    Spawn(TaskRecord),
    Shutdown(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct TaskSupervisor {
    tx: mpsc::UnboundedSender<SupervisorCommand>,
    closed: Arc<AtomicBool>,
    runner: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl TaskSupervisor {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let runner = tokio::spawn(run_supervisor(rx));
        Self {
            tx,
            closed: Arc::new(AtomicBool::new(false)),
            runner: Arc::new(Mutex::new(Some(runner))),
        }
    }

    pub fn spawn<F>(&self, name: &'static str, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_inner(name, future, false);
    }

    pub fn spawn_finite<F>(&self, name: &'static str, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_inner(name, future, true);
    }

    fn spawn_inner<F>(&self, name: &'static str, future: F, finite: bool)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if self.closed.load(Ordering::SeqCst) {
            tracing::error!(
                task = name,
                "Task-Supervisor ist heruntergefahren; Background-Task wird abgebrochen"
            );
            return;
        }
        let handle = tokio::spawn(future);
        let abort = handle.abort_handle();
        if self.closed.load(Ordering::SeqCst) {
            handle.abort();
            tracing::error!(
                task = name,
                "Task-Supervisor ist heruntergefahren; Background-Task wird abgebrochen"
            );
            return;
        }
        if let Err(error) = self.tx.send(SupervisorCommand::Spawn(TaskRecord {
            name,
            handle,
            finite,
        })) {
            match error.0 {
                SupervisorCommand::Spawn(record) => record.handle.abort(),
                SupervisorCommand::Shutdown(done) => {
                    let _ = done.send(());
                }
            }
            tracing::error!(
                task = name,
                "Task-Supervisor nicht verfuegbar; Background-Task wird abgebrochen"
            );
            return;
        }
        if self.closed.load(Ordering::SeqCst) {
            abort.abort();
        }
    }

    pub async fn shutdown(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            let (tx, rx) = oneshot::channel();
            if self.tx.send(SupervisorCommand::Shutdown(tx)).is_ok() {
                let _ = rx.await;
            }
        }
        if let Some(handle) = self.runner.lock().await.take() {
            if let Err(error) = handle.await {
                tracing::error!(%error, "Task-Supervisor fehlerhaft beendet");
            }
        }
    }
}

async fn run_supervisor(mut rx: mpsc::UnboundedReceiver<SupervisorCommand>) {
    let mut watchers = JoinSet::new();
    let mut aborts = Vec::<(&'static str, AbortHandle)>::new();
    loop {
        tokio::select! {
            command = rx.recv() => match command {
                Some(SupervisorCommand::Spawn(record)) => {
                    aborts.retain(|(_, abort)| !abort.is_finished());
                    aborts.push((record.name, record.handle.abort_handle()));
                    watchers.spawn(async move {
                        let result = record.handle.await;
                        (record.name, record.finite, result)
                    });
                }
                Some(SupervisorCommand::Shutdown(done)) => {
                    for (_, abort) in aborts.drain(..) {
                        abort.abort();
                    }
                    while let Some(joined) = watchers.join_next().await {
                        log_joined(joined, true);
                    }
                    let _ = done.send(());
                    break;
                }
                None => {
                    for (_, abort) in aborts.drain(..) {
                        abort.abort();
                    }
                    while let Some(joined) = watchers.join_next().await {
                        log_joined(joined, true);
                    }
                    break;
                }
            },
            joined = watchers.join_next(), if !watchers.is_empty() => {
                if let Some(joined) = joined {
                    log_joined(joined, false);
                    aborts.retain(|(_, abort)| !abort.is_finished());
                }
            }
        }
    }
}

fn log_joined(
    joined: Result<
        (&'static str, bool, Result<(), tokio::task::JoinError>),
        tokio::task::JoinError,
    >,
    shutting_down: bool,
) {
    match joined {
        Ok((name, false, Ok(()))) if !shutting_down => {
            tracing::error!(task = name, "Background-Task unerwartet beendet");
        }
        Ok((name, true, Ok(()))) if !shutting_down => {
            tracing::debug!(task = name, "Endlicher Background-Task beendet");
        }
        Ok((_, _, Ok(()))) => {}
        Ok((name, _, Err(error))) if error.is_panic() => {
            tracing::error!(task = name, %error, "Background-Task ist gepanikt");
        }
        Ok((name, _, Err(error))) if !shutting_down => {
            tracing::error!(task = name, %error, "Background-Task wurde abgebrochen");
        }
        Ok(_) => {}
        Err(error) if error.is_panic() => {
            tracing::error!(%error, "Task-Supervisor-Watcher ist gepanikt");
        }
        Err(error) if !shutting_down => {
            tracing::error!(%error, "Task-Supervisor-Watcher wurde abgebrochen");
        }
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tokio::time::{sleep, timeout, Duration};

    #[tokio::test]
    async fn shutdown_bricht_registrierte_tasks_ab_und_wartet() {
        let supervisor = TaskSupervisor::start();
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_in_task = Arc::clone(&stopped);

        supervisor.spawn("test_shutdown", async move {
            struct StopFlag(Arc<AtomicBool>);
            impl Drop for StopFlag {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            let _guard = StopFlag(stopped_in_task);
            std::future::pending::<()>().await;
        });
        sleep(Duration::from_millis(20)).await;

        timeout(Duration::from_secs(1), supervisor.shutdown())
            .await
            .expect("shutdown haengt nicht");

        assert!(stopped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn late_spawn_nach_shutdown_wird_sofort_abgebrochen() {
        let supervisor = TaskSupervisor::start();
        supervisor.shutdown().await;
        let ran = Arc::new(AtomicBool::new(false));
        let ran_in_task = Arc::clone(&ran);

        supervisor.spawn("late_spawn", async move {
            ran_in_task.store(true, Ordering::SeqCst);
        });
        sleep(Duration::from_millis(20)).await;

        assert!(!ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn endlicher_task_darf_regulaer_fertig_werden() {
        let supervisor = TaskSupervisor::start();
        let finished = Arc::new(AtomicBool::new(false));
        let finished_in_task = Arc::clone(&finished);

        supervisor.spawn_finite("finite_task", async move {
            finished_in_task.store(true, Ordering::SeqCst);
        });
        timeout(Duration::from_secs(1), async {
            while !finished.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("endlicher Task wird ausgeführt");

        supervisor.shutdown().await;
    }
}
