import argparse
import json
import sqlite3
import sys
from typing import List, Tuple

from .domain import LedgerError, check_balanced, check_date, check_type, parse_leg, signed_balance
from .store import StoreError, connect, init


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="ledgerd")
    parser.add_argument("--db", default="ledger.db")
    subs = parser.add_subparsers(dest="command", required=True)

    subs.add_parser("init")

    account = subs.add_parser("account")
    account_subs = account.add_subparsers(dest="account_command", required=True)
    add = account_subs.add_parser("add")
    add.add_argument("name")
    add.add_argument("--type", required=True)
    listing = account_subs.add_parser("list")
    listing.add_argument("--format", default="table")

    entry = subs.add_parser("entry")
    entry_subs = entry.add_subparsers(dest="entry_command", required=True)
    post = entry_subs.add_parser("post")
    post.add_argument("--date", required=True)
    post.add_argument("--memo", default="")
    post.add_argument("--leg", action="append", default=[])
    elist = entry_subs.add_parser("list")
    elist.add_argument("--format", default="table")
    reverse = entry_subs.add_parser("reverse")
    reverse.add_argument("id", type=int)

    balance = subs.add_parser("balance")
    balance.add_argument("name")
    balance.add_argument("--as-of", dest="as_of")

    trial = subs.add_parser("trial-balance")
    trial.add_argument("--as-of", dest="as_of")
    trial.add_argument("--format", default="table")
    return parser


def check_format(value: str) -> str:
    if value not in ("table", "json"):
        raise LedgerError(f"format must be table or json, got {value!r}")
    return value


def account_id(conn, name: str) -> int:
    row = conn.execute("SELECT id FROM accounts WHERE name = ?", (name,)).fetchone()
    if row is None:
        raise LedgerError(f"unknown account: {name}")
    return row["id"]


def do_account_add(conn, args) -> int:
    check_type(args.type)
    try:
        cur = conn.execute("INSERT INTO accounts(name, type) VALUES (?, ?)",
                           (args.name, args.type))
    except sqlite3.IntegrityError:
        raise LedgerError(f"account already exists: {args.name}")
    conn.commit()
    print(cur.lastrowid)
    return 0


def do_account_list(conn, args) -> int:
    rows = conn.execute("SELECT id, name, type FROM accounts ORDER BY id").fetchall()
    if check_format(args.format) == "json":
        print(json.dumps([{"id": r["id"], "name": r["name"], "type": r["type"]} for r in rows]))
    else:
        for r in rows:
            print(f"{r['id']:>4}  {r['name']:<24} {r['type']}")
    return 0


def do_entry_post(conn, args) -> int:
    check_date(args.date)
    legs: List[Tuple[str, int]] = [parse_leg(text) for text in args.leg]
    check_balanced(legs)
    resolved = [(account_id(conn, name), amount) for name, amount in legs]
    cur = conn.execute("INSERT INTO entries(date, memo) VALUES (?, ?)", (args.date, args.memo))
    entry_id = cur.lastrowid
    conn.executemany("INSERT INTO legs(entry_id, account_id, amount) VALUES (?, ?, ?)",
                     [(entry_id, aid, amount) for aid, amount in resolved])
    conn.commit()
    print(entry_id)
    return 0


def do_entry_list(conn, args) -> int:
    entries = conn.execute("SELECT id, date, memo FROM entries ORDER BY id").fetchall()
    payload = []
    for e in entries:
        legs = conn.execute(
            "SELECT a.name AS account, l.amount AS amount FROM legs l "
            "JOIN accounts a ON a.id = l.account_id WHERE l.entry_id = ? ORDER BY l.id",
            (e["id"],)).fetchall()
        payload.append({"id": e["id"], "date": e["date"], "memo": e["memo"],
                        "legs": [{"account": l["account"], "amount": l["amount"]} for l in legs]})
    if check_format(args.format) == "json":
        print(json.dumps(payload))
    else:
        for item in payload:
            print(f"{item['id']:>4}  {item['date']}  {item['memo']}")
            for leg in item["legs"]:
                print(f"        {leg['account']:<24} {leg['amount']}")
    return 0


def do_entry_reverse(conn, args) -> int:
    row = conn.execute("SELECT id, date, reversed_by, reverses FROM entries WHERE id = ?",
                       (args.id,)).fetchone()
    if row is None:
        raise LedgerError(f"unknown entry id: {args.id}")
    if row["reversed_by"] is not None:
        raise LedgerError(f"entry {args.id} has already been reversed")
    if row["reverses"] is not None:
        raise LedgerError(f"entry {args.id} is itself a reversal and cannot be reversed")
    legs = conn.execute("SELECT account_id, amount FROM legs WHERE entry_id = ? ORDER BY id",
                        (args.id,)).fetchall()
    cur = conn.execute("INSERT INTO entries(date, memo, reverses) VALUES (?, ?, ?)",
                       (row["date"], f"reversal of {args.id}", args.id))
    new_id = cur.lastrowid
    conn.executemany("INSERT INTO legs(entry_id, account_id, amount) VALUES (?, ?, ?)",
                     [(new_id, l["account_id"], -l["amount"]) for l in legs])
    conn.execute("UPDATE entries SET reversed_by = ? WHERE id = ?", (new_id, args.id))
    conn.commit()
    print(new_id)
    return 0


def _sums(conn, account_id_value: int, as_of):
    sql = ("SELECT l.amount FROM legs l JOIN entries e ON e.id = l.entry_id "
           "WHERE l.account_id = ?")
    params = [account_id_value]
    if as_of:
        sql += " AND e.date <= ?"
        params.append(as_of)
    debits = credits = 0
    for row in conn.execute(sql, params):
        if row["amount"] >= 0:
            debits += row["amount"]
        else:
            credits += -row["amount"]
    return debits, credits


def do_balance(conn, args) -> int:
    if args.as_of:
        check_date(args.as_of)
    row = conn.execute("SELECT id, type FROM accounts WHERE name = ?", (args.name,)).fetchone()
    if row is None:
        raise LedgerError(f"unknown account: {args.name}")
    debits, credits = _sums(conn, row["id"], args.as_of)
    print(signed_balance(row["type"], debits, credits))
    return 0


def do_trial_balance(conn, args) -> int:
    if args.as_of:
        check_date(args.as_of)
    check_format(args.format)
    accounts = conn.execute("SELECT id, name, type FROM accounts ORDER BY id").fetchall()
    rows, total_debits, total_credits = [], 0, 0
    for account in accounts:
        debits, credits = _sums(conn, account["id"], args.as_of)
        total_debits += debits
        total_credits += credits
        rows.append({"name": account["name"], "type": account["type"],
                     "debits": debits, "credits": credits})
    if args.format == "json":
        print(json.dumps({"accounts": rows,
                          "totals": {"debits": total_debits, "credits": total_credits}}))
    else:
        for r in rows:
            print(f"{r['name']:<24} {r['type']:<10} {r['debits']:>12} {r['credits']:>12}")
        print(f"{'TOTAL':<24} {'':<10} {total_debits:>12} {total_credits:>12}")
    return 0


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "init":
            init(args.db)
            return 0
        conn = connect(args.db)
        if args.command == "account":
            return do_account_add(conn, args) if args.account_command == "add" \
                else do_account_list(conn, args)
        if args.command == "entry":
            return {"post": do_entry_post, "list": do_entry_list,
                    "reverse": do_entry_reverse}[args.entry_command](conn, args)
        if args.command == "balance":
            return do_balance(conn, args)
        if args.command == "trial-balance":
            return do_trial_balance(conn, args)
    except (LedgerError, StoreError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
