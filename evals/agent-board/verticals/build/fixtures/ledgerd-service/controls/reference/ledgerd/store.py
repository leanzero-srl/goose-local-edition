import sqlite3
from pathlib import Path

SCHEMA = """
CREATE TABLE accounts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  type TEXT NOT NULL
);
CREATE TABLE entries (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  date TEXT NOT NULL,
  memo TEXT NOT NULL,
  reversed_by INTEGER,
  reverses INTEGER
);
CREATE TABLE legs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  entry_id INTEGER NOT NULL,
  account_id INTEGER NOT NULL,
  amount INTEGER NOT NULL
);
"""

ACCOUNT_TYPES = ("asset", "liability", "equity", "income", "expense")
DEBIT_POSITIVE = ("asset", "expense")


class StoreError(RuntimeError):
    pass


def connect(path: str, require_init: bool = True) -> sqlite3.Connection:
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    if require_init:
        row = conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='accounts'").fetchone()
        if row is None:
            raise StoreError("ledger is not initialised — run `init` first")
    return conn


def init(path: str) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(path)
    conn.executescript(SCHEMA)
    conn.commit()
    conn.close()
