"""Run one BUILD episode end to end: mock up, agent builds, grade the artifact, report.

Works for any entrant — a cloud model through `goose run`, the local fleet, or `goose swarm run` —
because the only thing that varies is how the agent is invoked. Everything downstream reads the
produced tree and the vendor's request trace.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path
from typing import Dict

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import bench_budget  # noqa: E402
import bench_cost  # noqa: E402
import isolated_tiers  # noqa: E402
import score_build  # noqa: E402
import vendor_service  # noqa: E402


def _regime():
    """sb-6 gate (--sb6 / BENCH_SB6): spec v3 + vendor_service_v2 + score_sb6. Read at CALL
    time (env-at-import would miss a sweep's per-arm env — the FEATURE_CHECKS precedent).
    Returns (scorer_module, vendor_module, default_spec_name). Default path byte-identical."""
    tier = isolated_tiers.active()
    if tier:
        import importlib
        import vendor_service_v3
        return importlib.import_module(tier.scorer), vendor_service_v3, tier.spec
    if os.environ.get("BENCH_SB8"):
        import score_sb8
        import vendor_service_v4
        return score_sb8, vendor_service_v4, "spec-build-sb8.md"
    if os.environ.get("BENCH_SB7"):
        import score_sb7  # noqa: PLC0415 — deliberately lazy, same as the sb-6 branch
        import vendor_service_v3  # noqa: PLC0415
        return score_sb7, vendor_service_v3, "spec-build-sb7.md"
    if not os.environ.get("BENCH_SB6"):
        return score_build, vendor_service, "spec-build.md"
    import score_sb6  # noqa: PLC0415 — deliberately lazy: sb-5 runs never import sb-6
    import vendor_service_v2  # noqa: PLC0415
    return score_sb6, vendor_service_v2, "spec-build-v3.md"

ROOT = HERE.parent
# BENCH_GOOSE (product contract 2026-08-17): the packaged desktop app passes its bundled engine
# here; the dev default stays the checkout's release binary.
GOOSE = Path(os.environ["BENCH_GOOSE"]) if os.environ.get("BENCH_GOOSE") \
    else Path.home() / "Projects/goose/target/release/goose"
MODELS = {
    "opus-5": "us.anthropic.claude-opus-5",
    "fable-5": "us.anthropic.claude-fable-5",
    "sonnet-5": "us.anthropic.claude-sonnet-5",
    "haiku-4.5": "us.anthropic.claude-haiku-4-5-20251001-v1:0",
    # OpenAI on Bedrock — the region inference-profile form is REQUIRED (the bare id fails with
    # 'on-demand throughput isn't supported'; measured, then verified with a live one-token call).
    "gpt-5.6-luna": "us.openai.gpt-5.6-luna",
    "gpt-5.6-sol": "us.openai.gpt-5.6-sol",
    "gpt-5.6-terra": "us.openai.gpt-5.6-terra",
}


# The closing sentence goose's reply loop writes when a provider call ends the turn
# (crates/goose/src/agents/agent.rs, split_record.rs; test_run_build_cloud pins them to the source).
# MEASURED 2026-10-02 (openrouter-cloud-01dae737): turn 87 of a live build ended on "Network error:
# Stream decode error: ... connection reset ... Please resend your message to try again.", goose run
# exited, and the half-built tree was graded as the model's result.
PROVIDER_ERROR_CLOSERS = (
    "Please resend your message to try again.",
    "Please retry if you think this is a transient or recoverable error.",
    "Sending the same request again will fail the same way until its cause is fixed.",
    "then resend your message to continue.",
    "resending this conversation is likely to be refused again.",
)
PROVIDER_ERROR_HEADS = ("Network error:", "Ran into this error:", "The provider refused this request.")
CREDITS_TOP_UP_LINE = "Visit this URL to top up credits:"


def provider_error_ending(tail: str) -> str | None:
    """The provider failure the session's console ended on, or None when it ended on anything else.

    The text runs from the failure's own head ("Network error: ...") to its closing sentence; the
    console may glue the head to the last tool output (the measured run printed "restoredNetwork
    error: ..."). A failure with no known head is recorded as its last two paragraphs, as printed.
    """
    text = tail.rstrip()
    lines = text.splitlines()
    if lines and lines[-1].startswith(CREDITS_TOP_UP_LINE):
        text = "\n".join(lines[:-1]).rstrip()
    if not text.endswith(PROVIDER_ERROR_CLOSERS):
        return None
    start = max(text.rfind(head) for head in PROVIDER_ERROR_HEADS)
    if start >= 0:
        return tail.rstrip()[start:]
    return "\n\n".join(re.split(r"\n\s*\n", tail.rstrip())[-2:]).strip()


def load_env(path: str = "~/.config/agent-board/bedrock.env") -> Dict[str, str]:
    env: Dict[str, str] = {}
    resolved = Path(path).expanduser()
    if not resolved.is_file():
        return env
    for raw in resolved.read_text().splitlines():
        line = raw.strip().removeprefix("export ").strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        env[key.strip()] = value.strip().strip("'\"")
    return env


def build_prompt(port: int) -> str:
    # BENCH3: an amend arm points BENCH_AMEND_SPEC at the brownfield spec; BENCH_SPEC is the
    # regime-level override (sb-5 product: spec-build-v2.md via REGIME.env); greenfield default.
    spec_file = os.environ.get("BENCH_AMEND_SPEC", "") or os.environ.get("BENCH_SPEC", "")
    _sc, _vn, default_spec = _regime()
    spec = (Path(spec_file) if spec_file else ROOT / default_spec).read_text()
    prompt = render_public_contract(spec, port, _vn)
    if isolated_tiers.active():
        prompt += ('\n\nA bundled browser is available for your own tests. Read BROWSER-TESTING.md '
                   'for the runtime paths and screenshot command. It contains no private tests.\n')
    return prompt


def render_public_contract(spec: str, port: int, vendor_module) -> str:
    docs_path = getattr(vendor_module, "DOCS_PATH", "/v1/docs")
    return (spec.replace("{DOCS_URL}", f"http://127.0.0.1:{port}{docs_path}")
                .replace("{BASE_URL}", f"http://127.0.0.1:{port}")
                .replace("{API_KEY}", getattr(vendor_module, "API_KEY", vendor_service.API_KEY)))


def invoke(entrant: str, workdir: Path, port: int, env: Dict[str, str], timeout: int,
           provider: str | None = None, model: str | None = None,
           snapshot: dict | None = None) -> Dict:
    prompt = build_prompt(port)
    (workdir / "benchmark-prompt.md").write_text(prompt)
    def secret_strings(value):
        if isinstance(value, str) and value:
            yield value
        elif isinstance(value, dict):
            for nested in value.values():
                yield from secret_strings(nested)
        elif isinstance(value, list):
            for nested in value:
                yield from secret_strings(nested)
    if snapshot:
        redactions = set(secret_strings(snapshot['secrets']))
        # Custom authentication headers need not use any standardized header name.
        for definition in snapshot['custom_providers'].values():
            redactions.update(secret_strings(definition.get('headers', {})))
    else:
        redactions = {value for name, value in env.items() if value and
                      re.search(r"(?:^|_)(?:KEY|TOKEN|SECRET|PASSWORD)$", name)}
    credential_values = tuple(sorted(redactions, key=len, reverse=True))
    # Every entrant records per-call token telemetry into the tree's own .swarm — the same
    # file the swarm engine defaults to, so telemetry_summary() finds it at scoring time and
    # cloud entries publish MEASURED rates instead of session-store recoveries. Truncated per
    # run (a reused workdir must never rank this run by a previous run's calls).
    # ABSOLUTE, or the engine (cwd = workdir) resolves it to a nested .swarm inside the
    # tree — measured on r15: the outer file stayed 0 bytes while the engine wrote a copy
    # two levels deep, and the scorer reads the outer path.
    tpath = (workdir / ".swarm" / "telemetry.jsonl").resolve()
    tpath.parent.mkdir(parents=True, exist_ok=True)
    tpath.write_text("")
    # II-7 time purge: GOOSE_SWARM_RUN_DEADLINE_UNIX_MS and GOOSE_SWARM_UNCAPPED are read by
    # NOTHING in crates/ any more (only history comments remain — the engine's deadline clamp
    # and the uncapped switch were deleted with the cap arithmetic they served, because the env
    # read re-armed a real wall through the back door), so the harness sets neither. --timeout
    # now bounds ONLY the subprocess wait below (0 = no kill, run to finish); the engine stops
    # on its own progress-based terminators (judge verdicts, repeat-break, dead stream).
    # NEVER let an unattended run block on a GUI dialog.
    #
    # MEASURED (swarm-3node-r0, 2026-08-27): the engine sat for 75m55s between pool_resolved and
    # levers_resolved with ZERO model calls, waiting on a macOS keychain prompt nobody was there to
    # answer. goose builds with the system-keyring feature, every keychain ACL binds to the code
    # identity, and every rebuild of an ad-hoc-signed binary is a NEW code identity - so a fresh build
    # re-prompts, and a benchmark is by definition the run with nobody watching it. Seventy-six minutes
    # vanished from the wall-clock and the run log recorded nothing at all in the gap.
    #
    # The parent resolves the selected provider through Goose's normal credential store before
    # isolation. The entrant receives only that snapshot and never opens the user's keychain.
    env = {**env, "GOOSE_SWARM_TELEMETRY_FILE": str(tpath), "GOOSE_DISABLE_KEYRING": "1"}
    if provider:
        if not model:
            raise ValueError("A cloud entrant requires an explicit model")
        cmd = [str(GOOSE), "run", "--provider", provider, "--model", model, "--no-profile", "--with-builtin", "developer", "-t", prompt]
    elif entrant in MODELS:
        cmd = [str(GOOSE), "run", "--provider", "aws_bedrock", "--model", MODELS[entrant],
               "-t", prompt]
    elif entrant == "local-single":
        cmd = [str(GOOSE), "run", "--provider", "lmstudio",
               "--model", "mihai-qwopus3.6-27b-coder-mtp", "-t", prompt]
    elif entrant.startswith("swarm"):
        # GOOSE_SWARM_MAX_NODES caps the auto-pool inside the engine. `swarm pool disable` cannot do
        # this: the pool is rebuilt from `lms ps` on every run, so a disabled-but-resident device is
        # silently re-added — measured, a 1-node and a 3-node run both reported the same 2-node pool.
        match = re.match(r"swarm-(\d+)node", entrant)
        nodes = int(match.group(1)) if match else 3
        # GOOSE_SWARM_READ_ON_FIX: the arm under test. A fix worker owns no files and is repairing a
        # defect the gates already reproduced by running the app; the implementer read-prohibitions
        # make a cross-module signature mismatch structurally invisible to it.
        # GOOSE_SWARM_PLANNER_ALSO_WORKS: the engine pushes the PLANNER on as an extra worker device
        # unless the planner's model is already in the pool (swarm.rs, `planner_also_works`, default
        # true). MEASURED: at MAX_NODES=1 the pool was one device, the planner model was not in it, the
        # planner was pushed, and the run dispatched 5 tasks to each with a peak of TWO devices working
        # at once — while `run_started.pool` reported 1, because it is emitted before the push. At
        # MAX_NODES=3 the planner model IS in the pool, nothing is pushed, and the run has three. So a
        # node-count sweep was about to compare 2 against 3 while labelling it 1 against 3.
        # Off here, so N nodes means N workers. The 3-node cell is unaffected (nothing was ever pushed
        # there), which is what makes this a correction rather than a change of subject.
        # GOOSE_SWARM_ASK_WAIT_SECS: when plan confidence lands under the ask floor the engine writes
        # .swarm/clarify-questions.json and BLOCK-POLLS for answers for 1800s — THIRTY MINUTES — then
        # proceeds and decides the questions itself. Nothing in this harness answers, so the wait is
        # always paid in full and always ends the same way. MEASURED on the first post-freeze unit:
        # confidence 68 against a floor of 85, low_confidence_ask at +26.6m, and all three nodes
        # GENERATING = 0 for the whole window.
        #
        # The outcome is IDENTICAL either way — the plan that ships after 1800s is the plan that would
        # have shipped after 5s — so the wait buys nothing and costs ~25% of a unit in node-independent
        # idle, injected straight into the wall-clock and occupancy figures the node curve compares.
        # Not answering (rather than authoring canonical answers) keeps the treatment constant across
        # cells without putting my judgement into the build.
        env = {**env, "GOOSE_SWARM_MAX_NODES": str(nodes),
               "GOOSE_SWARM_PLANNER_ALSO_WORKS": "0",
               "GOOSE_SWARM_ASK_WAIT_SECS": "5",
               "GOOSE_SWARM_READ_ON_FIX": os.environ.get("GOOSE_SWARM_READ_ON_FIX", "1")}
        cmd = [str(GOOSE), "swarm", "run", prompt, "--output-format", "json",
               "--log-file", str(workdir / "run.jsonl")]
    else:
        raise SystemExit(f"unknown entrant {entrant!r}")
    # THE CALL BUDGET (bench_budget.py): every single-model entrant of an isolated tier gets the same
    # published number of model calls, then goose ends the session and the harness scores what exists.
    # A swarm entrant (`goose swarm run`) is not budgeted: the engine's NO CAPS invariant stands.
    budgeted = bool(isolated_tiers.active()) and cmd[1] == "run"
    if budgeted:
        prompt_at = cmd.index("-t")
        cmd[prompt_at:prompt_at] = bench_budget.call_budget_args()
    wallet_limit = bench_budget.wallet_limit()

    child_env = {**os.environ, **env}
    if isolated_tiers.active():
        import bench_isolation
        prefix, isolated_env = bench_isolation.prepare(workdir, GOOSE, workdir.parent, snapshot=snapshot)
        cmd = prefix + cmd
        child_env = {key: value for key, value in os.environ.items()
                     if key in {"PATH", "LANG", "LC_ALL", "LC_CTYPE", "USER", "LOGNAME", "SHELL",
                                "GOOSE_SWARM_BENCHMARK", "GOOSE_SWARM_PROBE_ADVERTISED_POST",
                                "GOOSE_SWARM_SHIP_BEST", "GOOSE_SWARM_TESTGEN", "GOOSE_SWARM_DOC_PREFETCH",
                                "GOOSE_SWARM_DIVERSE_PLAN", "GOOSE_SWARM_TEMP", "GOOSE_SWARM_TOP_P",
                                "GOOSE_SWARM_TOP_K", "GOOSE_SWARM_MIN_P", "GOOSE_SWARM_REPEAT_PENALTY"}}
        child_env.update(env)
        child_env.update(isolated_env)
        # The sink lives outside the candidate's tree, which the entrant may clean (2026-10-02,
        # openrouter-cloud-73d233da ran `rm -rf` over harness files in its workdir); a lost sink is an
        # incomplete bill. land_telemetry copies it into .swarm/ after the entrant exits.
        runtime_telemetry = Path(isolated_env["BENCH_SB71_RUNTIME"]) / "telemetry.jsonl"
        runtime_telemetry.write_text("")
        tpath.unlink()
        child_env["GOOSE_SWARM_TELEMETRY_FILE"] = str(runtime_telemetry)
        if not provider:
            child_env['GOOSE_SWARM_RENDER_PROBE'] = str(workdir / 'browser-self-test.mjs')
            child_env['GOOSE_SWARM_RENDER_NODE'] = str(bench_isolation.node_runtime())
    started = time.time()
    # THE BENCHMARK INVARIANT (frame 1.14 §2.6): a scored run is knowledge-blind. The engine refuses
    # to read or write memories/skills under GOOSE_SWARM_BENCHMARK; this is the one place that can
    # SEE the real store, so it snapshots every knowledge directory before the run and refuses the
    # artifact if any file list or mtime moved. A benchmark that learned is not a measurement.
    knowledge_before = knowledge_store_snapshot(workdir)
    # F924: stream the engine's console to a file INSTEAD of buffering it to exit.
    # `capture_output=True` held every byte in memory until the process ended, so during a live
    # run the engine's stderr was unreadable — and the omni-judge reports its looks ONLY there.
    # That cost a whole 5-hour run: a call was looping in plain sight with no way to ask whether
    # the judge had even fired. The console now lands next to the tree while the run is going.
    console = workdir / "engine-console.log"
    session_leader = None
    console_missing = None
    guard = None
    wallet = None
    if wallet_limit is not None:
        if entrant.startswith("swarm") and not provider:
            wallet = bench_budget.wallet_unavailable(
                wallet_limit, None, "the swarm entrant is not guarded: it runs without a harness budget")
        elif provider != "openrouter":
            billing = provider or ("aws_bedrock" if entrant in MODELS else None)
            wallet = bench_budget.wallet_unavailable(wallet_limit, billing,
                                                     bench_cost.unavailable(billing)["reason"])
        elif not env.get("OPENROUTER_API_KEY"):
            wallet = bench_budget.wallet_unavailable(wallet_limit, provider,
                                                     "OPENROUTER_API_KEY is absent from the run's credentials")
    try:
        with console.open("w+", buffering=1, errors="replace") as fh:
            if provider or snapshot is not None:
                proc = subprocess.Popen(cmd, cwd=workdir, stdout=subprocess.PIPE,
                                        stderr=subprocess.STDOUT, text=True,
                                        env={**child_env, "GOOSE_MODE": "auto"},
                                        start_new_session=True)
                session_leader = proc.pid
                if wallet_limit is not None and wallet is None:
                    guard = bench_budget.WalletGuard(
                        proc, Path(child_env["GOOSE_SWARM_TELEMETRY_FILE"]), wallet_limit,
                        env["OPENROUTER_API_KEY"], env.get("OPENROUTER_HOST") or "https://openrouter.ai")
                    guard.start()
                try:
                    for line in proc.stdout:
                        for value in credential_values:
                            line = line.replace(value, "[REDACTED]")
                        fh.write(line)
                        print(line, end="", flush=True)
                    proc.stdout.close()
                    code = proc.wait()
                finally:
                    if guard is not None:
                        guard.stop()
                        wallet = guard.record()
            else:
                # Popen + wait is subprocess.run's own timeout semantics (kill, reap, re-raise), kept
                # so the session leader's pid is known to the teardown below.
                proc = subprocess.Popen(cmd, cwd=workdir, stdout=fh, stderr=subprocess.STDOUT, text=True,
                                        env=child_env, start_new_session=True)
                session_leader = proc.pid
                try:
                    code = proc.wait(timeout=(timeout if timeout and timeout > 0 else None))
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
                    raise
            # Read through the harness's own handle: it survives an unlink of the path.
            fh.seek(0)
            text = fh.read()
            held = os.fstat(fh.fileno()).st_ino
        try:
            replaced = console.stat().st_ino != held
            reason = "replaced"
        except FileNotFoundError:
            replaced, reason = True, "unlinked"
        if replaced:
            console.write_text(text)
            console_missing = {"path": str(console), "reason": reason,
                               "restored_from_open_handle_bytes": len(text.encode())}
            print(f"harness_console_missing: {json.dumps(console_missing)}", file=sys.stderr, flush=True)
        tail = text[-1500:]
    except subprocess.TimeoutExpired:
        code, tail = None, "timed out"
    result = {"exit": code, "secs": round(time.time() - started, 1), "tail": tail,
              "timed_out": code is None}
    if console_missing:
        result["harness_console_missing"] = console_missing
    # The entrant's own test instance must not outlive it: one left running (2026-10-01,
    # openrouter-cloud-c003209f) kept syncing against the scorer's vendor and was graded alongside.
    teardown = reap_entrant_survivors(workdir, session_leader, credential_values)
    result["reaped_processes"] = teardown["reaped_processes"]
    (workdir / "reaped-processes.json").write_text(json.dumps(teardown, indent=2))
    if isolated_tiers.active():
        result["telemetry_landing"] = land_telemetry(runtime_telemetry, tpath)
    if budgeted or wallet is not None:
        budget = {"max_calls": bench_budget.CALL_BUDGET} if budgeted else {}
        entrant_model = cmd[cmd.index("--model") + 1] if "--model" in cmd else None
        budget.update(bench_budget.entrant_calls(tpath, entrant_model))
        budget["stopped_by"] = bench_budget.stopped_by(result, wallet)
        if wallet is not None:
            budget["wallet"] = wallet
        result["budget"] = budget
        print("BENCH_BUDGET " + json.dumps(budget), flush=True)
    knowledge_after = knowledge_store_snapshot(workdir)
    if knowledge_after != knowledge_before:
        changed = sorted(set(knowledge_before.items()) ^ set(knowledge_after.items()))
        raise RuntimeError("REFUSED: the benchmark run touched the knowledge store "
                           "(memories/skills/proposals must stay byte-identical across a scored run): "
                           + "; ".join(f"{path}@{mtime}" for path, mtime in changed[:20]))
    if isolated_tiers.active():
        result["usage"] = bench_isolation.usage(Path(child_env["BENCH_SB71_RUNTIME"]))
        (workdir / "model-usage.json").write_text(json.dumps(result["usage"], indent=2))
        result["billed_cost"] = bench_cost.record(provider, model, env, Path(child_env["BENCH_SB71_RUNTIME"]),
                                                  workdir, result["usage"], budget=result.get("budget"))
    return result


def land_telemetry(source: Path, destination: Path) -> dict:
    """Copy the runtime telemetry sink to <workdir>/.swarm/telemetry.jsonl, where every reader looks.

    A missing source is the named absence harness_telemetry_missing, and any file the entrant left at
    the destination is removed rather than read as the run's calls. A planted symlink at the
    destination (or its directory) is replaced, never written through.
    """
    if destination.parent.is_symlink():
        destination.parent.unlink()
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_symlink() or destination.exists():
        destination.unlink()
    if not source.is_file():
        absence = {"status": "harness_telemetry_missing", "source": str(source),
                   "reason": "the runtime telemetry sink was absent after the entrant exited"}
        print(f"harness_telemetry_missing: {json.dumps(absence)}", file=sys.stderr, flush=True)
        return absence
    shutil.copyfile(source, destination)
    return {"status": "copied", "source": str(source), "destination": str(destination),
            "bytes": destination.stat().st_size}


KNOWLEDGE_DIRS = ("memory", "proposals")


def knowledge_store_snapshot(workdir: Path) -> dict[str, float]:
    """Every file under the global and project-local knowledge stores with its mtime.

    Global: ~/.config/goose/{memory,proposals}. Local: <workdir>/.goose/{memory,proposals}. The skill
    catalogue (~/.config/agents/skills, <workdir>/.goose/skills) rides too. A missing directory is an
    honest empty, not an error — a fresh machine has none of them.
    """
    roots = [Path.home() / ".config" / "goose" / d for d in KNOWLEDGE_DIRS]
    roots += [workdir / ".goose" / d for d in KNOWLEDGE_DIRS]
    roots += [Path.home() / ".config" / "agents" / "skills", workdir / ".goose" / "skills"]
    snapshot: dict[str, float] = {}
    for root in roots:
        if not root.is_dir():
            continue
        for path in sorted(p for p in root.rglob("*") if p.is_file()):
            snapshot[str(path)] = path.stat().st_mtime
    return snapshot


def _process_table() -> Dict[int, dict]:
    listing = subprocess.run(["ps", "-axo", "pid=,ppid=,command="], capture_output=True, text=True,
                             check=True).stdout
    table = {}
    for line in listing.splitlines():
        fields = line.split(None, 2)
        if len(fields) >= 2 and fields[0].isdigit() and fields[1].isdigit():
            table[int(fields[0])] = {"ppid": int(fields[1]), "args": fields[2] if len(fields) > 2 else ""}
    return table


def _process_cwds() -> Dict[int, str]:
    listing = subprocess.run(["lsof", "-a", "-d", "cwd", "-Fpn"], capture_output=True, text=True).stdout
    cwds, pid = {}, None
    for line in listing.splitlines():
        if line.startswith("p") and line[1:].isdigit():
            pid = int(line[1:])
        elif line.startswith("n") and pid is not None:
            cwds[pid] = line[1:]
    return cwds


def _alive(pid: int) -> bool:
    try:
        if os.waitpid(pid, os.WNOHANG)[0] == pid:
            return False
    except ChildProcessError:
        pass
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def reap_entrant_survivors(workdir: Path, session_leader: int | None, redactions=(),
                           grace_seconds: float = 5.0) -> dict:
    """Terminate, PER PID, every process the finished entrant left behind.

    An entrant's process is one in the entrant's session (start_new_session made its leader the
    session id; goose's shell tool regroups commands but keeps the session), a process whose cwd is
    inside the workdir AND whose orphan root (the topmost ancestor below launchd) is itself such a
    process, or a descendant of either. The root rule spares an operator's shell or tool that merely
    cd'd into the tree: its chain reaches a terminal or agent outside the tree, and it is recorded as
    skipped, never signalled. The harness and its ancestors (the desktop app) are never candidates.
    SIGTERM, then poll liveness, then SIGKILL survivors still running the same command (gate 4: no
    group signal). grace_seconds is teardown, not model work (gate 5). # measured: a python app
    server exited 0.001 s after SIGTERM (3 of 3, 2026-10-02); 5 s leaves a graceful handler three
    orders of magnitude of room before SIGKILL.
    """
    root = os.path.realpath(workdir)

    def inside(path: str | None) -> bool:
        return bool(path) and (path == root or path.startswith(root + os.sep))

    def redact(text: str) -> str:
        for value in redactions:
            text = text.replace(value, "[REDACTED]")
        return text

    errors = []
    table = _process_table()
    try:
        cwds = _process_cwds()
    except OSError as error:
        cwds = {}
        errors.append(f"lsof cwd scan unavailable ({type(error).__name__}); only the entrant's session was searched")
    protected, pid = set(), os.getpid()
    while pid in table and pid not in protected:
        protected.add(pid)
        pid = table[pid]["ppid"]
    protected.update({0, 1})

    def in_session(candidate: int) -> bool:
        try:
            return session_leader is not None and os.getsid(candidate) == session_leader
        except OSError:
            return False

    session = {p for p in table if p not in protected and in_session(p)}
    in_tree = {p for p, path in cwds.items() if inside(path) and p in table and p not in protected}

    def orphan_root(candidate: int) -> int:
        seen = set()
        while table.get(candidate, {}).get("ppid", 1) > 1 and candidate not in seen:
            seen.add(candidate)
            candidate = table[candidate]["ppid"]
        return candidate

    rooted = {p for p in in_tree if orphan_root(p) in in_tree | session}
    matched = {p: ["session"] for p in session}
    for p in rooted:
        matched.setdefault(p, []).append("cwd")
    frontier = list(matched)
    while frontier:
        parent = frontier.pop()
        for child, row in table.items():
            if row["ppid"] == parent and child not in matched and child not in protected:
                matched[child] = ["descendant"]
                frontier.append(child)

    reaped = []
    for p in sorted(matched):
        try:
            os.kill(p, signal.SIGTERM)
        except ProcessLookupError:
            continue
        except PermissionError as error:
            errors.append(f"pid {p}: SIGTERM refused ({error.strerror})")
            continue
        reaped.append({"pid": p, "args": redact(table[p]["args"]), "cwd": cwds.get(p),
                       "matched_by": matched[p], "signal": "SIGTERM"})
    pending = [row for row in reaped]
    step = grace_seconds / 100  # ratio: one hundredth of the grace per liveness poll
    waited = 0.0
    while pending and waited < grace_seconds:
        time.sleep(step)
        waited += step
        pending = [row for row in pending if _alive(row["pid"])]
    if pending:
        current = _process_table()
        for row in pending:
            if current.get(row["pid"], {}).get("args") == table[row["pid"]]["args"]:
                try:
                    os.kill(row["pid"], signal.SIGKILL)
                    row["signal"] = "SIGKILL"
                except ProcessLookupError:
                    pass
        waited = 0.0
        while pending and waited < grace_seconds:
            time.sleep(step)
            waited += step
            pending = [row for row in pending if _alive(row["pid"])]
        for row in pending:
            row["survived"] = True
            errors.append(f"pid {row['pid']} still alive after SIGKILL")
    for row in reaped:
        print(f"REAPED entrant survivor pid {row['pid']} ({row['signal']}, matched by "
              f"{'+'.join(row['matched_by'])}) cwd={row['cwd']}: {row['args']}", flush=True)
    record = {"reaped_processes": reaped}
    skipped = [{"pid": p, "args": redact(table[p]["args"]), "cwd": cwds.get(p),
                "reason": f"cwd inside the workdir but its root pid {orphan_root(p)} is outside it"}
               for p in sorted(in_tree - set(matched))]
    if skipped:
        record["skipped_processes"] = skipped
        for row in skipped:
            print(f"NOT REAPED pid {row['pid']} in the workdir: {row['reason']}: {row['args']}", flush=True)
    if errors:
        record["errors"] = errors
        for error in errors:
            print(f"REAP INCOMPLETE: {error}", flush=True)
    return record


def entrant_config(provider: str | None) -> dict:
    """Use the installed engine's own config/keychain semantics, before sandboxing."""
    with tempfile.TemporaryDirectory(prefix="goose-benchmark-config-") as directory:
        output = Path(directory) / "selected.json"
        command = [str(GOOSE), "benchmark-config", "--output", str(output)]
        if provider:
            command.extend(["--provider", provider])
        result = subprocess.run(command, capture_output=True, text=True)
        if result.returncode:
            if 'Benchmark isolation cannot attach the managed MLX swarm engine' in result.stderr:
                raise RuntimeError('Managed MLX swarm attachment is not supported in benchmark isolation. '
                                   'Use Single model with the configured running MLX API endpoint.')
            if 'Benchmarks require a Bedrock API key or explicit AWS credentials' in result.stderr:
                raise RuntimeError('Bedrock benchmark setup requires an API key or explicit AWS credentials. '
                                   'AWS profile/SSO files are not transferred into benchmark isolation.')
            # Provider errors can include endpoint credentials; never relay raw engine output.
            raise RuntimeError("Benchmark provider configuration could not be loaded. "
                               "Check the selected provider or swarm pool in Settings.")
        snapshot = json.loads(output.read_text())
    if snapshot.get("version") != 1 or not all(
            isinstance(snapshot.get(key), dict) for key in ("config", "secrets", "custom_providers")):
        raise RuntimeError("Unsupported benchmark configuration snapshot")
    return snapshot


def cloud_env(provider: str, snapshot: dict) -> Dict[str, str]:
    if provider not in snapshot["providers"]:
        raise ValueError("Selected provider is absent from benchmark configuration")
    return snapshot_environment(snapshot)


def snapshot_environment(snapshot: dict) -> Dict[str, str]:
    # Local HTTP discovery reads LMSTUDIO_HOST from the environment, while provider
    # construction reads Config. Preserve the same selected values on both paths.
    values = {key: value for key, value in snapshot['config'].items()
              if re.fullmatch(r'[A-Z][A-Z0-9_]*', key) and isinstance(value, (str, int, float, bool))}
    values.update(snapshot['secrets'])
    return {key: value if isinstance(value, str) else json.dumps(value) for key, value in values.items()}


def google_model_limits(model: str, credentials: Dict[str, str]) -> Dict[str, int]:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", model):
        raise ValueError("Invalid Google model ID")
    req = urllib.request.Request(
        f"https://generativelanguage.googleapis.com/v1beta/models/{model}",
        headers={"x-goog-api-key": credentials["GOOGLE_API_KEY"]})
    with urllib.request.urlopen(req, timeout=30) as response:
        metadata = json.load(response)
    limits = {name: metadata.get(name) for name in ("inputTokenLimit", "outputTokenLimit")}
    if any(type(value) is not int or value <= 0 for value in limits.values()):
        raise ValueError("Google model metadata did not provide positive input/output token limits")
    return limits


# Providers whose engine runs on this machine's own fleet: the brief and the scorer's local arms
# never relied on cloud metadata, and their windows are probed by the engine itself.
LOCAL_ENGINE_PROVIDERS = frozenset({"lmstudio", "swarm"})
# goose parses GOOSE_MAX_TOKENS as an i32 (crates/goose/src/config/base.rs get_goose_max_tokens);
# a larger value is a config error in the child, so it is refused here, before any model call.
GOOSE_MAX_TOKENS_TYPE_BOUND = 2**31 - 1


def _positive_limit(value, what: str) -> int:
    if type(value) is not int or value <= 0:
        raise RuntimeError(f"REFUSED: {what} is {value!r}, not a positive integer token limit")
    return value


def _openrouter_model_limits(model: str, credentials: Dict[str, str]) -> dict:
    key = credentials.get("OPENROUTER_API_KEY")
    if not key:
        raise RuntimeError("REFUSED: OPENROUTER_API_KEY is absent from the benchmark snapshot, so the "
                           "model's limits cannot be read")
    # The same host the engine's OpenRouter provider calls (OPENROUTER_HOST, default openrouter.ai),
    # so the limits describe the endpoint the run actually uses.
    host = credentials.get("OPENROUTER_HOST") or "https://openrouter.ai"
    url = host.rstrip("/") + "/api/v1/models"
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {key}"})
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            listing = json.load(response)
    except (OSError, ValueError) as error:
        raise RuntimeError(f"REFUSED: OpenRouter model metadata unreadable at {url}: "
                           f"{type(error).__name__}") from None
    entries = listing.get("data") if isinstance(listing, dict) else None
    if not isinstance(entries, list):
        raise RuntimeError(f"REFUSED: OpenRouter model listing at {url} carries no data array")
    matches = [entry for entry in entries if isinstance(entry, dict) and entry.get("id") == model]
    if len(matches) != 1:
        raise RuntimeError(f"REFUSED: OpenRouter lists {len(matches)} models with id exactly {model!r} "
                           f"(of {len(entries)}); the run's limits cannot be measured")
    entry = matches[0]
    top = entry.get("top_provider") if isinstance(entry.get("top_provider"), dict) else {}
    # A null max_completion_tokens was measured on 7 of 464 listed models on 2026-10-01, all of them
    # routers (openrouter/auto, openrouter/free, ...), never on a frontier model, so it is refused
    # rather than derived from the context window.
    context = _positive_limit(entry.get("context_length"), f"OpenRouter context_length for {model}")
    output = _positive_limit(top.get("max_completion_tokens"),
                             f"OpenRouter top_provider.max_completion_tokens for {model}")
    return {"context": context, "output": output,
            "provenance": {"source": "openrouter-model-metadata", "url": url,
                           "fields": {"context_length": context,
                                      "top_provider.max_completion_tokens": output}}}


def provider_model_limits(provider: str, model: str, credentials: Dict[str, str]) -> dict | None:
    """The context window and output cap a cloud entrant runs with, from a NAMED source.

    goose's canonical catalog lags new models, and an unknown model silently gets the engine's
    128,000 context fallback and the provider's own default output cap (Bedrock: 4,096). Measured on
    fable-5.1-r0: 7 of 22 calls ended at exactly 4,096 completion tokens with truncated write
    arguments, and compaction fired at 0.8 x 128k on a 1M-context model. So a cloud entrant gets
    limits from the operator (BENCH_CONTEXT_LIMIT/BENCH_MAX_TOKENS, which win everywhere) or from
    provider metadata (google, openrouter), recorded with their provenance. Bedrock without operator
    values is refused before the model is called: that is the measured harm (fable-5.1, gpt-6-astra).
    Every other provider proceeds on goose's own limits and the record says so by name (gate 1:
    a loud absence, never a silent default). Local fleets return None.
    """
    if provider in LOCAL_ENGINE_PROVIDERS:
        return None
    explicit = {name: os.environ.get(name) for name in ("BENCH_CONTEXT_LIMIT", "BENCH_MAX_TOKENS")}
    if any(explicit.values()):
        missing = [name for name, value in explicit.items() if not value]
        if missing:
            raise RuntimeError(f"REFUSED: {', '.join(missing)} unset; explicit limits need both "
                               "BENCH_CONTEXT_LIMIT and BENCH_MAX_TOKENS")
        parsed = {}
        for name, value in explicit.items():
            try:
                parsed[name] = int(value)
            except ValueError:
                parsed[name] = value
        context = _positive_limit(parsed["BENCH_CONTEXT_LIMIT"], "BENCH_CONTEXT_LIMIT")
        output = _positive_limit(parsed["BENCH_MAX_TOKENS"], "BENCH_MAX_TOKENS")
        provenance = {"source": "operator-env", "fields": explicit}
    elif provider == "google":
        google = google_model_limits(model, credentials)
        context, output = google["inputTokenLimit"], google["outputTokenLimit"]
        provenance = {"source": "google-model-metadata",
                      "url": f"https://generativelanguage.googleapis.com/v1beta/models/{model}",
                      "fields": google}
    elif provider == "openrouter":
        measured = _openrouter_model_limits(model, credentials)
        context, output, provenance = measured["context"], measured["output"], measured["provenance"]
    elif provider == "aws_bedrock":
        raise RuntimeError(
            f"REFUSED: no source for {provider}/{model}'s context window and output cap. goose would "
            "run it on its 128,000-token context fallback and Bedrock's default 4,096-token output "
            "cap (measured on fable-5.1-r0). Export BENCH_CONTEXT_LIMIT and BENCH_MAX_TOKENS from the "
            "model's documentation and launch again.")
    else:
        configured = {name: credentials[name] for name in ("GOOSE_CONTEXT_LIMIT", "GOOSE_MAX_TOKENS")
                      if name in credentials}
        reason = (f"no limits metadata path for provider {provider}; goose falls back to its catalog "
                  "or DEFAULT_CONTEXT_LIMIT 128000, and to the provider's own default output cap")
        if configured:
            reason += f", except where goose's own config sets {', '.join(sorted(configured))}"
        record = {"status": "provider_default_unverified", "reason": reason,
                  "provider": provider, "model": model}
        if configured:
            record["goose_config"] = configured
        return record
    if output > GOOSE_MAX_TOKENS_TYPE_BOUND:
        raise RuntimeError(f"REFUSED: output cap {output} for {model} exceeds goose's i32 GOOSE_MAX_TOKENS")
    provenance = {**provenance, "provider": provider, "model": model,
                  "resolved_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    limits = {"GOOSE_CONTEXT_LIMIT": str(context), "GOOSE_MAX_TOKENS": str(output)}
    if provider == "openrouter":
        # MEASURED 2026-10-01: sending the listed maximum (943,718 for deepseek-v4.1-flash) shrinks
        # OpenRouter's backend pool to the hosts that accept it and removes the host's own output
        # bound — run openrouter-cloud-9e652114 streamed 32k chars of degenerate thinking on its first
        # request. With no max_tokens OpenRouter served anthropic/claude-haiku-4.5 (8,004 tokens) and
        # openai/gpt-6-luna (8,176) to a natural stop, so the 4,096 truncation that destroyed the
        # Bedrock runs does not exist on this lane. The context window is the limit goose was missing.
        del limits["GOOSE_MAX_TOKENS"]
        provenance["max_tokens_sent"] = credentials.get("GOOSE_MAX_TOKENS")
    replaced = {name: credentials[name] for name, value in limits.items()
                if name in credentials and credentials[name] != value}
    if replaced:
        provenance["replaced_goose_config"] = replaced
    return {**limits, "provenance": provenance}


BUILD_VENDOR_TRACE = "vendor-build-trace.jsonl"


def stop_vendor(server) -> None:
    server.shutdown()
    server.server_close()


def serve_scoring_vendor(build_server, scorer, vendor, port: int, seed: str, build_trace: Path,
                         run_trace: Path):
    """Grade against a FRESH vendor, exactly as the hermetic scorer CLIs do.

    The build vendor served the entrant's own session: its walks, test sends, webhook registrations
    and the scheduled faults they armed. Graded in place (run 43896353 in-app 0.699 against 0.799
    hermetic on the same tree and seed), its trace counted 253 build-session sync-1 pages into
    c_paged_walk ("445/192 pages served ... 192 duplicate pages"), the session's own drop and 500
    into c_b1/c_b2, 28 of its sends into c_send_idempotency, and its last registered webhook — a
    test server already gone — took 18 of the graded deliveries ("57/75 deliveries 2xx-acked").
    The build traffic is kept beside the run as vendor-build-trace.jsonl; the run's trace.jsonl
    becomes the graded trace, header and seed included.
    """
    stop_vendor(build_server)
    shutil.copy2(build_trace, run_trace.parent / BUILD_VENDOR_TRACE)
    held = scorer._port_holder(port) if hasattr(scorer, "_port_holder") else None  # noqa: SLF001
    if held:
        raise RuntimeError(f"REFUSED: the scoring vendor cannot bind: {held}")
    return vendor.serve(port, run_trace, seed=seed)


def run(entrant: str, rep: int, out_root: Path, timeout: int, port: int,
        provider: str | None = None, model: str | None = None) -> Dict:
    workdir = out_root / f"{entrant}-r{rep}"
    if workdir.exists():
        raise FileExistsError(f"Benchmark tree already exists; preserve it and choose a new entrant: {workdir}")
    tier = isolated_tiers.active()
    # A malformed wallet limit refuses here, before a tree, a vendor or a model call exists.
    bench_budget.wallet_limit()
    snapshot = entrant_config(provider) if provider or tier else None
    credentials = (cloud_env(provider, snapshot) if provider else
                   snapshot_environment(snapshot) if snapshot else load_env())
    # A MODELS entrant is a Bedrock cloud model launched without --provider (invoke()'s second arm),
    # and fable-5.1-r0, the receipt for provider_model_limits, ran on exactly that arm.
    limits_provider = provider or ("aws_bedrock" if entrant in MODELS else None)
    limits_model = model or MODELS.get(entrant)
    model_limits = (provider_model_limits(limits_provider, limits_model, credentials)
                    if limits_provider else None)
    if model_limits and "GOOSE_CONTEXT_LIMIT" in model_limits:
        credentials = {**credentials, **{name: model_limits[name] for name in
                                         ("GOOSE_CONTEXT_LIMIT", "GOOSE_MAX_TOKENS") if name in model_limits}}
    elif model_limits:
        print(f"MODEL LIMITS UNVERIFIED: {limits_provider}/{limits_model} — {model_limits['reason']}. "
              "Set BENCH_CONTEXT_LIMIT and BENCH_MAX_TOKENS to pin them.", flush=True)
    workdir.mkdir(parents=True)
    if model_limits:
        (workdir / "model-limits.json").write_text(json.dumps(model_limits, indent=2))
    # F811 (Mihai: "next time we pause we won't lose anything"): RESUME A KILLED UNIT. When the
    # sweep points BENCH_RESUME_FROM at the voided unit's dir (same engine binary only — the
    # sweep enforces that), restore the partial tree + the run's own .swarm logs and arm the
    # engine's GOOSE_SWARM_RESUME: the engine reloads the plan from its log (skipping the
    # ~20-min prologue) and re-runs tasks against the warm tree. Deliberately re-runs rather
    # than trusts unfinished tasks — the engine's own resume semantics.
    # STATUS 2026-08-22: RESUME WORKS. It was broken from the day it shipped — the engine rebuilt
    # the recovered plan under "tasks" while its parser required "subtasks", so every resume died
    # with 'missing field `subtasks`' AFTER paying the full scout phase and this harness then
    # scored the unbuilt tree. Fixed in the engine (4c9530e70, test a_resumed_plan_parses_into_a_dag).
    # Do not read the older FINDINGS entries and conclude it is still broken.
    # REMAINING TRAP (not fixed): the bridge below reads <prev>/run.jsonl, but this bench passes
    # --log-file as a workdir-relative path that lands NESTED at
    # <tree>/runs/<out>/<entrant>-rN/run.jsonl — copy that nested log to <prev>/run.jsonl first or
    # the bridge silently finds nothing and the engine re-plans from scratch.
    resume_from = os.environ.get("BENCH_RESUME_FROM", "")
    if resume_from:
        prev = Path(resume_from)
        if prev.is_dir():
            for child in prev.iterdir():
                if child.name in {"graded.db", "verdict.json", "process.json",
                                  "nodeloop-result.json", "heartbeat", "vendor-trace.jsonl",
                                  "run.jsonl", "trace.jsonl", "__pycache__"}:
                    continue
                if child.is_dir():
                    shutil.copytree(child, workdir / child.name, dirs_exist_ok=True)
                else:
                    shutil.copy2(child, workdir / child.name)
            # The bench names the engine log run.jsonl (--log-file), but the engine's resume
            # reader globs .swarm/run-swarm-*.jsonl — bridge the name or resume NEVER finds its
            # history (measured: the first live resume re-planned the full 23-minute prologue).
            prev_log = prev / "run.jsonl"
            if prev_log.is_file():
                sw = workdir / ".swarm"
                sw.mkdir(exist_ok=True)
                shutil.copy2(prev_log, sw / "run-swarm-00-resumed.jsonl")
            os.environ["GOOSE_SWARM_RESUME"] = "1"
    # BENCH3 (BENCH3-AMEND.md): the brownfield mode. When the arm exports BENCH_SEED_TREE, the
    # unit starts from a COPY of that base app instead of an empty directory — the engine's
    # amendment path (working_dir_has_sources) then sees real sources. Greenfield arms are
    # byte-identical: no env, no copy.
    seed = os.environ.get("BENCH_SEED_TREE", "")
    if seed:
        base = Path(seed)
        if not base.is_dir():
            raise SystemExit(f"BENCH_SEED_TREE does not exist: {seed}")
        for child in base.iterdir():
            if child.name in {"__pycache__", ".swarm", "graded.db", "verdict.json",
                              "process.json", "run.jsonl", "vendor-trace.jsonl", "trace.jsonl",
                              "nodeloop-result.json", "heartbeat"}:
                continue
            if child.is_dir():
                shutil.copytree(child, workdir / child.name)
            else:
                shutil.copy2(child, workdir / child.name)
    if tier:
        if resume_from or seed:
            raise RuntimeError(f"REFUSED: {tier.version} starts from its public starter only")
        starter = ROOT / tier.starter
        shutil.copytree(starter, workdir, dirs_exist_ok=True,
                        ignore=shutil.ignore_patterns('__pycache__', '*.pyc', '.DS_Store'))
        for name, source in tier.public:
            text = render_public_contract((ROOT / source).read_text(), port, _regime()[1])
            (workdir / name).write_text(text)
        (workdir / "benchmark-prompt.md").write_text(build_prompt(port))
        shutil.copy2(HERE / 'browser-self-test.mjs', workdir / 'browser-self-test.mjs')
        (workdir / 'BROWSER-TESTING.md').write_text(
            '# Browser self-testing\n\n'
            'Python 3, Node, Playwright and a headless Chromium browser are supplied. '
            'Start your app with its documented command, then run '
            '`node browser-self-test.mjs http://127.0.0.1:PORT screenshot.png`. '
            'The helper reports page errors and saves a screenshot. '
            'You may modify it or write your own Playwright tests. '
            'Load Playwright with `require(process.env.BENCH_BROWSER_MODULE)` and launch Chromium '
            'with `executablePath: process.env.BENCH_BROWSER_EXECUTABLE`. '
            'These paths work inside the same isolation boundary as your app.\n')
        manifest = {str(path.relative_to(workdir)): hashlib.sha256(path.read_bytes()).hexdigest()
                    for path in workdir.rglob('*') if path.is_file()}
        (workdir / "input-manifest.json").write_text(json.dumps(manifest, indent=2))
    trace = out_root / f"trace-{entrant}-r{rep}.jsonl"

    scorer, vendor, _spec = _regime()
    # sb-7 needs ONE fixture seed shared by the serving vendor and the scorer — gather(seed=None)
    # builds no fixtures, which silently voids the expectation pack, the probe tokens and the
    # kill placements (the haiku canary measured exactly that: every pack-dependent probe
    # reported "harness failure"). Same hermetic wipes as score_sb7's own CLI.
    sb8 = bool(os.environ.get("BENCH_SB8"))
    sb7 = bool(os.environ.get("BENCH_SB7") or tier) and not sb8
    seeded = sb7 or sb8
    seed = None
    # REFUSE BEFORE BIND. vendor.serve() is a bare ThreadingHTTPServer: a held port is a traceback
    # in the launching shell and an engine that never starts (2026-08-29: an archive rescore held
    # 8850 while a launch was being prepared). And a probe that cannot load playwright grades a third
    # of the checks PROBE-UNAVAILABLE and blinds the engine's own render gate the same way -- the
    # same refuse-before-grade rule the scorer CLI applies. Not a model cap: nothing has started.
    held = scorer._port_holder(port) if hasattr(scorer, "_port_holder") else None  # noqa: SLF001
    if held:
        raise SystemExit(f"REFUSED: {held}. Stop it, or launch with --port <free port>.")
    if seeded and hasattr(scorer, "_probe_preflight") and not os.environ.get("BENCH_ALLOW_BLIND_PROBE"):
        why = scorer._probe_preflight()  # noqa: SLF001
        if why:
            raise SystemExit("REFUSED: the browser probe cannot run, so the render gate and a third "
                             "of the checks would be blind. Point GOOSE_SWARM_RENDER_NODE at a node "
                             "with playwright (npm root -g), or set BENCH_ALLOW_BLIND_PROBE=1 on "
                             f"purpose.\n  ✗ {why}")
    if seeded:
        seed = scorer._draw_seed()  # noqa: SLF001 — the scorer owns seed policy
        for leftover in ("sb7-tokens.json", "sb7-expect.json"):
            (workdir / leftover).unlink(missing_ok=True)
        for leftover_dir in ("graded-sb7-db", "sb7-empty-db", "sb7-combined-db", "sb7-shots"):
            shutil.rmtree(workdir / leftover_dir, ignore_errors=True)
        server = vendor.serve(port, trace, seed=seed)
    else:
        server = vendor.serve(port, trace)
    # THE TRACE LIVES WITH THE RUN. trace-<entrant>-r<rep>.jsonl is keyed by the run DIR name, which
    # the Benchmark view reuses across runs — so serve()'s truncate makes every run overwrite the
    # previous run's trace (r2's overwrote r0's; the fixture seed survived only in its ledger row).
    # serve() has already written the header row carrying fixture_seed, so this copy pins the seed to
    # the run even if it dies mid-flight; the finally refreshes it complete once the vendor stops.
    # The out_root path stays untouched for every existing reader, and a one-line .jsonl at the tree
    # root is invisible to the engine (its source manifest collects only .py) — invocation unchanged.
    run_trace = workdir / "trace.jsonl"
    shutil.copy2(trace, run_trace)
    # Quality screenshots (product contract 2026-08-17): the browser probe leaves
    # <epoch>-<scenario>.png in here on every render-gate pass DURING the run and again at
    # scoring — the repair-progression evidence the published post carries. Set in our own
    # environ too so score_build's in-process probe runs inherit it.
    os.environ["BENCH_SHOTS_DIR"] = str(workdir / "bench-shots")
    scoring_vendor = False
    try:
        print('BENCH_PHASE ' + json.dumps({'phase': 'build'}), flush=True)
        agent = invoke(entrant, workdir, port, credentials, timeout, provider, model, snapshot)
        # A budget stop (the call budget or the operator's wallet guard) is the run's SCORED end: the
        # harness ended it, so neither refusal below may read its console as a provider failure.
        harness_stop = bench_budget.is_harness_stop(agent)
        if provider and not harness_stop and \
                "The model returned an empty response. Please resend your message to continue." in agent["tail"]:
            (workdir / "incomplete-agent.json").write_text(json.dumps(agent, indent=2))
            raise RuntimeError("REFUSED: provider ended on empty responses; no completed benchmark artifact")
        # Same treatment as the empty-response ending above: a session the PROVIDER ended is no
        # completed artifact, and grading its half-built tree would publish an infrastructure fault
        # as the model's score. goose resends a transient failure under the provider's retry policy
        # first, so this fires only once that policy is spent or the failure is permanent.
        ended_on = provider_error_ending(agent["tail"]) if provider and not harness_stop else None
        if ended_on:
            agent["ended_on_provider_error"] = ended_on
            (workdir / "incomplete-agent.json").write_text(json.dumps(agent, indent=2))
            print(f"ENDED ON PROVIDER ERROR (not scored): {ended_on}", flush=True)
            raise RuntimeError("REFUSED: the session ended on a provider error, not on the model's "
                               f"finished work: {ended_on.splitlines()[0]}")
        completion_path = os.environ.get("BENCH_COMPLETION_RECEIPT")
        if tier and completion_path and (agent.get("exit") == 0 or
                                         (agent.get("budget") or {}).get("stopped_by") == "wallet_guard"):
            from bench_rescore import write_completion
            try:
                run_id = os.environ.get("BENCH_RUN_ID")
                if not run_id:
                    run_id = json.loads((workdir / ".swarm/current-run.json").read_text())["run_id"]
                write_completion(workdir, Path(completion_path), agent, run_id=run_id,
                                 started_at=os.environ.get("BENCH_STARTED_AT"), seed=seed,
                                 port=port, provider=provider, model=model, tier=tier)
            except (ValueError, OSError, KeyError) as error:
                print(f"Scoring retry unavailable: {error}", file=sys.stderr, flush=True)
        db = workdir / ("graded-sb8-db" if sb8 else "graded-sb7-db" if sb7 else "graded.db")
        scoring_started = time.monotonic()
        print('BENCH_PHASE ' + json.dumps({'phase': 'score'}), flush=True)
        graded_trace = trace
        if seeded:
            server = serve_scoring_vendor(server, scorer, vendor, port, seed, trace, run_trace)
            scoring_vendor = True
            graded_trace = run_trace
        ctx = scorer.gather(workdir, port, db, graded_trace,
                            mark_phase=vendor.mark_phase,
                            **({"seed": seed} if seeded else {}))
        scoring_seconds = round(time.monotonic() - scoring_started, 3)
    finally:
        stop_vendor(server)
        # Refresh the run's copy now the vendor has stopped appending (record() is write-through,
        # so the file is whole by the time shutdown returns) — the _sb4trees archive below then
        # carries the complete request log, not just the header. Once a scoring vendor served,
        # the run's copy IS the graded trace and the shared out_root name receives it.
        try:
            if scoring_vendor:
                shutil.copy2(run_trace, trace)
            else:
                shutil.copy2(trace, run_trace)
        except OSError:
            pass

    verdict = scorer.evaluate(ctx)
    verdict["scorer_seconds"] = scoring_seconds
    verdict["scoring"] = {"secs": scoring_seconds}
    if tier:
        verdict["starter_assisted"] = True
        verdict["input_manifest"] = manifest
    if provider:
        verdict["provider"] = provider
        verdict["model"] = model
    if provider or model_limits:
        verdict["model_limits"] = model_limits
    if "reaped_processes" in agent:
        verdict["reaped_processes"] = agent["reaped_processes"]
    if "budget" in agent:
        verdict["budget"] = agent["budget"]
    # BENCH2/F769: ARCHIVE THE TREE at score time — the sweep wipes the workdir within seconds
    # of [done] (dir reuse), which has already cost the campaign the 0.996 tree and the first
    # two dual-score windows. A tree copy is ~100KB and makes every scored artifact a permanent
    # forensic object (and the sb-3/sb-4 bridge feasible offline).
    try:
        dest = workdir.parent / "_sb4trees" / f"{workdir.name}-{int(time.time())}"
        dest.parent.mkdir(exist_ok=True)
        shutil.copytree(workdir, dest, ignore=shutil.ignore_patterns(
            ".swarm", "__pycache__", ".pytest_cache", "*.pyc"))
        # The verdict rides WITH its tree — nodeloop-result.json is written to the workdir
        # AFTER this block and dies in the next wipe; the first archive proved it (F771's
        # ledger gap). The archive is only a forensic object if it carries its own scoring.
        (dest / "sb4-verdict.json").write_text(json.dumps(verdict, indent=1, default=str))
    except Exception as e:
        print(f"tree-archive failed (non-fatal): {e}", file=sys.stderr)
    # The pool the run REALLY used, straight from run_started. A label like "swarm-3node" is an
    # intention; this is the fact, and a mismatch invalidates any node-scaling claim.
    actual_pool = None
    run_log = workdir / "run.jsonl"
    if run_log.is_file():
        for line in run_log.read_text(errors="replace").splitlines():
            try:
                e = json.loads(line)
            except Exception:
                continue
            if e.get("event") == "run_started":
                actual_pool = [d.get("model_id") for d in (e.get("pool") or [])]
                break
    verdict.update({"entrant": entrant, "rep": rep, "agent": agent,
                    "actual_pool": actual_pool,
                    "actual_nodes": len(actual_pool) if actual_pool is not None else None,
                    # The vendor port the spec's {BASE_URL} rendered with — any later
                    # standalone re-score MUST re-serve the vendor on this port, or apps
                    # that (legitimately) hardcoded the advertised URL grade as dead.
                    "vendor_port": port})
    (workdir / "verdict.json").write_text(json.dumps(verdict, indent=2))
    return verdict


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--entrant", default="opus-5")
    ap.add_argument("--provider", help="Configured provider ID for a single agent instead of a swarm")
    ap.add_argument("--model", help="Exact cloud model ID")
    ap.add_argument("--reps", type=int, default=1)
    ap.add_argument("--only-rep", type=int,
                    help="run exactly this rep index instead of 0..reps-1")
    ap.add_argument("--timeout", type=int, default=1800)
    ap.add_argument("--port", type=int, default=8850)
    ap.add_argument("--out", type=Path, default=ROOT / "runs/build")
    ap.add_argument("--sb6", action="store_true",
                    help="sb-6 regime: spec-build-v3 + vendor_service_v2 + score_sb6 "
                         "(equivalent to BENCH_SB6=1; default path stays byte-identical)")
    ap.add_argument("--sb7", action="store_true",
                    help="sb-7 regime: spec-build-sb7 + vendor_service_v3 + score_sb7 "
                         "(equivalent to BENCH_SB7=1; wins over --sb6)")
    ap.add_argument("--sb8", action="store_true", help="SB-8 compact transactional 3D benchmark")
    ap.add_argument("--sb71", action="store_true", help="SB7.1 payments landscape with isolated public starter")
    ap.add_argument("--sb72", action="store_true",
                    help="SB7.2: SB7.1's product and starter, 3D-weighted scorer, framed and legible overview")
    args = ap.parse_args()
    if bool(args.provider) != bool(args.model):
        ap.error("--provider and --model must be supplied together")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", args.entrant):
        ap.error("--entrant must be a directory-safe identifier")
    if args.sb6:
        os.environ["BENCH_SB6"] = "1"
    if args.sb7:
        os.environ["BENCH_SB7"] = "1"

    if args.sb8:
        os.environ["BENCH_SB8"] = "1"
    if args.sb71:
        os.environ["BENCH_SB71"] = "1"
    if args.sb72:
        os.environ["BENCH_SB72"] = "1"
    isolated_tiers.active()

    verdicts = []
    reps = [args.only_rep] if args.only_rep is not None else list(range(args.reps))
    for rep in reps:
        v = run(args.entrant, rep, args.out, args.timeout, args.port + rep, args.provider, args.model)
        verdicts.append(v)
        print(_regime()[0].format_report(
            v, f"{args.entrant} rep{rep} ({v['agent']['secs']}s)"), flush=True)
        print()

    if len(verdicts) > 1:
        scores = [100 * v["score"] for v in verdicts]
        print(f"spread {min(scores):.1f}% – {max(scores):.1f}%  "
              f"mean {sum(scores) / len(scores):.1f}%")
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / f"{args.entrant}.json").write_text(json.dumps(verdicts, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
