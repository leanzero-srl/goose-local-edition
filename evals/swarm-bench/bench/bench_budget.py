"""The benchmark budget: one call budget for every single-model entrant, and the operator's wallet guard.

Owner, 2026-10-02, after a GPT-6 Luna run on SB7.2 spent 3 h, 309 model calls and $9: asked whether to
"give every model the same budget — a call cap, after which the harness stops it and scores what's
there; plus a dollar guard", he answered "ok do it". This is a rule of the CLOUD BENCHMARK HARNESS only.
The swarm engine's NO CAPS invariant (AGENTS.md) is untouched: swarm entrants run without a budget.

CALL BUDGET (published, the same for every entrant of a tier): an isolated tier passes `--max-turns
<tier.call_budget>` to `goose run` (isolated_tiers.py; CALL_BUDGET unless the tier publishes its own, as
forge-2.0 does). goose counts one turn per model call in its reply loop (crates/goose/src/agents/agent.rs:
`turns_taken > max_turns` yields MAX_TURNS_MESSAGE and breaks); a headless `-t` run then exits 0 without
asking for input. Measured 2026-10-02 with anthropic/claude-haiku-4.5 on OpenRouter and --max-turns 3:
exit 0 after 5.3 s, three tool calls made, the console ending on MAX_TURNS_MESSAGE, three entrant
calls in the telemetry file plus one session-title call on the fast model. goose does not count a
resend after an empty reply or a transient provider error as a turn, so calls_used (provider calls
the telemetry file recorded) can exceed max_calls by exactly those resends.

WALLET GUARD (operator safety, optional): BENCH_MAX_USD arms it. While an OpenRouter entrant runs, the
guard reads each new generation id from the run's telemetry sink, bills it through OpenRouter's
/api/v1/generation (bench_cost's reader), and when the billed sum reaches the limit it sends SIGTERM to
the entrant's own pid (gate 4: never a group signal) and the harness scores what exists. The bill can
pass the limit by the call in flight when it trips.
"""
from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from decimal import Decimal, InvalidOperation
import json
import os
from pathlib import Path
import re
import signal
import sys
import threading
import time
import urllib.parse

import bench_cost

# policy: the published SB7.1/SB7.2 (and forge-1.0) call budget, the same for every single-model entrant.
# Owner, 2026-10-02: "ok do it". Receipt: the GPT-6 Luna run that prompted it made 309 calls in 3 h for $9;
# 150 calls is the budget he approved. A tier that publishes another number carries it as call_budget.
CALL_BUDGET = 150
# The sentence goose's reply loop writes when the turn budget is spent (agent.rs MAX_TURNS_MESSAGE;
# test_bench_budget pins it to the source).
MAX_TURNS_MESSAGE = ("I've reached the maximum number of actions I can do without user input. "
                     "Would you like me to continue?")
WALLET_ENV = 'BENCH_MAX_USD'
# Transport cadence, not a bound on model work: OpenRouter writes a generation record after the
# stream ends and bench_cost's late-record policy first re-asks after this long, so polling faster
# finds no new record to bill.
WALLET_POLL_SECONDS = bench_cost.LATE_RECORD_BACKOFF[0]
STATED_BUDGET = re.compile(r'budget of\s+([0-9][0-9,]*)\s+model calls', re.IGNORECASE)


def call_budget_args(tier) -> list[str]:
    return ['--max-turns', str(tier.call_budget)]


def stated_budgets(text: str) -> list[int]:
    """Every "budget of N model calls" a public contract states, so a test can hold it to its tier's budget."""
    return [int(match.replace(',', '')) for match in STATED_BUDGET.findall(text)]


def wallet_limit(environ=None) -> Decimal | None:
    """The operator's dollar limit, or None when unset. A value that is not a positive amount is
    refused before anything starts: a run the operator meant to guard never starts unguarded."""
    raw = (environ if environ is not None else os.environ).get(WALLET_ENV, '').strip()
    if not raw:
        return None
    try:
        limit = Decimal(raw.removeprefix('$'))
    except InvalidOperation:
        limit = None
    if limit is None or not limit.is_finite() or limit <= 0:
        raise RuntimeError(f'REFUSED: {WALLET_ENV}={raw!r} is not a positive dollar amount')
    return limit


def _telemetry_lines(path: Path, offset: int) -> tuple[list[dict], int]:
    """Whole JSON lines appended since offset, and the offset after the last whole line."""
    try:
        with path.open('rb') as handle:
            handle.seek(offset)
            chunk = handle.read()
    except FileNotFoundError:
        return [], offset
    end = chunk.rfind(b'\n')
    if end < 0:
        return [], offset
    entries = []
    for line in chunk[:end].splitlines():
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if isinstance(entry, dict):
            entries.append(entry)
    return entries, offset + end + 1


class WalletGuard(threading.Thread):
    """Bills the entrant's OpenRouter calls while it runs and stops its pid at the operator's limit."""

    def __init__(self, process, telemetry: Path, limit: Decimal, key: str, host: str,
                 urlopen=None, poll_seconds: float | None = None, grace_seconds: float = 5.0):
        super().__init__(name='bench-wallet-guard', daemon=True)
        self.process = process
        self.telemetry = telemetry
        self.limit = limit
        self.key = key
        self.base = host.rstrip('/') + '/api/v1/generation?id='
        self.urlopen = urlopen
        self.poll_seconds = WALLET_POLL_SECONDS if poll_seconds is None else poll_seconds
        # Teardown, not model work: the reaper's measured grace (run_build.reap_entrant_survivors).
        self.grace_seconds = grace_seconds
        self.finished = threading.Event()
        self.offset = 0
        self.billed: dict[str, Decimal] = {}
        self.unbilled: dict[str, str] = {}
        self.tripped: dict | None = None
        self.error: str | None = None

    def spent(self) -> Decimal:
        return sum(self.billed.values(), Decimal(0))

    def _poll(self) -> None:
        entries, self.offset = _telemetry_lines(self.telemetry, self.offset)
        for entry in entries:
            gen = entry.get('response_id')
            if isinstance(gen, str) and bench_cost.GENERATION_ID.fullmatch(gen) and gen not in self.billed:
                self.unbilled.setdefault(gen, 'not fetched')
        pending = list(self.unbilled)
        if not pending:
            return
        fetch = lambda gen: bench_cost._fetch(self.base + urllib.parse.quote(gen), self.key,  # noqa: E731,SLF001
                                              self.urlopen or bench_cost.urllib.request.urlopen)
        with ThreadPoolExecutor(max_workers=bench_cost.CONCURRENT_READS) as pool:
            answers = list(pool.map(fetch, pending))
        for gen, (data, failure) in zip(pending, answers):
            if data is None:
                self.unbilled[gen] = failure
            else:
                self.billed[gen] = Decimal(data['usage'])
                del self.unbilled[gen]

    def _stop_entrant(self) -> None:
        if self.process.poll() is not None:
            return  # it ended on its own first; the guard stopped nothing
        pid = self.process.pid
        spent = self.spent()
        print(f'WALLET GUARD: OpenRouter billed ${spent} of the ${self.limit} limit; '
              f'stopping entrant pid {pid} (SIGTERM)', file=sys.stderr, flush=True)
        self.tripped = {'spent_usd': float(spent), 'calls_billed': len(self.billed),
                        'signalled_pid': pid, 'signal': 'SIGTERM', 'at': time.time()}
        os.kill(pid, signal.SIGTERM)
        waited, step = 0.0, self.grace_seconds / 100  # ratio: one hundredth of the grace per check
        while waited < self.grace_seconds and self.process.poll() is None:
            time.sleep(step)
            waited += step
        if self.process.poll() is None:
            os.kill(pid, signal.SIGKILL)
            self.tripped['signal'] = 'SIGKILL'

    def run(self) -> None:
        try:
            while True:
                exited = self.finished.wait(self.poll_seconds)
                self._poll()
                if exited:
                    return
                if self.spent() >= self.limit:
                    self._stop_entrant()
                    return
        except Exception as error:  # the guard's failure is named in the record, never a silent no-guard
            reason = f'{type(error).__name__}: {error}'.replace(self.key, '[REDACTED]')
            self.error = reason
            print(f'WALLET GUARD FAILED (the run continues unguarded): {reason}', file=sys.stderr, flush=True)

    def stop(self) -> None:
        """Called once the entrant has exited: one last read bills its final calls."""
        self.finished.set()
        self.join()

    def record(self) -> dict:
        result = {'status': 'tripped' if self.tripped else 'failed' if self.error else 'armed',
                  'provider': 'openrouter', 'max_usd': float(self.limit),
                  'spent_usd_seen': float(self.spent()), 'calls_billed': len(self.billed)}
        if self.tripped:
            result['tripped'] = self.tripped
        if self.error:
            result['reason'] = self.error
        if self.unbilled:
            result['unbilled'] = [{'id': gen, 'reason': why} for gen, why in self.unbilled.items()]
        return result


def wallet_unavailable(limit: Decimal, provider: str | None, reason: str) -> dict:
    print(f'WALLET GUARD UNAVAILABLE ({provider or "swarm"}): {reason}. The ${limit} limit is not '
          'enforced on this run.', file=sys.stderr, flush=True)
    return {'status': 'unavailable', 'provider': provider, 'max_usd': float(limit), 'reason': reason}


def entrant_calls(telemetry: Path, model: str | None) -> dict:
    """The provider calls the run's telemetry recorded for the entrant's model, and for any other."""
    if not telemetry.is_file():
        return {'calls_used': None, 'calls_source': 'unavailable: no telemetry sink after the entrant exited'}
    entries, _ = _telemetry_lines(telemetry, 0)
    if not entries:
        return {'calls_used': None,
                'calls_source': 'unavailable: the provider wrote no per-call telemetry for this run'}
    others: dict[str, int] = {}
    used = 0
    for entry in entries:
        if entry.get('model') == model:
            used += 1
        else:
            name = str(entry.get('model'))
            others[name] = others.get(name, 0) + 1
    record = {'calls_used': used, 'calls_source': 'telemetry (provider calls on the entrant model)'}
    if others:
        # goose's session-title call runs on the fast model; billed, but not the entrant's work.
        record['calls_other_models'] = others
    return record


def stopped_by(agent: dict, wallet: dict | None) -> str:
    if wallet and wallet.get('status') == 'tripped':
        return 'wallet_guard'
    if agent.get('exit') == 0 and agent.get('tail', '').rstrip().endswith(MAX_TURNS_MESSAGE):
        return 'call_budget'
    if agent.get('exit') == 0:
        return 'model_finished'
    # Neither the model nor the harness ended it: the engine exited on its own non-zero status.
    return 'engine_exit'


def is_harness_stop(agent: dict) -> bool:
    """A budget stop is the run's scored end, never a provider failure to refuse."""
    return (agent.get('budget') or {}).get('stopped_by') in ('call_budget', 'wallet_guard')
