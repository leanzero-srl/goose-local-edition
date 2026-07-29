"""The ten trap checks, graded from the mock's request trace plus the client's return values.

Each check is INDEPENDENT and names its own real-world consequence. Independence is not a style
preference: a shared precondition that can fail is how a mostly-correct build once collapsed to 0/44
in this project. A client that gets pagination wrong must lose pagination points and nothing else.

Every consequence line below is the legitimacy justification for the deduction — if the harm cannot be
named, the check does not belong here.
"""

from __future__ import annotations

from typing import Dict, List, Optional

RETRY_AFTER_SECS = 2
EXPECTED_TOTAL = 47
PHASE_MARKER = "__phase__"


def exercise_phase(trace: List[Dict]) -> List[Dict]:
    """Only what the DELIVERED client did.

    Everything before the marker is the agent building and testing itself; grading that would score
    throwaway scratch code. The one exception is read_the_docs, which is deliberately about the BUILD
    phase — reading the docs is research, not client behaviour.
    """
    for i, entry in enumerate(trace):
        # ANY marker opens the graded window. Matching only the literal "exercise" meant the build
        # flow's sync1/sync2 markers were invisible and the whole trace was graded — including the
        # agent's own development testing, which is exactly what the marker exists to exclude.
        if entry.get(PHASE_MARKER):
            return trace[i + 1:]
    return trace  # no marker (e.g. an older trace) — grade everything rather than nothing


def _list_calls(trace: List[Dict]) -> List[Dict]:
    t = exercise_phase(trace)
    return [e for e in t if e["path"] == "/v1/payments" and e["method"] == "GET"]


def _posts(trace: List[Dict]) -> List[Dict]:
    t = exercise_phase(trace)
    return [e for e in t if e["path"] == "/v1/payments" and e["method"] == "POST"]


def check_read_the_docs(trace: List[Dict], _r: Dict) -> Dict:
    hit = any(e["path"] in ("/v1/docs", "/docs") for e in trace)
    return {"ok": hit, "detail": f"{sum(1 for e in trace if 'docs' in e['path'])} docs fetches",
            "consequence": "integrating against an assumed contract instead of the real one"}


def check_cursor_pagination(trace: List[Dict], _r: Dict) -> Dict:
    calls = _list_calls(trace)
    used_cursor = any("cursor" in e["query"] for e in calls)
    used_offset = any(k in e["query"] for e in calls for k in ("page", "offset", "skip"))
    return {"ok": used_cursor and not used_offset,
            "detail": f"cursor={used_cursor} offset-style={used_offset} over {len(calls)} calls",
            "consequence": "offset paging against a cursor API silently returns wrong or repeated rows"}

def check_fetched_every_page(trace: List[Dict], _r: Dict) -> Dict:
    """Did the client paginate to EXHAUSTION — i.e. see the page whose next_cursor is null?

    Was keyed on a `page` field the trace stopped emitting when paging became offset-based, so it
    read an empty set and scored zero against a client that had in fact fetched everything. Keyed on
    the server's own end-of-collection marker now, which cannot drift with the paging scheme.
    """
    ok200 = [e for e in _list_calls(trace) if e["status"] == 200]
    reached_end = any(e.get("last") for e in ok200)
    offsets = sorted({e.get("offset") for e in ok200 if e.get("offset") is not None})
    return {"ok": reached_end,
            "detail": f"{len(ok200)} pages, offsets {offsets[:6]}{'...' if len(offsets) > 6 else ''}, "
                      f"reached end: {reached_end}",
            "consequence": "stopping before next_cursor is null silently drops the tail"}


def check_short_page_not_treated_as_end(trace: List[Dict], _r: Dict) -> Dict:
    """Page 1 is SHORT (5 rows) while page 2 still follows — the docs warn about exactly this."""
    pages = {e.get("page") for e in _list_calls(trace) if e["status"] == 200}
    return {"ok": 2 in pages,
            "detail": "reached page 2 (after the short page 1)" if 2 in pages
                      else "stopped at or before the short page",
            "consequence": "stopping on a short page drops the final 17 payments"}


def check_all_payments_returned(_t: List[Dict], results: Dict) -> Dict:
    got = results.get("fetch_all_payments")
    n = len(got) if isinstance(got, list) else None
    return {"ok": n == EXPECTED_TOTAL, "detail": f"returned {n}, expected {EXPECTED_TOTAL}",
            "consequence": "the caller sees an incomplete ledger"}


def check_retry_after_honoured(trace: List[Dict], _r: Dict) -> Dict:
    """A 429 must be waited out for the number of seconds the RESPONSE states, then retried."""
    scoped = exercise_phase(trace)
    throttled = [e for e in scoped if e["status"] == 429 and e.get("form") != "http-date"]
    if not throttled:
        return {"ok": False, "detail": "the API never throttled — client did not reach page 2",
                "consequence": "n/a — masked by a pagination failure"}
    after = [e for e in scoped if e["seq"] > throttled[0]["seq"]
             and e["path"] == "/v1/payments" and e["method"] == "GET"]
    if not after:
        return {"ok": False, "detail": "gave up after the 429, never retried",
                "consequence": "a transient rate limit becomes a hard sync failure"}
    waited = round(after[0]["t"] - throttled[0]["t"], 2)
    return {"ok": waited >= RETRY_AFTER_SECS - 0.15,
            "detail": f"waited {waited}s, Retry-After said {RETRY_AFTER_SECS}s",
            "consequence": "retrying sooner than Retry-After re-triggers the limit and can get the key banned"}


def check_httpdate_retry_after(trace: List[Dict], _r: Dict) -> Dict:
    """RFC 7231 allows Retry-After as an HTTP-date as well as a delta-seconds. Meridian sends both."""
    from email.utils import parsedate_to_datetime
    scoped = exercise_phase(trace)
    dated = [e for e in scoped if e["status"] == 429 and e.get("form") == "http-date"]
    if not dated:
        return {"ok": False, "detail": "never reached the date-form throttle",
                "consequence": "n/a — masked by an earlier failure"}
    after = [e for e in scoped if e["seq"] > dated[0]["seq"]
             and e["path"] == "/v1/payments" and e["method"] == "GET"]
    if not after:
        return {"ok": False, "detail": "gave up on the date-form 429",
                "consequence": "an HTTP-date Retry-After is treated as unparseable and the sync dies"}
    try:
        until = parsedate_to_datetime(dated[0]["retry_after"]).timestamp()
    except Exception:
        return {"ok": False, "detail": "PROBE ERROR: unparseable date", "consequence": "probe bug"}
    waited = round(after[0]["t"] - until, 2)
    return {"ok": after[0]["t"] >= until - 0.15,
            "detail": f"retried {waited:+.2f}s relative to the stated moment",
            "consequence": "parsing only the integer form makes the client hammer through a date-based limit"}


def check_cursor_expiry_recovery(trace: List[Dict], _r: Dict) -> Dict:
    """On 410 the documented recovery is to restart from the first page, not to give up or re-send."""
    scoped = exercise_phase(trace)
    gone = [e for e in scoped if e["status"] == 410]
    if not gone:
        return {"ok": False, "detail": "never reached the expiring cursor",
                "consequence": "n/a — masked by an earlier failure"}
    after = [e for e in scoped if e["seq"] > gone[0]["seq"]
             and e["path"] == "/v1/payments" and e["method"] == "GET"]
    # A re-send only counts BEFORE the restart. Cursors are deterministic strings, so once the client
    # has correctly restarted, walking to page 2 again produces the same cursor value legitimately —
    # and it answers 200, not 410. Measured: Opus restarted correctly and this check still flagged it,
    # which would have been a fabricated deduction. Only the window between the 410 and the restart
    # can contain a genuine re-send.
    restart_at = next((e["seq"] for e in after if "cursor" not in e["query"]), None)
    restarted = restart_at is not None
    window = [e for e in after if restart_at is None or e["seq"] < restart_at]
    resent = any(e["query"].get("cursor") == gone[0]["query"].get("cursor") for e in window)
    return {"ok": restarted and not resent,
            "detail": f"restarted from page 0: {restarted}; re-sent the dead cursor first: {resent}",
            "consequence": "an expired cursor treated as the end of data silently truncates the ledger"}


def check_conditional_request(trace: List[Dict], _r: Dict) -> Dict:
    sent = [e for e in _list_calls(trace) if e.get("if_none_match")]
    got_304 = [e for e in exercise_phase(trace) if e["status"] == 304]
    return {"ok": bool(sent) and bool(got_304),
            "detail": f"{len(sent)} requests carried If-None-Match, {len(got_304)} answered 304",
            "consequence": "re-downloading an unchanged collection every sync — the documented top cause of rate-limit exhaustion"}


def check_total_semantics(_t: List[Dict], results: Dict) -> Dict:
    total = results.get("total_count")
    return {"ok": total == EXPECTED_TOTAL,
            "detail": f"total_count() returned {total}, expected {EXPECTED_TOTAL}",
            "consequence": "reporting the page size as the collection size understates the ledger"}


def check_chronological_order(_t: List[Dict], results: Dict) -> Dict:
    got = results.get("fetch_all_payments")
    expected = results.get("_true_order")
    if not isinstance(got, list) or not expected:
        return {"ok": False, "detail": "no list to order", "consequence": "n/a"}
    ids = [p.get("id") for p in got]
    if sorted(ids) != sorted(expected):
        return {"ok": False, "detail": "different payment set — ordering not assessable",
                "consequence": "n/a — masked by a completeness failure"}
    return {"ok": ids == expected,
            "detail": "chronological" if ids == expected else "string-sorted, not chronological",
            "consequence": "mixed UTC offsets make lexicographic order wrong — payments appear out of sequence"}


def check_idempotency_key_sent(trace: List[Dict], _r: Dict) -> Dict:
    posts = _posts(trace)
    keyed = [e for e in posts if e.get("idempotency_key")]
    return {"ok": bool(posts) and len(keyed) == len(posts),
            "detail": f"{len(keyed)}/{len(posts)} POSTs carried Idempotency-Key",
            "consequence": "without the key a retry creates a second real payment — a double charge"}


def check_replay_treated_as_success(_t: List[Dict], results: Dict) -> Dict:
    first, second = results.get("create_first"), results.get("create_replay")
    return {"ok": bool(first) and first == second,
            "detail": f"first={first!r} replay={second!r}",
            "consequence": "treating the documented 409 as an error makes callers resubmit — the double-charge path"}


def check_amounts_are_integer_minor(_t: List[Dict], results: Dict) -> Dict:
    got = results.get("fetch_all_payments")
    if not isinstance(got, list) or not got:
        return {"ok": False, "detail": "no payments to inspect", "consequence": "n/a"}
    bad = [p for p in got if not isinstance(p.get("amount_minor"), int)]
    return {"ok": not bad, "detail": f"{len(bad)} non-integer amounts of {len(got)}",
            "consequence": "float money accumulates rounding error across a ledger"}


CHECKS = [
    ("read_the_docs", check_read_the_docs),
    ("cursor_pagination", check_cursor_pagination),
    ("fetched_every_page", check_fetched_every_page),
    ("short_page_not_end", check_short_page_not_treated_as_end),
    ("all_payments_returned", check_all_payments_returned),
    ("retry_after_honoured", check_retry_after_honoured),
    ("httpdate_retry_after", check_httpdate_retry_after),
    ("cursor_expiry_recovery", check_cursor_expiry_recovery),
    ("conditional_request", check_conditional_request),
    ("total_semantics", check_total_semantics),
    ("chronological_order", check_chronological_order),
    ("idempotency_key_sent", check_idempotency_key_sent),
    ("replay_is_success", check_replay_treated_as_success),
    ("amounts_integer_minor", check_amounts_are_integer_minor),
]


def evaluate(trace: List[Dict], results: Dict) -> Dict:
    rows = []
    for name, fn in CHECKS:
        try:
            outcome = fn(trace, results)
        except Exception as exc:  # a broken probe must never look like a model failure
            outcome = {"ok": False, "detail": f"PROBE ERROR: {exc}", "consequence": "probe bug"}
        rows.append({"check": name, **outcome})
    passed = sum(1 for r in rows if r["ok"])
    return {"passed": passed, "total": len(rows),
            "score": round(passed / len(rows), 4), "checks": rows}


def format_report(result: Dict, title: str = "") -> str:
    lines = [f"{title}  {result['passed']}/{result['total']} = {100 * result['score']:.1f}%", ""]
    for row in result["checks"]:
        mark = "PASS" if row["ok"] else "MISS"
        lines.append(f"  [{mark}] {row['check']:<24} {row['detail']}")
        if not row["ok"] and row["consequence"] not in ("n/a", "probe bug"):
            lines.append(f"         └ consequence: {row['consequence']}")
    return "\n".join(lines)
