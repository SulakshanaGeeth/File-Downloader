use flow_core::{AddRequest, Download, Manager, Status};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::{JoinHandle, JoinSet},
    time::{sleep, timeout},
};

#[derive(Clone)]
enum Response {
    Body(Vec<u8>),
    Chunked(Vec<u8>),
    Missing,
    Disconnect,
    Held(Arc<Semaphore>, Vec<u8>),
    PartialBody(Arc<Semaphore>),
}

struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    active: Arc<AtomicUsize>,
    peak_active: Arc<AtomicUsize>,
    listener: JoinHandle<()>,
}

impl Server {
    async fn new(routes: Vec<(&str, Response)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: Arc<HashMap<String, Response>> = Arc::new(
            routes
                .into_iter()
                .map(|(path, response)| (path.to_owned(), response))
                .collect(),
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let peak_active = Arc::new(AtomicUsize::new(0));
        let task_requests = requests.clone();
        let task_active = active.clone();
        let task_peak = peak_active.clone();
        let listener = tokio::spawn(async move {
            // Dropping this set when the listener is aborted also cancels held requests.
            let mut handlers = JoinSet::new();
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                handlers.spawn(serve(
                    stream,
                    routes.clone(),
                    task_requests.clone(),
                    task_active.clone(),
                    task_peak.clone(),
                ));
            }
        });
        Self {
            base,
            requests,
            active,
            peak_active,
            listener,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn wait_for_requests(&self, count: usize) {
        timeout(Duration::from_secs(5), async {
            loop {
                if self.requests.lock().unwrap().len() >= count {
                    return;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("local HTTP requests did not arrive in time");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

async fn serve(
    mut stream: TcpStream,
    routes: Arc<HashMap<String, Response>>,
    requests: Arc<Mutex<Vec<String>>>,
    active: Arc<AtomicUsize>,
    peak_active: Arc<AtomicUsize>,
) {
    let mut request = Vec::new();
    loop {
        let mut buffer = [0_u8; 1024];
        let Ok(count) = stream.read(&mut buffer).await else {
            return;
        };
        if count == 0 || request.len() > 16_384 {
            return;
        }
        request.extend_from_slice(&buffer[..count]);
        if request.windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&request);
    let path = text.split_whitespace().nth(1).unwrap().to_owned();
    let response = routes.get(&path).cloned().unwrap_or(Response::Missing);
    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
    peak_active.fetch_max(current, Ordering::SeqCst);
    requests.lock().unwrap().push(path);

    match response {
        Response::Body(body) => {
            send_body(&mut stream, &body).await;
        }
        Response::Chunked(body) => {
            let _ = stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await;
            for chunk in body.chunks(127) {
                let _ = stream
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await;
                let _ = stream.write_all(chunk).await;
                let _ = stream.write_all(b"\r\n").await;
            }
            let _ = stream.write_all(b"0\r\n\r\n").await;
        }
        Response::Missing => {
            let _ = stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
        }
        Response::Disconnect => {
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\nConnection: close\r\n\r\nincomplete")
                .await;
        }
        Response::Held(gate, body) => {
            let permit = gate.acquire().await.unwrap();
            permit.forget();
            send_body(&mut stream, &body).await;
        }
        Response::PartialBody(gate) => {
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 65536\r\nConnection: close\r\n\r\n")
                .await;
            let _ = stream.write_all(&vec![17; 32_768]).await;
            let permit = gate.acquire().await.unwrap();
            permit.forget();
            let _ = stream.write_all(&vec![17; 32_768]).await;
        }
    }
    let _ = stream.shutdown().await;
    active.fetch_sub(1, Ordering::SeqCst);
}

async fn send_body(stream: &mut TcpStream, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(body).await;
}

struct Fixture {
    _temp: TempDir,
    db: PathBuf,
    destination: PathBuf,
    manager: Arc<Manager>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("history.sqlite3");
        let destination = temp.path().join("downloads");
        std::fs::create_dir(&destination).unwrap();
        let manager = Manager::new(db.clone(), destination.clone()).unwrap();
        Self {
            _temp: temp,
            db,
            destination,
            manager,
        }
    }
}

fn request(url: String, file_name: Option<&str>) -> AddRequest {
    AddRequest {
        url,
        file_name: file_name.map(str::to_owned),
        destination: None,
    }
}

async fn wait_for(manager: &Manager, predicate: impl Fn(&[Download]) -> bool) -> Vec<Download> {
    timeout(Duration::from_secs(5), async {
        loop {
            let downloads = manager.snapshot().unwrap().downloads;
            if predicate(&downloads) {
                return downloads;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "downloads did not reach expected state: {}",
            serde_json::to_string(&manager.snapshot().unwrap()).unwrap()
        )
    })
}

fn terminal(download: &Download) -> bool {
    matches!(download.status, Status::Completed | Status::Failed)
}

fn read_download(destination: &Path, download: &Download) -> Vec<u8> {
    std::fs::read(destination.join(&download.file_name)).unwrap()
}

#[tokio::test]
async fn downloads_known_and_unknown_length_and_persists_history() {
    let body: Vec<u8> = (0..32_768).map(|index| (index % 251) as u8).collect();
    let server = Server::new(vec![
        ("/known.bin", Response::Body(body.clone())),
        ("/unknown.bin", Response::Chunked(body.clone())),
    ])
    .await;
    let fixture = Fixture::new();
    fixture
        .manager
        .add(vec![
            request(server.url("/known.bin"), None),
            request(server.url("/unknown.bin"), None),
        ])
        .unwrap();
    fixture.manager.start();
    let downloads = wait_for(&fixture.manager, |items| items.iter().all(terminal)).await;
    assert_eq!(downloads.len(), 2);
    for download in &downloads {
        assert_eq!(download.status, Status::Completed);
        assert_eq!(download.downloaded_bytes, body.len() as u64);
        assert_eq!(read_download(&fixture.destination, download), body);
        assert!(download.error.is_none());
    }
    let known = downloads
        .iter()
        .find(|item| item.file_name == "known.bin")
        .unwrap();
    assert_eq!(known.total_bytes, Some(body.len() as u64));
    let unknown = downloads
        .iter()
        .find(|item| item.file_name == "unknown.bin")
        .unwrap();
    assert_eq!(unknown.total_bytes, None);
    fixture.manager.shutdown().await.unwrap();
    drop(fixture.manager);

    let reopened = Manager::new(fixture.db.clone(), fixture.destination.clone()).unwrap();
    let persisted = reopened.snapshot().unwrap();
    assert_eq!(persisted.downloads.len(), downloads.len());
    for original in downloads {
        let restored = persisted
            .downloads
            .iter()
            .find(|item| item.id == original.id)
            .unwrap();
        assert_eq!(restored.status, Status::Completed);
        assert_eq!(restored.downloaded_bytes, original.downloaded_bytes);
        assert_eq!(restored.file_name, original.file_name);
    }
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn http_errors_and_disconnections_never_publish_incomplete_files() {
    let server = Server::new(vec![
        ("/missing.bin", Response::Missing),
        ("/broken.bin", Response::Disconnect),
    ])
    .await;
    let fixture = Fixture::new();
    fixture.manager.start();
    fixture
        .manager
        .add(vec![
            request(server.url("/missing.bin"), None),
            request(server.url("/broken.bin"), None),
        ])
        .unwrap();
    let downloads = wait_for(&fixture.manager, |items| items.iter().all(terminal)).await;
    for download in downloads {
        assert_eq!(download.status, Status::Failed);
        assert!(!download.error.as_deref().unwrap_or_default().is_empty());
        assert!(!fixture.destination.join(download.file_name).exists());
    }
    fixture.manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn collisions_preserve_existing_files_and_each_successful_download() {
    let server = Server::new(vec![("/file.bin", Response::Body(b"new content".to_vec()))]).await;
    let fixture = Fixture::new();
    std::fs::write(fixture.destination.join("file.bin"), b"original content").unwrap();
    fixture
        .manager
        .add(vec![
            request(server.url("/file.bin"), None),
            request(server.url("/file.bin"), None),
        ])
        .unwrap();
    fixture.manager.start();
    let downloads = wait_for(&fixture.manager, |items| items.iter().all(terminal)).await;
    assert_eq!(
        std::fs::read(fixture.destination.join("file.bin")).unwrap(),
        b"original content"
    );
    assert_ne!(downloads[0].file_name, downloads[1].file_name);
    for download in downloads {
        assert_eq!(download.status, Status::Completed);
        assert_ne!(download.file_name, "file.bin");
        assert_eq!(
            read_download(&fixture.destination, &download),
            b"new content"
        );
    }
    fixture.manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn maximum_length_names_remain_valid_after_collision_numbering_and_reopen() {
    let server = Server::new(vec![(
        "/data",
        Response::Body(b"numbered content".to_vec()),
    )])
    .await;
    let fixture = Fixture::new();
    let file_name = format!("{}.bin", "x".repeat(216));
    assert_eq!(file_name.len(), 220);
    std::fs::write(fixture.destination.join(&file_name), b"existing content").unwrap();
    let added = fixture
        .manager
        .add(vec![
            request(server.url("/data"), Some(&file_name)),
            request(server.url("/data"), Some(&file_name)),
        ])
        .unwrap();
    assert_ne!(added[0].file_name, added[1].file_name);
    assert!(added.iter().all(|item| item.file_name != file_name));
    fixture.manager.shutdown().await.unwrap();
    drop(fixture.manager);

    let reopened = Manager::new(fixture.db.clone(), fixture.destination.clone()).unwrap();
    assert_eq!(reopened.snapshot().unwrap().downloads.len(), 2);
    reopened.start();
    let completed = wait_for(&reopened, |items| items.iter().all(terminal)).await;
    for item in completed {
        assert_eq!(item.status, Status::Completed, "{:?}", item.error);
        assert_eq!(
            read_download(&fixture.destination, &item),
            b"numbered content"
        );
    }
    assert_eq!(
        std::fs::read(fixture.destination.join(&file_name)).unwrap(),
        b"existing content"
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_unsafe_names_and_unsupported_urls_before_queueing() {
    let fixture = Fixture::new();
    for name in [
        "../escape.bin",
        "..\\escape.bin",
        "sub/file.bin",
        "sub\\file.bin",
        "C:\\escape.bin",
        "file:stream",
        "CON",
        "nul.txt",
        "AUX.bin",
        "COM1.txt",
        "LPT9.log",
        "trailing.",
        "trailing ",
        "",
        ".",
        "..",
    ] {
        assert!(
            fixture
                .manager
                .add(vec![request(
                    "http://127.0.0.1/file.bin".into(),
                    Some(name)
                )])
                .is_err(),
            "unsafe filename was accepted: {name:?}"
        );
    }
    for url in [
        "file:///tmp/file.bin",
        "ftp://127.0.0.1/file.bin",
        "not a URL",
        "http://127.0.0.1/%2e%2e%2fescape.bin",
        "http://127.0.0.1/%5cescape.bin",
    ] {
        assert!(
            fixture
                .manager
                .add(vec![request(url.into(), None)])
                .is_err(),
            "unsafe URL or filename was accepted: {url}"
        );
    }
    assert!(fixture.manager.snapshot().unwrap().downloads.is_empty());
    fixture.manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn decodes_url_names_and_honors_destination_override() {
    let server = Server::new(vec![(
        "/hello%20world.bin",
        Response::Body(b"named content".to_vec()),
    )])
    .await;
    let fixture = Fixture::new();
    let other = fixture._temp.path().join("other downloads");
    std::fs::create_dir(&other).unwrap();
    let mut add = request(server.url("/hello%20world.bin"), None);
    add.destination = Some(other.to_string_lossy().into_owned());
    fixture.manager.add(vec![add]).unwrap();
    fixture.manager.start();
    let downloads = wait_for(&fixture.manager, |items| items.iter().all(terminal)).await;
    assert_eq!(downloads[0].status, Status::Completed);
    assert_eq!(downloads[0].file_name, "hello world.bin");
    assert_eq!(read_download(&other, &downloads[0]), b"named content");
    assert!(!fixture.destination.join("hello world.bin").exists());
    fixture.manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn queue_respects_default_concurrency_order_and_idempotent_start() {
    let gates: Vec<_> = (0..5).map(|_| Arc::new(Semaphore::new(0))).collect();
    let server = Server::new(vec![
        ("/1", Response::Held(gates[0].clone(), vec![1])),
        ("/2", Response::Held(gates[1].clone(), vec![2])),
        ("/3", Response::Held(gates[2].clone(), vec![3])),
        ("/4", Response::Held(gates[3].clone(), vec![4])),
        ("/5", Response::Held(gates[4].clone(), vec![5])),
    ])
    .await;
    let fixture = Fixture::new();
    let queued = fixture
        .manager
        .add(
            (1..=5)
                .map(|index| request(server.url(&format!("/{index}")), None))
                .collect(),
        )
        .unwrap();
    assert!(queued.iter().all(|item| item.status == Status::Queued));
    assert!(
        queued
            .windows(2)
            .all(|pair| pair[0].queue_order < pair[1].queue_order)
    );
    assert!(server.requests.lock().unwrap().is_empty());
    fixture.manager.start();
    fixture.manager.start();
    server.wait_for_requests(3).await;
    sleep(Duration::from_millis(75)).await;
    let mut initial = server.requests.lock().unwrap().clone();
    initial.sort();
    assert_eq!(initial, ["/1", "/2", "/3"]);
    assert_eq!(server.active.load(Ordering::SeqCst), 3);
    let snapshot = fixture.manager.snapshot().unwrap();
    assert_eq!(
        snapshot
            .downloads
            .iter()
            .filter(|item| item.status == Status::Downloading)
            .count(),
        3
    );
    for download in snapshot.downloads {
        if download.status == Status::Downloading {
            assert_eq!(download.connections, 1);
        }
    }

    gates[0].add_permits(1);
    server.wait_for_requests(4).await;
    assert_eq!(server.requests.lock().unwrap()[3], "/4");
    gates[1].add_permits(1);
    server.wait_for_requests(5).await;
    assert_eq!(server.requests.lock().unwrap()[4], "/5");
    for gate in &gates[2..] {
        gate.add_permits(1);
    }
    let downloads = wait_for(&fixture.manager, |items| items.iter().all(terminal)).await;
    assert!(
        downloads
            .iter()
            .all(|item| item.status == Status::Completed)
    );
    assert_eq!(server.requests.lock().unwrap().len(), 5);
    assert!(server.peak_active.load(Ordering::SeqCst) <= 3);
    fixture.manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_interrupts_active_work_and_keeps_queued_downloads_for_reopen() {
    let gate = Arc::new(Semaphore::new(0));
    let server = Server::new(vec![
        ("/1", Response::Held(gate.clone(), vec![1])),
        ("/2", Response::Held(gate.clone(), vec![2])),
        ("/3", Response::Held(gate.clone(), vec![3])),
        ("/4", Response::Body(vec![4])),
    ])
    .await;
    let fixture = Fixture::new();
    fixture
        .manager
        .add(
            (1..=4)
                .map(|index| request(server.url(&format!("/{index}")), None))
                .collect(),
        )
        .unwrap();
    fixture.manager.start();
    server.wait_for_requests(3).await;
    timeout(Duration::from_secs(3), fixture.manager.shutdown())
        .await
        .expect("shutdown must cancel blocked transfers promptly")
        .unwrap();
    let stopped = fixture.manager.snapshot().unwrap().downloads;
    assert_eq!(
        stopped
            .iter()
            .filter(|item| item.status == Status::Failed)
            .count(),
        3
    );
    assert_eq!(
        stopped
            .iter()
            .filter(|item| item.status == Status::Queued)
            .count(),
        1
    );
    for download in &stopped {
        assert!(!fixture.destination.join(&download.file_name).exists());
        if download.status == Status::Failed {
            assert!(!download.error.as_deref().unwrap_or_default().is_empty());
        }
    }
    assert_eq!(server.requests.lock().unwrap().len(), 3);
    // A second shutdown must also return without waiting on an already stopped scheduler.
    timeout(Duration::from_secs(1), fixture.manager.shutdown())
        .await
        .unwrap()
        .unwrap();
    drop(fixture.manager);

    let reopened = Manager::new(fixture.db.clone(), fixture.destination.clone()).unwrap();
    let restored = reopened.snapshot().unwrap().downloads;
    assert_eq!(
        restored
            .iter()
            .filter(|item| item.status == Status::Failed)
            .count(),
        3
    );
    assert_eq!(
        restored
            .iter()
            .filter(|item| item.status == Status::Queued)
            .count(),
        1
    );
    reopened.start();
    let completed = wait_for(&reopened, |items| items.iter().all(terminal)).await;
    let last = completed.iter().find(|item| item.file_name == "4").unwrap();
    assert_eq!(last.status, Status::Completed);
    assert_eq!(read_download(&fixture.destination, last), vec![4]);
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn snapshot_serialization_matches_shared_contract_and_defaults() {
    let fixture = Fixture::new();
    let add: AddRequest = serde_json::from_value(serde_json::json!({
        "url": "https://example.test/file.bin"
    }))
    .unwrap();
    fixture.manager.add(vec![add]).unwrap();
    let snapshot = serde_json::to_value(fixture.manager.snapshot().unwrap()).unwrap();
    let settings = &snapshot["settings"];
    assert_eq!(settings["maxConcurrent"], 3);
    assert_eq!(settings["connectionsPerDownload"], 4);
    assert_eq!(settings["speedLimitBps"], 0);
    assert_eq!(settings["theme"], "system");
    assert_eq!(settings["notifications"], true);
    assert_eq!(
        settings["downloadDir"],
        fixture.destination.to_string_lossy().as_ref()
    );
    let mut setting_keys: Vec<_> = settings
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    setting_keys.sort_unstable();
    assert_eq!(
        setting_keys,
        [
            "connectionsPerDownload",
            "downloadDir",
            "maxConcurrent",
            "notifications",
            "speedLimitBps",
            "theme"
        ]
    );
    let download = &snapshot["downloads"][0];
    assert_eq!(download["status"], "queued");
    assert_eq!(download["fileName"], "file.bin");
    assert!(download["totalBytes"].is_null());
    assert!(download["etaSeconds"].is_null());
    assert!(download["error"].is_null());
    assert!(download["createdAt"].as_u64().unwrap() > 1_600_000_000_000);
    let mut download_keys: Vec<_> = download
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    download_keys.sort_unstable();
    assert_eq!(
        download_keys,
        [
            "connections",
            "createdAt",
            "destination",
            "downloadedBytes",
            "error",
            "etaSeconds",
            "fileName",
            "id",
            "queueOrder",
            "speedBps",
            "status",
            "totalBytes",
            "url"
        ]
    );
    fixture.manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn progress_is_visible_before_the_final_file_is_published() {
    let gate = Arc::new(Semaphore::new(0));
    let server = Server::new(vec![("/progress.bin", Response::PartialBody(gate.clone()))]).await;
    let fixture = Fixture::new();
    fixture
        .manager
        .add(vec![request(server.url("/progress.bin"), None)])
        .unwrap();
    fixture.manager.start();
    let progress = wait_for(&fixture.manager, |items| {
        items[0].status == Status::Downloading && items[0].downloaded_bytes == 32_768
    })
    .await;
    assert_eq!(progress[0].downloaded_bytes, 32_768);
    assert_eq!(progress[0].total_bytes, Some(65_536));
    assert_eq!(progress[0].connections, 1);
    assert!(progress[0].speed_bps > 0);
    assert!(progress[0].eta_seconds.is_some());
    assert!(!fixture.destination.join("progress.bin").exists());
    // An unrelated application can create the target while the transfer is running.
    std::fs::write(fixture.destination.join("progress.bin"), b"external file").unwrap();
    gate.add_permits(1);
    let completed = wait_for(&fixture.manager, |items| items.iter().all(terminal)).await;
    assert_eq!(completed[0].status, Status::Completed);
    assert_ne!(completed[0].file_name, "progress.bin");
    assert_eq!(
        std::fs::read(fixture.destination.join("progress.bin")).unwrap(),
        b"external file"
    );
    assert_eq!(
        read_download(&fixture.destination, &completed[0]),
        vec![17; 65_536]
    );
    fixture.manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_during_body_transfer_does_not_publish_partial_output() {
    let gate = Arc::new(Semaphore::new(0));
    let server = Server::new(vec![("/unfinished.bin", Response::PartialBody(gate))]).await;
    let fixture = Fixture::new();
    fixture
        .manager
        .add(vec![request(server.url("/unfinished.bin"), None)])
        .unwrap();
    fixture.manager.start();
    wait_for(&fixture.manager, |items| {
        items[0].downloaded_bytes == 32_768
    })
    .await;
    timeout(Duration::from_secs(3), fixture.manager.shutdown())
        .await
        .unwrap()
        .unwrap();
    let interrupted = fixture.manager.snapshot().unwrap().downloads.remove(0);
    assert_eq!(interrupted.status, Status::Failed);
    assert_eq!(interrupted.downloaded_bytes, 32_768);
    assert_eq!(interrupted.connections, 0);
    assert_eq!(interrupted.speed_bps, 0);
    assert!(interrupted.eta_seconds.is_none());
    assert!(!fixture.destination.join("unfinished.bin").exists());
}

#[tokio::test]
async fn recovery_marks_crashed_transfers_failed_and_preserves_queue_and_settings() {
    let fixture = Fixture::new();
    let added = fixture
        .manager
        .add(vec![
            request("http://127.0.0.1/interrupted.bin".into(), None),
            request("http://127.0.0.1/queued.bin".into(), None),
        ])
        .unwrap();
    fixture.manager.shutdown().await.unwrap();
    let connection = rusqlite::Connection::open(&fixture.db).unwrap();
    let mut interrupted = serde_json::to_value(&added[0]).unwrap();
    interrupted["status"] = serde_json::json!("downloading");
    interrupted["downloadedBytes"] = serde_json::json!(1234);
    interrupted["speedBps"] = serde_json::json!(100);
    interrupted["etaSeconds"] = serde_json::json!(20);
    interrupted["connections"] = serde_json::json!(1);
    connection
        .execute(
            "UPDATE downloads SET data = ?1 WHERE id = ?2",
            rusqlite::params![interrupted.to_string(), added[0].id],
        )
        .unwrap();
    let mut settings = serde_json::to_value(fixture.manager.snapshot().unwrap().settings).unwrap();
    settings["maxConcurrent"] = serde_json::json!(1);
    settings["theme"] = serde_json::json!("dark");
    settings["notifications"] = serde_json::json!(false);
    connection
        .execute(
            "UPDATE settings SET data = ?1 WHERE id = 1",
            [settings.to_string()],
        )
        .unwrap();
    drop(connection);
    drop(fixture.manager);

    let reopened = Manager::new(fixture.db.clone(), fixture.destination.clone()).unwrap();
    let snapshot = reopened.snapshot().unwrap();
    assert_eq!(snapshot.settings.max_concurrent, 1);
    assert_eq!(serde_json::to_value(&snapshot.settings).unwrap(), settings);
    let interrupted = snapshot
        .downloads
        .iter()
        .find(|item| item.id == added[0].id)
        .unwrap();
    assert_eq!(interrupted.status, Status::Failed);
    assert_eq!(interrupted.downloaded_bytes, 1234);
    assert_eq!(interrupted.speed_bps, 0);
    assert!(interrupted.eta_seconds.is_none());
    assert_eq!(interrupted.connections, 0);
    assert!(
        interrupted
            .error
            .as_deref()
            .unwrap()
            .to_lowercase()
            .contains("interrupt")
    );
    let queued = snapshot
        .downloads
        .iter()
        .find(|item| item.id == added[1].id)
        .unwrap();
    assert_eq!(queued.status, Status::Queued);
    assert_eq!(queued.queue_order, added[1].queue_order);
    reopened.shutdown().await.unwrap();

    // Recovery itself is persisted, rather than existing only in an in-memory snapshot.
    let connection = rusqlite::Connection::open(&fixture.db).unwrap();
    let stored: String = connection
        .query_row(
            "SELECT data FROM downloads WHERE id = ?1",
            [&added[0].id],
            |row| row.get(0),
        )
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored["status"], "failed");
}

#[tokio::test]
async fn invalid_batch_is_atomic_and_shutdown_rejects_new_work() {
    let fixture = Fixture::new();
    assert!(
        fixture
            .manager
            .add(vec![
                request("http://127.0.0.1/valid.bin".into(), None),
                request("http://127.0.0.1/unsafe.bin".into(), Some("../escape.bin")),
            ])
            .is_err()
    );
    assert!(fixture.manager.snapshot().unwrap().downloads.is_empty());
    fixture.manager.shutdown().await.unwrap();
    fixture.manager.start();
    assert!(
        fixture
            .manager
            .add(vec![request(
                "http://127.0.0.1/after-shutdown.bin".into(),
                None
            )])
            .is_err()
    );
}

#[tokio::test]
async fn database_has_one_owner_until_the_manager_is_dropped() {
    let fixture = Fixture::new();
    assert!(Manager::new(fixture.db.clone(), fixture.destination.clone()).is_err());
    fixture.manager.shutdown().await.unwrap();
    drop(fixture.manager);
    let reopened = Manager::new(fixture.db.clone(), fixture.destination.clone()).unwrap();
    assert!(reopened.snapshot().unwrap().downloads.is_empty());
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn fatal_persistence_error_surfaces_in_snapshot_and_shutdown() {
    let fixture = Fixture::new();
    fixture
        .manager
        .add(vec![request("http://127.0.0.1/queued.bin".into(), None)])
        .unwrap();
    let connection = rusqlite::Connection::open(&fixture.db).unwrap();
    connection.execute("DROP TABLE downloads", []).unwrap();
    drop(connection);
    fixture.manager.start();

    timeout(Duration::from_secs(3), async {
        loop {
            if fixture.manager.snapshot().is_err() {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("snapshot must surface a failed scheduler instead of reporting queued work forever");
    assert!(
        timeout(Duration::from_secs(3), fixture.manager.shutdown())
            .await
            .expect("shutdown must return after a scheduler persistence failure")
            .is_err()
    );
}

#[tokio::test]
async fn scheduling_uses_persisted_concurrency_limit() {
    let gate = Arc::new(Semaphore::new(0));
    let server = Server::new(vec![
        ("/first.bin", Response::Held(gate.clone(), vec![1])),
        ("/second.bin", Response::Body(vec![2])),
    ])
    .await;
    let fixture = Fixture::new();
    fixture.manager.shutdown().await.unwrap();
    let connection = rusqlite::Connection::open(&fixture.db).unwrap();
    let mut settings = serde_json::to_value(fixture.manager.snapshot().unwrap().settings).unwrap();
    settings["maxConcurrent"] = serde_json::json!(1);
    connection
        .execute(
            "UPDATE settings SET data = ?1 WHERE id = 1",
            [settings.to_string()],
        )
        .unwrap();
    drop(connection);
    drop(fixture.manager);
    let reopened = Manager::new(fixture.db.clone(), fixture.destination.clone()).unwrap();
    reopened
        .add(vec![
            request(server.url("/first.bin"), None),
            request(server.url("/second.bin"), None),
        ])
        .unwrap();
    reopened.start();
    server.wait_for_requests(1).await;
    sleep(Duration::from_millis(75)).await;
    assert_eq!(*server.requests.lock().unwrap(), ["/first.bin"]);
    gate.add_permits(1);
    let completed = wait_for(&reopened, |items| items.iter().all(terminal)).await;
    assert!(
        completed
            .iter()
            .all(|item| item.status == Status::Completed)
    );
    assert_eq!(server.peak_active.load(Ordering::SeqCst), 1);
    reopened.shutdown().await.unwrap();
}
