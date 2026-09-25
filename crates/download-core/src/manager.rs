use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, ensure};
use tokio::{
    sync::{Notify, watch},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{AddRequest, Download, Settings, Snapshot, Status, paths, store::Store};

pub(crate) struct State {
    pub(crate) store: Store,
    pub(crate) downloads: Vec<Download>,
    settings: Settings,
    started: bool,
    closing: bool,
}

/// Owns a persistent queue. A database may be owned by only one manager at a time.
///
/// `start` is idempotent. `shutdown` is terminal and may be called repeatedly or
/// concurrently; drop the manager afterward to release its database ownership.
pub struct Manager {
    state: Mutex<State>,
    pub(crate) client: reqwest::Client,
    pub(crate) cancellation: CancellationToken,
    wake: Arc<Notify>,
    finished: watch::Sender<Option<std::result::Result<(), String>>>,
}

impl Manager {
    pub fn new(db_path: PathBuf, default_download_dir: PathBuf) -> Result<Arc<Self>> {
        let directory = paths::absolute_directory(&default_download_dir)?;
        let defaults = Settings::new(paths::path_string(&directory)?);
        let (store, settings, downloads) = Store::open(&db_path, defaults)?;
        // No automatic decompression: saved bytes and Content-Length must agree.
        let client = reqwest::Client::builder()
            .user_agent(concat!("Flow/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .retry(reqwest::retry::never())
            .build()
            .context("Could not initialize HTTP client")?;
        let (finished, _) = watch::channel(None);
        Ok(Arc::new(Self {
            state: Mutex::new(State {
                store,
                downloads,
                settings,
                started: false,
                closing: false,
            }),
            client,
            cancellation: CancellationToken::new(),
            wake: Arc::new(Notify::new()),
            finished,
        }))
    }

    /// Starts queue processing. Must be called within a Tokio runtime.
    pub fn start(self: &Arc<Self>) {
        let mut state = match self.state() {
            Ok(state) => state,
            Err(error) => {
                self.finished.send_replace(Some(Err(format!("{error:#}"))));
                return;
            }
        };
        if state.started || state.closing {
            return;
        }
        let runtime = tokio::runtime::Handle::current();
        state.started = true;
        let manager = Arc::downgrade(self);
        let cancellation = self.cancellation.clone();
        let wake = self.wake.clone();
        let finished = self.finished.clone();
        runtime.spawn(async move {
            let result = schedule(manager, cancellation, wake).await;
            finished.send_replace(Some(result.map_err(|error| format!("{error:#}"))));
        });
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        if let Some(Err(error)) = &*self.finished.borrow() {
            return Err(anyhow!("Download scheduler failed: {error}"));
        }
        let state = self.state()?;
        Ok(Snapshot {
            downloads: state.downloads.clone(),
            settings: state.settings.clone(),
        })
    }

    /// Validates and inserts the entire batch atomically. Work can be added before start.
    pub fn add(&self, requests: Vec<AddRequest>) -> Result<Vec<Download>> {
        let mut state = self.state()?;
        ensure!(
            !state.closing && !self.cancellation.is_cancelled(),
            "Download manager has shut down"
        );
        let mut queue_order = match state.downloads.iter().map(|d| d.queue_order).max() {
            Some(order) => order
                .checked_add(1)
                .context("Download queue order overflow")?,
            None => 0,
        };
        let created_at = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
        let mut downloads = Vec::with_capacity(requests.len());
        let mut reservations = state.downloads.clone();
        for request in requests {
            let url = paths::validate_url(&request.url)?;
            let desired = paths::file_name(&url, request.file_name)?;
            let destination = paths::absolute_directory(Path::new(
                request
                    .destination
                    .as_deref()
                    .unwrap_or(&state.settings.download_dir),
            ))?;
            // Also reserve distinct names within this not-yet-committed batch.
            let file_name = paths::available_name(&destination, &desired, &reservations, None)?;
            let download = Download {
                id: Uuid::new_v4().to_string(),
                url: url.into(),
                file_name,
                destination: paths::path_string(&destination)?,
                status: Status::Queued,
                downloaded_bytes: 0,
                total_bytes: None,
                speed_bps: 0,
                eta_seconds: None,
                created_at,
                queue_order,
                error: None,
                connections: 0,
            };
            reservations.push(download.clone());
            downloads.push(download);
            queue_order = queue_order
                .checked_add(1)
                .context("Download queue order overflow")?;
        }
        state.store.insert(&downloads)?;
        state.downloads.extend(downloads.iter().cloned());
        drop(state);
        self.wake.notify_one();
        Ok(downloads)
    }

    /// Stops active requests, records their interruption, and leaves queued work intact.
    /// A new manager can process that queued work after this instance is dropped.
    pub async fn shutdown(&self) -> Result<()> {
        let mut finished = self.finished.subscribe();
        {
            let mut state = self.state()?;
            state.closing = true;
            self.cancellation.cancel();
            if !state.started {
                self.finished.send_replace(Some(Ok(())));
            }
        }
        loop {
            let result = finished.borrow_and_update().clone();
            if let Some(result) = result {
                return result.map_err(|error| anyhow!(error));
            }
            finished
                .changed()
                .await
                .context("Download scheduler stopped unexpectedly")?;
        }
    }

    pub(crate) fn state(&self) -> Result<MutexGuard<'_, State>> {
        self.state
            .lock()
            .map_err(|_| anyhow!("Download manager state lock was poisoned"))
    }

    fn claim_next(&self, active: usize) -> Result<Option<Download>> {
        let mut state = self.state()?;
        if state.closing
            || self.cancellation.is_cancelled()
            || active >= state.settings.max_concurrent as usize
        {
            return Ok(None);
        }
        let Some(index) = state
            .downloads
            .iter()
            .position(|download| download.status == Status::Queued)
        else {
            return Ok(None);
        };
        let mut download = state.downloads[index].clone();
        download.status = Status::Downloading;
        download.connections = 1;
        download.error = None;
        state.store.update(&download)?;
        state.downloads[index] = download.clone();
        Ok(Some(download))
    }

    pub(crate) fn update(&self, id: &str, change: impl FnOnce(&mut Download)) -> Result<()> {
        let mut state = self.state()?;
        let index = state
            .downloads
            .iter()
            .position(|download| download.id == id)
            .context("Download no longer exists")?;
        let mut download = state.downloads[index].clone();
        change(&mut download);
        // Persist before exposing the new snapshot so a terminal state is durable.
        state.store.update(&download)?;
        state.downloads[index] = download;
        Ok(())
    }

    fn fail_unfinished(&self, error: &str) -> Result<()> {
        let mut state = self.state()?;
        state.closing = true;
        for index in 0..state.downloads.len() {
            if state.downloads[index].status == Status::Downloading {
                let mut download = state.downloads[index].clone();
                download.status = Status::Failed;
                download.error = Some(error.to_owned());
                download.speed_bps = 0;
                download.eta_seconds = None;
                download.connections = 0;
                state.store.update(&download)?;
                state.downloads[index] = download;
            }
        }
        Ok(())
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

async fn schedule(
    manager: Weak<Manager>,
    cancellation: CancellationToken,
    wake: Arc<Notify>,
) -> Result<()> {
    let mut workers = JoinSet::new();
    let result: Result<()> = async {
        loop {
            if cancellation.is_cancelled() {
                break;
            }
            if let Some(manager) = manager.upgrade() {
                while let Some(download) = manager.claim_next(workers.len())? {
                    let manager = manager.clone();
                    workers.spawn(async move { manager.run_transfer(download).await });
                }
            } else {
                break;
            }
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => break,
                worker = workers.join_next(), if !workers.is_empty() => {
                    if let Some(worker) = worker {
                        worker.context("Download worker panicked")??;
                    }
                }
                _ = wake.notified() => {}
            }
        }
        Ok(())
    }
    .await;

    cancellation.cancel();
    let mut result = result;
    // Let workers release files and persist failures; aborting would skip cleanup.
    while let Some(worker) = workers.join_next().await {
        let worker_result = worker
            .context("Download worker panicked")
            .and_then(|result| result);
        if result.is_ok() {
            result = worker_result;
        }
    }
    if let Some(manager) = manager.upgrade() {
        let message = match &result {
            Ok(()) => "Download interrupted by manager shutdown".to_owned(),
            Err(error) => format!("Download scheduler stopped: {error:#}"),
        };
        let cleanup = manager.fail_unfinished(&message);
        if result.is_ok() {
            result = cleanup;
        }
    }
    result
}
