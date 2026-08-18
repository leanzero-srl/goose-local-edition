"""End-to-end smoke test for the golden vspro app against stub_vendor.

Run: python3 smoke_test.py [--port-base 8899]

Boots the stub vendor and the app as a subprocess (exactly how the harness boots entrants:
python -m vspro --db ... --port ... with MERIDIAN_BASE_URL/MERIDIAN_API_KEY in the env), then
walks the graded surface: trap-chain sync, idempotent second sync with conditionals, Berlin/DST
buckets vs an independently computed expectation, per-currency summary, validation envelopes,
the 412 conflict dance (both the recovered and the surfaced-409 case), batch partial failure,
the scripted webhook ledger (challenge uncounted, forged rejected, stale ignored), restart
persistence, and reads answering while a sync is stalled.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import stub_vendor  # noqa: E402

CHECKS = []


def check(name: str, ok: bool, detail: str = ""):
    CHECKS.append((name, bool(ok), detail))
    mark = "PASS" if ok else "FAIL"
    print(f"  [{mark}] {name}" + (f" — {detail}" if detail and not ok else ""))


def req(method: str, url: str, body=None, headers=None, timeout=30):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data=data, method=method)
    if data is not None:
        r.add_header("Content-Type", "application/json")
    for k, v in (headers or {}).items():
        r.add_header(k, v)
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            raw = resp.read()
            return resp.status, json.loads(raw) if raw else None
    except urllib.error.HTTPError as err:
        raw = err.read()
        try:
            return err.code, json.loads(raw) if raw else None
        except json.JSONDecodeError:
            return err.code, None


def wait_health(base: str, timeout=15) -> dict:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            status, body = req("GET", base + "/api/health", timeout=3)
            if status == 200:
                return body
        except (urllib.error.URLError, ConnectionError, TimeoutError):
            pass
        time.sleep(0.2)
    raise RuntimeError("app never became healthy")


def expected_buckets(payments) -> dict:
    berlin = ZoneInfo("Europe/Berlin")
    statuses = ["settled", "pending", "refunded", "failed"]
    counts = {}
    days = []
    for p in payments:
        raw = p["created_at"]
        if raw.endswith("Z"):
            raw = raw[:-1] + "+00:00"
        day = datetime.fromisoformat(raw).astimezone(berlin).date()
        days.append(day)
        counts[(day.isoformat(), p["status"])] = counts.get((day.isoformat(), p["status"]), 0) + 1
    first, last = min(days), max(days)
    cells, day_list = [], []
    day = first
    while day <= last:
        day_list.append(day.isoformat())
        for s in statuses:
            cells.append({"day": day.isoformat(), "status": s,
                          "count": counts.get((day.isoformat(), s), 0)})
        day += timedelta(days=1)
    return {"days": day_list, "cells": cells}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port-base", type=int, default=8899)
    args = ap.parse_args()
    vendor_port = args.port_base
    app_port = args.port_base + 1
    base = f"http://127.0.0.1:{app_port}"
    vendor_base = f"http://127.0.0.1:{vendor_port}"

    print("== boot ==")
    stub_vendor.serve(vendor_port)
    db = HERE / "smoke.db"
    for suffix in ("", "-wal", "-shm"):
        p = Path(str(db) + suffix)
        if p.exists():
            p.unlink()
    env = {"MERIDIAN_BASE_URL": vendor_base, "MERIDIAN_API_KEY": "sk_test_meridian",
           "PATH": "/usr/bin:/bin"}
    proc = subprocess.Popen([sys.executable, "-m", "vspro", "--db", str(db),
                             "--port", str(app_port)],
                            cwd=HERE, env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        health = wait_health(base)
        check("boot: healthy, 0 payments, never synced",
              health["status"] == "ok" and health["payments"] == 0
              and health["last_sync"] is None, json.dumps(health))
        deadline = time.time() + 10
        while time.time() < deadline:
            _, health = req("GET", base + "/api/health")
            if health["webhook"]["registered"]:
                break
            time.sleep(0.2)
        check("webhook registered (challenge handshake completed)",
              health["webhook"]["registered"])
        check("challenge did not touch the counters",
              health["webhook"]["received"] == 0 and health["webhook"]["rejected"] == 0,
              json.dumps(health["webhook"]))

        print("== sync #1 (trap chain) ==")
        n = len(stub_vendor.STATE.payments)
        status, sync1 = req("POST", base + "/api/sync", body={})
        check("sync 200 with full fixture",
              status == 200 and sync1["fetched"] == n and sync1["inserted"] == n
              and sync1["total"] == n, json.dumps(sync1))
        with stub_vendor.STATE.lock:
            fired = set(stub_vendor.STATE.fired)
        check("client walked 429-secs, 410, 429-date traps", fired == {"secs", "gone", "date"},
              str(fired))

        print("== payments API ==")
        _, page = req("GET", base + "/api/payments?limit=5&offset=0")
        keys = set(page["data"][0].keys())
        want_keys = {"id", "amount_minor", "currency", "created_at", "settled_at", "status",
                     "version", "note", "counterparty_name", "country"}
        check("row shape is exactly the 10 documented keys", keys == want_keys, str(keys))
        instants = []
        for row in page["data"]:
            raw = row["created_at"]
            raw = raw[:-1] + "+00:00" if raw.endswith("Z") else raw
            instants.append(datetime.fromisoformat(raw))
        check("default sort ascending by INSTANT",
              instants == sorted(instants), str(instants))
        _, filtered = req("GET", base + "/api/payments?status=pending&currency=USD")
        ok_rows = all(r["status"] == "pending" and r["currency"] == "USD"
                      for r in filtered["data"])
        want_total = sum(1 for p in stub_vendor.STATE.payments
                         if p["status"] == "pending" and p["currency"] == "USD")
        check("combined filters + filtered total",
              ok_rows and filtered["total"] == want_total,
              f"total={filtered['total']} want={want_total}")
        status, err = req("GET", base + "/api/payments?limit=abc")
        check("bad limit -> 400 envelope with field_errors",
              status == 400 and err["error"]["code"] == "bad_request"
              and any(fe["path"] == "limit" and fe["code"] == "not_an_integer"
                      for fe in err["error"]["field_errors"]), json.dumps(err))
        status, err = req("GET", base + "/api/payments?status=bogus")
        check("unknown status -> 400 unsupported, not empty result",
              status == 400 and any(fe["path"] == "status" and fe["code"] == "unsupported"
                                    for fe in err["error"]["field_errors"]), json.dumps(err))
        status, err = req("GET", base + "/api/nope")
        check("unknown path -> 404 envelope", status == 404
              and err["error"]["code"] == "not_found", json.dumps(err))

        print("== summary ==")
        _, summary = req("GET", base + "/api/summary")
        by_cur = {}
        for p in stub_vendor.STATE.payments:
            entry = by_cur.setdefault(p["currency"], {"count": 0, "total_minor": 0})
            entry["count"] += 1
            entry["total_minor"] += p["amount_minor"]
        got = {x["currency"]: {"count": x["count"], "total_minor": x["total_minor"]}
               for x in summary["by_currency"]}
        check("per-currency counts+totals exact", got == by_cur,
              json.dumps({"got": got, "want": by_cur}))
        check("currencies sorted ascending",
              [x["currency"] for x in summary["by_currency"]]
              == sorted(x["currency"] for x in summary["by_currency"]))
        grand = sum(v["total_minor"] for v in by_cur.values())
        check("no cross-currency money sum in the response",
              all(v != grand for k, v in summary.items() if isinstance(v, int) and k != "count"),
              json.dumps(summary))

        print("== buckets (Berlin, DST) ==")
        _, buckets = req("GET", base + "/api/buckets")
        want = expected_buckets(stub_vendor.STATE.payments)
        check("timezone label", buckets["timezone"] == "Europe/Berlin")
        check("days: full span, no gaps", buckets["days"] == want["days"],
              json.dumps({"got": buckets["days"], "want": want["days"]}))
        check("cells exact (day-major, frozen status order, zero-filled)",
              buckets["cells"] == want["cells"])
        got_dst1 = next((c for c in buckets["cells"]
                         if c["day"] == "2026-03-29" and c["status"] == "settled"), None)
        utc_day_of_dst1 = "2026-03-28"
        check("DST discriminator: 23:30Z lands on the NEXT Berlin day",
              got_dst1 is not None and got_dst1["count"] >= 1
              and utc_day_of_dst1 != "2026-03-29", json.dumps(got_dst1))

        print("== sync #2 (idempotent + cheap) ==")
        with stub_vendor.STATE.lock:
            trace_before = len(stub_vendor.STATE.trace)
        status, sync2 = req("POST", base + "/api/sync", body={})
        check("second sync inserts nothing, count stable",
              sync2["inserted"] == 0 and sync2["total"] == n, json.dumps(sync2))
        with stub_vendor.STATE.lock:
            recent = stub_vendor.STATE.trace[trace_before:]
        lists = [t for t in recent if t.get("p") == "/v2/payments" and t.get("m") == "GET"]
        conds = [t for t in lists if t.get("cond")]
        n304 = [t for t in lists if t.get("s") == 304]
        check("second sync used conditional requests and got 304s",
              len(lists) > 0 and len(conds) == len(lists) and len(n304) == len(lists),
              f"lists={len(lists)} cond={len(conds)} 304={len(n304)}")

        print("== note / conflict dance ==")
        target = stub_vendor.STATE.payments[0]["id"]
        status, note1 = req("POST", f"{base}/api/payments/{target}/note",
                            body={"note": "smoke note"})
        check("clean note write -> 200 with new version",
              status == 200 and note1["note"] == "smoke note" and note1["version"] >= 2,
              json.dumps(note1))
        req("POST", vendor_base + "/admin/force-412", body={"count": 1})
        status, note2 = req("POST", f"{base}/api/payments/{target}/note",
                            body={"note": "post-conflict note"})
        check("one 412: refetch + retry ONCE recovers",
              status == 200 and note2["note"] == "post-conflict note", json.dumps(note2))
        with stub_vendor.STATE.lock:
            patches = [t for t in stub_vendor.STATE.trace if t.get("m") == "PATCH"]
        check("every vendor write carried If-Match (428 never earned)",
              all(t.get("if_match") is not None for t in patches), json.dumps(patches))
        _, before_row = req("GET", f"{base}/api/payments/{target}")
        req("POST", vendor_base + "/admin/force-412", body={"count": 5})
        status, err = req("POST", f"{base}/api/payments/{target}/note",
                          body={"note": "never lands"})
        _, after_row = req("GET", f"{base}/api/payments/{target}")
        check("second 412 -> local 409 conflict envelope, row unchanged",
              status == 409 and err["error"]["code"] == "conflict"
              and after_row["note"] == before_row["note"], json.dumps(err))
        req("POST", vendor_base + "/admin/force-412", body={"count": 0})
        status, err = req("POST", f"{base}/api/payments/{target}/note", body={"note": ""})
        check("empty note -> 400 required", status == 400
              and any(fe["path"] == "note" for fe in err["error"]["field_errors"]))

        print("== batch ==")
        items = [
            {"amount": {"value_minor": 4500, "currency": "EUR"},
             "counterparty": {"name": "Alpha", "country": "DE"},
             "occurred_at": "2026-04-01T10:00:00Z", "idempotency_key": "smoke-b1"},
            {"amount": {"value_minor": 5_000_000, "currency": "USD"},
             "counterparty": {"name": "TooBig", "country": "US"},
             "occurred_at": "2026-04-01T11:00:00Z", "idempotency_key": "smoke-b2"},
            {"amount": {"value_minor": 900, "currency": "KWD"},
             "counterparty": {"name": "Gamma", "country": "KW"},
             "occurred_at": "2026-04-01T12:00:00+02:00", "idempotency_key": "smoke-b3"},
        ]
        status, batch = req("POST", base + "/api/payments/batch", body={"items": items})
        check("partial failure: 200, order kept, counts right",
              status == 200 and batch["succeeded"] == 2 and batch["failed"] == 1
              and [r["index"] for r in batch["results"]] == [0, 1, 2]
              and batch["results"][1]["status"] == "error"
              and batch["results"][1]["error"]["code"] == "amount_over_limit",
              json.dumps(batch))
        bad = {"items": [{"amount": {"value_minor": -5, "currency": "XXX"},
                          "counterparty": {"name": "", "country": "deu"},
                          "occurred_at": "yesterday", "idempotency_key": ""}]}
        status, err = req("POST", base + "/api/payments/batch", body=bad)
        paths = {fe["path"]: fe["code"] for fe in err["error"]["field_errors"]}
        check("invalid batch -> 400 with dot paths + frozen codes",
              status == 400
              and paths.get("items[0].amount.value_minor") == "not_positive"
              and paths.get("items[0].amount.currency") == "unsupported"
              and paths.get("items[0].counterparty.country") == "bad_format"
              and paths.get("items[0].occurred_at") == "bad_format"
              and paths.get("items[0].idempotency_key") == "required",
              json.dumps(paths))

        print("== webhooks (scripted ledger) ==")
        _, h0 = req("GET", base + "/api/health")
        w0 = h0["webhook"]
        victim = stub_vendor.STATE.payments[3]
        payment_v9 = dict(stub_vendor.Handler._public(victim))
        payment_v9["version"] = 9
        payment_v9["note"] = "webhook v9"
        payment_stale = dict(payment_v9)
        payment_stale["version"] = 2
        payment_stale["note"] = "stale must not land"
        events = [
            {"body": {"id": "evt_a", "type": "payment.updated",
                      "created_at": "2026-04-01T00:00:00Z", "data": payment_v9}},
            {"body": {"id": "evt_a", "type": "payment.updated",
                      "created_at": "2026-04-01T00:00:01Z", "data": payment_v9}},
            {"body": {"id": "evt_b", "type": "payment.updated",
                      "created_at": "2026-04-01T00:00:02Z", "data": payment_stale}},
            {"body": {"id": "evt_c", "type": "payment.updated",
                      "created_at": "2026-04-01T00:00:03Z", "data": payment_v9}, "forged": True},
        ]
        status, delivered = req("POST", vendor_base + "/admin/deliver", body={"events": events})
        check("delivery statuses: 200,200,200,401", delivered["statuses"] == [200, 200, 200, 401],
              json.dumps(delivered))
        _, h1 = req("GET", base + "/api/health")
        w1 = h1["webhook"]
        quad = {k: w1[k] - w0[k] for k in ("received", "applied", "ignored", "rejected")}
        check("counter quad: received 4, applied 1, ignored 2, rejected 1",
              quad == {"received": 4, "applied": 1, "ignored": 2, "rejected": 1},
              json.dumps(quad))
        _, row = req("GET", f"{base}/api/payments/{victim['id']}")
        check("applied landed, stale did not regress",
              row["version"] == 9 and row["note"] == "webhook v9", json.dumps(row))

        print("== reads during a stalled sync (threaded server) ==")
        req("POST", vendor_base + "/admin/list-delay", body={"seconds": 1.2})
        sync_done = threading.Event()
        sync_result = {}

        def run_sync():
            sync_result["resp"] = req("POST", base + "/api/sync", body={}, timeout=60)
            sync_done.set()

        threading.Thread(target=run_sync, daemon=True).start()
        time.sleep(0.3)
        latencies = []
        overlapped = 0
        for _ in range(8):
            t0 = time.time()
            status, page = req("GET", base + "/api/payments?limit=50", timeout=10)
            latencies.append((time.time() - t0) * 1000)
            if not sync_done.is_set():
                overlapped += 1
            check_ok = status == 200 and len(page["data"]) == 50
            if not check_ok:
                break
        sync_done.wait(60)
        req("POST", vendor_base + "/admin/list-delay", body={"seconds": 0})
        check("8 reads answered correctly while sync in flight",
              overlapped >= 4 and max(latencies) < 1000 and check_ok,
              f"overlap={overlapped}/8 max={max(latencies):.0f}ms")

        print("== restart persistence + re-registration ==")
        _, before = req("GET", base + "/api/health")
        proc.kill()
        proc.wait(10)
        proc2 = subprocess.Popen([sys.executable, "-m", "vspro", "--db", str(db),
                                  "--port", str(app_port)],
                                 cwd=HERE, env=env,
                                 stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        try:
            h2 = wait_health(base)
            check("rows survived kill + reboot on the same db",
                  h2["payments"] == before["payments"],
                  f"{h2['payments']} vs {before['payments']}")
            check("last_sync survived reboot", h2["last_sync"] is not None)
            deadline = time.time() + 10
            while time.time() < deadline:
                _, h2 = req("GET", base + "/api/health")
                if h2["webhook"]["registered"]:
                    break
                time.sleep(0.2)
            check("re-registration after restart (idempotent by URL)",
                  h2["webhook"]["registered"])
            check("fresh process counters start at zero",
                  h2["webhook"]["received"] == 0, json.dumps(h2["webhook"]))
        finally:
            proc2.kill()
            proc2.wait(10)
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(10)

    failed = [c for c in CHECKS if not c[1]]
    print(f"\n{len(CHECKS) - len(failed)}/{len(CHECKS)} checks passed")
    for name, _, detail in failed:
        print(f"  FAILED: {name} — {detail}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
