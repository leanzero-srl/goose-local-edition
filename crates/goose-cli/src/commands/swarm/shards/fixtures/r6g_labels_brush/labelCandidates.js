// ── web/viz.js · section 7 (Labels) · piece: candidate selection ────────────
// The 12 records with highest a_major, ties broken by id ASC. Recomputed from
// the live rec arrays on every call — streamed creates can change it. Returns
// stable arrival indices n in priority order (a_major DESC, id ASC), which is
// exactly the order updateLabels must consider them for collision culling.

function labelCandidates() {
  const N = rec.ids.length;
  if (N === 0) return [];
  const idx = new Array(N);
  for (let i = 0; i < N; i++) idx[i] = i;
  idx.sort((a, b) => {
    const d = rec.aMajor[b] - rec.aMajor[a]; // a_major DESC
    if (d !== 0) return d > 0 ? 1 : -1;
    const ia = rec.ids[a], ib = rec.ids[b];   // id ASC
    return ia < ib ? -1 : ia > ib ? 1 : 0;
  });
  return idx.slice(0, 12);
}
