#!/usr/bin/env python3
"""Deterministic seed data for brief 2026-10-31-4 (Rowerek city-bike support export + dock gateway log).
Usage: python3 -I gen_data.py <out_dir>. Same bytes every run (random.Random(1031)). Prints the grader numbers."""
import csv, random, sys, os, datetime as dt
from collections import Counter
out = sys.argv[1]; os.makedirs(out, exist_ok=True)
R = random.Random(1031)
STATIONS = [f"KRK-{n:03d}" for n in range(1, 61)]
BAD = {"KRK-017": "LOCK_TIMEOUT", "KRK-042": "LOCK_TIMEOUT", "KRK-008": "GSM_DROP"}
CATS = [("unlock_failed", 26), ("charged_after_return", 18), ("app_login", 14), ("bike_damage", 12), ("refund_request", 10), ("pricing_question", 9), ("account_delete", 6), ("other", 5)]
TEXT = {
 "unlock_failed": ["Bike won't unlock at {s}, app spins forever", "Scanned QR at {s}, nothing happens", "Dock at {s} beeped red twice, no bike"],
 "charged_after_return": ["Returned the bike at {s} but the ride kept running for {m} minutes", "Dock at {s} did not register my return, charged {e} zl", "Ride still open after I docked at {s}"],
 "app_login": ["Can't log in after update", "SMS code never arrives", "App logs me out every day"],
 "bike_damage": ["Chain broken on bike at {s}", "Flat tyre, bike left at {s}", "Brakes weak on bike from {s}"],
 "refund_request": ["Please refund my last ride, the bike was broken", "Double charge for monthly pass", "Refund the {e} zl from yesterday"],
 "pricing_question": ["How much is the student pass?", "Is the first 20 minutes still free?", "Why was I charged the weekend price?"],
 "account_delete": ["Please delete my account and data", "GDPR: remove my data", "Close my account"],
 "other": ["Great service, thanks", "Can you add a station near the university?", "Lost my phone on a bike at {s}"],
}
cats = [c for c, w in CATS for _ in range(w)]
start = dt.datetime(2026, 10, 1, 6, 0)
rows = []
for i in range(2400):
    t = start + dt.timedelta(minutes=R.randint(0, 30 * 24 * 60 - 1))
    c = R.choice(cats)
    s = R.choice(STATIONS)
    if c in ("unlock_failed", "charged_after_return") and R.random() < 0.55:
        s = R.choice(list(BAD))
    text = R.choice(TEXT[c]).format(s=s, m=R.randint(15, 240), e=f"{R.randint(4, 60)}.{R.choice(['00','50','90'])}")
    refund = ""
    status = R.choice(["open", "open", "pending", "solved", "solved", "solved"])
    if c in ("charged_after_return", "refund_request") and status == "solved":
        refund = f"{R.randint(4, 60)}.{R.choice(['00','50','90'])}"
    rows.append([f"RW-{10000+i}", t.strftime("%Y-%m-%d %H:%M"), R.choice(["app", "email", "phone"]), f"user{R.randint(1000,9999)}@example.pl", c, s, text, status, refund])
rows.sort(key=lambda r: r[1])
with open(os.path.join(out, "tickets-2026-10.csv"), "w", newline="") as f:
    w = csv.writer(f); w.writerow(["id", "created", "channel", "email", "category", "station", "text", "status", "refund_pln"]); w.writerows(rows)
log = []
t = dt.datetime(2026, 10, 1, 0, 0, 0)
errs = Counter()
for i in range(9000):
    t += dt.timedelta(seconds=R.randint(30, 400))
    s = R.choice(STATIONS)
    if s in BAD and R.random() < 0.35:
        code = BAD[s]; errs[(s, code)] += 1
        log.append(f"{t:%Y-%m-%dT%H:%M:%S}Z ERROR dockgw station={s} dock={R.randint(1,20)} code={code} attempt={R.randint(1,3)} msg=\"lock did not confirm\"")
    elif R.random() < 0.02:
        code = "FW_CHECKSUM"; errs[(s, code)] += 1
        log.append(f"{t:%Y-%m-%dT%H:%M:%S}Z ERROR dockgw station={s} dock={R.randint(1,20)} code={code} attempt=1 msg=\"firmware block rejected\"")
    else:
        log.append(f"{t:%Y-%m-%dT%H:%M:%S}Z INFO dockgw station={s} dock={R.randint(1,20)} event={R.choice(['unlock','lock','heartbeat','heartbeat','heartbeat'])} ms={R.randint(80,900)}")
with open(os.path.join(out, "dockgw-2026-10.log"), "w") as f:
    f.write("\n".join(log) + "\n")
cc = Counter(r[4] for r in rows)
print("tickets", len(rows), dict(cc.most_common()))
print("tickets mentioning bad stations", {s: sum(1 for r in rows if r[5] == s) for s in BAD})
print("unlock+charged at bad stations", sum(1 for r in rows if r[5] in BAD and r[4] in ("unlock_failed", "charged_after_return")))
print("refunds solved", sum(1 for r in rows if r[8]), "total_pln", round(sum(float(r[8]) for r in rows if r[8]), 2))
print("refund sum charged_after_return", round(sum(float(r[8]) for r in rows if r[8] and r[4] == "charged_after_return"), 2))
print("account_delete open/pending", sum(1 for r in rows if r[4] == "account_delete" and r[7] != "solved"))
print("log lines", len(log), "errors", sum(errs.values()), sorted(((k[0], k[1], v) for k, v in errs.items()), key=lambda x: -x[2])[:6])
print("last log ts", log[-1][:20])
