//! `goose serve --exit-when-stdin-closes`: goosed follows the app that started it.
//!
//! Q-223 (2026-09-27): the desktop's goosed (pid 10891) outlived its app by 1h22m as an orphan
//! (ppid 1) because nothing told it the app was gone — the app never sent SIGTERM on that quit,
//! and goosed had no way to notice on its own. It kept its mesh daemon, and the next app's Link
//! could not connect. A signal is something the parent must remember to send; a pipe is closed
//! by the kernel however the parent ends (quit, crash, SIGKILL). The desktop spawns goosed with
//! a stdin pipe it never writes to, so EOF on stdin is the proof that its end is gone.
//!
//! The read runs on a plain OS thread, never tokio's blocking pool: a blocking read on stdin
//! cannot be cancelled, and a runtime waiting on it at shutdown would hang (tokio's own
//! documentation for `tokio::io::stdin`).

use std::io::{ErrorKind, Read};

use tokio::sync::oneshot;

/// Resolves once `reader` reaches EOF. A read ERROR other than an interrupt is not EOF: it is
/// logged loudly and the watch never fires (a stdin we cannot read proves nothing about the
/// parent, so it must not stop the server either).
pub fn watch_for_eof<R: Read + Send + 'static>(mut reader: R) -> oneshot::Receiver<()> {
    let (gone, parent_gone) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("goose-serve-parent-watch".to_string())
        .spawn(move || {
            let mut buf = [0u8; 512];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        let _ = gone.send(());
                        return;
                    }
                    Ok(_) => continue,
                    Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                    Err(err) => {
                        tracing::error!(
                            error = %err,
                            "goose serve: stdin could not be read, so this server cannot tell \
                             when the app that started it is gone; it stops only on a signal"
                        );
                        // Keep the sender alive: dropping it would resolve the receiver.
                        loop {
                            std::thread::park();
                        }
                    }
                }
            }
        });
    if let Err(err) = spawned {
        tracing::error!(
            error = %err,
            "goose serve: the parent-watch thread did not start; this server stops only on a signal"
        );
    }
    parent_gone
}

/// The parent watch `goose serve` arms: stdin when asked to, otherwise a future that never
/// resolves (a terminal user or launchd owns the lifecycle then).
pub async fn parent_gone(watch: Option<oneshot::Receiver<()>>) {
    match watch {
        Some(receiver) => {
            if receiver.await.is_err() {
                // The watch thread never started (logged where it failed); nothing to wait on.
                std::future::pending::<()>().await;
            }
        }
        None => std::future::pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    /// Bounded only so a broken watch fails the test instead of hanging CI.
    const TEST_WAIT: Duration = Duration::from_secs(10);

    #[cfg(unix)]
    #[tokio::test]
    async fn the_watch_fires_when_the_last_writer_closes_and_not_before() {
        let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut watch = watch_for_eof(reader);

        writer.write_all(b"bytes are ignored\n").unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(300), &mut watch)
                .await
                .is_err(),
            "data on stdin is not EOF — the parent is still there"
        );

        drop(writer);
        tokio::time::timeout(TEST_WAIT, watch)
            .await
            .expect("EOF resolves the watch")
            .expect("the watch sends, it does not drop");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_killed_parent_closes_the_pipe_and_fires_the_watch() {
        // The real shape: a parent PROCESS holds the write end and dies by SIGKILL, running no
        // handler at all. `sleep` holds the only write end, as its stdout.
        let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
        let writer_fd: std::os::fd::OwnedFd = writer.into();
        let mut parent = std::process::Command::new("sleep")
            .arg("600")
            .stdout(std::process::Stdio::from(writer_fd))
            .spawn()
            .unwrap();
        let watch = watch_for_eof(reader);

        parent.kill().unwrap();
        parent.wait().unwrap();

        tokio::time::timeout(TEST_WAIT, watch)
            .await
            .expect("the kernel closed the dead parent's end")
            .expect("the watch sends, it does not drop");
    }

    #[tokio::test]
    async fn no_watch_never_resolves() {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), parent_gone(None))
                .await
                .is_err()
        );
    }

    struct Unreadable;
    impl Read for Unreadable {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                "EBADF-like",
            ))
        }
    }

    #[tokio::test]
    async fn an_unreadable_stdin_is_not_a_dead_parent() {
        let watch = watch_for_eof(Unreadable);
        assert!(
            tokio::time::timeout(Duration::from_millis(300), parent_gone(Some(watch)))
                .await
                .is_err(),
            "a read error must never stop the server"
        );
    }
}
