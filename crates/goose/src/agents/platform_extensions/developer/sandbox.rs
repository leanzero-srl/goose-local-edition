#[cfg(target_os = "macos")]
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(super) const ROOT_ENV: &str = "GOOSE_TOOL_SANDBOX_ROOT";
pub(super) const HOME_ENV: &str = "GOOSE_TOOL_SANDBOX_HOME";
pub(super) const DENY_ROOT_ENV: &str = "GOOSE_TOOL_SANDBOX_DENY_ROOT";
pub(super) const DENY_LOCAL_PORTS_ENV: &str = "GOOSE_TOOL_SANDBOX_DENY_LOCAL_PORTS";

pub(super) fn root() -> Result<Option<PathBuf>, String> {
    let Some(value) = std::env::var_os(ROOT_ENV) else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("tool sandbox root is unavailable: {error}"))?;
    if !canonical.is_dir() {
        return Err("tool sandbox root is not a directory".to_string());
    }
    Ok(Some(canonical))
}

pub(super) fn checked_path(path: PathBuf) -> Result<PathBuf, String> {
    let Some(root) = root()? else {
        return Ok(path);
    };
    let comparable = canonical_target(&path)?;
    if !comparable.starts_with(&root) {
        return Err(format!(
            "benchmark tool sandbox denied path outside {}",
            root.display()
        ));
    }
    Ok(path)
}

fn canonical_target(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|error| format!("cannot resolve tool path: {error}"));
    }

    let mut ancestor = path;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| "cannot resolve tool path ancestor".to_string())?;
        suffix.push(name.to_os_string());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "cannot resolve tool path ancestor".to_string())?;
    }
    let mut canonical = ancestor
        .canonicalize()
        .map_err(|error| format!("cannot resolve tool path ancestor: {error}"))?;
    for component in suffix.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

#[cfg(target_os = "macos")]
pub(super) fn macos_profile(root: &Path, home: &Path, temp: &Path) -> Result<String, String> {
    let deny_root = std::env::var_os(DENY_ROOT_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{DENY_ROOT_ENV} is required when {ROOT_ENV} is set"))?;
    let deny_root = deny_root
        .canonicalize()
        .map_err(|error| format!("tool sandbox deny root is unavailable: {error}"))?;
    for allowed in [root, home, temp] {
        if !allowed.starts_with(&deny_root) {
            return Err("tool sandbox paths must be contained by its deny root".to_string());
        }
    }
    let denied_local_ports =
        parse_denied_local_ports(std::env::var_os(DENY_LOCAL_PORTS_ENV).as_deref())?;
    Ok(macos_profile_for_paths(
        root,
        home,
        temp,
        &deny_root,
        &denied_local_ports,
    ))
}

#[cfg(target_os = "macos")]
fn parse_denied_local_ports(value: Option<&OsStr>) -> Result<Vec<u16>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let value = value
        .to_str()
        .ok_or_else(|| format!("{DENY_LOCAL_PORTS_ENV} must be UTF-8"))?;
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let mut ports = Vec::new();
    for raw in value.split(',') {
        if raw.is_empty() || raw.trim() != raw || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!(
                "{DENY_LOCAL_PORTS_ENV} must be a sorted comma-separated list of ports"
            ));
        }
        let port = raw
            .parse::<u16>()
            .map_err(|_| format!("{DENY_LOCAL_PORTS_ENV} contains an invalid port `{raw}`"))?;
        if port == 0 || ports.last().is_some_and(|previous| *previous >= port) {
            return Err(format!(
                "{DENY_LOCAL_PORTS_ENV} must contain unique ascending non-zero ports"
            ));
        }
        ports.push(port);
    }
    Ok(ports)
}

#[cfg(target_os = "macos")]
fn macos_profile_for_paths(
    root: &Path,
    home: &Path,
    temp: &Path,
    deny_root: &Path,
    denied_local_ports: &[u16],
) -> String {
    let port_denials = denied_local_ports
        .iter()
        .map(|port| {
            format!(
                "(deny network-inbound (local ip \"localhost:{port}\")) \
                 (deny network-outbound (remote ip \"localhost:{port}\")) "
            )
        })
        .collect::<String>();
    format!(
        "(version 1) (allow default) \
         (deny process-info*) \
         (allow process-info* (target self)) \
         (allow process-info-codesignature) \
         (deny signal) \
         (allow signal (target self)) \
         (allow signal (target same-sandbox)) \
         (deny mach-lookup) \
         (deny network*) \
         (allow network-inbound \
             (local tcp \"localhost:*\") \
             (local udp \"localhost:*\")) \
         (allow network-outbound \
             (remote tcp \"localhost:*\") \
             (remote udp \"localhost:*\")) \
         {port_denials}\
         (deny file-write*) \
         (deny file-read* file-write* (subpath \"{}\")) \
         (deny file-read* file-write* (subpath \"/private/tmp\")) \
         (allow file-write* (literal \"/dev/null\")) \
         (allow file-read* file-write* (subpath \"{}\")) \
         (allow file-read* file-write* (subpath \"{}\")) \
         (allow file-read* file-write* (subpath \"{}\"))",
        profile_escape(deny_root),
        profile_escape(root),
        profile_escape(home),
        profile_escape(temp),
    )
}

#[cfg(target_os = "macos")]
fn profile_escape(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_target_detects_existing_symlink_escape() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
            assert!(canonical_target(&root.join("escape/file"))
                .unwrap()
                .starts_with(outside.canonicalize().unwrap()));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn denied_local_ports_require_a_canonical_unique_order() {
        assert_eq!(parse_denied_local_ports(None).unwrap(), Vec::<u16>::new());
        assert_eq!(
            parse_denied_local_ports(Some(OsStr::new("1234,41258,65535"))).unwrap(),
            vec![1234, 41258, 65535]
        );
        for invalid in [
            "0",
            "1234,1234",
            "41258,1234",
            " 1234",
            "1234,",
            "1234.5",
            "65536",
        ] {
            assert!(
                parse_denied_local_ports(Some(OsStr::new(invalid))).is_err(),
                "accepted non-canonical denied port list: {invalid}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_profile_keeps_python_self_access_and_denies_selected_listeners() {
        let profile = macos_profile_for_paths(
            Path::new("/Users/example/campaign/tree"),
            Path::new("/Users/example/campaign/profile"),
            Path::new("/Users/example/campaign/profile/tmp"),
            Path::new("/Users/example"),
            &[1234, 41258],
        );
        for clause in [
            "(deny process-info*)",
            "(allow process-info* (target self))",
            "(allow process-info-codesignature)",
            "(deny signal)",
            "(allow signal (target self))",
            "(allow signal (target same-sandbox))",
            "(deny mach-lookup)",
            "(deny network*)",
            "(allow network-inbound",
            "(local tcp \"localhost:*\")",
            "(local udp \"localhost:*\")",
            "(allow network-outbound",
            "(remote tcp \"localhost:*\")",
            "(remote udp \"localhost:*\")",
            "(deny network-inbound (local ip \"localhost:1234\"))",
            "(deny network-outbound (remote ip \"localhost:1234\"))",
            "(deny network-inbound (local ip \"localhost:41258\"))",
            "(deny network-outbound (remote ip \"localhost:41258\"))",
            "(deny file-write*)",
            "(allow file-write* (literal \"/dev/null\"))",
        ] {
            assert!(profile.contains(clause), "profile omitted {clause}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn generated_macos_profile_runs_sb7_python_but_blocks_escapes() {
        use std::net::TcpListener;
        use std::process::Command;

        let deny_root = PathBuf::from(std::env::var_os("HOME").unwrap())
            .canonicalize()
            .unwrap();
        let case = tempfile::Builder::new()
            .prefix("goose-seatbelt-")
            .tempdir_in(&deny_root)
            .unwrap();
        let root = case.path().join("tree");
        let home = case.path().join("profile");
        let temp = home.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        let root = root.canonicalize().unwrap();
        let home = home.canonicalize().unwrap();
        let temp = temp.canonicalize().unwrap();
        let outside = case.path().join("outside-secret.txt");
        let secret = "SB7_PARENT_SECRET_7f517f8caef54d6f";
        std::fs::write(&outside, secret).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape-link")).unwrap();

        let allowed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let denied_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let denied_bind = TcpListener::bind("127.0.0.1:0").unwrap();
        let allowed_port = allowed_listener.local_addr().unwrap().port();
        let denied_port = denied_listener.local_addr().unwrap().port();
        let denied_bind_port = denied_bind.local_addr().unwrap().port();
        drop(denied_bind);
        let mut denied_ports = vec![denied_port, denied_bind_port];
        denied_ports.sort_unstable();
        let profile = macos_profile_for_paths(&root, &home, &temp, &deny_root, &denied_ports);

        let mut secret_parent = Command::new("/bin/sleep")
            .arg("30")
            .env("SB7_PARENT_SECRET", secret)
            .spawn()
            .unwrap();
        let script = r#"
import pathlib
import os
import signal
import socket
import sqlite3
import subprocess
import sys

outside = pathlib.Path(sys.argv[1])
allowed_port = int(sys.argv[2])
denied_port = int(sys.argv[3])
denied_bind_port = int(sys.argv[4])
parent_pid = int(sys.argv[5])
secret = sys.argv[6]

db = sqlite3.connect("probe.db")
db.execute("create table evidence(value text)")
db.execute("insert into evidence values ('ok')")
db.commit()
db.close()
assert pathlib.Path("roundtrip.bin").write_bytes(b"SB7") == 3
assert pathlib.Path("roundtrip.bin").read_bytes() == b"SB7"
subprocess.run(["/usr/bin/python3", "-c", "print(123)"], check=True)
owned_child = subprocess.Popen(["/bin/sleep", "30"])
owned_child.terminate()
assert owned_child.wait(timeout=2) == -signal.SIGTERM
try:
    os.kill(parent_pid, signal.SIGTERM)
except OSError:
    pass
else:
    raise AssertionError("sandbox signaled a process outside its inherited profile")

server = socket.socket()
server.bind(("127.0.0.1", 0))
server.listen()
client = socket.create_connection(server.getsockname(), timeout=1)
accepted, _ = server.accept()
client.close()
accepted.close()
server.close()
denied_server = socket.socket()
try:
    denied_server.bind(("127.0.0.1", denied_bind_port))
except OSError:
    pass
else:
    raise AssertionError(f"sandbox bound reserved localhost port {denied_bind_port}")
finally:
    denied_server.close()
socket.create_connection(("127.0.0.1", allowed_port), timeout=1).close()
for host, port in [("127.0.0.1", denied_port), ("1.1.1.1", 80)]:
    try:
        socket.create_connection((host, port), timeout=1).close()
    except OSError:
        pass
    else:
        raise AssertionError(f"sandbox reached denied network target {host}:{port}")

for candidate in [outside, pathlib.Path("escape-link")]:
    try:
        candidate.read_bytes()
    except OSError:
        pass
    else:
        raise AssertionError(f"sandbox read outside its tree through {candidate}")
outside_write = outside.parent / "outside-write.txt"
try:
    outside_write.write_text("escape")
except OSError:
    pass
else:
    raise AssertionError(f"sandbox wrote outside its tree through {outside_write}")

try:
    probe = subprocess.run(
        ["/bin/ps", "eww", "-p", str(parent_pid)], capture_output=True, text=True
    )
except OSError:
    pass
else:
    assert secret not in probe.stdout + probe.stderr
for command in [["/usr/bin/pbpaste"], ["/usr/bin/security", "list-keychains"]]:
    try:
        probe = subprocess.run(command, capture_output=True, text=True)
    except OSError:
        pass
    else:
        assert probe.returncode != 0, f"sandbox exposed personal service through {command}"
print("SB7_PROFILE_OK")
"#;
        let output = Command::new("/usr/bin/sandbox-exec")
            .args(["-p", &profile, "/usr/bin/python3", "-c", script])
            .arg(&outside)
            .arg(allowed_port.to_string())
            .arg(denied_port.to_string())
            .arg(denied_bind_port.to_string())
            .arg(secret_parent.id().to_string())
            .arg(secret)
            .current_dir(&root)
            .env_clear()
            .env("HOME", &home)
            .env("TMPDIR", &temp)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .output()
            .unwrap();
        let _ = secret_parent.kill();
        let _ = secret_parent.wait();
        assert!(
            output.status.success(),
            "generated Seatbelt profile rejected its SB7 contract:\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("SB7_PROFILE_OK"));
    }
}
