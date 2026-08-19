"""python -m app — convenience wrapper booting BOTH services.

The harness starts and kills the two services independently; this wrapper spawns each as
its own `python -m app.<service>` child in the same process group so a killpg on the
wrapper takes both down.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--db-dir", type=Path, required=True)
    ap.add_argument("--ledger-port", type=int, required=True)
    ap.add_argument("--notifier-port", type=int, required=True)
    ap.add_argument("--vendor", type=str, required=True)
    ap.add_argument("--tokens-file", type=Path, required=True)
    args = ap.parse_args()

    notifier = subprocess.Popen(
        [sys.executable, "-m", "app.notifierd", "--db-dir", str(args.db_dir),
         "--port", str(args.notifier_port)])
    ledger = subprocess.Popen(
        [sys.executable, "-m", "app.ledgerd", "--db-dir", str(args.db_dir),
         "--port", str(args.ledger_port),
         "--notifier", f"http://127.0.0.1:{args.notifier_port}",
         "--vendor", args.vendor, "--tokens-file", str(args.tokens_file)])
    try:
        while True:
            for proc, other in ((ledger, notifier), (notifier, ledger)):
                if proc.poll() is not None:
                    other.terminate()
                    return proc.returncode or 1
            time.sleep(1.0)
    except KeyboardInterrupt:
        ledger.terminate()
        notifier.terminate()
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
