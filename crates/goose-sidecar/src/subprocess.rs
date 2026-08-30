//! Mirrors `crates/goose/src/subprocess.rs` (`configure_subprocess`). This crate cannot
//! depend on `goose` without pulling the whole core in, so the idiom is duplicated;
//! keep the two in sync if either changes.
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW_FLAG: u32 = 0x08000000;

#[cfg(target_os = "linux")]
fn configure_parent_death_signal(command: &mut Command) {
    let parent_pid = unsafe { libc::getpid() };

    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }

            if libc::getppid() != parent_pid {
                return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
            }

            Ok(())
        });
    }
}

#[allow(unused_variables)]
pub fn configure_subprocess(command: &mut Command) {
    // Own process group so the sidecar does not receive the terminal's Ctrl+C SIGINT;
    // termination is always explicit and per-pid (never killpg).
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "linux")]
    configure_parent_death_signal(command);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW_FLAG);
}
