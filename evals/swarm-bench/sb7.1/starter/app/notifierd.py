"""Notifier process entrypoint; application behavior remains candidate-owned."""
import argparse
from pathlib import Path
from .transport import UnimplementedApplication, serve


def create_application(args):
    return UnimplementedApplication()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--db-dir', type=Path, required=True)
    parser.add_argument('--port', type=int, required=True)
    args = parser.parse_args()
    serve(args.port, create_application(args))


if __name__ == '__main__':
    main()
