"""Entry point for the VendorSync Pro local server."""
from __future__ import annotations

import argparse
import os
import threading

from .api import serve
from .meridian import MeridianClient
from .store import Store


def main() -> None:
    parser = argparse.ArgumentParser(description="VendorSync Pro")
    parser.add_argument("--db", required=True, help="SQLite database path")
    parser.add_argument("--port", type=int, required=True, help="HTTP port on 127.0.0.1")
    parser.add_argument(
        "--base-url",
        default=os.environ.get("MERIDIAN_BASE_URL", "http://127.0.0.1:9008"),
        help="Meridian base URL (default: MERIDIAN_BASE_URL or http://127.0.0.1:9008)",
    )
    parser.add_argument(
        "--api-key",
        default=os.environ.get("MERIDIAN_API_KEY", "sk_test_meridian"),
        help="Meridian API key (default: MERIDIAN_API_KEY or sk_test_meridian)",
    )
    args = parser.parse_args()
    server = serve(args.port, Store(args.db), MeridianClient(args.base_url, args.api_key))
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        server.shutdown()


if __name__ == "__main__":
    main()
