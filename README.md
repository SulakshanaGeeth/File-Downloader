# Flow

Flow is a Windows-first download manager. Part 1 provides a standalone Rust download core and a command-line example. The shared JSON and future application interfaces are described in [IMPLEMENTATION.md](IMPLEMENTATION.md).

## Run a download

Install the stable Rust toolchain and the platform's C/C++ build tools, which are needed to build bundled SQLite. On Windows, use the MSVC Rust toolchain and Visual Studio Build Tools with the **Desktop development with C++** workload.

From the repository root:

```powershell
cargo run -p flow-core --example download -- "https://example.com/file.zip" ".\downloads"
```

Replace the example URL with a direct HTTP or HTTPS file URL. The program prints status, bytes, speed, and ETA every half second, then reopens the history database to verify the completed record. An unknown content length or ETA is shown as `unknown`. Failures and Ctrl-C exit with a nonzero status after shutting down the manager.

History defaults to `<destination-directory>/.flow-history.sqlite3`. Supply a third argument to use another database:

```powershell
cargo run -p flow-core --example download -- "https://example.com/file.zip" ".\downloads" ".\flow.sqlite3"
```

The example adds a new download on each run. Starting a manager also schedules queued work already in its database.

## Use the core

`flow-core` has no Tauri dependency. Create it inside a Tokio runtime, start its scheduler, and poll snapshots for progress:

```rust,no_run
use std::{path::PathBuf, time::Duration};
use flow_core::{AddRequest, Manager, Status};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manager = Manager::new(
        PathBuf::from("flow.sqlite3"),
        PathBuf::from("downloads"),
    )?;
    manager.start();
    let outcome = async {
        let added = manager.add(vec![AddRequest {
            url: "https://example.com/file.zip".into(),
            file_name: None,
            destination: None,
        }])?;
        let id = &added[0].id;
        loop {
            let snapshot = manager.snapshot()?;
            let download = snapshot.downloads.iter().find(|item| &item.id == id).unwrap();
            match download.status {
                Status::Completed => break Ok::<(), anyhow::Error>(()),
                Status::Failed | Status::Cancelled | Status::Paused => {
                    anyhow::bail!("Download stopped: {:?}", download.error);
                }
                Status::Queued | Status::Downloading => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }.await;
    let shutdown = manager.shutdown().await;
    outcome?;
    shutdown?;
    Ok(())
}
```

The [download example](crates/download-core/examples/download.rs) also includes progress output and Ctrl-C handling. Always call `shutdown().await` before exiting, including after a polling error. A database permits one manager at a time; drop all references to a shut-down manager before reopening it. Snapshots and requests serialize with the contract's camelCase field names.

## Part 1 behavior

- Downloads run in queue order, with three files active by default and one connection per active file. Persisted settings default to four connections per download, unlimited speed, the system theme, and notifications enabled; only the queue concurrency applies in this milestone.
- HTTP and HTTPS transfers support optional content lengths, progress, speed, ETA, and readable errors. HTTPS uses the platform's native TLS backend. Custom authentication headers and credentials embedded in URLs are not supported.
- Files are written to temporary files beside their destinations and published only after success. Existing files are preserved by selecting numbered names. Filenames that could escape the destination directory are rejected.
- Filenames come from an explicit `fileName` or the URL's final path segment, with `download.bin` as the fallback for an empty segment. `Content-Disposition` does not determine filenames. Supplied and URL-derived names are limited to 220 UTF-8 bytes.
- SQLite stores settings and download history. Queued work survives a restart; unfinished active records are marked failed with an interruption message when the database is reopened.
- Pause/resume, action commands, retry, reordering, segmented transfers, speed limiting, and settings updates are deferred. The desktop UI, browser extension, and installer are also deferred.

## Development checks

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
```

Core transfer tests use a local HTTP server, so they do not depend on a public download service.
