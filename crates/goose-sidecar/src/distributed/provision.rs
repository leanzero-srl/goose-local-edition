//! The ranks' Python, provisioned by goose the way the single engine is (uv, pinned), so nobody
//! types an interpreter path. Each node gets a goose-owned venv under
//! `$HOME/.goose/distributed/<env>` built by that node's own `uv`; the script is idempotent (an env
//! that already imports the pinned versions is left untouched) and prints one `GOOSE_PROV` line per
//! step, which the caller streams as progress. A node without `uv` fails LOUDLY with the places
//! looked — never a guess at another interpreter.

use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::config::Runner;
use super::exec::{sh_quote, SSH_OPTIONS};

/// measured: the pair every distributed run so far was proven on — the JACCL smoke, STEP1b's
/// 62-minute soak and goose's own launcher on the 27B (mlx-jaccl-cluster skill, 2026-09-23/24).
/// The rank wrapper patches mlx_lm.server internals written against exactly this mlx_lm.
pub const MLX_VERSION: &str = "0.32.2";
pub const MLX_LM_VERSION: &str = "0.31.3";
/// measured: the interpreter both jaccl-smoke venvs were built with (`uv venv --python 3.12`).
pub const PYTHON_VERSION: &str = "3.12";
/// The fork commit the qwen4_exp split runs (branch lz/pipeline-qwen4): `pipeline_qwen4
/// {plan,serve,run}`, `plan --json` (with `slots`), the OpenAI server rank 0 serves (272cb0643),
/// a node below its pressure floor refusing with the numbers instead of crashing (9f861d9e1), and
/// `serve --slots` with KV admission by every rank's planned budget and the slot figures on
/// `/v1/status` (ea6f8dee1), idle worker ranks parked in a blocking recv instead of spinning a core in the collective (286ed77f7, 2026-09-24),
/// the node budget = available − 7% of RAM capped at Metal's working set (2ce699589), and IMAGE
/// INPUT — the vision tower on rank 0, (t, h, w) RoPE on every rank, OpenAI image content parts,
/// `/v1/models` `capabilities: ["text", "vision", …]` (d40e9e363, dabcc67b2, 2f7cdf27c), and the
/// available margin backed off 7% → 9.3% of RAM after the M4 Max warned at 7.3% (2f02ac645), and
/// a PREFIX CACHE (b7bd1afc2, branch lz/pipeline-prefix-cache, tag lz-pipeline-qwen4.1, Q-75):
/// every rank snapshots its own layers at the single engine's stable boundary and restores the
/// SAME prefix rank 0 chose, bounded inside each rank's planned KV budget minus the live batch;
/// usage carries `prompt_tokens_details.cached_tokens`, `/v1/status` a `prefix_cache` block, and
/// `/v1/models` declares `rapid_mlx_transient_tail` (so the omlx provider sends the tail).
/// Pinned by commit, never by branch: the rank program's argv and the plan JSON are a contract.
pub const PIPELINE_FORK_COMMIT: &str = "b7bd1afc2fd1d75ed366c09c3e41b96be2cfbb68";
/// The fork carrying `rapid_mlx.distributed.pipeline_qwen4` at [`PIPELINE_FORK_COMMIT`].
pub const PIPELINE_FORK: &str =
    "rapid-mlx @ git+https://github.com/leanzero-srl/Rapid-MLX@b7bd1afc2fd1d75ed366c09c3e41b96be2cfbb68";
/// mlx-vlm carries the vision tower, the image processor and the RoPE index rank 0 serves images
/// with — the fork's own `[vision]` pin, installed alone: the extra also pulls torch/torchvision,
/// which nothing here imports.
pub const MLX_VLM_VERSION: &str = "0.7.1";

/// The fork env's directory name before its Python version.
const PIPELINE_ENV_STEM: &str = "rapid-mlx-pipeline-qwen4-py";

/// Where every goose-managed env lives, relative to the node's `$HOME`.
pub const ENVS_DIR: &str = ".goose/distributed";

/// Prefix of every progress line the provisioning script prints.
pub const PROGRESS_MARKER: &str = "GOOSE_PROV ";

/// One goose-managed interpreter: its directory name, what `uv pip install` puts in it, and the
/// import that proves it (printing exactly `expect`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvSpec {
    pub name: String,
    pub packages: Vec<String>,
    pub check: String,
    pub expect: String,
}

impl EnvSpec {
    /// mlx + mlx_lm for the tensor runner (`mlx_lm.server` under goose's rank wrapper).
    pub fn tensor() -> Self {
        Self {
            name: format!("mlx{MLX_VERSION}-mlxlm{MLX_LM_VERSION}-py{PYTHON_VERSION}"),
            packages: vec![
                format!("mlx=={MLX_VERSION}"),
                format!("mlx-lm=={MLX_LM_VERSION}"),
            ],
            check: "import mlx.core as mx, mlx_lm; print(mx.__version__, mlx_lm.__version__)"
                .to_string(),
            expect: format!("{MLX_VERSION} {MLX_LM_VERSION}"),
        }
    }

    /// The fork for the qwen4_exp pipeline runner, on the same mlx pair. The proof prints the
    /// installed fork commit (PEP 610 `direct_url.json`, which uv writes for a git install —
    /// measured 2026-09-24), so an env built from an earlier pin fails the proof and the next
    /// provisioning reinstalls it in place instead of leaving a stale server behind.
    pub fn pipeline() -> Self {
        Self {
            name: format!("{PIPELINE_ENV_STEM}{PYTHON_VERSION}"),
            packages: vec![
                PIPELINE_FORK.to_string(),
                format!("mlx=={MLX_VERSION}"),
                format!("mlx-lm=={MLX_LM_VERSION}"),
                format!("mlx-vlm=={MLX_VLM_VERSION}"),
            ],
            check: "import json, importlib.metadata as md, mlx.core as mx, mlx_lm, mlx_vlm, \
                    rapid_mlx.distributed.pipeline_qwen4, rapid_mlx.distributed.pipeline_qwen4_serve; \
                    print(mx.__version__, mlx_lm.__version__, json.loads(md.distribution(\"rapid-mlx\")\
                    .read_text(\"direct_url.json\") or \"{}\").get(\"vcs_info\", {}).get(\"commit_id\"))"
                .to_string(),
            expect: format!("{MLX_VERSION} {MLX_LM_VERSION} {PIPELINE_FORK_COMMIT}"),
        }
    }

    /// The env a runner needs on every node.
    pub fn for_runner(runner: Runner) -> Self {
        match runner {
            Runner::MlxLmTensor => Self::tensor(),
            Runner::PipelineQwen4 => Self::pipeline(),
        }
    }

    pub fn dir(&self, home: &str) -> String {
        format!("{}/{ENVS_DIR}/{}", home.trim_end_matches('/'), self.name)
    }

    pub fn python(&self, home: &str) -> String {
        format!("{}/bin/python", self.dir(home))
    }

    /// The managed env `python` points into, if it is one of goose's (an operator's own
    /// interpreter is never provisioned or touched).
    pub fn managed_by(python: &str) -> Option<Self> {
        [Self::tensor(), Self::pipeline()]
            .into_iter()
            .find(|spec| python.ends_with(&format!("/{ENVS_DIR}/{}/bin/python", spec.name)))
    }

    /// The current spec by its directory name.
    pub fn named(name: &str) -> Option<Self> {
        [Self::tensor(), Self::pipeline()]
            .into_iter()
            .find(|spec| spec.name == name)
    }

    /// Whether `name` is this env's directory or one an EARLIER goose gave it: the tensor env's
    /// name carries its pins (`mlx<v>-mlxlm<v>-py<v>`) and the fork env's its Python, so a bump
    /// renames the directory while a saved config keeps naming the old one.
    fn family_has(&self, name: &str) -> bool {
        if self.name == Self::tensor().name {
            name.starts_with("mlx") && name.contains("-mlxlm") && name.contains("-py")
        } else {
            name.starts_with(PIPELINE_ENV_STEM)
        }
    }

    /// The goose-managed env `python` lives in — ANY interpreter under a node's
    /// `$HOME/.goose/distributed/<env>/bin/python` whose `<env>` is this env or an earlier goose's
    /// name for it — with the current pin it must prove and the path goose builds it at. One rule
    /// for both runners: such an env that fails its pinned proof is goose's to rebuild; an
    /// interpreter anywhere else is the operator's, never rebuilt.
    pub fn goose_managed(python: &str) -> Option<ManagedPython> {
        let (home, rest) = python.split_once(&format!("/{ENVS_DIR}/"))?;
        let name = rest.strip_suffix("/bin/python")?;
        if home.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        [Self::tensor(), Self::pipeline()]
            .into_iter()
            .find(|spec| spec.family_has(name))
            .map(|spec| ManagedPython {
                spec,
                home: home.to_string(),
            })
    }
}

/// A goose-managed interpreter as configured: the env's CURRENT spec and the node's home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedPython {
    pub spec: EnvSpec,
    pub home: String,
}

impl ManagedPython {
    /// Where goose builds this env now; differs from the configured path when an earlier goose
    /// named the directory (a pin bump), and the config is pointed here once it is built.
    pub fn target(&self) -> String {
        self.spec.python(&self.home)
    }
}

/// Where a node's `uv` is looked for, in order (shell words): the login shell's PATH (Homebrew's
/// shellenv is set in `.zprofile`, which a non-login ssh command never reads — the workhorse trap),
/// then the installers' default locations.
pub const UV_CANDIDATES: [&str; 5] = [
    "\"$(/bin/zsh -lc 'command -v uv' </dev/null 2>/dev/null | /usr/bin/tail -1)\"",
    "\"$HOME/.local/bin/uv\"",
    "/opt/homebrew/bin/uv",
    "/usr/local/bin/uv",
    "\"$HOME/.cargo/bin/uv\"",
];

/// Prints `path|version` per executable `uv` candidate.
pub fn uv_candidates_script() -> String {
    uv_candidates_script_from(&UV_CANDIDATES)
}

fn uv_candidates_script_from(candidates: &[&str]) -> String {
    format!(
        "for c in {}; do if [ -n \"$c\" ] && [ -x \"$c\" ]; then echo \"$c|$(\"$c\" --version 2>&1)\"; fi; done",
        candidates.join(" ")
    )
}

/// The places [`uv_candidates_script`] looks, for the loud "uv not found" message.
pub const UV_LOOKED: &str = "the login shell's PATH (zsh -lc 'command -v uv'), ~/.local/bin/uv, \
                             /opt/homebrew/bin/uv, /usr/local/bin/uv, ~/.cargo/bin/uv";

/// The provisioning script for one node: find uv, then (unless the env already proves itself)
/// create the venv with the pinned Python and install the pinned packages, then prove the import.
/// Every step prints a `GOOSE_PROV <step> …` line; the last is `done …` or `fail …`.
pub fn provision_script(spec: &EnvSpec) -> String {
    provision_script_from(spec, &UV_CANDIDATES)
}

fn provision_script_from(spec: &EnvSpec, uv_candidates: &[&str]) -> String {
    let dir = format!("\"$HOME\"/{}/{}", ENVS_DIR, sh_quote(&spec.name));
    let check = sh_quote(&spec.check);
    let expect = sh_quote(&spec.expect);
    let packages: Vec<String> = spec.packages.iter().map(|p| sh_quote(p)).collect();
    let marker = PROGRESS_MARKER.trim_end();
    let uv = format!("{{ {}; }}", uv_candidates_script_from(uv_candidates));
    format!(
        r#"V={dir}; P="$V/bin/python"
echo "{marker} check $P"
if [ -x "$P" ] && [ "$("$P" -c {check} 2>/dev/null)" = {expect} ]; then echo "{marker} done already $P $("$P" -c {check})"; exit 0; fi
UV=""; for c in $({uv} | /usr/bin/cut -d'|' -f1); do UV="$c"; break; done
if [ -z "$UV" ]; then echo "{marker} fail uv not found on this node (looked: {looked})"; exit 3; fi
echo "{marker} uv $UV $("$UV" --version 2>&1)"
echo "{marker} venv $V (python {python})"
"$UV" venv --allow-existing --python {python} "$V" 2>&1 || {{ echo "{marker} fail uv venv exited $?"; exit 4; }}
echo "{marker} install {listed}"
"$UV" pip install --python "$P" {packages} 2>&1 || {{ echo "{marker} fail uv pip install exited $?"; exit 5; }}
OUT="$("$P" -c {check} 2>&1 | /usr/bin/tail -1)"
if [ "$OUT" = {expect} ]; then echo "{marker} done installed $P $OUT"; else echo "{marker} fail the env imports '$OUT', expected {expect_plain}"; exit 6; fi
"#,
        looked = UV_LOOKED,
        python = PYTHON_VERSION,
        listed = spec.packages.join(" "),
        packages = packages.join(" "),
        expect_plain = spec.expect,
    )
}

/// One `GOOSE_PROV` line, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressLine {
    /// check | uv | venv | install | done | fail
    pub step: String,
    pub detail: String,
}

pub fn parse_progress(line: &str) -> Option<ProgressLine> {
    let rest = line.trim().strip_prefix(PROGRESS_MARKER)?;
    let (step, detail) = rest.split_once(' ').unwrap_or((rest, ""));
    Some(ProgressLine {
        step: step.to_string(),
        detail: detail.to_string(),
    })
}

/// Build `spec` on a node, streaming every output line to `on_line`: a LeanZero Link node builds
/// it itself from its own pins (`link_control::provision`); ssh and this Mac run
/// [`provision_script`]. The one path both "Save and provision" and a Run that finds a stale env
/// take.
pub async fn provision_on(
    host: Option<&str>,
    spec: &EnvSpec,
    on_line: &mut (dyn FnMut(&str) + Send),
) -> Result<Option<i32>> {
    match super::link_control::link_peer(host) {
        Some(peer) => super::link_control::provision(peer, spec, on_line).await,
        None => run_streaming(host, &provision_script(spec), on_line).await,
    }
}

/// Run `script` on a node, handing every output line (stdout and stderr) to `on_line` as it
/// arrives. No time bound: a cold install downloads wheels (and possibly a Python); ssh's own
/// transport options still end a session whose peer vanished.
pub async fn run_streaming(
    host: Option<&str>,
    script: &str,
    mut on_line: impl FnMut(&str) + Send,
) -> Result<Option<i32>> {
    let mut cmd = match host {
        None => {
            let mut cmd = Command::new("/bin/sh");
            cmd.arg("-c")
                .arg(script)
                .env("PATH", crate::engine::sidecar_spawn_path());
            cmd
        }
        Some(alias) => {
            let mut cmd = Command::new("/usr/bin/ssh");
            cmd.args(SSH_OPTIONS)
                .args(["-o", "LogLevel=ERROR"])
                .arg(alias)
                .arg(format!("/bin/sh -c {}", sh_quote(script)));
            cmd
        }
    };
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("provisioning on {}", host.unwrap_or("this Mac")))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    for reader in [
        child
            .stdout
            .take()
            .map(|r| Box::new(r) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        child
            .stderr
            .take()
            .map(|r| Box::new(r) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    while let Some(line) = rx.recv().await {
        on_line(line.trim_end_matches('\r'));
    }
    Ok(child.wait().await?.code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tensor_env_pins_the_proven_pair() {
        let spec = EnvSpec::tensor();
        assert_eq!(spec.name, "mlx0.32.2-mlxlm0.31.3-py3.12");
        assert_eq!(spec.packages, vec!["mlx==0.32.2", "mlx-lm==0.31.3"]);
        assert_eq!(
            spec.python("/Users/workhorse"),
            "/Users/workhorse/.goose/distributed/mlx0.32.2-mlxlm0.31.3-py3.12/bin/python"
        );
        assert_eq!(
            EnvSpec::managed_by(&spec.python("/Users/w")),
            Some(EnvSpec::tensor())
        );
        assert_eq!(
            EnvSpec::managed_by("/tmp/jaccl-smoke/.venv/bin/python"),
            None
        );
    }

    /// Goose-managed = under a node's `~/.goose/distributed`, in this env's directory or one an
    /// earlier goose named for it; the target is always the CURRENT pin's path. Anything else is
    /// the operator's.
    #[test]
    fn goose_managed_recognises_its_envs_under_any_name_an_earlier_goose_gave_them() {
        let managed =
            EnvSpec::goose_managed(&EnvSpec::pipeline().python("/Users/workhorse")).unwrap();
        assert_eq!(managed.spec, EnvSpec::pipeline());
        assert_eq!(
            managed.target(),
            EnvSpec::pipeline().python("/Users/workhorse")
        );

        let older_tensor = "/Users/w/.goose/distributed/mlx0.31.0-mlxlm0.30.2-py3.11/bin/python";
        let managed = EnvSpec::goose_managed(older_tensor).unwrap();
        assert_eq!(managed.spec, EnvSpec::tensor());
        assert_eq!(managed.target(), EnvSpec::tensor().python("/Users/w"));

        let older_fork = "/Users/w/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.11/bin/python";
        assert_eq!(
            EnvSpec::goose_managed(older_fork).unwrap().target(),
            EnvSpec::pipeline().python("/Users/w")
        );

        for own in [
            "/tmp/jaccl-smoke/.venv/bin/python",
            "/Users/w/.goose/distributed/my-own-env/bin/python",
            "/Users/w/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.12/bin/python3",
            "/Users/w/.goose/distributed/a/b/bin/python",
        ] {
            assert_eq!(EnvSpec::goose_managed(own), None, "{own}");
        }
    }

    #[test]
    fn the_pipeline_env_pins_the_fork_by_commit_and_proves_it() {
        let spec = EnvSpec::pipeline();
        assert!(PIPELINE_FORK.ends_with(&format!("@{PIPELINE_FORK_COMMIT}")));
        assert_eq!(spec.packages[0], PIPELINE_FORK);
        assert_eq!(
            spec.expect,
            format!("0.32.2 0.31.3 {PIPELINE_FORK_COMMIT}"),
            "an env on another fork commit fails the proof and is reinstalled"
        );
        assert!(spec
            .check
            .contains("rapid_mlx.distributed.pipeline_qwen4_serve"));
        assert!(
            spec.packages
                .contains(&format!("mlx-vlm=={MLX_VLM_VERSION}"))
                && spec.check.contains("mlx_vlm"),
            "rank 0 loads the vision tower through mlx-vlm; an env without it fails the proof"
        );
        assert!(
            !spec.check.contains('\''),
            "the check rides sh_quote unescaped"
        );
    }

    /// The proof's Python, run for real against a stand-in `rapid-mlx` dist whose
    /// `direct_url.json` names a commit: it prints exactly the pinned expectation, and an env
    /// installed from another commit prints something else.
    #[tokio::test]
    async fn the_pipeline_proof_reads_the_installed_commit() {
        let root = tempfile::tempdir().unwrap();
        let site = root.path();
        for module in [
            "mlx",
            "mlx_lm",
            "mlx_vlm",
            "rapid_mlx",
            "rapid_mlx/distributed",
        ] {
            std::fs::create_dir_all(site.join(module)).unwrap();
        }
        std::fs::write(site.join("mlx/__init__.py"), "").unwrap();
        std::fs::write(site.join("mlx/core.py"), "__version__ = '0.32.2'\n").unwrap();
        std::fs::write(site.join("mlx_lm/__init__.py"), "__version__ = '0.31.3'\n").unwrap();
        std::fs::write(site.join("mlx_vlm/__init__.py"), "").unwrap();
        std::fs::write(site.join("rapid_mlx/__init__.py"), "").unwrap();
        std::fs::write(site.join("rapid_mlx/distributed/__init__.py"), "").unwrap();
        std::fs::write(site.join("rapid_mlx/distributed/pipeline_qwen4.py"), "").unwrap();
        std::fs::write(
            site.join("rapid_mlx/distributed/pipeline_qwen4_serve.py"),
            "",
        )
        .unwrap();
        let dist = site.join("rapid_mlx-0.14.3.dist-info");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(
            dist.join("METADATA"),
            "Metadata-Version: 2.1\nName: rapid-mlx\nVersion: 0.14.3\n",
        )
        .unwrap();
        let run = |commit: &str| {
            std::fs::write(
                dist.join("direct_url.json"),
                format!(
                    r#"{{"url":"https://github.com/leanzero-srl/Rapid-MLX","vcs_info":{{"vcs":"git","commit_id":"{commit}"}}}}"#
                ),
            )
            .unwrap();
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(format!(
                    "/usr/bin/python3 -c {}",
                    sh_quote(&EnvSpec::pipeline().check)
                ))
                .env("PYTHONPATH", site)
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
                + &String::from_utf8_lossy(&out.stderr)
        };
        assert_eq!(run(PIPELINE_FORK_COMMIT), EnvSpec::pipeline().expect);
        assert_ne!(
            run("e7d49b355fe2692d54b04332c19fc541e5e120fd"),
            EnvSpec::pipeline().expect
        );
    }

    #[test]
    fn progress_lines_are_read_and_other_lines_are_not() {
        assert_eq!(
            parse_progress("GOOSE_PROV done already /p 0.32.2 0.31.3"),
            Some(ProgressLine {
                step: "done".to_string(),
                detail: "already /p 0.32.2 0.31.3".to_string()
            })
        );
        assert_eq!(parse_progress("Resolved 31 packages in 1.2s"), None);
    }

    /// The script, run for real against a fake `uv` on PATH and a fake HOME: first run builds the
    /// venv through `uv venv` + `uv pip install` and proves the import; the second run finds the
    /// env already proven and touches nothing. A node without uv fails with the places looked.
    #[tokio::test]
    async fn the_script_is_idempotent_and_loud_without_uv() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let spec = EnvSpec {
            name: "t".to_string(),
            packages: vec!["pkg==1".to_string()],
            check: "print('1 2')".to_string(),
            expect: "1 2".to_string(),
        };
        // Only the HOME candidate: this Mac's real uv must not satisfy the "no uv" leg.
        let script = provision_script_from(&spec, &["\"$HOME/.local/bin/uv\""]);
        let run = |script: String, home: std::path::PathBuf| async move {
            let out = tokio::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .env("HOME", &home)
                .env("PATH", "/usr/bin:/bin")
                .output()
                .await
                .unwrap();
            (
                out.status.code(),
                String::from_utf8_lossy(&out.stdout).into_owned(),
            )
        };

        let (code, out) = run(script.clone(), home.path().to_path_buf()).await;
        assert_eq!(code, Some(3), "{out}");
        assert!(out.contains("GOOSE_PROV fail uv not found"), "{out}");

        // A fake uv: `venv` makes bin/python a shell printing the check's answer; `pip` logs.
        let fake = bin.join("uv");
        std::fs::write(
            &fake,
            "#!/bin/sh\ncase \"$1\" in\n--version) echo 'uv 9.9.9';;\nvenv) d=\"$5\"; mkdir -p \"$d/bin\"; printf '#!/bin/sh\\necho 1 2\\n' > \"$d/bin/python\"; chmod +x \"$d/bin/python\";;\npip) echo \"installed $*\" >> \"$HOME/pip.log\";;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();

        let (code, out) = run(script.clone(), home.path().to_path_buf()).await;
        assert_eq!(code, Some(0), "{out}");
        let steps: Vec<String> = out
            .lines()
            .filter_map(parse_progress)
            .map(|p| p.step)
            .collect();
        assert_eq!(steps, ["check", "uv", "venv", "install", "done"], "{out}");
        assert!(out.contains("done installed"), "{out}");
        let log = std::fs::read_to_string(home.path().join("pip.log")).unwrap();
        assert!(log.contains("'pkg==1'") || log.contains("pkg==1"), "{log}");

        let (code, out) = run(script, home.path().to_path_buf()).await;
        assert_eq!(code, Some(0), "{out}");
        assert!(out.contains("GOOSE_PROV done already"), "{out}");
        assert!(!out.contains("GOOSE_PROV install"), "{out}");
    }

    #[tokio::test]
    async fn streaming_hands_over_every_line_and_the_exit_code() {
        let mut seen = Vec::new();
        let code = run_streaming(None, "echo one; echo two >&2; exit 7", |l| {
            seen.push(l.to_string())
        })
        .await
        .unwrap();
        seen.sort();
        assert_eq!(seen, ["one", "two"]);
        assert_eq!(code, Some(7));
    }

    /// Provisions the tensor env on the `workhorse` alias for real and prints every line with its
    /// elapsed time. `GOOSE_PROV_HOST=<alias>` picks another node.
    /// `cargo test -p goose-sidecar --lib live_provision -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "installs mlx + mlx_lm into ~/.goose/distributed on a real node over ssh"]
    async fn live_provision() {
        let host = std::env::var("GOOSE_PROV_HOST").unwrap_or_else(|_| "workhorse".to_string());
        // GOOSE_PROV_ENV=pipeline builds the fork env instead of the tensor one; GOOSE_PROV_HOST=local
        // provisions this Mac.
        let spec = match std::env::var("GOOSE_PROV_ENV").as_deref() {
            Ok("pipeline") => EnvSpec::pipeline(),
            _ => EnvSpec::tensor(),
        };
        let host = (host != "local").then_some(host);
        let started = std::time::Instant::now();
        let mut last = None;
        let code = run_streaming(host.as_deref(), &provision_script(&spec), |line| {
            println!("{:>7.1}s {line}", started.elapsed().as_secs_f64());
            if let Some(p) = parse_progress(line) {
                last = Some(p);
            }
        })
        .await
        .unwrap();
        println!(
            "exit {code:?} after {:.1}s",
            started.elapsed().as_secs_f64()
        );
        assert_eq!(code, Some(0));
        assert_eq!(last.unwrap().step, "done");
    }
}
