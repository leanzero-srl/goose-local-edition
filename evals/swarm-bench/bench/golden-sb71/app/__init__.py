"""Meridian Payments Console — reference implementation (sb-7 golden).

Two cooperating services over one boot contract:

  python -m app            — boots ledgerd + notifierd together
  python -m app.ledgerd    — vendor sync, event ledger, API, UI host
  python -m app.notifierd  — idempotent notification consumer

Standard library only. ledgerd owns ledger.db, notifierd owns notifier.db; cross-service
truth flows over HTTP only.
"""
