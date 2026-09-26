"""compare.py <ref.jsonl> <test.jsonl> — per request (by sequence position): cached tokens on each side,
the first divergent generated token, and at that position the reference's and the test's top-2 margins
and each side's logprob for the other's choice. Also the max |Δlogprob| of the chosen token over the
common prefix (how far apart the two computations are BEFORE they diverge)."""
import json
import sys

a = [json.loads(l) for l in open(sys.argv[1])]
b = [json.loads(l) for l in open(sys.argv[2])]


def lp_of(top, tok):
    for t, lp in top:
        if t == tok:
            return lp
    return None


def cached(r):
    return ((r.get("usage") or {}).get("prompt_tokens_details") or {}).get("cached_tokens")


same = 0
for ra, rb in zip(a, b):
    ta, tb = ra["tokens"], rb["tokens"]
    n = min(len(ta), len(tb))
    d = next((i for i in range(n) if ta[i] != tb[i]), None)
    pre = range(d if d is not None else n)
    dmax = max((abs(ra["lps"][i] - rb["lps"][i]) for i in pre if ra["lps"][i] is not None and rb["lps"][i] is not None), default=0.0)
    head = f"seq {ra['seq']:2} req {ra['req']:2} cached {cached(ra)}/{cached(rb)} len {len(ta)}/{len(tb)} maxΔlp-before {dmax:.3f}"
    if d is None and len(ta) == len(tb):
        same += 1
        print(head, "IDENTICAL")
        continue
    if d is None:
        print(head, f"prefix-equal, lengths differ ({ra['finish']}/{rb['finish']})")
        continue
    topa, topb = ra["top"][d], rb["top"][d]
    ma = topa[0][1] - topa[1][1] if len(topa) > 1 and topa[1][1] is not None else None
    mb = topb[0][1] - topb[1][1] if len(topb) > 1 and topb[1][1] is not None else None
    print(head, f"DIVERGE@{d}: ref {ta[d]!r} ({ra['lps'][d]}) margin {ma} | test {tb[d]!r} ({rb['lps'][d]}) margin {mb}"
          f" | ref-lp(test tok) {lp_of(topa, tb[d])} test-lp(ref tok) {lp_of(topb, ta[d])}"
          f" | ctx {''.join(ta[max(0, d - 12):d])!r}")
print(f"identical {same}/{min(len(a), len(b))}")
