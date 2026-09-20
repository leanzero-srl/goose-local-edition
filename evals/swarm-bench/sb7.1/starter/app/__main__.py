"""Convenience launcher; each service can also be started independently."""
import argparse
import signal
import subprocess
import sys
import threading


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--db-dir', required=True)
    parser.add_argument('--ledger-port', type=int, required=True)
    parser.add_argument('--notifier-port', type=int, required=True)
    parser.add_argument('--vendor', required=True)
    parser.add_argument('--tokens-file', required=True)
    args = parser.parse_args()
    stopping = threading.Event()
    for number in (signal.SIGINT, signal.SIGTERM):
        signal.signal(number, lambda *_: stopping.set())
    children = []
    try:
        for service, options in (
            ('notifierd', ['--port', str(args.notifier_port)]),
            ('ledgerd', ['--port', str(args.ledger_port), '--notifier',
                         f'http://127.0.0.1:{args.notifier_port}', '--vendor', args.vendor,
                         '--tokens-file', args.tokens_file]),
        ):
            children.append(subprocess.Popen([sys.executable, '-m', f'app.{service}',
                                              '--db-dir', args.db_dir, *options]))
        while not stopping.wait(.1):
            for child in children:
                status = child.poll()
                if status is not None:
                    return status or 1
        return 0
    finally:
        for child in children:
            if child.poll() is None:
                child.terminate()
        for child in children:
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()


if __name__ == '__main__':
    raise SystemExit(main())
