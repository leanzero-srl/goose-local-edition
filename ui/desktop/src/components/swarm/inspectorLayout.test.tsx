import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import path from 'node:path';

/**
 * THE INSPECTOR MUST NOT SPEND HALF ITSELF ON AN EMPTY BOX.
 *
 * The panes were a fixed `lg:grid-cols-2`, so through the whole OPEN and RESEARCH stretch — where the
 * model does nothing but reason — half the modal was a dead box captioned "Nothing emitted yet" while
 * the reasoning it WAS producing was squeezed into the other half. Mihai, having opened it to watch a
 * node work: "what is generating cause I can't see shit in it".
 *
 * Asserted on the source because the grid class is the whole mechanism and rendering the modal needs a
 * portal, a lane fixture and a run; this pins the rule that produced the bug at the line that fixes it.
 */
const SRC = readFileSync(
  path.join(__dirname, 'SwarmRunPanel.tsx'),
  'utf8'
);

describe('node inspector layout', () => {
  // The predicate moved from `outText` to `hasWork` in the same commit that removed `recent` from
  // `inspectorOutputText`. A tool lane's narration is ~1 character, so an `outText`-shaped predicate now
  // collapses the column on precisely the lane that has 60 tool calls to show.
  it('gives the work column its space whenever there is EITHER a call or narration', () => {
    expect(SRC).toMatch(/hasWork \? 'grid-cols-1 lg:grid-cols-2' : 'grid-cols-1'/);
    expect(SRC).toMatch(/const hasWork = calls\.length > 0 \|\| narration\.length > 0;/);
  });

  // A component declared in a render body is a new type every render, so React remounts the whole subtree
  // at the 500ms poll — which reset `follow` and re-fired the jump-to-bottom twice a second. The panes must
  // be declared at module scope.
  it('declares the panes at module scope, not inside the inspector body', () => {
    expect(SRC).toMatch(/^const InspectorPane: React\.FC</m);
    expect(SRC).toMatch(/^const WorkPane: React\.FC</m);
    expect(SRC).not.toMatch(/^ {2}const Pane: React\.FC</m);
  });

  it('never hard-codes the two-column grid on the pane container', () => {
    const container = SRC.slice(SRC.indexOf('flex-1 min-h-0 grid gap-3 p-3'));
    expect(container.slice(0, 200)).not.toMatch(/className="[^"]*lg:grid-cols-2/);
  });

  it('the empty Output says what will fill it, not just that it is empty', () => {
    expect(SRC).toContain('Still thinking — this fills with tool calls and written text once it starts acting.');
    expect(SRC).not.toContain('Nothing emitted yet — reasoning, but no tool call and no text.');
  });

  it('the supervisor badge no longer claims the lane is frozen', () => {
    expect(SRC).toContain("{'being reviewed'}");
    expect(SRC).not.toMatch(/buffers the worker's stream instead of processing it, so the counters below are genuinely\s*\n\s*\* *frozen/);
  });
});
