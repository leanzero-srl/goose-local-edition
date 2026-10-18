"""forge2_probe.mjs never spreads an array into a call.

`a.push(...xs)` and `Math.max(...xs)` pass every element as an argument and throw "RangeError: Maximum call stack
size exceeded" past V8's argument limit. Measured 2026-10-18: build cloud-03aded39 recorded enough platform calls in
the live phase that `ph.calls.push(...takeCalls(emu))` threw on one scoring site, 39 rows went UNAVAILABLE and the
verdict was unpublishable for a harness reason. The probe uses pushAll / maxOf / minOf instead; this test keeps the
spread form from coming back. The one allowed spread is the instrumented push wrapper, whose `...xs` is a single
call's own arguments.
"""
import pathlib
import re
import subprocess
import unittest

PROBE = pathlib.Path(__file__).with_name("forge2_probe.mjs")


def code_lines():
    for n, line in enumerate(PROBE.read_text().splitlines(), 1):
        code = line.split("//", 1)[0]
        if code.strip():
            yield n, code


class ProbeNeverSpreadsAnArrayIntoACall(unittest.TestCase):
    def test_no_spread_push(self):
        bad = [(n, c.strip()) for n, c in code_lines()
               if ".push(..." in c and "arr.push = (...xs)" not in c]
        self.assertEqual(bad, [], "use pushAll(arr, xs): a spread push overflows the stack on a call-heavy app")

    def test_no_spread_max_min(self):
        bad = [(n, c.strip()) for n, c in code_lines() if re.search(r"Math\.(max|min)\(\.\.\.", c)]
        self.assertEqual(bad, [], "use maxOf/minOf: a spread Math.max/min overflows the stack on a large array")

    def test_helpers_survive_an_array_the_spread_cannot(self):
        src = PROBE.read_text()
        helpers = "\n".join(l for l in src.splitlines() if l.startswith(("const pushAll", "const maxOf", "const minOf")))
        self.assertEqual(helpers.count("\n"), 2, helpers)
        js = helpers + """
const big = Array.from({ length: 600000 }, (_, i) => i);
let spread = 'ok'; try { [].push(...big); } catch (e) { spread = e.constructor.name; }
const a = []; pushAll(a, big);
console.log(JSON.stringify({ spread, pushed: a.length, max: maxOf(big), min: minOf(big),
  emptyMax: maxOf([]) === Math.max(), emptyMin: minOf([]) === Math.min() }));
"""
        out = subprocess.run(["node", "-e", js], capture_output=True, text=True, check=True).stdout
        self.assertEqual(out.strip(), '{"spread":"RangeError","pushed":600000,"max":599999,"min":0,"emptyMax":true,"emptyMin":true}')


if __name__ == "__main__":
    unittest.main()
