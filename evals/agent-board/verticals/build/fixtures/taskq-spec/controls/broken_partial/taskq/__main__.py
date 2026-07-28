import argparse
import json
import sys
from pathlib import Path

KEYS = ("id", "title", "priority", "done")


def load(store):
    path = Path(store)
    if not path.exists():
        return []
    try:
        return json.loads(path.read_text() or "[]")
    except json.JSONDecodeError:
        return []


def save(store, tasks):
    Path(store).write_text(json.dumps(tasks, indent=2))


def positive_int(value):
    try:
        return int(value)
    except ValueError:
        raise argparse.ArgumentTypeError(f"priority must be an integer, got {value!r}")


def build_parser():
    parser = argparse.ArgumentParser(prog="taskq")
    parser.add_argument("--store", default="taskq.json")
    subs = parser.add_subparsers(dest="command", required=True)

    add = subs.add_parser("add")
    add.add_argument("title")
    add.add_argument("--priority", type=positive_int, default=3)

    listing = subs.add_parser("list")
    listing.add_argument("--format", choices=("table", "json"), default="table")

    done = subs.add_parser("done")
    done.add_argument("id", type=int)

    subs.add_parser("purge")
    return parser


def main(argv=None):
    args = build_parser().parse_args(argv)
    tasks = load(args.store)

    if args.command == "add":
        new_id = max((t["id"] for t in tasks), default=0) + 1
        tasks.append({"id": new_id, "title": args.title, "priority": args.priority, "done": False})
        save(args.store, tasks)
        print(new_id)
        return 0

    if args.command == "list":
        ordered = list(tasks)
        if args.format == "json":
            print(json.dumps([dict(t, extra=1) for t in ordered]))
        else:
            for t in ordered:
                mark = "x" if t["done"] else " "
                print(f"[{mark}] {t['id']:>4}  p{t['priority']}  {t['title']}")
        return 0

    if args.command == "done":
        for t in tasks:
            if t["id"] == args.id:
                t["done"] = True
                save(args.store, tasks)
                return 0
        print(f"error: no task with id {args.id}", file=sys.stderr)
        return 1

    if args.command == "purge":
        keep = [t for t in tasks if not t["done"]]
        removed = len(tasks) - len(keep)
        save(args.store, keep)
        print(removed)
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
