"""export_rows.py <out.json> <label=sessions.db[:session,session]>... — messages rows as `sqlite3 -json`
gives them, the session id prefixed with the label so censuses from different roots never collide."""
import json
import sqlite3
import sys

out = []
for arg in sys.argv[2:]:
    label, spec = arg.split("=", 1)
    path, _, only = spec.partition(":")
    keep = set(only.split(",")) if only else None
    db = sqlite3.connect(path)
    db.row_factory = sqlite3.Row
    for row in db.execute(
        "select id, session_id, message_id, role, content_json, created_timestamp, metadata_json from messages order by id"
    ):
        if keep and row["session_id"] not in keep:
            continue
        r = dict(row)
        r["session_id"] = f"{label}/{r['session_id']}"
        out.append(r)
json.dump(out, open(sys.argv[1], "w"))
print(len(out), "rows")
