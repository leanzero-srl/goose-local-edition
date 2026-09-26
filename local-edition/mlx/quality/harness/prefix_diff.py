"""How much of a request's prompt an engine's prefix cache can reuse from the call before it —
rendered offline through the model's OWN chat template on CPU (the Q-135 method), no engine, no GPU.

    <mlx-venv>/bin/python prefix_diff.py --model <model dir> [--outside tools.json] req1.jsonl req2.jsonl ...

Each reqN is an llm_request log (goose writes ~/.local/state/goose/logs/llm_request.<n>.jsonl; the
first line's `input` is the request). Consecutive pairs are compared: the identical prefix in chars
and tokens, where it breaks (inside the tool block or after it), and the tokens left to prefill cold.

--outside <tools.json> ({extension: [MCP tools/list entries]}) adds two arms rebuilt from the same
messages (Q-107): `skeleton` — every outside tool declared from the first call as goose's
tool_deferral declares it (first sentence + schema without description/title/examples) and the
"Deferred tools" section it writes — and `full` — deferral off, every schema in full. The system
prompt of both is the recorded one with its "# Deferred tools" section replaced.
"""
import argparse
import copy
import json

from transformers import AutoTokenizer

DOC_KEYWORDS = {"description", "title", "examples"}
SCHEMA_MAPS = {"properties", "patternProperties", "$defs", "definitions", "dependentSchemas"}
SCHEMA_SUBS = {"items", "additionalProperties", "not", "if", "then", "else", "contains", "propertyNames",
               "unevaluatedProperties", "unevaluatedItems", "additionalItems", "anyOf", "oneOf", "allOf",
               "prefixItems"}


def bare(schema):
    """tool_deferral.rs `bare_schema`: documentation keywords out, schema positions walked only."""
    if isinstance(schema, list):
        return [bare(s) for s in schema]
    if not isinstance(schema, dict):
        return schema
    out = {}
    for key, value in schema.items():
        if key in DOC_KEYWORDS:
            continue
        if key in SCHEMA_MAPS and isinstance(value, dict):
            out[key] = {name: bare(sub) for name, sub in value.items()}
        elif key in SCHEMA_SUBS:
            out[key] = bare(value)
        else:
            out[key] = value
    return out


def first_sentence(description):
    lines = description.strip().splitlines()
    line = lines[0] if lines else ""
    i = line.find(". ")
    return line[: i + 1] if i >= 0 else line


def declared(name, description, schema):
    return {"type": "function", "function": {"name": name, "description": description, "parameters": schema}}


def section(extensions):
    """tool_deferral.rs `catalogue`, verbatim."""
    return (
        "\n\n# Deferred tools\n\nThe tools of " + ", ".join(extensions) + " are in your tool list with a "
        "one-sentence summary and bare parameters. Before the first call to one of them, call "
        "extensionmanager__load_tools with its name (or a query describing what you need) to read its full "
        "description and parameter notes, then call it by that exact name. Prefer one of them over a shell "
        "workaround when it does the job.\n"
    )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--outside")
    ap.add_argument("requests", nargs="+")
    args = ap.parse_args()
    tok = AutoTokenizer.from_pretrained(args.model)
    reqs = [json.loads(open(p).readline())["input"] for p in args.requests]

    def render(req):
        msgs = copy.deepcopy(req["messages"])
        for m in msgs:
            if m.get("content") is None:
                m["content"] = ""
            for tc in m.get("tool_calls") or []:
                a = tc["function"].get("arguments")
                if isinstance(a, str) and a:
                    tc["function"]["arguments"] = json.loads(a)
        kwargs = dict(req.get("chat_template_kwargs") or {})
        kwargs.setdefault("enable_thinking", False)
        return tok.apply_chat_template(msgs, tools=req.get("tools"), add_generation_prompt=True,
                                       tokenize=False, **kwargs)

    def common(a, b):
        n = 0
        for x, y in zip(a, b):
            if x != y:
                break
            n += 1
        return n

    def arm(label, rs):
        print(f"--- {label}")
        rendered = [render(r) for r in rs]
        tokens = [tok.encode(t) for t in rendered]
        print(f"  call 1: {len(rs[0].get('tools') or [])} tools, {len(rendered[0]):,} chars, {len(tokens[0]):,} tokens")
        for i in range(1, len(rs)):
            a, b, ta, tb = rendered[i - 1], rendered[i], tokens[i - 1], tokens[i]
            cc, ct = common(a, b), common(ta, tb)
            end = b.find("</tools>")
            where = "INSIDE the tool block" if 0 <= cc < end else "after the tool block"
            print(f"  call {i + 1}: {len(rs[i].get('tools') or [])} tools, {len(tb):,} tokens; identical prefix "
                  f"{ct:,} tokens ({ct / len(ta):.3f} of the call before), breaks {where}; cold {len(tb) - ct:,}")
            print(f"      breaks at: before {a[cc:cc + 70]!r} / after {b[cc:cc + 70]!r}")

    print("model:", args.model)
    arm("recorded", reqs)
    if not args.outside:
        return
    outside = json.load(open(args.outside))
    full, skel = [], []
    for ext, listed in outside.items():
        for t in listed:
            name = f"{ext}__{t['name']}"
            full.append(declared(name, t.get("description", ""), t["inputSchema"]))
            skel.append(declared(name, first_sentence(t.get("description", "")), bare(t["inputSchema"])))
    names = {t["function"]["name"] for t in full}
    core = [t for t in reqs[0]["tools"] if t["function"]["name"] not in names]

    def rebuilt(extra, prompt_tail):
        out = []
        for r in reqs:
            r = copy.deepcopy(r)
            system = r["messages"][0]["content"]
            cut = system.find("\n\n# Deferred tools")
            r["messages"][0]["content"] = (system[:cut] if cut >= 0 else system) + prompt_tail
            r["tools"] = sorted(core + extra, key=lambda t: t["function"]["name"])
            out.append(r)
        return out

    arm("skeleton (tool_deferral as of Q-107)", rebuilt(skel, section(sorted(outside))))
    arm("full (deferral off)", rebuilt(full, ""))
    size = lambda ts: sum(len(json.dumps(t, ensure_ascii=False)) for t in ts)
    print(f"\n{len(full)} outside tools: full {size(full):,} chars, skeleton {size(skel):,} "
          f"({size(skel) / size(full):.3f})")


if __name__ == "__main__":
    main()
