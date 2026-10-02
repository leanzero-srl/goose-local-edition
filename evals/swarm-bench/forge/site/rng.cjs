'use strict';
// xorshift128+ seeded from a 16-hex-char seed through splitmix64. No Math.random, no clock:
// the same seed yields the same stream on every host and Node version (BigInt arithmetic only).
const MASK = (1n << 64n) - 1n;
const SEED_RE = /^[0-9a-f]{16}$/;

function createRng(seedHex) {
  if (!SEED_RE.test(seedHex)) throw new Error(`seed must be 16 lowercase hex chars, got ${JSON.stringify(seedHex)}`);
  let sm = BigInt('0x' + seedHex);
  const splitmix = () => {
    sm = (sm + 0x9e3779b97f4a7c15n) & MASK;
    let z = sm;
    z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & MASK;
    z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & MASK;
    return z ^ (z >> 31n);
  };
  let s0 = splitmix();
  let s1 = splitmix();
  if (s0 === 0n && s1 === 0n) s1 = 1n;
  const next64 = () => {
    let a = s0;
    const b = s1;
    s0 = b;
    a = (a ^ ((a << 23n) & MASK)) & MASK;
    a ^= a >> 17n;
    a ^= b ^ (b >> 26n);
    s1 = a;
    return (s0 + s1) & MASK;
  };
  const float = () => Number(next64() >> 11n) / 2 ** 53;
  const int = (lo, hi) => lo + Math.floor(float() * (hi - lo + 1));
  const rng = {
    float,
    int,
    chance: (p) => float() < p,
    pick: (arr) => arr[Math.floor(float() * arr.length)],
    hex: (n) => {
      let s = '';
      while (s.length < n) s += next64().toString(16).padStart(16, '0');
      return s.slice(0, n);
    },
    uuid: () => {
      const h = rng.hex(32);
      return `${h.slice(0, 8)}-${h.slice(8, 12)}-4${h.slice(13, 16)}-${'89ab'[parseInt(h[16], 16) & 3]}${h.slice(17, 20)}-${h.slice(20, 32)}`;
    },
    shuffle: (arr) => {
      const a = arr.slice();
      for (let i = a.length - 1; i > 0; i--) {
        const j = Math.floor(float() * (i + 1));
        [a[i], a[j]] = [a[j], a[i]];
      }
      return a;
    },
    sample: (arr, n) => rng.shuffle(arr).slice(0, n),
    // n distinct integers in [lo, hi], ascending.
    distinctInts: (n, lo, hi) => {
      if (hi - lo + 1 < n) throw new Error('range too small');
      const set = new Set();
      while (set.size < n) set.add(int(lo, hi));
      return [...set].sort((x, y) => x - y);
    },
  };
  return rng;
}

module.exports = { createRng, SEED_RE };
