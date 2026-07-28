"""Pin the export contract — above all the profile hash, which is what makes a result auditable.

The manual positive control that proved the hash reacts to a fixture edit belongs in the suite, not
in a terminal someone ran once. If this ever stops failing on a changed fixture, results silently
stop being tied to the tasks that produced them.
"""

from __future__ import annotations

import json

import export


def test_profile_hash_covers_a_real_number_of_files():
    profile = export.profile_hash()
    assert profile["files"] > 20, profile
    assert len(profile["sha256"]) == 64


def test_profile_hash_is_stable_across_calls():
    assert export.profile_hash()["sha256"] == export.profile_hash()["sha256"]


def test_editing_a_fixture_changes_the_hash_and_restoring_reverts_it():
    """The control that matters: a task cannot drift without detaching from its baselines."""
    target = export.BOARD / "verticals/repair/fixtures/slugkit-easy/prompt.md"
    original = target.read_bytes()
    before = export.profile_hash()["sha256"]
    try:
        target.write_bytes(original + b"\n# edited\n")
        assert export.profile_hash()["sha256"] != before
    finally:
        target.write_bytes(original)
    assert export.profile_hash()["sha256"] == before


def test_editing_a_PROBE_also_changes_the_hash():
    """Grading bytes count too — a probe loosened after the fact is the same failure as a prompt."""
    target = export.BOARD / "probes/repair.py"
    original = target.read_bytes()
    before = export.profile_hash()["sha256"]
    try:
        target.write_bytes(original + b"\n# edited\n")
        assert export.profile_hash()["sha256"] != before
    finally:
        target.write_bytes(original)
    assert export.profile_hash()["sha256"] == before


def test_export_carries_the_refusals_and_the_not_measured_list():
    payload = export.export(export.BOARD / "runs")
    assert payload["refusals"], "the page must not be able to quietly drop them"
    assert any("composite" in r for r in payload["refusals"])
    for card_payload in payload["cards"]:
        assert card_payload["not_measured"], card_payload["vertical"]
    json.dumps(payload)  # must stay serialisable for the website


def test_export_reports_integrity_counters():
    payload = export.export(export.BOARD / "runs")
    integrity = payload["integrity"]
    assert integrity["crashes_and_timeouts_stay_in_denominator"] is True
    assert integrity["episodes"] >= 0
    assert isinstance(integrity["scored_zero_for_not_finishing"], list)
