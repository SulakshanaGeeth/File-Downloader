# Flow

Flow is a Windows-first download manager with a React desktop interface, a Tauri 2 shell, and a standalone Rust download engine. The shared interfaces and deferred features are described in [IMPLEMENTATION.md](IMPLEMENTATION.md).

## Run the desktop app

Prerequisites: Node.js 22.12+ (24 LTS recommended), npm, stable MSVC Rust, Visual Studio Build Tools with **Desktop development with C++**, and Microsoft Edge WebView2 Runtime. See the [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/).

From the repository root:

```powershell
npm ci
npm run tauri -- dev
```

Click **Add download**, paste a direct HTTP/HTTPS URL, optionally choose a filename and destination, and start the download. The sidebar filters downloads; search matches filenames and source URLs. Select a row for details. Completed files can be opened or revealed in Explorer. The interface follows the system light/dark appearance.

History is stored in `flow.sqlite3` inside Tauri's application-local data directory (`%LOCALAPPDATA%\com.flow.download-manager` on Windows). New downloads default to the Windows Downloads folder; the Add dialog can override that folder per download. The command-line example below uses a separate database and does not share desktop history.

Closing the window quits the app after graceful shutdown. When downloads are running or queued, Flow asks before quitting: interrupted active downloads become failed and cannot resume yet; queued downloads are retained and start next time. A second launch focuses the existing window. There is no background tray mode.

Build a standalone Windows executable (installer packaging is deferred):

```powershell
npm run tauri -- build --no-bundle
.\target\release\flowdm.exe
```

`npm run dev` serves only the frontend. Downloading and folder/file actions require the Tauri desktop runtime; browser-only mode displays a connection error instead of simulated downloads.

If using the portable Node runtime prepared in this checkout, enable it for the current PowerShell session first:

```powershell
$env:Path = (Join-Path (Get-Location) '.tools\node-v24.21.0-win-x64') + ';' + $env:Path
```

## Desktop smoke test

Run `node scripts/smoke-server.mjs` in another terminal. Add the printed local URLs in Flow:

- `/sample.txt`: known-size transfer with visible progress; add twice to check numbered filenames. Open the completed file and show it in its folder.
- `/stream.txt`: unknown-size transfer; progress stays indeterminate until completion.
- `/missing.txt`: HTTP failure with a readable error in the details panel.
- `/slow.txt`: long transfer for closing/reopening tests. Add four copies to leave queued work behind the three active transfers; confirm quitting, then reopen and verify interrupted records and queued recovery.

Also verify invalid URLs and filenames remain in the Add dialog with errors, an unavailable destination reports a failure, and moving a completed file produces a readable Open File error. Check search, all sidebar filters, keyboard focus/escape in the dialog, and both system appearances. Stop the local server with Ctrl+C.

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

## Download behavior and current limits

- Downloads run in queue order, with three files active by default and one connection per active file. Persisted settings default to four connections per download, unlimited speed, the system theme, and notifications enabled; only the queue concurrency applies in this milestone.
- HTTP and HTTPS transfers support optional content lengths, progress, speed, ETA, and readable errors. HTTPS uses the platform's native TLS backend. Custom authentication headers and credentials embedded in URLs are not supported.
- Files are written to temporary files beside their destinations and published only after success. Existing files are preserved by selecting numbered names. Filenames that could escape the destination directory are rejected.
- Filenames come from an explicit `fileName` or the URL's final path segment, with `download.bin` as the fallback for an empty segment. `Content-Disposition` does not determine filenames. Supplied and URL-derived names are limited to 220 UTF-8 bytes.
- SQLite stores settings and download history. Queued work survives a restart; unfinished active records are marked failed with an interruption message when the database is reopened.
- Pause/resume, cancel/retry/remove actions, reordering, segmented transfers, speed limiting, and settings updates are deferred. Browser integration, scheduling, notifications, tray mode, and installer packaging are also deferred. The UI exposes only supported actions.

## Development checks

```powershell
npm run typecheck
npm test
npm run build
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
```

Core transfer tests use a local HTTP server, so they do not depend on a public download service.
