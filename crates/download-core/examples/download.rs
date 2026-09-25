use std::{env, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use flow_core::{AddRequest, Manager, Status};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let url = args
        .next()
        .context("Usage: download <URL> <destination-directory> [database-path]")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("The URL must be valid UTF-8"))?;
    let destination = PathBuf::from(
        args.next()
            .context("Usage: download <URL> <destination-directory> [database-path]")?,
    );
    std::fs::create_dir_all(&destination).context("Cannot create destination directory")?;
    let destination = destination
        .canonicalize()
        .context("Cannot resolve destination directory")?;
    let database = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| destination.join(".flow-history.sqlite3"));
    if args.next().is_some() {
        bail!("Usage: download <URL> <destination-directory> [database-path]");
    }

    let manager = Manager::new(database.clone(), destination.clone())?;
    manager.start();
    let outcome = download(&manager, url, &destination).await;
    let shutdown = manager.shutdown().await;
    drop(manager);

    let id = match (outcome, shutdown) {
        (Ok(id), Ok(())) => id,
        (Err(error), Ok(())) => return Err(error),
        (Ok(_), Err(error)) => {
            return Err(error.context("Could not shut down the download manager"));
        }
        (Err(error), Err(shutdown_error)) => {
            return Err(error.context(format!("Shutdown also failed: {shutdown_error:#}")));
        }
    };

    let reopened = Manager::new(database.clone(), destination)?;
    let snapshot = reopened.snapshot()?;
    let record = snapshot
        .downloads
        .iter()
        .find(|record| record.id == id)
        .context("Completed download was missing after reopening the history database")?;
    if !matches!(record.status, Status::Completed) {
        bail!("Download completion was not preserved in history");
    }
    println!(
        "Saved: {}",
        PathBuf::from(&record.destination)
            .join(&record.file_name)
            .display()
    );
    println!("History verified: {}", database.display());
    reopened.shutdown().await?;
    Ok(())
}

async fn download(
    manager: &Arc<Manager>,
    url: String,
    destination: &std::path::Path,
) -> Result<String> {
    let added = manager.add(vec![AddRequest {
        url,
        file_name: None,
        destination: Some(
            destination
                .to_str()
                .context("The destination path must be valid UTF-8")?
                .to_owned(),
        ),
    }])?;
    let id = added
        .first()
        .context("The manager did not return the added download")?
        .id
        .clone();
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    let interrupt = tokio::signal::ctrl_c();
    tokio::pin!(interrupt);

    loop {
        tokio::select! {
            result = &mut interrupt => {
                result.context("Cannot listen for Ctrl-C")?;
                bail!("Interrupted; stopping downloads and saving history");
            }
            _ = interval.tick() => {
                let snapshot = manager.snapshot()?;
                let record = snapshot.downloads.iter().find(|record| record.id == id)
                    .context("The download disappeared from the manager snapshot")?;
                let total = record.total_bytes.map_or_else(|| "unknown".to_owned(), |n| n.to_string());
                let eta = record.eta_seconds.map_or_else(|| "unknown".to_owned(), |n| format!("{n:.0}s"));
                println!(
                    "{:?}: {} / {} bytes | {:.0} B/s | ETA {}",
                    record.status, record.downloaded_bytes, total, record.speed_bps, eta
                );
                match record.status {
                    Status::Completed => return Ok(id),
                    Status::Failed => bail!("Download failed: {}", record.error.as_deref().unwrap_or("unknown error")),
                    Status::Cancelled => bail!("Download was cancelled"),
                    Status::Paused => bail!("Download is paused"),
                    Status::Queued | Status::Downloading => {}
                }
            }
        }
    }
}
