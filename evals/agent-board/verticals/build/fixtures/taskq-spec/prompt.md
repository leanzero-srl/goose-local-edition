Build a command-line task queue in Python. Package name `taskq`, run as `python -m taskq`.

Persist to a JSON file given by a GLOBAL `--store PATH` option that comes BEFORE the subcommand.

Subcommands:
  add TITLE --priority N     add a task; priority is an integer, default 3; prints the new task id
  list [--format FORMAT]     FORMAT is table or json, default table; ordered by priority ascending
                             then by id ascending
  done ID                    mark a task complete
  purge                      remove every completed task; prints how many were removed

`list --format json` must print a JSON array of objects with exactly the keys
id, title, priority, done.

Exit with a clear error and a NONZERO exit code for each of: an unknown id on `done`;
a non-integer priority on `add`; and an invalid `--format` value.

Include your own tests. Do not create a directory other than `taskq/` for the package itself.
