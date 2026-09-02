//! `goose serve` under SIGTERM: the stop must run the supervised teardown and exit with the
//! conventional `128 + 15` code. Before the signal path existed the default action ended
//! the process (exit status = signal, no code) with nothing torn down — the packaged app's
//! measured orphan class (tailscaled on the mesh socket, rapid-mlx on the engine port).

#![cfg(unix)]

use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const LOOK: Duration = Duration::from_millis(100);
const STARTUP_LOOKS: u32 = 600;
const EXIT_LOOKS: u32 = 300;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn read_stderr(child: &mut Child) -> String {
    let mut text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        use std::io::Read;
        let _ = stderr.read_to_string(&mut text);
    }
    text
}

/// Every file under `root` (recursively) whose content carries `needle`, concatenated.
fn files_mentioning(root: &Path, needle: &str) -> String {
    let mut found = String::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.push_str(&files_mentioning(&path, needle));
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            if text.contains(needle) {
                found.push_str(&text);
            }
        }
    }
    found
}

#[test]
fn sigterm_runs_the_supervised_teardown_and_exits_143() {
    let root = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_goose"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--dangerously-unauthenticated",
        ])
        .env("GOOSE_PATH_ROOT", root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn goose serve");

    let mut listening = false;
    for _ in 0..STARTUP_LOOKS {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            listening = true;
            break;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!(
                "goose serve exited before listening: {status:?}\n{}",
                read_stderr(&mut child)
            );
        }
        std::thread::sleep(LOOK);
    }
    if !listening {
        let _ = child.kill();
        panic!(
            "goose serve never listened on {port}\n{}",
            read_stderr(&mut child)
        );
    }

    let pid = child.id();
    let sent = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .unwrap();
    assert!(sent.success(), "kill -TERM {pid} failed");

    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let status = child.wait();
        let stderr = read_stderr(&mut child);
        let _ = tx.send((status, stderr));
    });
    let (status, stderr) = match rx.recv_timeout(LOOK * EXIT_LOOKS) {
        Ok(result) => result,
        Err(_) => {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
            panic!("goose serve did not exit after SIGTERM within {EXIT_LOOKS} looks");
        }
    };
    waiter.join().unwrap();
    let status = status.unwrap();
    assert_eq!(
        status.code(),
        Some(143),
        "expected 128+SIGTERM, got {status:?}\n{stderr}"
    );

    let log = files_mentioning(root.path(), "goose serve: teardown");
    assert!(
        log.contains("leanzero-link mesh") && log.contains("no mesh daemon running"),
        "the mesh teardown step never reported under {}:\n{log}",
        root.path().display()
    );
    assert!(
        log.contains("mlx engine") && log.contains("nothing supervised"),
        "the engine teardown step never reported under {}:\n{log}",
        root.path().display()
    );
}
