"""Hidden contract for ledgerd. Never present in the workspace during a run.

Every assertion here traces to a sentence in prompt.md. Nothing tests an implementation detail, an
internal module layout, or a message string — only behaviour the spec promises, so a correct build
that looks nothing like the reference still scores full marks.

This is the grader that has to produce SPREAD. It is deliberately wide: sign conventions per account
type, as-of filtering, reversal semantics, the trial-balance invariant, exact JSON key sets, exact
minor-unit arithmetic, and nine distinct error paths. A model that builds "most of it" lands in the
40-70 band rather than at 100.
"""

import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent


def run(db, *args, timeout=60):
    return subprocess.run([sys.executable, "-m", "ledgerd", "--db", str(db), *args],
                          cwd=ROOT, capture_output=True, text=True, timeout=timeout)


def ok(db, *args):
    result = run(db, *args)
    assert result.returncode == 0, f"{args} failed: {result.stdout}{result.stderr}"
    return result.stdout.strip()


@pytest.fixture
def led(tmp_path):
    """Best-effort setup. `init` is graded by its OWN tests below and must never be able to zero
    the suite: an early version asserted success here, so one missing subcommand cascaded into all
    44 tests failing on a build that was otherwise largely correct. A contract suite with a shared
    setup step that can fail has no resolution, which is the same disease as saturation."""
    db = tmp_path / "l.db"
    run(db, "init")
    return db


@pytest.fixture
def books(led):
    ok(led, "account", "add", "cash", "--type", "asset")
    ok(led, "account", "add", "revenue", "--type", "income")
    ok(led, "account", "add", "loan", "--type", "liability")
    ok(led, "account", "add", "rent", "--type", "expense")
    return led


# ── accounts ──────────────────────────────────────────────────────────────────────────────────

def test_init_is_a_supported_command(tmp_path):
    assert run(tmp_path / "i.db", "init").returncode == 0


def test_account_add_prints_an_integer_id(led):
    assert ok(led, "account", "add", "cash", "--type", "asset").isdigit()


def test_account_ids_are_distinct(led):
    first = ok(led, "account", "add", "cash", "--type", "asset")
    second = ok(led, "account", "add", "bank", "--type", "asset")
    assert first != second


def test_account_list_json_has_exactly_the_contract_keys(books):
    rows = json.loads(ok(books, "account", "list", "--format", "json"))
    assert isinstance(rows, list) and len(rows) == 4
    assert set(rows[0]) == {"id", "name", "type"}


def test_account_list_defaults_to_table(books):
    assert "cash" in ok(books, "account", "list")


@pytest.mark.parametrize("kind", ["asset", "liability", "equity", "income", "expense"])
def test_every_account_type_is_accepted(led, kind):
    assert ok(led, "account", "add", f"a_{kind}", "--type", kind).isdigit()


# ── posting ───────────────────────────────────────────────────────────────────────────────────

def test_post_prints_an_entry_id(books):
    assert ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "sale",
              "--leg", "cash:100.00", "--leg", "revenue:-100.00").isdigit()


def test_amounts_are_stored_in_minor_units(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
       "--leg", "cash:1.23", "--leg", "revenue:-1.23")
    assert ok(books, "balance", "cash") == "123"


def test_ties_round_away_from_zero(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
       "--leg", "cash:0.125", "--leg", "revenue:-0.125")
    assert ok(books, "balance", "cash") == "13"


def test_large_amounts_stay_exact(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
       "--leg", "cash:99999999999999.99", "--leg", "revenue:-99999999999999.99")
    assert ok(books, "balance", "cash") == "9999999999999999"


def test_entry_list_json_has_exactly_the_contract_keys(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "sale",
       "--leg", "cash:10.00", "--leg", "revenue:-10.00")
    rows = json.loads(ok(books, "entry", "list", "--format", "json"))
    assert set(rows[0]) == {"id", "date", "memo", "legs"}
    assert set(rows[0]["legs"][0]) == {"account", "amount"}
    assert rows[0]["memo"] == "sale"
    assert rows[0]["date"] == "2026-01-05"


def test_an_entry_may_have_more_than_two_legs(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "split",
       "--leg", "cash:30.00", "--leg", "revenue:-10.00", "--leg", "loan:-20.00")
    assert ok(books, "balance", "cash") == "3000"
    assert ok(books, "balance", "loan") == "2000"


# ── sign conventions ──────────────────────────────────────────────────────────────────────────

def test_asset_balance_is_debits_minus_credits(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
       "--leg", "cash:50.00", "--leg", "revenue:-50.00")
    assert ok(books, "balance", "cash") == "5000"


def test_income_balance_is_credits_minus_debits(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
       "--leg", "cash:50.00", "--leg", "revenue:-50.00")
    assert ok(books, "balance", "revenue") == "5000"


def test_liability_balance_is_credits_minus_debits(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "borrow",
       "--leg", "cash:80.00", "--leg", "loan:-80.00")
    assert ok(books, "balance", "loan") == "8000"


def test_expense_balance_is_debits_minus_credits(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "rent",
       "--leg", "rent:20.00", "--leg", "cash:-20.00")
    assert ok(books, "balance", "rent") == "2000"


def test_a_normal_balance_is_positive_for_every_type(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "a",
       "--leg", "cash:70.00", "--leg", "revenue:-70.00")
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "b",
       "--leg", "rent:10.00", "--leg", "loan:-10.00")
    for account in ("cash", "revenue", "loan", "rent"):
        assert int(ok(books, "balance", account)) > 0, account


# ── as-of ─────────────────────────────────────────────────────────────────────────────────────

def test_as_of_excludes_later_entries(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "a",
       "--leg", "cash:10.00", "--leg", "revenue:-10.00")
    ok(books, "entry", "post", "--date", "2026-02-05", "--memo", "b",
       "--leg", "cash:5.00", "--leg", "revenue:-5.00")
    assert ok(books, "balance", "cash", "--as-of", "2026-01-31") == "1000"


def test_as_of_includes_the_boundary_date(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "a",
       "--leg", "cash:10.00", "--leg", "revenue:-10.00")
    assert ok(books, "balance", "cash", "--as-of", "2026-01-05") == "1000"


def test_as_of_applies_to_the_trial_balance(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "a",
       "--leg", "cash:10.00", "--leg", "revenue:-10.00")
    ok(books, "entry", "post", "--date", "2026-03-05", "--memo", "b",
       "--leg", "cash:7.00", "--leg", "revenue:-7.00")
    payload = json.loads(ok(books, "trial-balance", "--as-of", "2026-01-31", "--format", "json"))
    assert payload["totals"]["debits"] == 1000


# ── reversal ──────────────────────────────────────────────────────────────────────────────────

def test_reversal_zeroes_the_balance(books):
    entry = ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
               "--leg", "cash:40.00", "--leg", "revenue:-40.00")
    ok(books, "entry", "reverse", entry)
    assert ok(books, "balance", "cash") == "0"


def test_reversal_prints_a_new_entry_id(books):
    entry = ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
               "--leg", "cash:40.00", "--leg", "revenue:-40.00")
    new = ok(books, "entry", "reverse", entry)
    assert new.isdigit() and new != entry


def test_reversal_carries_the_original_date_and_memo(books):
    entry = ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
               "--leg", "cash:40.00", "--leg", "revenue:-40.00")
    ok(books, "entry", "reverse", entry)
    rows = json.loads(ok(books, "entry", "list", "--format", "json"))
    reversal = [r for r in rows if r["memo"] == f"reversal of {entry}"]
    assert reversal, rows
    assert reversal[0]["date"] == "2026-01-05"


def test_reversal_legs_are_the_negation(books):
    entry = ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
               "--leg", "cash:40.00", "--leg", "revenue:-40.00")
    ok(books, "entry", "reverse", entry)
    rows = json.loads(ok(books, "entry", "list", "--format", "json"))
    reversal = next(r for r in rows if r["memo"] == f"reversal of {entry}")
    amounts = sorted(leg["amount"] for leg in reversal["legs"])
    assert amounts == [-4000, 4000]


# ── trial balance ─────────────────────────────────────────────────────────────────────────────

def test_trial_balance_json_has_exactly_the_contract_keys(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
       "--leg", "cash:10.00", "--leg", "revenue:-10.00")
    payload = json.loads(ok(books, "trial-balance", "--format", "json"))
    assert set(payload) == {"accounts", "totals"}
    assert set(payload["totals"]) == {"debits", "credits"}
    assert set(payload["accounts"][0]) == {"name", "type", "debits", "credits"}


def test_trial_balance_totals_are_equal(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "a",
       "--leg", "cash:33.33", "--leg", "revenue:-33.33")
    ok(books, "entry", "post", "--date", "2026-01-06", "--memo", "b",
       "--leg", "rent:7.77", "--leg", "loan:-7.77")
    payload = json.loads(ok(books, "trial-balance", "--format", "json"))
    assert payload["totals"]["debits"] == payload["totals"]["credits"]


def test_trial_balance_columns_are_never_negative(books):
    ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
       "--leg", "cash:10.00", "--leg", "revenue:-10.00")
    payload = json.loads(ok(books, "trial-balance", "--format", "json"))
    for row in payload["accounts"]:
        assert row["debits"] >= 0 and row["credits"] >= 0, row


def test_trial_balance_survives_a_reversal(books):
    entry = ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
               "--leg", "cash:10.00", "--leg", "revenue:-10.00")
    ok(books, "entry", "reverse", entry)
    payload = json.loads(ok(books, "trial-balance", "--format", "json"))
    assert payload["totals"]["debits"] == payload["totals"]["credits"]


# ── validation: nine distinct error paths ─────────────────────────────────────────────────────

def test_unbalanced_entry_is_rejected(books):
    assert run(books, "entry", "post", "--date", "2026-01-05",
               "--leg", "cash:5.00", "--leg", "revenue:-4.00").returncode != 0


def test_single_leg_entry_is_rejected(books):
    assert run(books, "entry", "post", "--date", "2026-01-05", "--leg", "cash:0.00").returncode != 0


def test_unknown_account_in_a_leg_is_rejected(books):
    assert run(books, "entry", "post", "--date", "2026-01-05",
               "--leg", "nosuch:5.00", "--leg", "cash:-5.00").returncode != 0


def test_duplicate_account_name_is_rejected(books):
    assert run(books, "account", "add", "cash", "--type", "asset").returncode != 0


def test_invalid_account_type_is_rejected(led):
    assert run(led, "account", "add", "x", "--type", "banana").returncode != 0


def test_invalid_amount_is_rejected(books):
    assert run(books, "entry", "post", "--date", "2026-01-05",
               "--leg", "cash:abc", "--leg", "revenue:-1.00").returncode != 0


def test_invalid_date_is_rejected(books):
    assert run(books, "entry", "post", "--date", "05-01-2026",
               "--leg", "cash:1.00", "--leg", "revenue:-1.00").returncode != 0


def test_reversing_an_unknown_entry_is_rejected(books):
    assert run(books, "entry", "reverse", "9999").returncode != 0


def test_reversing_twice_is_rejected(books):
    entry = ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
               "--leg", "cash:40.00", "--leg", "revenue:-40.00")
    ok(books, "entry", "reverse", entry)
    assert run(books, "entry", "reverse", entry).returncode != 0


def test_a_reversal_cannot_itself_be_reversed(books):
    entry = ok(books, "entry", "post", "--date", "2026-01-05", "--memo", "s",
               "--leg", "cash:40.00", "--leg", "revenue:-40.00")
    reversal = ok(books, "entry", "reverse", entry)
    assert run(books, "entry", "reverse", reversal).returncode != 0


def test_unknown_account_balance_is_rejected(books):
    assert run(books, "balance", "nosuch").returncode != 0


def test_invalid_format_is_rejected(books):
    assert run(books, "account", "list", "--format", "yaml").returncode != 0


def test_commands_before_init_fail_without_crashing(tmp_path):
    result = run(tmp_path / "fresh.db", "account", "list")
    assert result.returncode != 0
    assert "Traceback" not in result.stderr
