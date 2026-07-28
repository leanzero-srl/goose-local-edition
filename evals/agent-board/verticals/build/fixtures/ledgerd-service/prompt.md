Build a double-entry accounting ledger in Python. Package name `ledgerd`, run as `python -m ledgerd`.

Persist to SQLite via a GLOBAL `--db PATH` option that comes BEFORE the subcommand.

MONEY. Every amount is given on the command line as a decimal string ("12.34", "-0.05") and stored
and reported as an INTEGER number of minor units (cents). Ties round away from zero. Never use
floats — amounts must stay exact for values far beyond float precision.

ACCOUNTS. `account add NAME --type TYPE` where TYPE is one of asset, liability, equity, income,
expense. Prints the new integer account id. Names are unique. `account list [--format FORMAT]`
where FORMAT is table or json (default table); json prints an array of objects with exactly the keys
id, name, type.

ENTRIES. `entry post --date YYYY-MM-DD --memo TEXT --leg ACCOUNT:AMOUNT` where `--leg` is REPEATABLE
and ACCOUNT is an account NAME. A positive amount is a debit, a negative amount is a credit. An
entry must have at least two legs and its legs must sum to exactly zero. Prints the new entry id.

`entry list [--format FORMAT]` — json prints an array of objects with exactly the keys
id, date, memo, legs, where legs is an array of objects with exactly the keys account, amount.

`entry reverse ID` — posts a NEW entry, dated the same day, whose legs are the negation of entry
ID's, with memo "reversal of <ID>". Prints the new entry id. An entry may only be reversed once, and
a reversal may not itself be reversed.

BALANCES. `balance NAME [--as-of DATE]` prints the account's balance in minor units as a single
integer, counting only entries dated on or before DATE when given. The sign convention is by account
type: for asset and expense accounts the balance is (debits - credits); for liability, equity and
income accounts it is (credits - debits). So a normal balance is POSITIVE for every account type.

`trial-balance [--as-of DATE] [--format FORMAT]` — json prints an object with exactly the keys
accounts and totals. accounts is an array of objects with exactly the keys name, type, debits,
credits, all in minor units and never negative. totals is an object with exactly the keys debits and
credits, which MUST be equal.

VALIDATION. Exit with a clear message and a NONZERO exit code for EACH of: an entry whose legs do
not sum to zero; an entry with fewer than two legs; a leg naming an unknown account; a duplicate
account name; an invalid account type; an amount that is not a valid decimal; a date that is not
YYYY-MM-DD; reversing an unknown entry id; and reversing an entry that has already been reversed.

Any command run before `init` must fail with a nonzero exit code rather than crash.

Include your own tests. Put the package in `ledgerd/` and nothing else at the top level.
