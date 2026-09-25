use std::{
    io::ErrorKind,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use reqwest::{StatusCode, header};
use tempfile::TempPath;
use tokio::io::AsyncWriteExt;

use crate::{Download, Manager, Status, paths};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

impl Manager {
    pub(crate) async fn run_transfer(&self, download: Download) -> Result<()> {
        let result = self.transfer(&download).await;
        if let Err(error) = result {
            self.update(&download.id, |download| {
                download.status = Status::Failed;
                download.error = Some(format!("{error:#}"));
                download.speed_bps = 0;
                download.eta_seconds = None;
                download.connections = 0;
            })
            .context("Could not save failed download")?;
        }
        Ok(())
    }

    async fn transfer(&self, download: &Download) -> Result<()> {
        let request = self
            .client
            .get(&download.url)
            .header(header::ACCEPT_ENCODING, "identity")
            .send();
        let mut response = tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => bail!("Download interrupted by manager shutdown"),
            response = request => response.context("HTTP request failed")?,
        };
        ensure!(
            response.status().is_success(),
            "Server returned HTTP {}",
            response.status()
        );
        ensure!(
            response.status() != StatusCode::PARTIAL_CONTENT
                && !response.headers().contains_key(header::CONTENT_RANGE),
            "Server returned an unsolicited partial response; the full file was not downloaded"
        );
        let total = response.content_length();
        self.update(&download.id, |download| {
            download.total_bytes = total;
        })?;
        let directory = Path::new(&download.destination);
        tokio::fs::create_dir_all(directory)
            .await
            .context("Could not create destination directory")?;
        // Windows' no-clobber move requires extended-length absolute paths for
        // destinations beyond MAX_PATH; canonicalize also gives the temp path one.
        let temporary_directory = tokio::fs::canonicalize(directory)
            .await
            .context("Could not resolve destination directory")?;
        let temporary = tempfile::Builder::new()
            .prefix(&format!(".flow-{}-", download.id))
            .suffix(".part")
            .tempfile_in(&temporary_directory)
            .context("Could not create temporary download file")?;
        let (file, temporary_path) = temporary.into_parts();
        let mut file = tokio::fs::File::from_std(file);
        let started = Instant::now();
        let mut progress_tick = tokio::time::interval(PROGRESS_INTERVAL);
        progress_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut downloaded = 0_u64;

        let stream_result: Result<()> = async {
            loop {
                let chunk = tokio::select! {
                    biased;
                    _ = self.cancellation.cancelled() => bail!("Download interrupted by manager shutdown"),
                    _ = progress_tick.tick() => {
                        self.progress(&download.id, downloaded, total, started)?;
                        continue;
                    }
                    chunk = response.chunk() => chunk.context("Transfer disconnected or response body was invalid")?,
                };
                let Some(chunk) = chunk else { break; };
                file.write_all(&chunk).await.context("Could not write download data")?;
                downloaded = downloaded.checked_add(chunk.len() as u64).context("Download size overflow")?;
            }
            if let Some(total) = total {
                ensure!(downloaded == total, "Incomplete response: received {downloaded} of {total} bytes");
            }
            Ok(())
        }.await;

        // Await pending filesystem work even on cancellation before removing the temp path.
        let flush_result = file.flush().await.context("Could not flush download data");
        let sync_result = if stream_result.is_ok() && flush_result.is_ok() {
            file.sync_all()
                .await
                .context("Could not sync download data to disk")
        } else {
            Ok(())
        };
        drop(file);
        self.progress(&download.id, downloaded, total, started)?;
        stream_result?;
        flush_result?;
        sync_result?;
        ensure!(
            !self.cancellation.is_cancelled(),
            "Download interrupted by manager shutdown"
        );
        self.publish(download, temporary_path, downloaded)
    }

    fn progress(
        &self,
        id: &str,
        downloaded: u64,
        total: Option<u64>,
        started: Instant,
    ) -> Result<()> {
        let speed = (downloaded as f64 / started.elapsed().as_secs_f64().max(0.001)) as u64;
        self.update(id, |download| {
            download.downloaded_bytes = downloaded;
            download.speed_bps = speed;
            download.eta_seconds = total.and_then(|total| {
                (speed > 0).then(|| total.saturating_sub(downloaded).div_ceil(speed))
            });
        })
    }

    fn publish(
        &self,
        download: &Download,
        mut temporary_path: TempPath,
        downloaded: u64,
    ) -> Result<()> {
        let mut state = self.state()?;
        ensure!(
            !self.cancellation.is_cancelled(),
            "Download interrupted by manager shutdown"
        );
        let index = state
            .downloads
            .iter()
            .position(|item| item.id == download.id)
            .context("Download no longer exists")?;
        let directory = Path::new(&download.destination);
        let publication_directory =
            std::fs::canonicalize(directory).context("Could not resolve publication directory")?;
        for _ in 0..100 {
            let name = paths::available_name(
                directory,
                &download.file_name,
                &state.downloads,
                Some(&download.id),
            )?;
            let mut completed = state.downloads[index].clone();
            completed.file_name = name;
            // Save the chosen name before publication, for recovery after an interruption.
            state.store.update(&completed)?;
            state.downloads[index] = completed.clone();
            match temporary_path.persist_noclobber(publication_directory.join(&completed.file_name))
            {
                Ok(()) => {
                    completed.status = Status::Completed;
                    completed.downloaded_bytes = downloaded;
                    completed.speed_bps = 0;
                    completed.eta_seconds = None;
                    completed.connections = 0;
                    completed.error = None;
                    state
                        .store
                        .update(&completed)
                        .context("File saved, but completion could not be persisted")?;
                    state.downloads[index] = completed;
                    return Ok(());
                }
                Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
                    // Another program may create the destination after our name check.
                    temporary_path = error.path;
                }
                Err(error) => {
                    return Err(error.error).context("Could not publish completed download");
                }
            }
        }
        bail!("Destination filenames kept changing; could not publish download safely")
    }
}
