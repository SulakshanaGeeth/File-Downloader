use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use flow_core::{AddRequest, Download, Manager as DownloadManager, Snapshot, Status};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

type CommandResult<T> = Result<T, String>;

/// One state and manager per process. The gate makes enqueue and beginning a
/// quit atomic, including while a native confirmation dialog is visible.
struct DesktopState {
    manager: CommandResult<Arc<DownloadManager>>,
    mutation_gate: Mutex<()>,
    quit_requested: AtomicBool,
    exit_ready: AtomicBool,
}

impl DesktopState {
    fn new(manager: CommandResult<Arc<DownloadManager>>) -> Self {
        Self {
            manager,
            mutation_gate: Mutex::new(()),
            quit_requested: AtomicBool::new(false),
            exit_ready: AtomicBool::new(false),
        }
    }

    fn manager(&self) -> CommandResult<&Arc<DownloadManager>> {
        self.manager.as_ref().map_err(Clone::clone)
    }

    fn snapshot(&self) -> CommandResult<Snapshot> {
        self.manager()?.snapshot().map_err(display_error)
    }

    fn add(&self, requests: Vec<AddRequest>) -> CommandResult<Vec<Download>> {
        let _gate = self
            .mutation_gate
            .lock()
            .map_err(|_| "The application state is unavailable. Please restart Flow.".to_owned())?;
        if self.quit_requested.load(Ordering::Acquire) {
            return Err("Flow is closing. Cancel closing before adding another download.".into());
        }
        self.manager()?.add(requests).map_err(display_error)
    }

    fn completed_path(&self, id: &str) -> CommandResult<PathBuf> {
        let snapshot = self.snapshot()?;
        let download = snapshot
            .downloads
            .iter()
            .find(|download| download.id == id)
            .ok_or_else(|| "This download no longer exists.".to_owned())?;
        if download.status != Status::Completed {
            return Err("This file is not ready. Wait for the download to complete.".into());
        }
        let path = Path::new(&download.destination).join(&download.file_name);
        match path.metadata() {
            Ok(metadata) if metadata.is_file() => Ok(path),
            Ok(_) => Err(
                "The saved path is no longer a file. It may have been moved or replaced.".into(),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(
                "The downloaded file could not be found. It may have been moved or deleted.".into(),
            ),
            Err(error) => Err(format!(
                "The downloaded file could not be accessed: {error}"
            )),
        }
    }

    fn begin_quit(&self) -> CommandResult<bool> {
        let _gate = self
            .mutation_gate
            .lock()
            .map_err(|_| "The application state is unavailable. Please restart Flow.".to_owned())?;
        Ok(!self.quit_requested.swap(true, Ordering::AcqRel))
    }

    fn cancel_quit(&self) {
        self.quit_requested.store(false, Ordering::Release);
    }
}

fn display_error(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}

#[tauri::command]
async fn get_snapshot(state: State<'_, DesktopState>) -> CommandResult<Snapshot> {
    state.snapshot()
}

#[tauri::command]
async fn add_downloads(
    state: State<'_, DesktopState>,
    requests: Vec<AddRequest>,
) -> CommandResult<Vec<Download>> {
    state.add(requests)
}

#[tauri::command]
async fn open_download(
    app: AppHandle,
    state: State<'_, DesktopState>,
    id: String,
    reveal: bool,
) -> CommandResult<()> {
    let path = state.completed_path(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        if reveal {
            app.opener().reveal_item_in_dir(&path)
        } else {
            app.opener()
                .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        }
        .map_err(|error| format!("Could not open the downloaded file: {error}"))
    })
    .await
    .map_err(display_error)?
}

#[tauri::command]
async fn quit_app(app: AppHandle) -> CommandResult<()> {
    request_quit(app).await
}

async fn confirm(app: AppHandle, message: String, title: &str, accept: &str) -> bool {
    let title = title.to_owned();
    let accept = accept.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app
            .dialog()
            .message(message)
            .title(title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                accept,
                "Keep Flow open".into(),
            ));
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.parent(&window);
        }
        dialog.blocking_show()
    })
    .await
    .unwrap_or(false)
}

async fn request_quit(app: AppHandle) -> CommandResult<()> {
    let state = app.state::<DesktopState>();
    if !state.begin_quit()? {
        return Ok(());
    }
    // Queued work can start while the dialog is open, so it also requires consent.
    // If the scheduler cannot provide a snapshot, conservatively ask as well.
    let has_work = state.manager.is_ok()
        && state.snapshot().map_or(true, |snapshot| {
            snapshot
                .downloads
                .iter()
                .any(|download| matches!(download.status, Status::Queued | Status::Downloading))
        });
    if has_work
        && !confirm(
            app.clone(),
            "Downloads are still active. Closing Flow will interrupt running downloads, and they cannot resume in this stage. Downloads still queued when Flow closes will be kept for the next launch.".into(),
            "Close Flow?",
            "Close Flow",
        )
        .await
    {
        state.cancel_quit();
        return Ok(());
    }
    if let Ok(manager) = state.manager() {
        if let Err(error) = manager.shutdown().await {
            let message = format!(
                "Flow could not finish saving the download state: {error:#}\n\nThe download engine has stopped. Quit anyway, or keep the window open to inspect the error?"
            );
            if !confirm(
                app.clone(),
                message,
                "Could not finish closing",
                "Quit anyway",
            )
            .await
            {
                state.cancel_quit();
                return Err(format!(
                    "Could not shut down the download engine: {error:#}"
                ));
            }
        }
    }
    state.exit_ready.store(true, Ordering::Release);
    app.exit(0);
    Ok(())
}

fn request_quit_from_window(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = request_quit(app.clone()).await {
            app.dialog()
                .message(error)
                .title("Flow could not close")
                .kind(MessageDialogKind::Error)
                .show(|_| {});
        }
    });
}

pub fn run() {
    let app = tauri::Builder::default()
        // Register first, before creating the manager or touching its database.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let manager = (|| -> CommandResult<_> {
                let database = app
                    .path()
                    .app_local_data_dir()
                    .map_err(display_error)?
                    .join("flow.sqlite3");
                let downloads = app.path().download_dir().map_err(display_error)?;
                // Both client initialization and scheduler start run inside Tokio.
                tauri::async_runtime::block_on(async {
                    let manager =
                        DownloadManager::new(database, downloads).map_err(display_error)?;
                    manager.start();
                    Ok(manager)
                })
            })()
            .map_err(|error| format!("Could not start Flow's download engine: {error}"));
            // Keeping initialization errors in state lets the frontend render a
            // readable failure and still gives the user a normal close action.
            app.manage(DesktopState::new(manager));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            add_downloads,
            open_download,
            quit_app
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<DesktopState>();
                if !state.exit_ready.load(Ordering::Acquire) {
                    api.prevent_close();
                    request_quit_from_window(window.app_handle());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("Could not start the Flow desktop application");

    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if !app
                .state::<DesktopState>()
                .exit_ready
                .load(Ordering::Acquire)
            {
                api.prevent_exit();
                request_quit_from_window(app);
            }
        }
    });
}

#[cfg(test)]
mod tests;
