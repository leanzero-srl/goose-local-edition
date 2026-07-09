#!/usr/bin/env python3
"""Reflect an exploratory swarm run as ONE user session in the goose desktop sidebar.

Design: SWARM-AS-MODEL-DESIGN.md §3 — the swarm is a real provider, so a run is reflected as a normal
two-message user session (user = brief, assistant = run summary). Workers stay Hidden; this is the single
user-visible session per run.

Row shape matches what the normal goose path writes, so the desktop's ACP `session/load` replay renders it:
  - messages.metadata_json = {"userVisible":true,"agentVisible":true}  (the ACP replay in
    crates/goose/src/acp/server/load_session.rs skips messages whose metadata.user_visible is false;
    a NULL column defaults to true in Rust but we write it explicitly so a reflected row is byte-shaped
    like a real one and never depends on that default).
  - created_timestamp in SECONDS (matches goose's own writes).

Idempotent: re-reflecting the same slug deletes the prior `explore-<slug>` session and its messages first
(exact-name match on the sessions table — never a filesystem glob), so retries don't pile up duplicates or
leave a half-hydrated row behind.
"""
import sqlite3, uuid, sys, time, os, json

DB = os.path.expanduser('~/.local/share/goose/sessions/sessions.db')

MSG_META = json.dumps({"userVisible": True, "agentVisible": True})
# A swarm session's model config — coherent with provider 'swarm' so goosed does not backfill a foreign one.
MODEL_CONFIG = json.dumps({
    "model_name": "swarm", "context_limit": None, "temperature": None,
    "max_tokens": None, "toolshim": False, "toolshim_model": None,
})


def reflect(slug, app_dir, brief, report_summary):
    sid = str(uuid.uuid4())
    name = f"explore-{slug}"
    now = int(time.time())
    con = sqlite3.connect(DB, timeout=10)
    try:
        con.execute("PRAGMA busy_timeout=8000")
        con.execute("PRAGMA foreign_keys=ON")
        # Idempotent re-reflect: drop any prior reflection of THIS exact slug (and its messages) first.
        old = [r[0] for r in con.execute("SELECT id FROM sessions WHERE name=?", (name,)).fetchall()]
        for oid in old:
            con.execute("DELETE FROM messages WHERE session_id=?", (oid,))
            con.execute("DELETE FROM sessions WHERE id=?", (oid,))
        con.execute(
            "INSERT INTO sessions "
            "(id,name,description,user_set_name,session_type,working_dir,provider_name,model_config_json,goose_mode) "
            "VALUES (?,?,?,?,?,?,?,?,?)",
            (sid, name, report_summary[:200], 1, 'user', app_dir, 'swarm', MODEL_CONFIG, 'auto'))

        def msg(role, text, t):
            con.execute(
                "INSERT INTO messages "
                "(message_id,session_id,role,content_json,created_timestamp,metadata_json) "
                "VALUES (?,?,?,?,?,?)",
                (str(uuid.uuid4()), sid, role,
                 json.dumps([{"type": "text", "text": text}]), t, MSG_META))

        msg('user', brief, now)
        msg('assistant', report_summary, now + 1)
        con.commit()
        return sid
    finally:
        con.close()


if __name__ == '__main__':
    # test/CLI: reflect_run.py <slug> <app_dir> <brief> <summary>
    sid = reflect(sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4])
    print(sid)
