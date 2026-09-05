"""Boot BOTH services in one process (convenience wrapper).

Usage:
    python3 -m app --db-dir P --ledger-port N --notifier-port M \
        --vendor URL --tokens-file T

The harness normally starts the two services independently via
``python3 -m app.ledgerd`` / ``python3 -m app.notifierd``; this form just
runs both in threads of one process and is not a lifecycle contract.
"""

import argparse
import os
import threading


def main(argv=None):
    parser = argparse.ArgumentParser(
        prog="python3 -m app",
        description="Meridian Payments Console: boot ledgerd and notifierd together.",
    )
    parser.add_argument(
        "--db-dir",
        default="./data",
        help="directory holding ledger.db and notifier.db",
    )
    parser.add_argument(
        "--ledger-port", type=int, default=8080, help="ledgerd TCP port on 127.0.0.1"
    )
    parser.add_argument(
        "--notifier-port", type=int, default=8081, help="notifierd TCP port on 127.0.0.1"
    )
    parser.add_argument(
        "--vendor",
        default="http://127.0.0.1:8850",
        help="Meridian API v3 base URL",
    )
    parser.add_argument(
        "--tokens-file",
        default=None,
        help="JSON file with maker/checker/admin bearer tokens",
    )
    args = parser.parse_args(argv)

    os.makedirs(args.db_dir, exist_ok=True)

    # Import after arg parsing so --help never pays for service imports.
    from .ledgerd import run as ledgerd_run
    from .notifierd import run as notifierd_run

    notifier_url = f"http://127.0.0.1:{args.notifier_port}"
    ledger_thread = threading.Thread(
        target=ledgerd_run,
        name="ledgerd",
        kwargs=dict(
            db_dir=args.db_dir,
            port=args.ledger_port,
            notifier_url=notifier_url,
            vendor_url=args.vendor,
            tokens_file=args.tokens_file,
        ),
    )
    notifier_thread = threading.Thread(
        target=notifierd_run,
        name="notifierd",
        kwargs=dict(db_dir=args.db_dir, port=args.notifier_port),
    )
    ledger_thread.start()
    notifier_thread.start()
    for thread in (ledger_thread, notifier_thread):
        thread.join()


if __name__ == "__main__":
    main()
