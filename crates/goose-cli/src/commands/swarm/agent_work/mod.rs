//! AGENT WORK — the second kind of swarm operation. A PROJECT run builds software; an AGENT run
//! keeps a desk: it polls, investigates, keeps ledgers and a scratchpad, drafts, has its drafts
//! attacked, posts through one gated script, and asks the human when only the human can decide.
//! The operator's desk skills (client desks, the community desks) are the model; `agent.yaml`
//! names their scripts, surgeons, window and files so goose can run them across the fleet.
//!
//! `goose swarm agent init <dir> --name x`   write a starter agent.yaml
//! `goose swarm agent run <dir>`             the serve loop: tick on cadence inside the window
//! `goose swarm agent tick <dir>`            one tick now, then exit
//! `goose swarm agent status <dir>`          the desk's state.json, human-readable
//!
//! The desktop's Agent Work view drives `run` and reads `.swarm/agent/` + `.swarm/activity/`.

pub mod manifest;
pub mod prompts;
pub mod runtime;
pub mod scripts;
pub mod store;
pub mod tick;
pub mod window;

use anyhow::{anyhow, Result};
use chrono::Utc;
use std::path::PathBuf;
use std::sync::Arc;

use manifest::AgentManifest;
use store::{now_rfc3339, DeskState, RuntimeDir};
use window::DeskClock;

#[derive(clap::Subcommand, Debug)]
pub enum AgentCommand {
    /// Write a starter agent.yaml (and the files it names) into a directory.
    Init {
        dir: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    /// Run the desk: tick on its cadence inside its window until stopped.
    Run {
        dir: PathBuf,
        /// One tick now, then exit.
        #[arg(long)]
        once: bool,
    },
    /// One tick now, then exit (alias of `run --once`).
    Tick { dir: PathBuf },
    /// Print the desk's state.
    Status { dir: PathBuf },
    /// Validate agent.yaml and print what the desk would run.
    Check { dir: PathBuf },
}

pub async fn handle(cmd: AgentCommand) -> Result<()> {
    match cmd {
        AgentCommand::Init { dir, name } => init(&dir, name.as_deref()),
        AgentCommand::Run { dir, once } => serve(&dir, once).await,
        AgentCommand::Tick { dir } => serve(&dir, true).await,
        AgentCommand::Status { dir } => status(&dir),
        AgentCommand::Check { dir } => check(&dir),
    }
}

fn init(dir: &std::path::Path, name: Option<&str>) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = AgentManifest::path_in(dir);
    if path.exists() {
        return Err(anyhow!(
            "{} already exists — edit it instead",
            path.display()
        ));
    }
    let name = name
        .map(|s| s.to_string())
        .or_else(|| {
            dir.canonicalize()
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
        })
        .unwrap_or_else(|| "desk".to_string());
    let name = store::sanitize_name(&name);
    std::fs::write(&path, AgentManifest::starter(&name))?;
    for (rel, text) in [
        ("CHARTER.md", "# Charter\n\nThe desk's rules go here: who it speaks as, what it may touch, what it must ask about first.\n"),
        ("SCRATCHPAD.md", ""),
        ("DAILY-LOG.md", ""),
        ("PENDING.md", "# Pending — things only the human can clear\n"),
    ] {
        let p = dir.join(rel);
        if !p.exists() {
            std::fs::write(&p, text)?;
        }
    }
    let rt = RuntimeDir::new(dir);
    rt.ensure()?;
    println!("wrote {}", path.display());
    Ok(())
}

fn check(dir: &std::path::Path) -> Result<()> {
    let m = AgentManifest::load(dir)?;
    let clock = DeskClock::new(&m.timezone, &m.window, &m.cadence)
        .ok_or_else(|| anyhow!("agent.yaml: window/cadence/timezone do not resolve"))?;
    let now = Utc::now();
    println!("agent {} ({})", m.name, m.display_title());
    println!("  charter: {} chars", m.charter_text(dir).chars().count());
    println!(
        "  window: {} — open now: {}",
        clock.local_label(now),
        clock.is_open(now)
    );
    let (next, why) = clock.next_tick(None, now);
    println!(
        "  next tick: {} ({why})",
        next.map(|t| clock.local_label(t))
            .unwrap_or_else(|| "never".into())
    );
    println!("  guard: {}", m.guard.len());
    println!("  poll: {}", m.poll.len());
    println!(
        "  surgeons: {}",
        m.surgeons
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  review lenses: {}", m.review.lenses.join(", "));
    println!(
        "  post: {}",
        m.post
            .as_ref()
            .map(|p| format!("{} (approval: {:?})", p.command, p.approval))
            .unwrap_or_else(|| "none — staged drafts wait for the human".into())
    );
    if let Some(e) = &m.env_file {
        match scripts::load_env_file(&dir.join(e)) {
            Ok(v) => println!("  env_file: {} keys", v.len()),
            Err(err) => println!("  env_file: NOT READABLE — {err}"),
        }
    }
    Ok(())
}

fn status(dir: &std::path::Path) -> Result<()> {
    let rt = RuntimeDir::new(dir);
    match rt.read_state() {
        Err(e) => println!("state.json cannot be read: {e}"),
        Ok(None) => println!(
            "no state at {} — the desk has never run",
            rt.state_path().display()
        ),
        Ok(Some(st)) => {
            println!("{}", serde_json::to_string_pretty(&st)?);
            if let Some(pid) = rt.lock_holder() {
                println!("running: pid {pid}");
            } else {
                println!("running: no");
            }
        }
    }
    Ok(())
}

/// The serve loop. `once` = a single tick now. Flags under `.swarm/agent/` steer it: `paused`
/// (ticks hold), `tick-now` (start a tick immediately), `stop` (exit after the current tick).
async fn serve(dir: &std::path::Path, once: bool) -> Result<()> {
    let dir = dir
        .canonicalize()
        .map_err(|e| anyhow!("agent dir {}: {e}", dir.display()))?;
    let manifest = Arc::new(AgentManifest::load(&dir)?);
    let clock = DeskClock::new(&manifest.timezone, &manifest.window, &manifest.cadence)
        .ok_or_else(|| anyhow!("agent.yaml: window/cadence/timezone do not resolve"))?;
    let rt = RuntimeDir::new(&dir);
    rt.ensure()?;
    rt.take_lock()?;
    rt.clear_stop();
    let run_id = format!(
        "agent-{}-{}",
        manifest.name,
        Utc::now().format("%Y%m%d-%H%M%S")
    );
    let sink: Arc<dyn goose_swarm::EventSink> =
        Arc::new(super::JsonlSink::new(&rt.events_path(), run_id.clone())?);
    let env = match &manifest.env_file {
        Some(rel) => {
            match scripts::load_env_file(&dir.join(rel)) {
                Ok(v) => v,
                Err(e) => {
                    sink.write_value(serde_json::json!({"event": "env_file_unreadable", "file": rel, "error": e}));
                    eprintln!("env_file {rel} could not be read: {e} — scripts run without it");
                    Vec::new()
                }
            }
        }
        None => Vec::new(),
    };
    let charter = Arc::new(manifest.charter_text(&dir));
    sink.write_value(serde_json::json!({
        "event": "agent_started",
        "agent": manifest.name, "title": manifest.display_title(), "dir": dir.display().to_string(),
        "cadence": manifest.cadence, "timezone": manifest.timezone, "window": manifest.window,
        "once": once, "pid": std::process::id(), "charter_chars": charter.chars().count(),
        "surgeons": manifest.surgeons.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
        "lenses": manifest.review.lenses, "post": manifest.post.is_some(),
    }));

    let mut st = match rt.read_state() {
        Ok(Some(s)) => s,
        Ok(None) => DeskState::default(),
        Err(e) => {
            sink.write_value(serde_json::json!({"event": "state_unreadable", "error": e}));
            eprintln!("state.json could not be read ({e}) — the desk starts its count from zero");
            DeskState::default()
        }
    };
    st.agent = manifest.name.clone();
    st.title = manifest.display_title().to_string();
    st.pid = Some(std::process::id());
    st.run_id = run_id.clone();
    st.cadence = manifest.cadence.clone();
    st.timezone = manifest.timezone.clone();
    st.status = "starting".into();
    st.phase = "fleet".into();
    st.phase_started_at = Some(now_rfc3339());
    rt.write_state(&mut st);
    rt.touch_heartbeat();

    let fleet = match runtime::resolve_fleet(&dir, sink.clone(), &manifest.extensions).await {
        Ok(f) => Arc::new(f),
        Err(e) => {
            sink.write_value(
                serde_json::json!({"event": "fleet_unresolved", "error": e.to_string()}),
            );
            st.status = "stopped".into();
            st.phase = "idle".into();
            st.hold_reason = Some(format!("no fleet: {e}"));
            rt.write_state(&mut st);
            rt.release_lock();
            return Err(e);
        }
    };
    st.planner_model = fleet.planner_model.clone();
    st.devices = fleet.devices.clone();
    let ctx = tick::TickCtx {
        manifest: manifest.clone(),
        dir: dir.clone(),
        rt: rt.clone(),
        fleet,
        sink: sink.clone(),
        env,
        charter,
        clock: clock.clone(),
    };

    let mut last_start: Option<chrono::DateTime<Utc>> = st
        .last_tick
        .as_ref()
        .and_then(|t| store::parse_ts(&t.started_at));
    let mut next_tick = st.tick;
    let result: Result<()> = async {
        loop {
            let now = Utc::now();
            let (at, why) = if once || rt.take_tick_now() {
                (Some(now), "requested now")
            } else {
                clock.next_tick(last_start, now)
            };
            st.next_tick_at = at.map(|t| t.to_rfc3339());
            st.next_tick_reason = why.to_string();
            st.next_tick_local = at.map_or_else(|| "none".to_string(), |t| clock.local_label(t));
            st.window_open = clock.is_open(now);
            st.status = if rt.is_paused() { "paused" } else { "waiting" }.into();
            st.phase = "idle".into();
            st.phase_started_at = None;
            rt.write_state(&mut st);
            sink.write_value(serde_json::json!({
                "event": "next_tick", "at": st.next_tick_at, "reason": why, "local": st.next_tick_local,
            }));
            let Some(at) = at else {
                return Err(anyhow!("the desk's window names no open day — nothing to wait for"));
            };
            // Wait, touching the heartbeat, until the tick is due or the human steers.
            loop {
                if rt.stop_requested() {
                    return Ok(());
                }
                if rt.take_tick_now() {
                    break;
                }
                let paused = rt.is_paused();
                let due = Utc::now() >= at;
                if due && !paused {
                    break;
                }
                let status = if paused { "paused" } else { "waiting" };
                if st.status != status {
                    st.status = status.into();
                    rt.write_state(&mut st);
                }
                rt.touch_heartbeat();
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            next_tick += 1;
            last_start = Some(Utc::now());
            let hb = {
                let rt = rt.clone();
                tokio::spawn(async move {
                    loop {
                        rt.touch_heartbeat();
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    }
                })
            };
            let outcome = tick::run_tick(&ctx, &mut st, next_tick).await;
            hb.abort();
            if let Err(e) = outcome {
                sink.write_value(serde_json::json!({"event": "tick_failed", "tick": next_tick, "error": e.to_string()}));
                eprintln!("tick {next_tick} failed: {e}");
            }
            if once || rt.stop_requested() {
                return Ok(());
            }
        }
    }
    .await;
    st.status = "stopped".into();
    st.phase = "idle".into();
    st.pid = None;
    st.next_tick_at = None;
    st.next_tick_reason = if once {
        "ran once".into()
    } else {
        "stopped".into()
    };
    rt.write_state(&mut st);
    sink.write_value(
        serde_json::json!({"event": "agent_stopped", "ticks": next_tick, "once": once}),
    );
    rt.clear_stop();
    rt.release_lock();
    result
}
