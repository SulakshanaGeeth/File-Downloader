# Flow implementation contract

Flow is a Windows-first local download manager. Components share this contract.

## JSON (camelCase throughout)

Download: id:string, url:string, fileName:string, destination:string (directory), status:queued|downloading|paused|completed|failed|cancelled, downloadedBytes:number, totalBytes:number|null, speedBps:number, etaSeconds:number|null, createdAt:number (Unix milliseconds), queueOrder:number, error:string|null, connections:number.

Settings: downloadDir:string, maxConcurrent:number (default 3), connectionsPerDownload:number (default 4), speedLimitBps:number (0 unlimited), theme:system|light|dark, notifications:boolean.

Snapshot: downloads:Download[], settings:Settings.
AddRequest: url:string, fileName?:string|null, destination?:string|null.

## Tauri commands

Stage 1 implements `get_snapshot`, `add_downloads`, `open_download`, and `quit_app`. The remaining commands below are reserved for later milestones. The React UI polls snapshots every 500 ms after the previous request finishes and refreshes after adding downloads; the events below are also reserved. Native folder selection uses the dialog plugin. All DTOs retain this contract's camelCase names.

- get_snapshot() -> Snapshot
- add_downloads({requests:AddRequest[]}) -> Download[]
- action_download({id:string, action:pause|resume|cancel|retry|remove}) -> void
- reorder_download({id:string, direction:up|down}) -> void
- update_settings({settings:Settings}) -> Settings
- open_download({id:string, reveal:boolean}) -> void
- take_browser_links() -> string[] (drains pending handoffs)
- quit_app() -> void

Events: download-snapshot -> Snapshot; browser-links -> notification to drain take_browser_links.
Folder selection: @tauri-apps/plugin-dialog open({directory:true,multiple:false}).

`open_download` resolves a completed download by ID in Rust and opens/reveals its saved file; it does not accept arbitrary frontend paths. Missing files and shell failures return readable command errors. `quit_app` and native window close use the same confirmation and awaited shutdown flow. A pending close request prevents duplicate confirmations. The app uses a single instance, an application-local database, and the system Downloads directory by default.

## Rust core API (crates/download-core)

Package flow-core; exports Manager, Download, Settings, Snapshot, AddRequest, Action, Direction, Status.
Currently implemented manager operations are `new`, `start`, `snapshot`, `add`, and `shutdown`; action/reorder/settings-update methods below remain reserved.
Manager::new(db_path:PathBuf, default_download_dir:PathBuf) -> Result<Arc<Manager>>.
manager.start() (self: &Arc<Self>, requires Tokio runtime); snapshot() -> Result<Snapshot>; add(requests:Vec<AddRequest>) -> Result<Vec<Download>>; action(id:&str, action:Action) -> Result<()>; reorder(id:&str,direction:Direction) -> Result<()>; update_settings(settings:Settings) -> Result<Settings>; shutdown().await -> Result<()>.
Manager owns scheduling, persistence, state, transfer cancellation and graceful shutdown. No Tauri dependency. Implement blocking DB locking briefly; never hold locks across awaits.

## Browser bridge (crates/native-host)

Package/binary flowdm-bridge. Host name com.flow.download_manager.
App executable flowdm.exe in same installed directory as bridge.
Bridge validates caller extension origin and HTTP/HTTPS URL, then spawns flowdm.exe --browser-url <url> without shell. App single-instance handler queues handoffs and shows Add Download dialog (never automatic transfer).
Extension source TypeScript with Manifest V3, stable manifest key/ID, context menu and action popup. Host manifest generated at installation with absolute bridge path, allowed extension IDs. NSIS hook registers HKCU Chrome/Edge and cleans entries on uninstall. Root Cargo workspace includes src-tauri, crates/download-core, crates/native-host.

## Ownership

Frontend agent: src/, index.html, package.json, tsconfig*, vite.config.ts, frontend tests and public/.
Engine agent: crates/download-core/.
Browser agent: extension/, crates/native-host/, installer/, scripts/build-extension.mjs, scripts/register-browser.ps1, scripts/unregister-browser.ps1.
Root: src-tauri/, root Cargo.toml, README, other scripts, integration and validation.
