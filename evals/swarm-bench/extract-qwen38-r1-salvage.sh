#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 RUN_JSONL [SOURCE_LABEL]" >&2
  exit 2
fi

run_log=$1
source_label=${2:-evals/swarm-bench/runs/sb7-fleet38/swarm-3node-r1/runs/sb7-fleet38/swarm-3node-r1/run.jsonl}
run_sha=$(shasum -a 256 "$run_log" | awk '{print $1}')

jq -s --arg source_label "$source_label" --arg run_sha "$run_sha" '
  (map(select(.event == "task_completed" and .task_id == "test-webhook")) | first) as $transient
  | (map(select(.event == "run_finished")) | first) as $finished
  | ($finished.report.tasks | map(select(.task_id == "test-webhook")) | first) as $task
  | {
      source: {
        run_log_path: $source_label,
        run_log_sha256: $run_sha
      },
      transient_completion: {
        event: $transient.event,
        task_id: $transient.task_id,
        status: $transient.status,
        salvaged: $transient.salvaged,
        attempts: $transient.attempts,
        elapsed_ms: $transient.elapsed_ms,
        session_id: $transient.session_id,
        tool_calls: $transient.tool_calls,
        ts: $transient.ts,
        seq: $transient.seq
      },
      final_report: {
        event: $finished.event,
        task_id: $task.task_id,
        status: $task.status,
        salvaged: $task.salvaged,
        completion: $task.completion,
        listed_in_done: ($finished.report.done | index("test-webhook") != null),
        attempts: $task.attempts,
        elapsed_ms: $task.elapsed_ms,
        session_id: $task.session_id,
        tool_calls: $task.tool_calls,
        owns_nothing: $task.owns_nothing,
        ts: $finished.ts,
        seq: $finished.seq
      },
      engine_fact: "The transient event called the task salvaged, while the final report erased that fact and counted it as ordinary done."
    }
' "$run_log"
