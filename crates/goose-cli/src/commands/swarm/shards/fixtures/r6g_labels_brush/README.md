PROVIDES: formatAmount(amountMinor: number, currency: string): string — integer minor units in the record's OWN currency, exponent from CURRENCY_EXP (JPY no decimals, KWD 3), thousands grouping, e.g. "EUR 1,299.00", "JPY 58", "KWD 46.700"
PROVIDES: fmtGroupDigits(intVal: number): string — private helper for formatAmount (locale-free thousands grouping)
PROVIDES: labelCandidates(): number[] — indices of the 12 records with highest a_major, ties id ASC, in that priority order; recomputed from live rec arrays each call
PROVIDES: updateLabels(): void — syncs #viz-labels DOM (class viz-label, data-id, inline border-box 110x18, single-line ellipsized) at (A.sx+10, A.sy-9); anchor project(x,h,z); eligible iff inside canvas AND pickAt returns that instance; culled on >=1px overlap with already-shown rects in priority order — hidden (display:none) or removed, never nudged
PROVIDES: LABEL_W/LABEL_H/LABEL_DX/LABEL_DY (const 110/18/10/-9), labelHost (let), labelEls (Map id->element), ensureLabelEl(id) — private label DOM state/helpers
PROVIDES: brushSet (const Set<string>) — the ONE brush set, single source of truth shared with app.js
PROVIDES: uBrushActive (let number 0/1) — derived uniform, kept == brushSet.size>0; shader dim = uBrushActive*(1-flag)
PROVIDES: dimFlags (const Uint8Array(65536)) — per-instance dim flag mirror (1=member); scene shard's writeInstance/appendInstance must read this (or brushSet.has(id)); NOT declared anywhere else
PROVIDES: brushCallbacks (const array), idToIndex(id): number, brushIdsAsc(): string[], updateBrushCount(): void, notifyBrush(idsAsc, clickedId) — private brush state/helpers
PROVIDES: toggleBrush(id: string): void — flips set + dimFlags[n] + uBrushActive, one writeInstance(n) (<= INSTANCE_STRIDE bytes, no realloc), updates #brush-count, requestRender(), notifies callbacks with (idsAsc, id)
PROVIDES: clearBrush(): void — empties set, zeros member flags via writeInstance per member (no realloc), uBrushActive=0, updates #brush-count, requestRender(), notifies with ([], null)
PROVIDES: onBrushChange(cb: (idsAsc: string[], clickedId: string|null) => void): void — registers a listener fired after every brush change
PROVIDES: window.vs7 = { toggleBrush, onBrushChange } — assigned at load by this section; exactly those two keys
ASSUMES: rec shared state (scene-data-render shard): parallel arrays ids/amountMinor/currency/status/aMajor plus x/z/h, n = stable arrival index, N = rec.ids.length
ASSUMES: S shared state {gl, Wcss, Hcss, DPR, Wdev, Hdev} is sized by resizeCanvas before the first renderFrame; updateLabels no-ops until then
ASSUMES: CURRENCY_EXP constant exists from section 1 (constants shard) with EUR:2 USD:2 JPY:0 KWD:3
ASSUMES: project(x,y,z) -> {sx,sy}|null per the printed contract and pickAt(sx,sy) -> {id,index}|null (camera-pick shard); pickAt refreshes the pick buffer if dirty and causes 0 default-FBO draws, so updateLabels' up-to-12 pickAt calls per frame are budget-safe
ASSUMES: writeInstance(n) (scene-data-render shard) uploads instance n's flag byte from dimFlags[n] (or equivalently brushSet.has(rec.ids[n])) via bufferSubData <= INSTANCE_STRIDE bytes with no realloc; the scene shard must NOT declare its own dimFlags/uBrushActive/brushSet — those belong to this section
ASSUMES: renderFrame (scene-data-render shard) calls updateLabels() at the end of every frame, so labels re-cull after setCamera, drags, coast ticks and stream batches without extra wiring here
ASSUMES: index.html provides #viz-labels (absolutely positioned over the canvas) and a #brush-count element; label geometry is set inline in JS so styles.css is not required for the 110x18 border-box / ellipsis contract
ASSUMES: app.js calls window.vs7.toggleBrush(id) from table row clicks (table->3D) and performs 3D->table navigation (page under active filters/sort, data-brushed=true, scroll into view) inside an onBrushChange callback; records filtered out just toggle
UNFINISHED: none
CHECKED_WITH: node --check on each of formatAmount.js labelCandidates.js updateLabels.js brushState.js brushApi.js -> all "OK" (exit 0, no output); also concatenated in assembly order (section 7 then 8) to a temp file and node --check passed, proving no duplicate declarations across my pieces; pieces reference sibling globals (rec, S, CURRENCY_EXP, project, pickAt, writeInstance, requestRender) so checks are parse-only per shard instructions
