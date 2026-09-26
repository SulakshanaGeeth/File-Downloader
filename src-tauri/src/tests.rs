use std::time::Duration;

use flow_core::{AddRequest, Status};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::*;

fn state(directory: &TempDir) -> DesktopState {
    DesktopState::new(
        DownloadManager::new(
            directory.path().join("flow.sqlite3"),
            directory.path().join("downloads"),
        )
        .map_err(display_error),
    )
}

fn request(url: impl Into<String>) -> AddRequest {
    AddRequest {
        url: url.into(),
        file_name: None,
        destination: None,
    }
}

#[tokio::test]
async fn quit_blocks_additions_and_cancel_restores_them() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(&directory);
    assert!(state.begin_quit().unwrap());
    assert!(!state.begin_quit().unwrap());
    assert!(
        state
            .add(vec![request("http://127.0.0.1/example.txt")])
            .unwrap_err()
            .contains("closing")
    );
    assert!(state.snapshot().unwrap().downloads.is_empty());
    state.cancel_quit();
    assert_eq!(
        state
            .add(vec![request("http://127.0.0.1/example.txt")])
            .unwrap()
            .len(),
        1
    );
    state.manager().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn commands_reject_invalid_destinations_and_unready_files() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(&directory);
    let destination_file = directory.path().join("not-a-folder");
    std::fs::write(&destination_file, b"existing data").unwrap();
    let mut invalid = request("http://127.0.0.1/example.txt");
    invalid.destination = Some(destination_file.to_str().unwrap().into());
    assert!(
        state
            .add(vec![invalid])
            .unwrap_err()
            .contains("not a directory")
    );
    assert!(state.snapshot().unwrap().downloads.is_empty());
    let download = state
        .add(vec![request("http://127.0.0.1/example.txt")])
        .unwrap()
        .remove(0);
    assert!(
        state
            .completed_path("missing-id")
            .unwrap_err()
            .contains("no longer exists")
    );
    assert!(
        state
            .completed_path(&download.id)
            .unwrap_err()
            .contains("not ready")
    );
    state.manager().unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn download_can_be_resolved_after_restart_and_missing_file_is_reported() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/example.txt", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 2048];
        socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nFlow desktop",
            )
            .await
            .unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let desktop = state(&directory);
    let download = desktop.add(vec![request(url)]).unwrap().remove(0);
    desktop.manager().unwrap().start();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = desktop.snapshot().unwrap();
            if snapshot.downloads[0].status == Status::Completed {
                assert_eq!(snapshot.downloads[0].downloaded_bytes, 12);
                let json = serde_json::to_value(snapshot).unwrap();
                assert_eq!(json["downloads"][0]["fileName"], "example.txt");
                assert_eq!(json["downloads"][0]["downloadedBytes"], 12);
                assert!(json["settings"]["downloadDir"].is_string());
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    server.await.unwrap();
    let path = desktop.completed_path(&download.id).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"Flow desktop");
    desktop.manager().unwrap().shutdown().await.unwrap();
    drop(desktop);

    let reopened = state(&directory);
    assert_eq!(reopened.completed_path(&download.id).unwrap(), path);
    std::fs::remove_file(&path).unwrap();
    assert!(
        reopened
            .completed_path(&download.id)
            .unwrap_err()
            .contains("moved or deleted")
    );
    std::fs::create_dir(&path).unwrap();
    assert!(
        reopened
            .completed_path(&download.id)
            .unwrap_err()
            .contains("no longer a file")
    );
    reopened.manager().unwrap().shutdown().await.unwrap();
}

#[test]
fn startup_failures_remain_visible_to_the_frontend() {
    let state = DesktopState::new(Err("Could not open download database".into()));
    assert_eq!(
        state.snapshot().unwrap_err(),
        "Could not open download database"
    );
    assert!(state.begin_quit().unwrap());
}
