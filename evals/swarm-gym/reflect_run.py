#!/usr/bin/env python3
"""Reflect an exploratory swarm run as ONE user session in the goose desktop sidebar.
INSERT-only (fresh uuid) so it can never corrupt existing rows. Workers stay Hidden; this is the single
user-visible session per run (design: SWARM-AS-MODEL-DESIGN.md §3)."""
import sqlite3, uuid, sys, time, os, json

DB = os.path.expanduser('~/.local/share/goose/sessions/sessions.db')

def reflect(slug, app_dir, brief, report_summary):
    sid = str(uuid.uuid4())
    name = f"explore-{slug}"
    now = int(time.time())
    con = sqlite3.connect(DB, timeout=10)
    try:
        con.execute("PRAGMA busy_timeout=8000")
        con.execute(
            "INSERT INTO sessions (id,name,description,user_set_name,session_type,working_dir,provider_name) "
            "VALUES (?,?,?,?,?,?,?)",
            (sid, name, report_summary[:200], 1, 'user', app_dir, 'swarm'))
        def msg(role, text, t):
            con.execute(
                "INSERT INTO messages (message_id,session_id,role,content_json,created_timestamp) VALUES (?,?,?,?,?)",
                (str(uuid.uuid4()), sid, role, json.dumps([{"type":"text","text":text}]), t))
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
