//! Run one test of this binary in a child process that has its own environment.
//!
//! Every test thread of a `cargo test` binary shares one process environment. A test that sets a
//! variable the shell spawn path reads (GOOSE_SHELL, SHELL, PATH, GOOSE_TOOL_SHIM_DIR) changes it
//! for every test spawning a shell at that moment: `env_lock` serialises only the tests that take
//! it, never the ones that just read. Q-163 (CI, 2026-09-26, twice): `pwd` through the shell tool
//! returned `/bin/sh: <tmp>/login-shell: No such file or directory` because the Q-102 node test had
//! GOOSE_SHELL pointing at a fake shell in its temp dir; that test finished and deleted the dir
//! between the `pwd` spawn's exec and /bin/sh opening the script. A child process receives its
//! environment at spawn, so this process's environment never changes.

use std::process::Command;

/// Variables the shell tool's spawn path reads from the process environment. No in-process test
/// may set them; `run_ignored_test_in_child` hands them to a child instead.
const SPAWN_ENV: [&str; 4] = ["GOOSE_SHELL", "SHELL", "PATH", "GOOSE_TOOL_SHIM_DIR"];

/// The libtest name of `name` in `module` (a `module_path!()`, whose leading crate name libtest
/// does not print).
pub fn test_name(module: &str, name: &str) -> String {
    let module = module.split_once("::").map_or("", |(_, rest)| rest);
    format!("{module}::{name}")
}

/// Runs the `#[ignore]`d test `test` of this binary in a child process with `set` applied and
/// `unset` removed, and returns the child's stdout. Panics with the child's whole output unless
/// exactly that test ran and passed: a filter that matched nothing is a failure, not a pass.
pub fn run_ignored_test_in_child(test: &str, set: &[(&str, &str)], unset: &[&str]) -> String {
    let exe = std::env::current_exe().expect("the running test binary");
    let mut command = Command::new(exe);
    command.args([
        test,
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    for (key, value) in set {
        command.env(key, value);
    }
    for key in unset {
        command.env_remove(key);
    }
    let output = command.output().expect("spawn the test binary");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "child run of {test} ({}):\nstdout:\n{stdout}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

#[cfg(test)]
mod tests {
    use super::SPAWN_ENV;

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// The Q-163 tripwire: a `lock_env` or `set_var` naming a variable the shell spawn path reads
    /// re-opens the race in which a sibling's `pwd` execs a fake shell that is being deleted.
    #[test]
    fn no_in_process_test_sets_what_shell_spawns_read() {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        rust_files(&crate_dir.join("src"), &mut files);
        rust_files(&crate_dir.join("tests"), &mut files);
        assert!(files.len() > 100, "scanned only {} files", files.len());
        let names_one = |text: &str| {
            SPAWN_ENV
                .iter()
                .any(|key| text.contains(&format!("\"{key}\"")))
                || text.contains("TOOL_SHIM_DIR_ENV")
        };
        let mut offenders = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            for after in text.split("lock_env(").skip(1) {
                let args = after.split("]);").next().unwrap_or(after);
                if names_one(args) {
                    offenders.push(format!(
                        "{}: lock_env({}",
                        file.display(),
                        args.replace('\n', " ")
                    ));
                }
            }
            for line in text.lines() {
                if (line.contains("set_var(") || line.contains("remove_var(")) && names_one(line) {
                    offenders.push(format!("{}: {}", file.display(), line.trim()));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "these change a variable every concurrent shell spawn reads; run the test in a child \
             with test_env_child::run_ignored_test_in_child (Q-163):\n{}",
            offenders.join("\n")
        );
    }
}
