"""fixture.py <sessions.db> <out.json> — a census session as the transcript a unit test rebuilds: the
request, then each model response (its text and calls) with the results, the work dir spelled WORKDIR."""
import json
import sqlite3
import sys

db = sqlite3.connect(sys.argv[1])
rows = list(db.execute("select role, content_json from messages order by id"))
workdir = None
out = []
for role, content in rows:
    items = json.loads(content)
    for c in items:
        if c.get("type") == "text" and role == "user" and workdir is None:
            text = c["text"]
            workdir = text.split("Work only inside the directory ", 1)[1].split(". ", 1)[0]
    entry = {"role": role, "text": "", "calls": [], "results": []}
    for c in items:
        t = c.get("type")
        if t == "text":
            entry["text"] += c["text"]
        elif t == "toolRequest":
            v = c["toolCall"].get("value") or {}
            entry["calls"].append({"id": c["id"], "name": v.get("name"), "arguments": v.get("arguments")})
        elif t == "toolResponse":
            r = c["toolResult"]
            v = r.get("value") or {}
            text = " ".join(x.get("text", "") for x in v.get("content", []) if isinstance(x, dict))
            entry["results"].append({"id": c["id"], "text": text, "error": bool(r.get("status") != "success" or v.get("isError"))})
    out.append(entry)
blob = json.dumps(out, indent=1, ensure_ascii=False).replace(workdir, "WORKDIR")
open(sys.argv[2], "w").write(blob + "\n")
print(len(out), "messages", len(blob), "chars")
