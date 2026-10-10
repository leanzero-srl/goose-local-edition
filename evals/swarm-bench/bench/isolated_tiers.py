"""The isolated benchmark tiers: one table that run_build.py and bench_rescore.py both read.

An isolated tier runs its entrant inside bench_isolation with only the public contract, the shared
SB7.1 starter, the bundled browser and the provider snapshot, grades against a fresh vendor and
writes a completion receipt the desktop's scoring retry can replay. SB7.2 is SB7.1's product with a
different public visual contract and scorer; everything else about the run is identical.

`public` maps each workdir file name to the repository file run_build renders into it (placeholders
substituted). SB7.1 hands out the frozen spec-build-sb7.md; SB7.2 hands out its own trimmed copies
of the behavioural contract and starter note (sb7.2/DESIGN.md "Public contract trim"), so SB7.1's
bytes never move when SB7.2's text does.

forge-1.0 (forge/DESIGN.md §12) is its own family: a different vendor module (`forge_site`, the dev Jira
site), a fenced network (localhost plus the provider relay), the pinned Forge kit cloned into the workdir and a
pinned reasoning effort. No tier arms a dollar stop: BENCH_MAX_USD applies only when the operator sets it (owner,
2026-10-02: "Let it run until the credits go away as long as the model is not stuck"). The defaults below keep SB7.1/SB7.2 identical in
behaviour: payments family, vendor_service_v3, open network, no kit, no effort pin.

forge-2.0 (forge2/SPEC.md) is the second Forge era, wholly in its own files: forge2/{public,starter,kit,site} and the
bench/*forge2* modules, so forge-1.0 stays reproducible byte for byte. It is the one tier with its own call budget.
"""
from __future__ import annotations

import os
from dataclasses import dataclass

from bench_budget import CALL_BUDGET


@dataclass(frozen=True)
class IsolatedTier:
    flag: str
    version: str
    scorer: str
    spec: str
    visual_contract: str
    starter: str
    scorer_files: tuple
    public: tuple
    family: str = 'payments'
    vendor: str = 'vendor_service_v3'
    network: str = 'open'
    kit: bool = False
    # policy: the provider reasoning effort pinned for every entrant (DESIGN §11: "medium unless the owner sets
    # otherwise"; BENCH_REASONING_EFFORT overrides). None = the model's own default, as SB7.x always ran.
    reasoning_effort: str | None = None
    # The scorer starts its own seeded site instead of run_build re-serving the vendor (forge/DESIGN.md §12).
    own_scoring_site: bool = False
    # The model calls a single-model entrant gets (bench_budget.call_budget_args); the tier's prompt states this
    # number (test_bench_budget holds every stated budget to it).
    call_budget: int = CALL_BUDGET

    @property
    def contracts(self):
        return (self.spec, *(source for _name, source in self.public))


SB71 = IsolatedTier('BENCH_SB71', 'sb-7.1', 'score_sb71', 'spec-build-sb71.md', 'sb7.1/VISUAL-CONTRACT.md',
                    'sb7.1/starter',
                    ('score_sb71.py', 'score_sb7.py', 'product_probe_sb71.mjs', 'product_probe_v3.mjs'),
                    (('SB7-CONTRACT.md', 'spec-build-sb7.md'), ('VISUAL-CONTRACT.md', 'sb7.1/VISUAL-CONTRACT.md')))
SB72 = IsolatedTier('BENCH_SB72', 'sb-7.2', 'score_sb72', 'spec-build-sb72.md', 'sb7.2/VISUAL-CONTRACT.md',
                    'sb7.1/starter',
                    ('score_sb72.py', 'score_sb71.py', 'score_sb7.py', 'product_probe_sb72.mjs',
                     'product_probe_sb71.mjs', 'product_probe_v3.mjs', 'sb72-thresholds.json'),
                    (('SB7-CONTRACT.md', 'sb7.2/SB7-CONTRACT.md'), ('VISUAL-CONTRACT.md', 'sb7.2/VISUAL-CONTRACT.md'),
                     ('STARTER.md', 'sb7.2/STARTER.md')))
FORGE10 = IsolatedTier('BENCH_FORGE10', 'forge-1.0', 'score_forge', 'forge/public/spec-build-forge.md', '',
                       'forge/starter',
                       ('score_forge.py', 'forge_oracle.py', 'forge_probe.mjs', 'forge-thresholds.json',
                        'forge_site.py', 'forge_kit.py', 'media_sb71.mjs'),
                       (('FORGE-CONTRACT.md', 'forge/public/FORGE-CONTRACT.md'), ('STARTER.md', 'forge/public/STARTER.md')),
                       family='forge', vendor='forge_site', network='fenced', kit=True,
                       reasoning_effort='medium', own_scoring_site=True)
FORGE20 = IsolatedTier('BENCH_FORGE20', 'forge-2.0', 'score_forge2', 'forge2/public/spec-build-forge2.md', '',
                       'forge2/starter',
                       ('score_forge2.py', 'forge2_checks.py', 'forge2_oracle.py', 'forge2_probe.mjs',
                        'forge2-thresholds.json', 'forge2_site.py', 'forge2_kit.py', 'media_sb71.mjs', 'forge2_uikit_shot.mjs'),
                       (('FORGE2-CONTRACT.md', 'forge2/public/FORGE2-CONTRACT.md'),
                        ('STARTER.md', 'forge2/public/STARTER.md'),
                        ('RATE-MODEL.json', 'forge2/public/RATE-MODEL.json'),
                        ('BROWSER-TESTING.md', 'forge2/public/BROWSER-TESTING.md')),
                       family='forge', vendor='forge2_site', network='fenced', kit=True,
                       reasoning_effort='medium', own_scoring_site=True,
                       # policy: forge2/SPEC.md §3 P11 — 300 calls ("ensure it involves a ton of calls", owner 2026-10-09;
                       # 1.0's 150 stays for every other tier, so SB7.2 and forge-1.0 results stay comparable).
                       call_budget=300)
TIERS = (SB71, SB72, FORGE10, FORGE20)
BY_VERSION = {tier.version: tier for tier in TIERS}


def active():
    """The isolated tier this process runs, or None. Two flags at once is a launcher defect."""
    chosen = [tier for tier in TIERS if os.environ.get(tier.flag)]
    if len(chosen) > 1:
        raise RuntimeError('REFUSED: more than one isolated tier flag is set: ' + ', '.join(t.flag for t in chosen))
    return chosen[0] if chosen else None
