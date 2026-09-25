use std::{
    fs::{File, OpenOptions},
    path::Path,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};

use crate::{Download, Settings, Status};

pub(crate) struct Store {
    connection: Connection,
    // An OS lock prevents two managers from independently scheduling the same queue.
    // The lock file is deliberately retained: unlinking it would permit lock races.
    _ownership: File,
}

impl Store {
    pub(crate) fn open(path: &Path, defaults: Settings) -> Result<(Self, Settings, Vec<Download>)> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).context("Could not create database directory")?;
        }
        let mut lock_name = path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let ownership = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_name)
            .context("Could not open database ownership lock")?;
        ownership
            .try_lock()
            .context("Download database is already open by another manager")?;
        let mut connection = Connection::open(path).context("Could not open download database")?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;")?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        ensure!(
            version <= 1,
            "Download database schema is newer than this version of Flow"
        );
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS downloads (id TEXT PRIMARY KEY NOT NULL, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
             PRAGMA user_version = 1;"
        )?;
        transaction.execute(
            "INSERT OR IGNORE INTO settings (id, data) VALUES (1, ?1)",
            [serde_json::to_string(&defaults)?],
        )?;
        let settings: Settings = serde_json::from_str(&transaction.query_row(
            "SELECT data FROM settings WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )?)
        .context("Invalid persisted settings")?;
        ensure!(
            settings.max_concurrent > 0,
            "Persisted maxConcurrent must be greater than zero"
        );
        let mut downloads = {
            let mut statement = transaction.prepare("SELECT id, data FROM downloads")?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut downloads = Vec::new();
            for row in rows {
                let (id, json) = row?;
                let download: Download =
                    serde_json::from_str(&json).context("Invalid persisted download")?;
                ensure!(
                    id == download.id,
                    "Download database contains a mismatched ID"
                );
                crate::paths::validate_file_name(&download.file_name)?;
                crate::paths::validate_url(&download.url)?;
                ensure!(
                    Path::new(&download.destination).is_absolute(),
                    "Persisted destination must be absolute"
                );
                downloads.push(download);
            }
            downloads
        };
        for download in &mut downloads {
            if download.status == Status::Downloading {
                download.status = Status::Failed;
                download.error =
                    Some("Download interrupted before the previous session finished".to_owned());
            }
            download.speed_bps = 0;
            download.eta_seconds = None;
            download.connections = 0;
            transaction.execute(
                "UPDATE downloads SET data = ?1 WHERE id = ?2",
                params![serde_json::to_string(download)?, download.id],
            )?;
        }
        downloads.sort_by_key(|download| {
            (
                download.queue_order,
                download.created_at,
                download.id.clone(),
            )
        });
        transaction.commit()?;
        Ok((
            Self {
                connection,
                _ownership: ownership,
            },
            settings,
            downloads,
        ))
    }

    pub(crate) fn insert(&mut self, downloads: &[Download]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        for download in downloads {
            transaction.execute(
                "INSERT INTO downloads (id, data) VALUES (?1, ?2)",
                params![download.id, serde_json::to_string(download)?],
            )?;
        }
        transaction
            .commit()
            .context("Could not persist new downloads")
    }

    pub(crate) fn update(&self, download: &Download) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE downloads SET data = ?1 WHERE id = ?2",
            params![serde_json::to_string(download)?, download.id],
        )?;
        ensure!(changed == 1, "Persisted download record is missing");
        Ok(())
    }
}
