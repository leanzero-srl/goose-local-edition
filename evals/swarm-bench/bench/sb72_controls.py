"""SB7.2 visual controls: one-defect copies of golden-sb72 for full-scorer mutant runs.

    python3 sb72_controls.py <out-dir> [name ...]

Each control changes exactly one visual property of the reference; the expected loss is the credit
that property justifies (sb7.2/DESIGN.md, "Proof"). Scoring is the caller's job — serially, through
score_sb72.py, with the reference's own seed. Never a model run.
"""
from __future__ import annotations

import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
GOLDEN = HERE / 'golden-sb72'

CONTROLS = {
    # The overview draws at 8% of its published colours: a near-black field (DeepSeek run 6's shape).
    'unlit-field': [('web/viz.js', 'vColor = uColors[idx] * shade;', 'vColor = uColors[idx] * shade * 0.08;')],
    # SB7.1's 460 px canvas under SB7.2's camera: the field no longer fills the canvas, caps shrink.
    'misframed-canvas': [('web/styles.css', '#viz3d { height: 640px; }', '#viz3d { height: 460px; }')],
    # Pending and failed swap colours everywhere: the field contradicts the published status legend.
    'status-swapped': [('web/viz.js', 'var STATUS_RGB = [[5, 150, 105], [217, 119, 6], [124, 58, 237], [185, 28, 28]];',
                        'var STATUS_RGB = [[5, 150, 105], [185, 28, 28], [124, 58, 237], [217, 119, 6]];')],
    # Solid cuboid towers: the shaft fills the footprint in the overview and in inspection.
    'cuboid-towers': [('web/viz.js', 'part(.27, .12, .90, false, false);', 'part(.45, .12, .90, false, false);'),
                      ('web/viz.js', 'box(0,0,.38,.38,.12,.90,0);', 'box(0,0,.90,.90,.12,.90,0);')],
    # The committed-update collar never moves (live or replay).
    'frozen-animation': [('web/viz.js', 'if (id !== inspectedId) return;', 'return;')],
    # The disabled Replay button's label drops to 3.37:1 effective contrast.
    'low-contrast-button': [('web/styles.css',
                             '#inspector-controls button:disabled { opacity:1; color:#b9c9dd; background:#263447; }',
                             '#inspector-controls button:disabled { opacity:1; color:#848484; background:#263447; }')],
}


def build(out: Path, name: str) -> Path:
    tree = out / name
    if tree.exists():
        raise FileExistsError(tree)
    shutil.copytree(GOLDEN, tree, ignore=shutil.ignore_patterns('__pycache__'))
    for file, old, new in CONTROLS[name]:
        path = tree / file
        source = path.read_text()
        if source.count(old) != 1:
            raise ValueError(f'{name}: {file} must contain the patched text exactly once')
        path.write_text(source.replace(old, new))
    return tree


def main(argv):
    out = Path(argv[0])
    out.mkdir(parents=True, exist_ok=True)
    for name in argv[1:] or CONTROLS:
        print(build(out, name))
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
