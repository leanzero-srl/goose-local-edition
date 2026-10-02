"""The isolated benchmark tiers: one table that run_build.py and bench_rescore.py both read.

An isolated tier runs its entrant inside bench_isolation with only the public contract, the shared
SB7.1 starter, the bundled browser and the provider snapshot, grades against a fresh vendor and
writes a completion receipt the desktop's scoring retry can replay. SB7.2 is SB7.1's product with a
different public visual contract and scorer; everything else about the run is identical.

`public` maps each workdir file name to the repository file run_build renders into it (placeholders
substituted). SB7.1 hands out the frozen spec-build-sb7.md; SB7.2 hands out its own trimmed copies
of the behavioural contract and starter note (sb7.2/DESIGN.md "Public contract trim"), so SB7.1's
bytes never move when SB7.2's text does.
"""
from __future__ import annotations

import os
from dataclasses import dataclass


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
TIERS = (SB71, SB72)
BY_VERSION = {tier.version: tier for tier in TIERS}


def active():
    """The isolated tier this process runs, or None. Two flags at once is a launcher defect."""
    chosen = [tier for tier in TIERS if os.environ.get(tier.flag)]
    if len(chosen) > 1:
        raise RuntimeError('REFUSED: more than one isolated tier flag is set: ' + ', '.join(t.flag for t in chosen))
    return chosen[0] if chosen else None
