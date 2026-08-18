"""Entry point: python -m vspro --db PATH --port N.

Binds the HTTP server first, then registers the webhook in the background — the vendor's
challenge handshake needs a listening server, and a briefly unreachable vendor must not stop
the app from serving whatever is already local.
"""

from __future__ import annotations

import argparse
import os
import sys
import threading
import time

from .api import register_webhook, serve
from .meridian import MeridianClient
from .store import Store


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        prog="python -m vspro",
        description="VendorSync Pro — sync Meridian payments, serve the finance dashboard.")
    parser.add_argument("--db", default="vspro.db", help="SQLite database path (created if absent)")
    parser.add_argument("--port", type=int, default=8790, help="HTTP port on 127.0.0.1")
    parser.add_argument("--base-url", default=os.environ.get("MERIDIAN_BASE_URL",
                                                             "http://127.0.0.1:8787"),
                        help="Meridian API base URL (env MERIDIAN_BASE_URL)")
    parser.add_argument("--api-key", default=os.environ.get("MERIDIAN_API_KEY",
                                                            "sk_test_meridian"),
                        help="Meridian API key (env MERIDIAN_API_KEY)")
    args = parser.parse_args(argv)

    store = Store(args.db)
    client = MeridianClient(args.base_url, args.api_key)
    server, ctx = serve(args.port, store, client)
    threading.Thread(target=register_webhook, args=(ctx, args.port), daemon=True).start()
    print(f"vspro on http://127.0.0.1:{args.port}  db={args.db}  vendor={args.base_url}",
          flush=True)
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        server.shutdown()
        return 0


if __name__ == "__main__":
    sys.exit(main())
